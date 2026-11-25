use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

fn run(bin: &str, args: &[&str], envs: &[(&str, &str)], stdin: &str) -> Output {
    let mut child = Command::new(bin)
        .arg("setup")
        .args(args)
        .envs(envs.iter().copied())
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

fn setup(config: &Path, args: &[&str], stdin: &str) -> Output {
    run(
        BIN,
        &[&["--config", config.to_str().unwrap()], args].concat(),
        &[],
        stdin,
    )
}

#[test]
fn writes_only_after_yes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude_desktop_config.json");
    assert!(setup(&config, &[], "n\n").status.success());
    assert!(!config.exists());
    // The test binary lives under target/, so the prompt warns.
    let out = setup(&config, &[], "y\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Warning"));
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.contains(BIN),
        "the entry points at the running binary: {text}"
    );
    assert!(text.contains("\"mcp\""));
}

#[test]
fn yes_is_refused_from_target_and_works_from_a_stable_path() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("c.json");
    let cfg = config.to_str().unwrap();
    let out = setup(&config, &["--yes"], "");
    assert!(!out.status.success() && !config.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("changes when you rebuild"));
    let copy = dir.path().join("mockingbird-studio");
    std::fs::copy(BIN, &copy).unwrap();
    assert!(
        run(copy.to_str().unwrap(), &["--config", cfg, "--yes"], &[], "")
            .status
            .success()
    );
    assert!(config.exists());
}

#[test]
fn home_override_is_made_absolute_and_empty_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("mockingbird-studio");
    std::fs::copy(BIN, &copy).unwrap();
    let bin = copy.to_str().unwrap();
    let config = dir.path().join("c.json");
    let cfg = config.to_str().unwrap();
    let args = ["--config", cfg, "--yes"];
    assert!(
        run(bin, &args, &[("MOCKINGBIRD_STUDIO_HOME", "")], "")
            .status
            .success()
    );
    assert!(
        !std::fs::read_to_string(&config)
            .unwrap()
            .contains("MOCKINGBIRD_STUDIO_HOME")
    );
    std::fs::remove_file(&config).unwrap();
    assert!(
        run(bin, &args, &[("MOCKINGBIRD_STUDIO_HOME", "rel/dir")], "")
            .status
            .success()
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    let home = v["mcpServers"]["mockingbird-studio"]["env"]["MOCKINGBIRD_STUDIO_HOME"]
        .as_str()
        .unwrap();
    assert!(
        Path::new(home).is_absolute() && home.ends_with("rel/dir"),
        "{home}"
    );
}

#[test]
fn no_home_and_no_config_is_an_error() {
    let out = Command::new(BIN)
        .arg("setup")
        .env_remove("HOME")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--config"));
}
