use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

#[test]
fn mcp_exits_with_empty_stdout() {
    let out = Command::new(BIN).arg("mcp").output().unwrap();
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "stdout is for protocol messages only"
    );
}

#[test]
fn other_arguments_go_to_the_cli() {
    let out = Command::new(BIN).arg("status").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--project"));
}
