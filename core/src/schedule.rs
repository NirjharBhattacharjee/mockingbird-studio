//! The schedule: a launchd agent in `~/Library/LaunchAgents/` that runs
//! `mockingbird-studio scan` at `[scan] time`. Written when you set a time,
//! removed when you clear it. launchd runs a missed time after the Mac wakes.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use jiff::Timestamp;
use serde_json::{Value, json};

use crate::ops::Args;
use crate::project::{Result, Via};
use crate::scan;
use crate::store;
use crate::youtube::test_var;

const LABEL: &str = "dev.mockingbird.studio.scan";

/// `~/Library/LaunchAgents`. A debug build's tests point it at a temp folder.
fn agents_dir() -> PathBuf {
    test_var("MOCKINGBIRD_STUDIO_LAUNCH_AGENTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/LaunchAgents")
        })
}

fn plist_path() -> PathBuf {
    agents_dir().join(format!("{LABEL}.plist"))
}

fn log_path() -> PathBuf {
    store::home().join("scan.log")
}

/// This user's GUI domain, where a LaunchAgent lives.
fn domain() -> String {
    // SAFETY: getuid takes nothing and can't fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

/// Runs launchctl. A debug build's tests swap in a fake one. An error carries
/// launchctl's exit code, when it ran.
fn launchctl(args: &[&str]) -> std::result::Result<(), (Option<i32>, String)> {
    let prog = test_var("MOCKINGBIRD_STUDIO_LAUNCHCTL").unwrap_or_else(|| "/bin/launchctl".into());
    let out = Command::new(&prog)
        .args(args)
        .output()
        .map_err(|e| (None, format!("can't run {prog}: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    Err((
        out.status.code(),
        format!(
            "launchctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    ))
}

/// The tests' two stand-ins go together, so a test can't reach the real
/// launchd with a temp agents folder, or the real folder with a fake launchd.
fn stand_ins_paired() -> Result<()> {
    let dir = test_var("MOCKINGBIRD_STUDIO_LAUNCH_AGENTS").is_some();
    if dir != test_var("MOCKINGBIRD_STUDIO_LAUNCHCTL").is_some() {
        return Err("set both MOCKINGBIRD_STUDIO_LAUNCH_AGENTS and MOCKINGBIRD_STUDIO_LAUNCHCTL, or neither".into());
    }
    Ok(())
}

/// Where `cmd` would be found on `path`: itself when absolute.
fn on_path(cmd: &str, path: &str) -> Option<PathBuf> {
    if cmd.starts_with('/') {
        return Some(PathBuf::from(cmd)).filter(|p| p.is_file());
    }
    std::env::split_paths(path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
}

/// "7:30" or "07:30" is (7, 30). Seconds and 12-hour times are refused, and
/// minutes take two digits: "7:3" is a typo, not 07:03.
fn parse_time(t: &str) -> Result<(u8, u8)> {
    let bad = || format!("the scan time should be HH:MM in 24-hour time, such as 07:30, not {t:?}");
    let num = |s: &str, len: std::ops::RangeInclusive<usize>, max: u8| {
        Some(s)
            .filter(|s| len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse().ok())
            .filter(|n| *n <= max)
    };
    let (h, m) = t.trim().split_once(':').ok_or_else(bad)?;
    num(h, 1..=2, 23).zip(num(m, 2..=2, 59)).ok_or_else(bad)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The agent: run `exe scan` at hour:minute, with the studio's home and the
/// caller's PATH (so a shortlist command such as `claude` is found), and
/// both outputs appended to the log.
fn plist(exe: &str, hour: u8, minute: u8, env: &[(&str, String)], log: &str) -> String {
    let env: String = env
        .iter()
        .map(|(k, v)| format!("\t\t<key>{k}</key>\n\t\t<string>{}</string>\n", esc(v)))
        .collect();
    let (exe, log) = (esc(exe), esc(log));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
		<string>scan</string>
	</array>
	<key>StartCalendarInterval</key>
	<dict>
		<key>Hour</key>
		<integer>{hour}</integer>
		<key>Minute</key>
		<integer>{minute}</integer>
	</dict>
	<key>EnvironmentVariables</key>
	<dict>
{env}	</dict>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#
    )
}

fn shown(warnings: &[String]) -> Result<Value> {
    let c = store::read_config()?;
    let scan = c.rest.get("scan");
    let time = scan
        .and_then(|s| s.get("time"))
        .and_then(toml::Value::as_str)
        .filter(|t| !t.is_empty());
    let set_by = scan
        .and_then(|s| s.get("time_set"))
        .and_then(|t| t.get("via"))
        .and_then(toml::Value::as_str)
        .filter(|_| time.is_some());
    let mut out = json!({
        "time": time,
        "set_by": set_by,
        "installed": plist_path().exists(),
        "agent": plist_path(),
        "log": log_path(),
    });
    if !warnings.is_empty() {
        out["warning"] = warnings.join(". ").into();
    }
    Ok(out)
}

/// Writes `[scan] time` and who set it, or takes both out when `time` is None.
fn save(time: Option<(String, Via)>) -> Result<()> {
    store::update_config(|c| {
        let scan = c
            .rest
            .entry("scan")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or("[scan] in config.toml isn't a table; fix it first")?;
        match time {
            Some((t, via)) => {
                scan.insert("time".into(), t.into());
                let mut set = toml::Table::new();
                set.insert(
                    "via".into(),
                    toml::Value::try_from(via).expect("a Via is a string"),
                );
                set.insert("at".into(), Timestamp::now().to_string().into());
                scan.insert("time_set".into(), set.into());
            }
            None => {
                scan.remove("time");
                scan.remove("time_set");
            }
        }
        Ok(())
    })
}

/// Stops the loaded agent, if there is one. Not being loaded (3, or 113 on
/// older macOS) isn't an error.
fn unload() -> Result<()> {
    match launchctl(&["bootout", &format!("{}/{LABEL}", domain())]) {
        Err((Some(3 | 113), _)) | Ok(()) => Ok(()),
        Err((_, e)) => Err(e),
    }
}

/// Loads the agent. launchd can still be tearing down the one just booted
/// out, so a refusal is retried a few times.
fn load(path: &str) -> Result<()> {
    let mut tries = 0;
    loop {
        match launchctl(&["bootstrap", &domain(), path]) {
            Ok(()) => return Ok(()),
            Err((_, e)) if tries == 4 => return Err(e),
            Err(_) => tries += 1,
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(crate) fn scan_time(a: &Args, via: Via) -> Result<Value> {
    stand_ins_paired()?;
    let Some(t) = a.opt("time") else {
        return shown(&[]);
    };
    let t = t.trim();
    if t.is_empty() {
        unload()?;
        match fs::remove_file(plist_path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("can't remove {}: {e}", plist_path().display()));
            }
            _ => {}
        }
        save(None)?;
        return shown(&[]);
    }
    let (hour, minute) = parse_time(t)?;
    let exe =
        std::env::current_exe().map_err(|e| format!("can't find this program's path: {e}"))?;
    let exe = exe
        .to_str()
        .ok_or("can't find this program's path: it isn't valid UTF-8")?;
    if exe.contains("/AppTranslocation/") {
        return Err(
            "macOS is running the app from a temporary copy; move the app to /Applications first"
                .into(),
        );
    }
    let mut warnings = vec![];
    if crate::setup::fragile(exe) {
        warnings.push("this path changes when you rebuild or move the app, and the schedule would stop; set the time from /Applications/mockingbird-studio.app instead".to_string());
    }
    let home = std::path::absolute(store::home())
        .map_err(|e| format!("can't resolve the studio's home: {e}"))?;
    let path_var = std::env::var("PATH").unwrap_or_default();
    if let Some(cmd) = scan::command(&store::read_config()?)?.first()
        && on_path(cmd, &path_var).is_none()
    {
        warnings.push(format!(
            "{cmd} isn't on the PATH the schedule will use; put its full path in [scan] command"
        ));
    }
    let mut env = vec![("MOCKINGBIRD_STUDIO_HOME", home.display().to_string())];
    if !path_var.is_empty() {
        env.push(("PATH", path_var));
    }
    // ponytail: launchd appends to scan.log and nothing trims it, so it grows
    // forever (a few KB a day). Rotate it in `scan` if that ever matters.
    let log = home.join("scan.log");
    fs::create_dir_all(&home).map_err(|e| format!("can't make {}: {e}", home.display()))?;
    let path = plist_path();
    fs::create_dir_all(agents_dir())
        .map_err(|e| format!("can't make {}: {e}", agents_dir().display()))?;
    unload()?;
    let off = |e: String| -> Result<Value> {
        let _ = fs::remove_file(&path);
        save(None)?;
        Err(format!("{e}. The schedule is now off"))
    };
    if let Err(e) = fs::write(
        &path,
        plist(exe, hour, minute, &env, &log.display().to_string()),
    ) {
        return off(format!("can't write {}: {e}", path.display()));
    }
    if let Err(e) = load(&path.display().to_string()) {
        return off(e);
    }
    save(Some((format!("{hour:02}:{minute:02}"), via)))?;
    shown(&warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(parse_time("07:30").unwrap(), (7, 30));
        assert_eq!(parse_time(" 7:05 ").unwrap(), (7, 5));
        assert_eq!(parse_time("23:59").unwrap(), (23, 59));
        assert_eq!(parse_time("00:00").unwrap(), (0, 0));
        for bad in [
            "24:00", "7:60", "7:3", "730", "+7:30", "07:30:00", "7pm", ":30", "007:30",
        ] {
            assert!(parse_time(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn commands_are_found_on_the_given_path() {
        assert_eq!(
            on_path("sh", "/usr/bin:/bin"),
            Some(PathBuf::from("/bin/sh"))
        );
        assert_eq!(on_path("/bin/sh", ""), Some(PathBuf::from("/bin/sh")));
        assert_eq!(on_path("sh", "/nowhere:relative"), None);
        assert_eq!(on_path("/no/such/thing", "/bin"), None);
    }

    #[test]
    fn the_plist_runs_scan_at_the_time_and_escapes_paths() {
        let p = plist(
            "/Applications/a&b.app/x",
            7,
            5,
            &[("PATH", "/usr/bin".into())],
            "/h/scan.log",
        );
        assert!(
            p.contains("<string>/Applications/a&amp;b.app/x</string>\n\t\t<string>scan</string>")
        );
        assert!(p.contains("<key>Hour</key>\n\t\t<integer>7</integer>"));
        assert!(p.contains("<key>Minute</key>\n\t\t<integer>5</integer>"));
        assert!(p.contains("<key>PATH</key>\n\t\t<string>/usr/bin</string>"));
        assert!(p.contains("<key>StandardErrorPath</key>\n\t<string>/h/scan.log</string>"));
        assert!(!p.contains("RunAtLoad"));
    }
}
