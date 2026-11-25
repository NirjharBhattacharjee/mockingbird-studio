//! Starts `mockingbird-studio mcp` and speaks JSON-RPC over its stdio.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use studio_core::ops::OPS;

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Client {
    fn start(home: &std::path::Path) -> Client {
        let mut child = Command::new(BIN)
            .arg("mcp")
            .env("MOCKINGBIRD_STUDIO_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin: Some(stdin),
            stdout,
            id: 0,
        };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"},
            }),
        );
        assert!(init["instructions"].as_str().unwrap().contains("status"));
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin.as_mut().unwrap(), "{msg}").unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server hung up"
            );
            let msg: Value = serde_json::from_str(&line).expect("stdout is JSON-RPC only");
            if msg["id"] == id {
                assert!(msg["error"].is_null(), "{msg}");
                return msg["result"].clone();
            }
        }
    }

    /// Calls a tool. Returns the JSON reply and the waiting line after it.
    fn call(&mut self, name: &str, args: Value) -> (Value, String) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let text = r["content"][0]["text"].as_str().unwrap();
        let (body, line) = text.rsplit_once('\n').unwrap();
        assert_eq!(r["isError"], false, "{name}: {body}");
        (serde_json::from_str(body).unwrap(), line.to_string())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_server_exits_cleanly_when_stdin_closes() {
    let home = tempfile::tempdir().unwrap();
    let mut c = Client::start(home.path());
    c.call("projects", json!({}));
    c.stdin = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = c.child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "still running after stdin closed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn every_operation_is_an_mcp_tool_with_a_schema() {
    let home = tempfile::tempdir().unwrap();
    let mut c = Client::start(home.path());
    let tools = c.request("tools/list", json!({}))["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    let ops: Vec<&str> = OPS.iter().map(|o| o.name).collect();
    assert_eq!(names, ops);
    let submit = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "submit")
        .unwrap();
    assert_eq!(
        submit["inputSchema"]["required"],
        json!(["project", "step", "files"])
    );
    assert_eq!(
        submit["inputSchema"]["properties"]["files"]["type"],
        "array"
    );
}

#[test]
fn every_operation_is_a_cli_command() {
    for op in OPS {
        let cmd = op.name.replace('_', "-");
        let out = Command::new(BIN).args([&cmd, "--help"]).output().unwrap();
        assert!(out.status.success(), "no CLI command {cmd}");
    }
}

#[test]
fn the_notes_loop_over_json_rpc() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let dir = work.path().join("loop");
    let mut c = Client::start(home.path());

    let (_, line) = c.call("project_new", json!({"dir": dir, "title": "Loop"}));
    assert!(
        line.starts_with("Open notes: 0. Steps waiting on you: loop/research"),
        "{line}"
    );
    std::fs::write(dir.join("research/topic.md"), "x").unwrap();
    let p = |extra: Value| {
        let mut v = json!({"project": "loop"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    let files = json!({"step": "research", "files": ["research/topic.md"]});
    c.call("submit", p(files.clone()));
    let (_, line) = c.call(
        "note_add",
        p(json!({"step": "research", "text": "sharper"})),
    );
    assert!(line.starts_with("Open notes: 1."), "{line}");
    c.call("redo", p(json!({"step": "research", "note": "tighten it"})));
    let (notes, line) = c.call("notes", p(json!({})));
    assert_eq!(notes["redo"][0]["note"], "tighten it");
    assert!(line.contains("loop/research (redo)"), "{line}");
    let id = notes["notes"][0]["id"].clone();
    c.call("note_done", p(json!({"id": id, "reply": "done"})));
    c.call("submit", p(files));
    let (d, line) = c.call("approve", p(json!({"step": "research", "note": "yes"})));
    assert_eq!(d["via"], "mcp");
    assert!(line.starts_with("Open notes: 0."), "{line}");

    let toml = std::fs::read_to_string(dir.join("project.toml")).unwrap();
    assert!(toml.contains("via = \"mcp\""), "{toml}");

    // A refused call is a tool error the agent can read, still with the line.
    let r = c.request(
        "tools/call",
        json!({"name": "approve", "arguments": {"project": "loop", "step": "script", "via": "window"}}),
    );
    assert_eq!(r["isError"], true);
    let text = r["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("no argument \"via\""), "{text}");
    assert!(text.ends_with("."), "{text}");

    // Two projects named "loop": the line shows full paths. One that won't
    // parse is named in the line, not skipped.
    let twin = work.path().join("b/loop");
    let (_, line) = c.call("project_new", json!({"dir": twin, "title": "Twin"}));
    let twin = twin.canonicalize().unwrap();
    assert!(
        line.contains(&format!("{}/research", twin.display())),
        "{line}"
    );
    std::fs::write(twin.join("project.toml"), "not toml [").unwrap();
    let (_, line) = c.call("projects", json!({}));
    assert!(
        line.ends_with(&format!("{}: can't read project.toml.", twin.display())),
        "{line}"
    );
}
