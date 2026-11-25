use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("MOCKINGBIRD_STUDIO_HOME", home)
        .output()
        .unwrap()
}

/// Runs `args --json`, expects success and returns the one line of JSON. The
/// tests match substrings: `app` has no JSON parser and M1 adds no dependency.
fn ok(home: &Path, args: &[&str]) -> String {
    let out = run(home, &[args, &["--json"]].concat());
    assert!(out.status.success(), "{args:?}: {out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.trim_end().lines().count(), 1, "--json is one line");
    text
}

#[test]
fn the_notes_loop_runs_from_the_command_line() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    let dir = root.join("video");
    let (d, p) = (dir.to_str().unwrap(), dir.to_str().unwrap());

    ok(&home, &["project-new", "--dir", d, "--title", "Example"]);
    fs::write(dir.join("research/notes.md"), "facts").unwrap();
    let toml = || fs::read_to_string(dir.join("project.toml")).unwrap();

    ok(
        &home,
        &[
            "submit",
            "--project",
            p,
            "--step",
            "research",
            "--files",
            "research/notes.md",
        ],
    );
    assert!(toml().contains("review"));

    let note = ok(
        &home,
        &[
            "note-add",
            "--project",
            p,
            "--step",
            "research",
            "--text",
            "cite the source",
        ],
    );
    assert!(note.contains("cite the source"));
    ok(
        &home,
        &[
            "redo",
            "--project",
            p,
            "--step",
            "research",
            "--note",
            "add sources",
        ],
    );
    assert!(toml().contains("redo"));

    let notes = ok(&home, &["notes", "--project", p]);
    assert!(notes.contains("cite the source") && notes.contains("add sources"));
    ok(
        &home,
        &["note-done", "--project", p, "--id", "1", "--reply", "added"],
    );
    assert!(ok(&home, &["notes", "--project", p]).contains(r#""notes":[]"#));

    ok(
        &home,
        &[
            "submit",
            "--project",
            p,
            "--step",
            "research",
            "--files",
            "research/notes.md",
        ],
    );
    ok(&home, &["approve", "--project", p, "--step", "research"]);
    let toml = toml();
    assert!(
        toml.contains("approved") && toml.contains(r#"via = "cli""#),
        "{toml}"
    );
}

#[test]
fn errors_go_to_stderr_with_a_failure_status() {
    let home = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-errors");
    let out = run(&home, &["status", "--project", "/no/such/project"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no project"));
}
