//! `scan-time` against a temp LaunchAgents folder and a fake launchctl, so
//! the test never touches the real ~/Library/LaunchAgents or launchd.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

#[test]
fn setting_and_clearing_the_time_writes_and_removes_the_agent() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sched-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (home, agents) = (root.join("home"), root.join("LaunchAgents"));
    fs::create_dir_all(&root).unwrap();
    // Logs its arguments, and fails a bootstrap while `refuse` exists.
    let fake = root.join("launchctl");
    let calls = root.join("calls");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$1\" != bootstrap ] || [ ! -e '{}' ] || {{ echo nope >&2; exit 5; }}\n",
            calls.display(),
            root.join("refuse").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let run = |args: &[&str]| -> Output {
        Command::new(BIN)
            .args(args)
            .arg("--json")
            .env("MOCKINGBIRD_STUDIO_HOME", &home)
            .env("MOCKINGBIRD_STUDIO_LAUNCH_AGENTS", &agents)
            .env("MOCKINGBIRD_STUDIO_LAUNCHCTL", &fake)
            .output()
            .unwrap()
    };
    let ok = |args: &[&str]| {
        let out = run(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    let plist = agents.join("dev.mockingbird.studio.scan.plist");
    let config = || fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let calls_text = || fs::read_to_string(&calls).unwrap_or_default();

    assert!(ok(&["scan-time"]).contains(r#""installed":false"#));
    assert!(ok(&["scan-time"]).contains(r#""time":null"#));

    // A bad time writes nothing and calls nothing.
    assert!(!run(&["scan-time", "--time", "25:00"]).status.success());
    assert!(!plist.exists() && calls_text().is_empty());

    let out = ok(&["scan-time", "--time", "7:05"]);
    assert!(
        out.contains(r#""time":"07:05""#) && out.contains(r#""installed":true"#),
        "{out}"
    );
    // A cargo build's path won't last; the reply says so.
    assert!(
        out.contains("/Applications/mockingbird-studio.app"),
        "{out}"
    );
    assert!(config().contains("time = \"07:05\""), "{}", config());
    let p = fs::read_to_string(&plist).unwrap();
    assert!(
        p.contains(&format!(
            "<string>{BIN}</string>\n\t\t<string>scan</string>"
        )),
        "{p}"
    );
    assert!(
        p.contains("<key>Hour</key>\n\t\t<integer>7</integer>"),
        "{p}"
    );
    assert!(
        p.contains("<key>Minute</key>\n\t\t<integer>5</integer>"),
        "{p}"
    );
    assert!(
        p.contains(&format!("<string>{}</string>", home.display())),
        "{p}"
    );
    assert!(
        p.contains(&format!("<string>{}/scan.log</string>", home.display())),
        "{p}"
    );
    let c = calls_text();
    let lines: Vec<_> = c.lines().collect();
    assert!(
        lines[0].starts_with("bootout gui/") && lines[0].ends_with("/dev.mockingbird.studio.scan"),
        "{c}"
    );
    assert!(
        lines[1].starts_with("bootstrap gui/") && lines[1].ends_with(&plist.display().to_string()),
        "{c}"
    );

    // launchd refuses a new time: no agent file is left, the old time stays.
    fs::write(root.join("refuse"), "").unwrap();
    let out = run(&["scan-time", "--time", "08:00"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nope"),
        "{out:?}"
    );
    assert!(!plist.exists());
    assert!(config().contains("time = \"07:05\""));
    fs::remove_file(root.join("refuse")).unwrap();

    // Clearing unloads the agent, deletes its file and the time.
    fs::remove_file(&calls).unwrap();
    ok(&["scan-time", "--time", "08:00"]);
    let out = ok(&["scan-time", "--time", ""]);
    assert!(
        out.contains(r#""time":null"#) && out.contains(r#""installed":false"#),
        "{out}"
    );
    assert!(!plist.exists());
    assert!(!config().contains("time ="), "{}", config());
    assert!(calls_text().lines().last().unwrap().starts_with("bootout "));
}
