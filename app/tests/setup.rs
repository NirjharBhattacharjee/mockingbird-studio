use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

fn setup(config: &std::path::Path, args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(BIN)
        .arg("setup")
        .arg("--config")
        .arg(config)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn writes_only_after_yes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude_desktop_config.json");
    assert!(setup(&config, &[], "n\n").status.success());
    assert!(!config.exists());
    assert!(setup(&config, &[], "y\n").status.success());
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.contains(BIN),
        "the entry points at the running binary: {text}"
    );
    assert!(text.contains("\"mcp\""));
}

#[test]
fn yes_flag_skips_the_question() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("c.json");
    assert!(setup(&config, &["--yes"], "").status.success());
    assert!(config.exists());
}
