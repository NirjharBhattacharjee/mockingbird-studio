//! Jobs run in a detached runner: they outlive whoever queued them, run two
//! at a time, stop when cancelled, and a crashed runner's jobs are marked
//! interrupted by the next one.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{BIN, Client};
use jiff::Timestamp;
use serde_json::{Value, json};

fn home(name: &str) -> PathBuf {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("jobs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("config.toml"),
        r#"
[tool.slow]
command = ["/bin/sh", "-c", "sleep 1; echo made > \"$1\"", "sh", "{out}"]

[tool.nap]
command = ["/bin/sh", "-c", "sleep {secs}"]
"#,
    )
    .unwrap();
    root
}

fn cli(home: &Path, args: &[&str]) -> Result<Value, String> {
    let out = Command::new(BIN)
        .args(args)
        .arg("--json")
        .env("MOCKINGBIRD_STUDIO_HOME", home)
        .output()
        .unwrap();
    if out.status.success() {
        Ok(serde_json::from_slice(&out.stdout).unwrap())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn job(home: &Path, id: &str) -> Value {
    cli(home, &["job", "--id", id]).unwrap()
}

fn start(home: &Path, tool: &str, values: &[&str]) -> String {
    let mut args = vec!["job-start", "--kind", "tool", "--tool", tool, "--values"];
    args.extend(values);
    cli(home, &args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_state(home: &Path, id: &str, state: &str) -> Value {
    let mut j = Value::Null;
    wait_for(&format!("{id} to be {state}"), || {
        j = job(home, id);
        j["state"] == state
    });
    j
}

fn time(v: &Value) -> Timestamp {
    v.as_str().unwrap().parse().unwrap()
}

fn group_gone(pid: u64) -> bool {
    let alive = Command::new("kill")
        .args(["-0", "--", &format!("-{pid}")])
        .status()
        .unwrap();
    !alive.success()
}

fn kill(args: &[&str]) {
    Command::new("kill").args(args).status().unwrap();
}

#[test]
fn a_job_outlives_the_process_that_queued_it() {
    let home = home("outlive");
    let dir = home.join("video");
    let mut c = Client::start(&home);
    c.call("project_new", json!({"dir": dir, "title": "Example"}));
    let (queued, _) = c.call(
        "job_start",
        json!({"kind": "tool", "tool": "slow", "project": dir,
               "step": "research", "values": ["out=research/made.txt"]}),
    );
    assert_eq!(queued["state"], "queued");
    let id = queued["id"].as_str().unwrap();

    // Claude Desktop quitting: the MCP server is killed outright.
    let began = Instant::now();
    c.child.kill().unwrap();
    c.child.wait().unwrap();

    let done = wait_state(&home, id, "done");
    eprintln!("job finished {:?} after its caller died", began.elapsed());
    assert_eq!(done["outputs"], json!(["research/made.txt"]));
    assert_eq!(
        fs::read_to_string(dir.join("research/made.txt")).unwrap(),
        "made\n"
    );
    // The job is listed with the project, and its output isn't submitted.
    let status = cli(&home, &["status", "--project", dir.to_str().unwrap()]).unwrap();
    assert_eq!(status["jobs"][0]["id"], id);
    assert_eq!(status["steps"][0]["state"], "todo");

    // A step's job is refused under the submit rule, and a tool only takes
    // the values its command names.
    let p = dir.to_str().unwrap();
    let err = cli(
        &home,
        &[
            "job-start",
            "--kind",
            "tool",
            "--tool",
            "slow",
            "--project",
            p,
            "--step",
            "script",
            "--values",
            "out=script/x.txt",
        ],
    )
    .unwrap_err();
    assert!(err.contains("waits for research"), "{err}");
    for (tool, values) in [("slow", "secs=1"), ("rm", "out=x"), ("slow", "out=../x")] {
        let args = [
            "job-start",
            "--kind",
            "tool",
            "--tool",
            tool,
            "--values",
            values,
        ];
        assert!(cli(&home, &args).is_err(), "{tool} {values}");
    }
}

#[test]
fn two_at_a_time_then_cancel() {
    let home = home("two");
    let ids: Vec<String> = (0..3).map(|_| start(&home, "nap", &["secs=1"])).collect();
    let jobs: Vec<Value> = ids.iter().map(|id| wait_state(&home, id, "done")).collect();
    let (a, b, c) = (&jobs[0], &jobs[1], &jobs[2]);
    // The first two ran together; the third waited for one of them.
    assert!(time(&b["started"]) < time(&a["ended"]), "{a}\n{b}");
    let first_end = time(&a["ended"]).min(time(&b["ended"]));
    assert!(time(&c["started"]) >= first_end, "{a}\n{b}\n{c}");

    // A cancelled job's whole process group stops: sh and its sleep.
    let id = start(&home, "nap", &["secs=30"]);
    let pid = wait_state(&home, &id, "running")["pid"].as_u64().unwrap();
    cli(&home, &["job-cancel", "--id", &id]).unwrap();
    let j = wait_state(&home, &id, "cancelled");
    assert!(time(&j["ended"]) > time(&j["started"]));
    assert!(group_gone(pid), "process group {pid} still running");
    assert!(cli(&home, &["job-cancel", "--id", &id]).is_err());
}

#[test]
fn a_crashed_runner_leaves_its_jobs_interrupted() {
    let home = home("crash");
    let id = start(&home, "nap", &["secs=30"]);
    let pid = wait_state(&home, &id, "running")["pid"].as_u64().unwrap();
    let runner = fs::read_to_string(home.join("runner.lock")).unwrap();
    kill(&["-9", runner.trim()]);

    // Looking at the job starts a new runner, which finds it running, stops
    // the process group the dead runner left behind, and marks it.
    let j = wait_state(&home, &id, "failed");
    assert!(group_gone(pid), "process group {pid} still running");
    assert_eq!(
        (j["state"].as_str(), j["error"].as_str()),
        (Some("failed"), Some("interrupted"))
    );
}

fn mb_start(home: &Path, project: &Path, tool: &str, args: Value) -> Result<Value, String> {
    let (p, a) = (project.to_str().unwrap(), args.to_string());
    let argv = [
        "job-start",
        "--kind",
        "mockingbird",
        "--tool",
        tool,
        "--project",
        p,
        "--args",
        &a,
    ];
    cli(home, &argv)
}

/// Waits for a job to end however long it takes, and returns it.
fn wait_end(home: &Path, id: &str) -> Value {
    loop {
        let j = job(home, id);
        if !matches!(j["state"].as_str(), Some("queued" | "running")) {
            return j;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn a_mockingbird_job_calls_one_tool_over_mcp() {
    let home = home("mockingbird");
    let dir = home.join("video");
    cli(
        &home,
        &[
            "project-new",
            "--dir",
            dir.to_str().unwrap(),
            "--title",
            "Example",
        ],
    )
    .unwrap();
    let out = dir.canonicalize().unwrap().join("renders/a.mp4");
    // A stand-in for `mockingbird mcp`: it answers, sends a notification and
    // a request of its own, and replies only once the client has answered.
    let fake = home.join("fake-mockingbird");
    fs::write(
        &fake,
        format!(
            r#"#!/bin/sh
[ "$1" = mcp ] || exit 9
read l; echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-06-18","capabilities":{{}},"serverInfo":{{"name":"fake","version":"0"}}}}}}'
read l; read call
echo '{{"jsonrpc":"2.0","method":"notifications/progress","params":{{}}}}'
echo '{{"jsonrpc":"2.0","id":"s1","method":"ping"}}'
read l; case "$l" in *'"id":"s1"'*) ;; *) exit 3;; esac
case "$call" in *'"name":"motion"'*) ;; *) exit 4;; esac
case "$call" in *'"duration":2'*) ;; *) exit 5;; esac
mkdir -p '{dir}'; printf made > '{out}'
echo '{{"jsonrpc":"2.0","id":2,"result":{{"content":[{{"type":"text","text":"rendered"}}],"isError":false}}}}'
cat > /dev/null
"#,
            dir = out.parent().unwrap().display(),
            out = out.display()
        ),
    )
    .unwrap();
    Command::new("chmod")
        .arg("755")
        .arg(&fake)
        .status()
        .unwrap();
    fs::write(
        home.join("config.toml"),
        format!("mockingbird = {:?}\n", fake.to_str().unwrap()),
    )
    .unwrap();

    let args = json!({"script": "/x/page.html", "duration": 2, "out": out});
    let id = mb_start(&home, &dir, "motion", args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let j = wait_end(&home, &id);
    assert_eq!(j["state"], "done", "{j}");
    assert_eq!(j["kind"], "mockingbird");
    assert_eq!(j["outputs"], json!(["renders/a.mp4"]));
    assert!(j["log"].as_str().unwrap().contains("rendered"), "{j}");

    // mockingbird going away without a reply fails the job.
    let args = json!({"script": "/x/page.html", "duration": 3});
    let id = mb_start(&home, &dir, "motion", args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(wait_end(&home, &id)["state"], "failed");

    for (tool, args) in [
        ("transcribe", json!({})),
        ("motion", json!(["not", "an", "object"])),
    ] {
        assert!(mb_start(&home, &dir, tool, args).is_err(), "{tool}");
    }
    let p = dir.to_str().unwrap();
    let with_values = [
        "job-start",
        "--kind",
        "mockingbird",
        "--tool",
        "motion",
        "--project",
        p,
        "--args",
        "{}",
        "--values",
        "out=x",
    ];
    assert!(cli(&home, &with_values).is_err());
}

/// S4: a long `motion` call through the runner, with no timeout. The length
/// is `MOTION_SECONDS` of video (default 2); about 5 minutes of rendering is
/// the measurement. Needs mockingbird and its Chrome.
#[test]
#[ignore]
fn a_real_motion_call_runs_to_the_end() {
    let home = home("motion");
    let dir = home.join("video");
    cli(
        &home,
        &[
            "project-new",
            "--dir",
            dir.to_str().unwrap(),
            "--title",
            "Example",
        ],
    )
    .unwrap();
    let program = std::env::var("MOCKINGBIRD")
        .unwrap_or_else(|_| format!("{}/.local/bin/mockingbird", std::env::var("HOME").unwrap()));
    fs::write(
        home.join("config.toml"),
        format!("mockingbird = {program:?}\n"),
    )
    .unwrap();
    let dir = dir.canonicalize().unwrap();
    let page = dir.join("page.html");
    fs::write(
        &page,
        "<!doctype html><style>body{margin:0;background:#123}\
         div{width:200px;height:200px;margin:200px;background:#fc0;\
         animation:spin 2s linear infinite}\
         @keyframes spin{to{transform:rotate(360deg)}}</style><div></div>",
    )
    .unwrap();
    let secs: f64 = std::env::var("MOTION_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let out = dir.join("motion.mp4");
    let args = json!({"script": page, "duration": secs, "size": "640x360", "out": out});
    let id = mb_start(&home, &dir, "motion", args).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let began = Instant::now();
    let j = wait_end(&home, &id);
    eprintln!(
        "motion of {secs} s: {} after {:?}\n{}",
        j["state"],
        began.elapsed(),
        j["log"].as_str().unwrap_or_default()
    );
    assert_eq!(j["state"], "done", "{j}");
    assert_eq!(j["outputs"], json!(["motion.mp4"]));
    assert!(fs::metadata(&out).unwrap().len() > 0);
}
