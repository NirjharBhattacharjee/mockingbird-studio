//! Jobs: long tasks that keep going after the chat or window that queued
//! them has gone. Each job is a folder in `~/.mockingbird-studio/jobs/`.
//! The caller writes `job.toml` as queued and makes sure a runner is up.
//! From then on only the runner writes it. The runner is
//! `mockingbird-studio runner` in its own session: it holds `runner.lock`,
//! runs queued jobs oldest first, a few at a time, and exits when idle.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::project::Result;
use crate::store;

const TICK: Duration = Duration::from_millis(200);
const IDLE: Duration = Duration::from_secs(30);
const KILL_AFTER: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub tool: String,
    /// The command with its placeholders filled, fixed when the job is
    /// queued.
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub state: JobState,
    pub created: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// The `out` files that exist after the job is done, relative to the
    /// folder it ran in. They aren't submitted: the agent looks first.
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The values given for the tool's placeholders.
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(flatten)]
    pub rest: toml::Table,
}

impl Job {
    /// The project folder, or the job's own folder when it has no project.
    fn cwd(&self) -> PathBuf {
        self.project.clone().unwrap_or_else(|| job_dir(&self.id))
    }
}

pub fn jobs_dir() -> PathBuf {
    store::home().join("jobs")
}

fn job_dir(id: &str) -> PathBuf {
    jobs_dir().join(id)
}

fn cancel_file(id: &str) -> PathBuf {
    job_dir(id).join("cancel")
}

fn save(job: &Job) -> Result<()> {
    store::write(&job_dir(&job.id).join("job.toml"), job)
}

/// A job by id. Ids are made here, so anything else is refused before it
/// becomes a path.
pub fn read(id: &str) -> Result<Job> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(format!("no job {id:?}"));
    }
    let path = job_dir(id).join("job.toml");
    if !path.is_file() {
        return Err(format!("no job {id:?}"));
    }
    store::read(&path)
}

/// Every job, oldest first, optionally only one project's.
// ponytail: reads every job.toml on each call, and the runner calls it every
// tick. Move finished jobs aside if the folder grows into the thousands.
pub fn list(project: Option<&Path>) -> Vec<Job> {
    let Ok(dirs) = fs::read_dir(jobs_dir()) else {
        return vec![];
    };
    let mut jobs: Vec<Job> = dirs
        .flatten()
        .filter_map(|d| store::read(&d.path().join("job.toml")).ok())
        .filter(|j: &Job| project.is_none_or(|p| j.project.as_deref() == Some(p)))
        .collect();
    jobs.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
    jobs
}

/// The last few kilobytes of a job's log, from the start of a line.
pub fn log_tail(id: &str) -> String {
    const TAIL: u64 = 8192;
    let Ok(mut f) = File::open(job_dir(id).join("log.txt")) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(TAIL);
    let mut bytes = Vec::new();
    let _ = f
        .seek(SeekFrom::Start(from))
        .and_then(|_| f.read_to_end(&mut bytes));
    let text = String::from_utf8_lossy(&bytes);
    match text.split_once('\n') {
        Some((_, rest)) if from > 0 => rest.to_string(),
        _ => text.into_owned(),
    }
}

/// A declared tool's command, from `[tool.<name>]` in config.toml.
fn declared(name: &str) -> Result<Vec<String>> {
    let config = store::read_config()?;
    let tool = config
        .rest
        .get("tool")
        .and_then(|t| t.get(name))
        .ok_or_else(|| {
            format!(
                "no tool {name:?} in [tool] in {}",
                store::config_path().display()
            )
        })?;
    let command: Option<Vec<String>> = tool
        .get("command")
        .and_then(|c| c.as_array())
        .map(|c| c.iter().map(|p| p.as_str().map(str::to_string)).collect())
        .and_then(|c: Option<Vec<String>>| c);
    match command {
        Some(c) if !c.is_empty() && !c[0].is_empty() => Ok(c),
        _ => Err(format!(
            "tool {name:?} needs command = [\"program\", \"arg\", …]"
        )),
    }
}

fn is_out(name: &str) -> bool {
    name == "out" || name.starts_with("out_")
}

