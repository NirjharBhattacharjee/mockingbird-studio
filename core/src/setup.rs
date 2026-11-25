//! `mockingbird-studio setup`: add this binary to Claude Desktop's MCP config,
//! but only after the user says yes. The one place the app writes a file that
//! isn't the user's own project or the studio's home.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};
use serde_json::{Map, Value, json};

const NAME: &str = "mockingbird-studio";

fn default_config() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join("Library/Application Support/Claude/claude_desktop_config.json")
}

/// Runs `setup` with the arguments after the word `setup`.
pub fn run(args: &[String]) -> ExitCode {
    let cmd = Command::new("mockingbird-studio setup")
        .about("Add the MCP server to Claude Desktop's config, after you say yes.")
        .arg(
            Arg::new("yes")
                .long("yes")
                .short('y')
                .action(ArgAction::SetTrue)
                .help("Skip the question, for scripts."),
        )
        .arg(Arg::new("config").long("config").help(
            "Config file to edit. Default: Claude Desktop's, in ~/Library/Application Support/Claude.",
        ));
    let m = match cmd
        .try_get_matches_from(std::iter::once("setup").chain(args.iter().map(String::as_str)))
    {
        Ok(m) => m,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(e.exit_code() as u8);
        }
    };
    let config = m
        .get_one::<String>("config")
        .map_or_else(default_config, PathBuf::from);
    let result = std::env::current_exe()
        .map_err(|e| format!("can't find this program's path: {e}"))
        .and_then(|exe| {
            let mut entry = json!({ "command": exe, "args": ["mcp"] });
            // Projects somewhere else: the server must see the same folder as the CLI.
            if let Some(home) = std::env::var_os("MOCKINGBIRD_STUDIO_HOME") {
                entry["env"] = json!({ "MOCKINGBIRD_STUDIO_HOME": home });
            }
            apply(
                &config,
                entry,
                m.get_flag("yes"),
                &mut std::io::stdin().lock(),
                &mut std::io::stdout(),
            )
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Puts `entry` under `mcpServers` in `config`, keeping every other key.
/// Nothing is written unless `yes` or the user answers y.
pub fn apply(
    config: &Path,
    entry: Value,
    yes: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<(), String> {
    // Edit the real file when the path is a symlink.
    let config = config
        .canonicalize()
        .unwrap_or_else(|_| config.to_path_buf());
    let mut root = match std::fs::read_to_string(&config) {
        // A file we can't parse is the user's; never replace it.
        Ok(text) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| format!("{} isn't a JSON object; fix it first", config.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("can't read {}: {e}", config.display())),
    };
    let servers = root
        .as_object_mut()
        .expect("checked above")
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("mcpServers in the config isn't an object; fix it first")?;
    if servers.get(NAME) == Some(&entry) {
        return say(
            out,
            format!(
                "{} already has this entry. Nothing to do.",
                config.display()
            ),
        );
    }
    let verb = if servers.contains_key(NAME) {
        "replace the existing"
    } else {
        "add the"
    };
    say(
        out,
        format!(
            "This will {verb} \"{NAME}\" entry in {}:\n{}",
            config.display(),
            serde_json::to_string_pretty(&entry).expect("a Value always serializes")
        ),
    )?;
    if !yes {
        write!(out, "Write it? [y/N] ")
            .and_then(|_| out.flush())
            .map_err(|e| e.to_string())?;
        let mut answer = String::new();
        // EOF or a read error counts as no.
        let _ = input.read_line(&mut answer);
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            return say(out, "Nothing written.".into());
        }
    }
    servers.insert(NAME.into(), entry);
    write_atomic(&config, &root)?;
    say(
        out,
        "Done. Quit Claude Desktop (⌘Q) and open it again.".into(),
    )
}

fn say(out: &mut impl Write, line: String) -> Result<(), String> {
    writeln!(out, "{line}").map_err(|e| e.to_string())
}

/// Writes next to the target, then renames over it, so a crash leaves either
/// the old file or the new one.
fn write_atomic(path: &Path, value: &Value) -> Result<(), String> {
    let err = |e: std::io::Error| format!("can't write {}: {e}", path.display());
    let dir = path.parent().ok_or("the config path has no folder")?;
    std::fs::create_dir_all(dir).map_err(err)?;
    let tmp = dir.join(format!(".{NAME}-setup.tmp"));
    let mut text = serde_json::to_string_pretty(value).expect("a Value always serializes");
    text.push('\n');
    std::fs::write(&tmp, text).map_err(err)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Value {
        json!({ "command": "/x/mockingbird-studio", "args": ["mcp"] })
    }

    fn go(path: &Path, yes: bool, answer: &str) -> (Result<(), String>, String) {
        let mut out = Vec::new();
        let r = apply(path, entry(), yes, &mut answer.as_bytes(), &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn no_answer_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        for answer in ["", "n\n", "\n", "maybe\n"] {
            assert!(go(&path, false, answer).0.is_ok());
            assert!(!path.exists(), "answer {answer:?}");
        }
    }

    #[test]
    fn yes_keeps_other_entries_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/c.json");
        std::fs::create_dir(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"theme":"dark","mcpServers":{"other":{"command":"o"},"mockingbird-studio":{"command":"old"}}}"#,
        )
        .unwrap();
        assert!(go(&path, false, "y\n").0.is_ok());
        assert_eq!(
            read(&path),
            json!({"theme":"dark","mcpServers":{"other":{"command":"o"},"mockingbird-studio":entry()}})
        );
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn same_entry_is_left_alone_and_a_missing_file_is_made_with_yes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        assert!(go(&path, true, "").0.is_ok());
        assert_eq!(
            read(&path),
            json!({"mcpServers":{"mockingbird-studio":entry()}})
        );
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let (r, said) = go(&path, false, "");
        assert!(r.is_ok() && said.contains("already"), "{said}");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
    }

    #[test]
    fn a_broken_file_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        for bad in ["{not json", "[1]", r#"{"mcpServers":[]}"#] {
            std::fs::write(&path, bad).unwrap();
            assert!(go(&path, true, "").0.is_err(), "{bad}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
        }
    }
}
