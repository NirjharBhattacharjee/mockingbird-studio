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
    // Logs its arguments. A bootout exits with the code in `bootout-code`,
    // and a bootstrap fails while `refuse` exists.
    let fake = root.join("launchctl");
    let calls = root.join("calls");
    let (code, refuse) = (root.join("bootout-code"), root.join("refuse"));
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$1\" in\n\
             bootout) [ -e '{c}' ] && {{ echo gone >&2; exit $(cat '{c}'); }} ;;\n\
             bootstrap) [ -e '{r}' ] && {{ echo nope >&2; exit 5; }} ;;\nesac\nexit 0\n",
            calls.display(),
            c = code.display(),
            r = refuse.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    // A shortlist command the schedule's PATH won't have.
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join("config.toml"),
        "[scan]\ncommand = [\"no-such-shortlister\", \"-p\"]\n",
    )
    .unwrap();
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

    // One stand-in without the other is refused, before anything runs.
    let out = Command::new(BIN)
        .args(["scan-time", "--time", "07:00"])
        .env("MOCKINGBIRD_STUDIO_HOME", &home)
        .env("MOCKINGBIRD_STUDIO_LAUNCH_AGENTS", &agents)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("or neither"),
        "{out:?}"
    );
    assert!(!plist.exists() && calls_text().is_empty());

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
    assert!(
        out.contains("no-such-shortlister isn't on the PATH the schedule will use"),
        "{out}"
    );
    assert!(out.contains(r#""set_by":"cli""#), "{out}");
    assert!(config().contains("time = \"07:05\""), "{}", config());
    assert!(config().contains("via = \"cli\""), "{}", config());
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

    // launchd refuses a new time five times: the old agent is gone, so the
    // schedule is off, and the reply says so.
    fs::remove_file(&calls).unwrap();
    fs::write(&refuse, "").unwrap();
    let out = run(&["scan-time", "--time", "08:00"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nope") && err.contains("The schedule is now off"),
        "{err}"
    );
    assert_eq!(calls_text().matches("bootstrap ").count(), 5);
    assert!(!plist.exists());
    assert!(!config().contains("time"), "{}", config());
    fs::remove_file(&refuse).unwrap();

    // Clearing unloads the agent, deletes its file and the time. Not loaded
    // (3) is fine; any other bootout failure is reported and keeps the file.
    ok(&["scan-time", "--time", "08:00"]);
    fs::write(&code, "5").unwrap();
    let out = run(&["scan-time", "--time", ""]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("gone"),
        "{out:?}"
    );
    assert!(plist.exists());
    fs::write(&code, "3").unwrap();
    let out = ok(&["scan-time", "--time", ""]);
    assert!(
        out.contains(r#""time":null"#) && out.contains(r#""installed":false"#),
        "{out}"
    );
    assert!(!plist.exists());
    assert!(!config().contains("time"), "{}", config());
    assert!(calls_text().lines().last().unwrap().starts_with("bootout "));
}