/// The command with each `{name}` replaced by its value. Every placeholder
/// needs a value and every value a placeholder. A value fills its place in
/// one argument and never goes through a shell.
fn fill(command: &[String], values: &BTreeMap<String, String>) -> Result<Vec<String>> {
    for (name, v) in values {
        if v.starts_with('-') {
            return Err(format!("{name}: a value can't start with '-'"));
        }
        let p = Path::new(v);
        if is_out(name)
            && (v.is_empty()
                || p.is_absolute()
                || p.components().any(|c| c == Component::ParentDir))
        {
            return Err(format!(
                "{name}: an out file is a path inside the folder the job runs in"
            ));
        }
    }
    let mut used = BTreeSet::new();
    let mut missing = BTreeSet::new();
    let mut filled = Vec::new();
    for part in command {
        let mut out = String::new();
        let mut rest = part.as_str();
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let name = after.find('}').map(|j| &after[..j]).filter(|n| {
                !n.is_empty()
                    && n.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            });
            match name {
                Some(name) => {
                    match values.get(name) {
                        Some(v) => out.push_str(v),
                        None => {
                            missing.insert(name);
                        }
                    }
                    used.insert(name);
                    rest = &after[name.len() + 1..];
                }
                None => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        filled.push(out);
    }
    if !missing.is_empty() {
        let m: Vec<_> = missing.into_iter().collect();
        return Err(format!("needs values for {}", m.join(", ")));
    }
    let extra: Vec<_> = values
        .keys()
        .filter(|k| !used.contains(k.as_str()))
        .map(String::as_str)
        .collect();
    if !extra.is_empty() {
        return Err(format!("has no placeholder for {}", extra.join(", ")));
    }
    Ok(filled)
}

/// `20261125-182512-7f3a`: the time, then four hex digits.
fn new_dir(now: Timestamp) -> Result<String> {
    fs::create_dir_all(jobs_dir())
        .map_err(|e| format!("can't make {}: {e}", jobs_dir().display()))?;
    let seed = (now.subsec_nanosecond() as u32) ^ std::process::id().rotate_left(16);
    for n in 0..64u32 {
        let id = format!(
            "{}-{:04x}",
            now.strftime("%Y%m%d-%H%M%S"),
            seed.wrapping_add(n.wrapping_mul(40503)) & 0xffff
        );
        match fs::create_dir(job_dir(&id)) {
            Ok(()) => return Ok(id),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("can't make a job folder: {e}")),
        }
    }
    Err("can't find a free job id".into())
}

/// Queues a `tool` job and makes sure a runner is up. `project` is
/// canonical and `step`, if any, has passed the submit rule.
pub fn start(
    tool: &str,
    values: BTreeMap<String, String>,
    project: Option<PathBuf>,
    step: Option<String>,
) -> Result<Job> {
    let command = fill(&declared(tool)?, &values).map_err(|e| format!("tool {tool:?} {e}"))?;
    let now = Timestamp::now();
    let job = Job {
        id: new_dir(now)?,
        kind: "tool".into(),
        tool: tool.into(),
        command,
        project,
        step,
        state: JobState::Queued,
        created: now,
        started: None,
        ended: None,
        pid: None,
        outputs: vec![],
        error: None,
        values,
        rest: Default::default(),
    };
    save(&job)?;
    // Written before the runner check, so a runner on its way out sees it.
    ensure_runner()?;
    Ok(job)
}

/// Asks the runner to stop a queued or running job.
pub fn cancel(id: &str) -> Result<Job> {
    let job = read(id)?;
    if !matches!(job.state, JobState::Queued | JobState::Running) {
        return Err(format!("job {id} is already {:?}", job.state).to_lowercase());
    }
    File::create(cancel_file(id)).map_err(|e| format!("can't cancel {id}: {e}"))?;
    Ok(job)
}

fn open_lock() -> Result<File> {
    let path = store::home().join("runner.lock");
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("can't open {}: {e}", path.display()))
}

/// Starts `mockingbird-studio runner` in its own session unless one holds
/// the lock. Two callers may both start one; the second exits at once.
fn ensure_runner() -> Result<()> {
    match open_lock()?.try_lock() {
        Ok(()) => {} // Released when the file drops, for the runner to take.
        Err(TryLockError::WouldBlock) => return Ok(()),
        Err(TryLockError::Error(e)) => return Err(format!("can't check the runner: {e}")),
    }
    let path = store::home().join("runner.log");
    let io = |e: std::io::Error| format!("can't start the runner: {e}");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    let mut cmd = Command::new(std::env::current_exe().map_err(io)?);
    cmd.arg("runner")
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(io)?)
        .stderr(log);
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(io)?;
    // Reaped here so a long-lived MCP server doesn't collect zombies.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// `mockingbird-studio runner`. Holds `runner.lock` while it runs jobs and
/// exits after 30 idle seconds. If a job was queued as it let go of the
/// lock, it takes the lock back.
pub fn run_runner() -> ExitCode {
    loop {
        let lock = match open_lock() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("mockingbird-studio runner: {e}");
                return ExitCode::FAILURE;
            }
        };
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return ExitCode::SUCCESS,
            Err(TryLockError::Error(e)) => {
                eprintln!("mockingbird-studio runner: {e}");
                return ExitCode::FAILURE;
            }
        }
        let _ = lock.set_len(0);
        let _ = writeln!(&lock, "{}", std::process::id());
        if let Err(e) = serve() {
            eprintln!("mockingbird-studio runner: {e}");
            return ExitCode::FAILURE;
        }
        drop(lock);
        if !list(None).iter().any(|j| j.state == JobState::Queued) {
            return ExitCode::SUCCESS;
        }
    }
}

struct Running {
    job: Job,
    child: Child,
    term_at: Option<Instant>,
}

fn signal(pid: u32, sig: libc::c_int) {
    // The job leads its own process group, so this reaches what it started.
    // SAFETY: kill takes plain integers.
    unsafe { libc::kill(-(pid as libc::pid_t), sig) };
}

