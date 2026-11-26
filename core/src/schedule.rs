//! The schedule: a launchd agent in `~/Library/LaunchAgents/` that runs
//! `mockingbird-studio scan` at `[scan] time`. Written when you set a time,
//! removed when you clear it. launchd runs a missed time after the Mac wakes.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

use crate::ops::Args;
use crate::project::Result;
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

/// Runs launchctl. A debug build's tests swap in a fake one.
fn launchctl(args: &[&str]) -> Result<()> {
    let prog = test_var("MOCKINGBIRD_STUDIO_LAUNCHCTL").unwrap_or_else(|| "/bin/launchctl".into());
    let out = Command::new(&prog)
        .args(args)
        .output()
        .map_err(|e| format!("can't run {prog}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "launchctl {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    ))
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

fn shown(warning: Option<&str>) -> Result<Value> {
    let c = store::read_config()?;
    let time = c
        .rest
        .get("scan")
        .and_then(|s| s.get("time"))
        .and_then(toml::Value::as_str)
        .filter(|t| !t.is_empty());
    let mut out = json!({
        "time": time,
        "installed": plist_path().exists(),
        "agent": plist_path(),
        "log": log_path(),
    });
    if let Some(w) = warning {
        out["warning"] = w.into();
    }
    Ok(out)
}

/// Writes `[scan] time`, or takes it out when `time` is None.
fn save(time: Option<String>) -> Result<()> {
    store::update_config(|c| {
        let scan = c
            .rest
            .entry("scan")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or("[scan] in config.toml isn't a table; fix it first")?;
        match time {
            Some(t) => scan.insert("time".into(), t.into()),
            None => scan.remove("time"),
        };
        Ok(())
    })
}

/// Stops the loaded agent, if there is one. Not being loaded isn't an error.
fn unload() {
    let _ = launchctl(&["bootout", &format!("{}/{LABEL}", domain())]);
}

pub(crate) fn scan_time(a: &Args) -> Result<Value> {
    let Some(t) = a.opt("time") else {
        return shown(None);
    };
    let t = t.trim();
    if t.is_empty() {
        unload();
        match fs::remove_file(plist_path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("can't remove {}: {e}", plist_path().display()));
            }
            _ => {}
        }
        save(None)?;
        return shown(None);
    }
    let (hour, minute) = parse_time(t)?;
    let exe =
        std::env::current_exe().map_err(|e| format!("can't find this program's path: {e}"))?;
    let exe = exe
        .to_str()
        .ok_or("can't find this program's path: it isn't valid UTF-8")?;
    let home = std::path::absolute(store::home())
        .map_err(|e| format!("can't resolve the studio's home: {e}"))?;
    let mut env = vec![("MOCKINGBIRD_STUDIO_HOME", home.display().to_string())];
    if let Ok(path) = std::env::var("PATH") {
        env.push(("PATH", path));
    }
    let log = home.join("scan.log");
    fs::create_dir_all(&home).map_err(|e| format!("can't make {}: {e}", home.display()))?;
    let path = plist_path();
    fs::create_dir_all(agents_dir())
        .map_err(|e| format!("can't make {}: {e}", agents_dir().display()))?;
    unload();
    fs::write(
        &path,
        plist(exe, hour, minute, &env, &log.display().to_string()),
    )
    .map_err(|e| format!("can't write {}: {e}", path.display()))?;
    let path_str = path.display().to_string();
    if let Err(e) = launchctl(&["bootstrap", &domain(), &path_str]) {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    save(Some(format!("{hour:02}:{minute:02}")))?;
    shown(crate::setup::fragile(exe).then_some(
        "this path changes when you rebuild or move the app, and the schedule would stop; set the time from /Applications/mockingbird-studio.app instead",
    ))
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
