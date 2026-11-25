//! Starts `mockingbird-studio mcp` and speaks JSON-RPC over its stdio.

mod common;

use std::process::Command;
use std::time::{Duration, Instant};

use common::{BIN, Client};
use serde_json::{Value, json};
use studio_core::ops::OPS;

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

#[test]
fn a_visibility_set_from_the_cli_is_read_through_mcp() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join("config.toml"),
        "projects = []\nnote = \"kept\"\n\n[tool.voice]\ncommand = [\"tts\"]\n",
    )
    .unwrap();
    let cli = |args: &[&str]| {
        let out = Command::new(BIN)
            .args(args)
            .env("MOCKINGBIRD_STUDIO_HOME", home.path())
            .output()
            .unwrap();
        (out.status.success(), String::from_utf8(out.stderr).unwrap())
    };
    assert!(
        cli(&[
            "niche-add",
            "--name",
            "AI news",
            "--feeds",
            "https://a.example/feed.xml"
        ])
        .0
    );
    assert!(
        cli(&[
            "niche-set",
            "--niche",
            "ai-news",
            "--visibility",
            "unlisted"
        ])
        .0
    );
    let (ok, err) = cli(&["niche-set", "--niche", "ai-news", "--visibility", "secret"]);
    assert!(!ok && err.contains("visibility should be one of"), "{err}");

    let mut c = Client::start(home.path());
    let (r, _) = c.call("niches", json!({}));
    assert_eq!(r["niches"][0]["visibility"], "unlisted");
    // A niche with no feeds still says so, for the window.
    assert!(cli(&["niche-add", "--name", "Empty"]).0);
    assert_eq!(
        c.call("niches", json!({})).0["niches"][1]["feeds"],
        json!([])
    );
    assert_eq!(r["niches"][0]["feeds"][0], "https://a.example/feed.xml");
    // An agent can change a feed too, and the CLI sees it.
    c.call(
        "feed_add",
        json!({"niche": "ai-news", "url": "https://b.example/rss"}),
    );
    c.call(
        "feed_remove",
        json!({"niche": "ai-news", "url": "https://a.example/feed.xml"}),
    );
    let text = std::fs::read_to_string(home.path().join("config.toml")).unwrap();
    assert!(
        text.contains("note = \"kept\"") && text.contains("[tool.voice]"),
        "{text}"
    );
    assert!(
        text.contains("https://b.example/rss") && !text.contains("a.example"),
        "{text}"
    );
    c.call("niche_remove", json!({"niche": "ai-news"}));
    c.call("niche_remove", json!({"niche": "Empty"}));
    assert_eq!(c.call("niches", json!({})).0["niches"], json!([]));
}