fn at_once() -> usize {
    store::read_config()
        .ok()
        .and_then(|c| c.rest.get("jobs_at_once").and_then(|v| v.as_integer()))
        .filter(|n| *n > 0)
        .map_or(2, |n| n as usize)
}

fn spawn(job: &Job) -> std::io::Result<Child> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(job_dir(&job.id).join("log.txt"))?;
    Command::new(&job.command[0])
        .args(&job.command[1..])
        .current_dir(job.cwd())
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()
}

fn finish(r: &mut Running, status: std::io::Result<ExitStatus>) {
    let job = &mut r.job;
    job.ended = Some(Timestamp::now());
    match status {
        Ok(s) if s.success() => {
            job.state = JobState::Done;
            let cwd = job.cwd();
            job.outputs = job
                .values
                .iter()
                .filter(|(k, v)| is_out(k) && cwd.join(v).is_file())
                .map(|(_, v)| v.clone())
                .collect();
        }
        _ if r.term_at.is_some() => job.state = JobState::Cancelled,
        Ok(s) => {
            job.state = JobState::Failed;
            job.error = Some(match (s.code(), s.signal()) {
                (Some(c), _) => format!("exited with code {c}"),
                (_, Some(sig)) => format!("killed by signal {sig}"),
                _ => "stopped".into(),
            });
        }
        Err(e) => {
            job.state = JobState::Failed;
            job.error = Some(format!("can't wait for it: {e}"));
        }
    }
}

/// Runs jobs until there's been nothing to do for `IDLE`.
fn serve() -> Result<()> {
    // Whoever ran these is gone: only the lock holder runs jobs.
    for mut job in list(None) {
        if job.state == JobState::Running {
            job.state = JobState::Failed;
            job.error = Some("interrupted".into());
            job.ended = Some(Timestamp::now());
            save(&job)?;
        }
    }
    let mut running: Vec<Running> = vec![];
    let mut idle_since = Instant::now();
    loop {
        for r in &mut running {
            let pid = r.child.id();
            match r.term_at {
                None if cancel_file(&r.job.id).exists() => {
                    signal(pid, libc::SIGTERM);
                    r.term_at = Some(Instant::now());
                }
                Some(t) if t.elapsed() >= KILL_AFTER => signal(pid, libc::SIGKILL),
                _ => {}
            }
        }
        let mut still = vec![];
        for mut r in running {
            match r.child.try_wait() {
                Ok(None) => still.push(r),
                Ok(Some(s)) => {
                    finish(&mut r, Ok(s));
                    save(&r.job)?;
                }
                Err(e) => {
                    finish(&mut r, Err(e));
                    save(&r.job)?;
                }
            }
        }
        running = still;

        let limit = at_once();
        let mut waiting = false;
        for mut job in list(None)
            .into_iter()
            .filter(|j| j.state == JobState::Queued)
        {
            if cancel_file(&job.id).exists() {
                job.state = JobState::Cancelled;
                job.ended = Some(Timestamp::now());
                save(&job)?;
                continue;
            }
            if running.len() >= limit {
                waiting = true;
                continue;
            }
            job.started = Some(Timestamp::now());
            match spawn(&job) {
                Ok(child) => {
                    job.state = JobState::Running;
                    job.pid = Some(child.id());
                    save(&job)?;
                    running.push(Running {
                        job,
                        child,
                        term_at: None,
                    });
                }
                Err(e) => {
                    job.state = JobState::Failed;
                    job.ended = job.started;
                    job.error = Some(format!("can't start {}: {e}", job.command[0]));
                    save(&job)?;
                }
            }
        }

        if !running.is_empty() || waiting {
            idle_since = Instant::now();
        } else if idle_since.elapsed() >= IDLE {
            return Ok(());
        }
        std::thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn cmd(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn placeholders_are_filled_and_checked() {
        let c = cmd(&["tts", "--out={out}", "{script}", "{not a name}", "{"]);
        let v = vals(&[("out", "voice/a.wav"), ("script", "script/s.md")]);
        assert_eq!(
            fill(&c, &v).unwrap(),
            cmd(&[
                "tts",
                "--out=voice/a.wav",
                "script/s.md",
                "{not a name}",
                "{"
            ])
        );
        let err = fill(&c, &vals(&[("out", "a.wav")])).unwrap_err();
        assert!(err.contains("needs values for script"), "{err}");
        let mut extra = v.clone();
        extra.insert("x".into(), "1".into());
        assert!(
            fill(&c, &extra)
                .unwrap_err()
                .contains("no placeholder for x")
        );
        for (k, bad) in [
            ("script", "--rm"),
            ("out", "/etc/passwd"),
            ("out", "../x"),
            ("out_b", "a/../../x"),
        ] {
            let mut v = v.clone();
            v.insert(k.into(), bad.into());
            assert!(fill(&c, &v).is_err(), "{k}={bad}");
        }
    }

    #[test]
    fn a_job_id_is_not_a_path() {
        for bad in ["", "../x", "a/b", "20261125-182512-7f3a/.."] {
            assert!(read(bad).unwrap_err().starts_with("no job"), "{bad}");
        }
    }
}
