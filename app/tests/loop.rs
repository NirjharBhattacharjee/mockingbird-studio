//! The PRD's five "done when" steps, headless: the MCP server is the agent,
//! the CLI is the person. Both see the same `MOCKINGBIRD_STUDIO_HOME`.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{BIN, Client};
use serde_json::{Value, json};
use studio_core::project::{Project, Verdict, Via};
use studio_core::store;

/// The person's side: one CLI call, expected to succeed, with its JSON.
fn person(home: &Path, args: &[&str]) -> Value {
    let out = Command::new(BIN)
        .args(args)
        .arg("--json")
        .env("MOCKINGBIRD_STUDIO_HOME", home)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

fn state(status: &Value, step: &str) -> String {
    let steps = status["steps"].as_array().unwrap();
    let s = steps.iter().find(|s| s["id"] == step).unwrap();
    s["state"].as_str().unwrap().to_string()
}

#[test]
fn the_notes_loop_end_to_end() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let dir = work.path().join("video");
    let p = dir.to_str().unwrap();
    let mut agent = Client::start(home.path());
    let project = json!({"project": p});
    let with = |extra: Value| {
        let mut v = project.clone();
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };

    // 1. The agent creates a project and submits script/script.md. The steps
    // before it are approved first, as the gate demands.
    agent.call("project_new", json!({"dir": p, "title": "Loop"}));
    fs::write(dir.join("research/topic.md"), "topic").unwrap();
    agent.call(
        "submit",
        with(json!({"step": "research", "files": ["research/topic.md"]})),
    );
    person(
        home.path(),
        &["approve", "--project", p, "--step", "research"],
    );
    fs::write(dir.join("script/script.md"), "Draft one.").unwrap();
    let script = json!({"step": "script", "files": ["script/script.md"]});
    agent.call("submit", with(script.clone()));

    // 2. The script is waiting on the person, who adds two notes and redoes.
    let (status, line) = agent.call("status", project.clone());
    assert_eq!(state(&status, "script"), "review");
    // "Waiting on you" means the agent's turn, and it has none.
    assert!(line.ends_with("Steps waiting on you: none."), "{line}");
    for text in ["cut the intro", "name the source"] {
        person(
            home.path(),
            &[
                "note-add",
                "--project",
                p,
                "--step",
                "script",
                "--text",
                text,
            ],
        );
    }
    person(
        home.path(),
        &[
            "redo",
            "--project",
            p,
            "--step",
            "script",
            "--note",
            "two notes",
        ],
    );

    // 3. The agent reads the notes, submits again and marks them done.
    let (notes, line) = agent.call("notes", project.clone());
    let texts: Vec<&str> = notes["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["cut the intro", "name the source"]);
    assert_eq!(notes["redo"][0]["note"], "two notes");
    assert!(line.contains("video/script (redo)"), "{line}");
    assert!(line.starts_with("Open notes: 2."), "{line}");
    fs::write(dir.join("script/script.md"), "Draft two.").unwrap();
    agent.call("submit", with(script));
    for n in notes["notes"].as_array().unwrap() {
        agent.call("note_done", with(json!({"id": n["id"], "reply": "done"})));
    }
    let (_, line) = agent.call("notes", project.clone());
    assert!(line.starts_with("Open notes: 0."), "{line}");

    // 4. The person approves. Both decisions are in project.toml with a time.
    person(
        home.path(),
        &["approve", "--project", p, "--step", "script"],
    );
    // `at` is a Timestamp, so reading the file proves each decision has one.
    let project: Project = store::read(&dir.join("project.toml")).unwrap();
    let script: Vec<_> = project
        .decisions
        .iter()
        .filter(|d| d.step == "script")
        .collect();
    let verdicts: Vec<Verdict> = script.iter().map(|d| d.decision).collect();
    assert_eq!(verdicts, [Verdict::Redo, Verdict::Approved]);
    assert!(script.iter().all(|d| d.via == Via::Cli));

    // 5. The agent submits a clip to previews. The person adds a note at 0:04
    // and the agent reads it with its time. The steps before previews are
    // approved or done first.
    for step in ["voice", "look"] {
        let f = format!("{step}/{step}.txt");
        fs::write(dir.join(&f), step).unwrap();
        agent.call("submit", with(json!({"step": step, "files": [f]})));
        person(home.path(), &["approve", "--project", p, "--step", step]);
    }
    fs::write(dir.join("assets/a.txt"), "a").unwrap();
    agent.call(
        "submit",
        with(json!({"step": "assets", "files": ["assets/a.txt"]})),
    );
    fs::write(dir.join("previews/clip.mp4"), "synthetic").unwrap();
    agent.call(
        "submit",
        with(json!({"step": "previews", "files": ["previews/clip.mp4"]})),
    );
    person(
        home.path(),
        &[
            "note-add",
            "--project",
            p,
            "--step",
            "previews",
            "--path",
            "previews/clip.mp4",
            "--at",
            "4",
            "--text",
            "cut here",
        ],
    );
    let (notes, _) = agent.call("notes", with(json!({"step": "previews"})));
    assert_eq!(notes["notes"][0]["text"], "cut here");
    assert_eq!(notes["notes"][0]["path"], "previews/clip.mp4");
    assert_eq!(notes["notes"][0]["at"], 4.0);
}
