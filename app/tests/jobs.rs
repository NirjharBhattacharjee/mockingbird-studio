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
