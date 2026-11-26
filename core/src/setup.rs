//! `mockingbird-studio setup`: add this binary to Claude Desktop's MCP config,
//! but only after the user says yes. The one place the app writes a file that
//! isn't the user's own project or the studio's home.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};
use serde_json::{Map, Value, json};

const NAME: &str = "mockingbird-studio";

fn default_config() -> Result<PathBuf, String> {
    match std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        Some(home) => Ok(PathBuf::from(home)
            .join("Library/Application Support/Claude/claude_desktop_config.json")),
        None => Err("HOME isn't set; pass --config <file>".into()),
    }
}

/// A path that changes when the app is rebuilt or moved: a cargo build, or
/// a copy macOS runs from a random folder because it wasn't moved yet.
pub(crate) fn fragile(exe: &str) -> bool {
    exe.contains("/target/") || exe.contains("/AppTranslocation/")
}

/// The entry for this binary, and a warning when its path won't last.
fn this_entry() -> Result<(Value, Option<&'static str>), String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("can't find this program's path: {e}"))?;
    let exe = exe
        .to_str()
        .ok_or("can't find this program's path: it isn't valid UTF-8")?;
    let mut entry = json!({ "command": exe, "args": ["mcp"] });
    // Projects somewhere else: the server must see the same folder as the CLI.
    if let Some(home) = std::env::var_os("MOCKINGBIRD_STUDIO_HOME").filter(|h| !h.is_empty()) {
        let home = std::path::absolute(home)
            .map_err(|e| format!("can't resolve MOCKINGBIRD_STUDIO_HOME: {e}"))?;
        let home = home
            .to_str()
            .ok_or("MOCKINGBIRD_STUDIO_HOME isn't valid UTF-8")?;
        entry["env"] = json!({ "MOCKINGBIRD_STUDIO_HOME": home });
    }
    Ok((
        entry,
        fragile(exe).then_some(
            "this path changes when you rebuild or move the app; run setup from /Applications/mockingbird-studio.app instead",
        ),
    ))
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
    let result = (|| {
        let config = match m.get_one::<String>("config") {
            Some(c) => PathBuf::from(c),
            None => default_config()?,
        };
        let (entry, warn) = this_entry()?;
        apply(
            &config,
            entry,
            warn,
            m.get_flag("yes"),
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
        )
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the config and returns it with `entry` under `mcpServers`, and the
/// entry it replaces. A replaced entry's `env` is kept when `entry` has none.
fn plan(config: &Path, mut entry: Value) -> Result<(Value, Value, Option<Value>), String> {
    let mut root = match std::fs::read_to_string(config) {
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
    let old = servers.get(NAME).cloned();
    if let (Some(env), None) = (old.as_ref().and_then(|o| o.get("env")), entry.get("env")) {
        entry["env"] = env.clone();
    }
    servers.insert(NAME.into(), entry.clone());
    Ok((root, entry, old))
}

/// Puts `entry` under `mcpServers` in `config`, keeping every other key.
/// Nothing is written unless `yes` or the user answers y. `warn` is shown
/// with the question, and makes `yes` an error: nobody read it.
pub fn apply(
    config: &Path,
    entry: Value,
    warn: Option<&str>,
    yes: bool,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<(), String> {
    if let (Some(w), true) = (warn, yes) {
        return Err(format!("{w}. Run without --yes to write it anyway."));
    }
    // Edit the real file when the path is a symlink.
    let config = config
        .canonicalize()
        .unwrap_or_else(|_| config.to_path_buf());
    let pretty = |v: &Value| serde_json::to_string_pretty(v).expect("a Value always serializes");
    let (_, entry, old) = plan(&config, entry)?;
    if old.as_ref() == Some(&entry) {
        return say(
            out,
            format!(
                "{} already has this entry. Nothing to do.",
                config.display()
            ),
        );
    }
    let mut text = match &old {
        Some(old) => format!(
            "This will replace the \"{NAME}\" entry in {}.\nNow:\n{}\nNew:\n{}",
            config.display(),
            pretty(old),
            pretty(&entry)
        ),
        None => format!(
            "This will add the \"{NAME}\" entry in {}:\n{}",
            config.display(),
            pretty(&entry)
        ),
    };
    if let Some(w) = warn {
        text.push_str(&format!("\nWarning: {w}."));
    }
    say(out, text)?;
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
    // The file may have changed while the question was open.
    let (root, ..) = plan(&config, entry)?;
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
        let r = apply(path, entry(), None, yes, &mut answer.as_bytes(), &mut out);
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

    #[test]
    fn a_fragile_path_is_warned_about_and_refused_with_yes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        let run = |yes, answer: &str| {
            let mut out = Vec::new();
            let r = apply(
                &path,
                entry(),
                Some("it moves"),
                yes,
                &mut answer.as_bytes(),
                &mut out,
            );
            (r, String::from_utf8(out).unwrap())
        };
        let (r, _) = run(true, "");
        assert!(r.unwrap_err().contains("it moves"));
        assert!(!path.exists());
        let (r, said) = run(false, "y\n");
        assert!(r.is_ok() && said.contains("Warning: it moves"), "{said}");
        assert!(path.exists());
    }

    #[test]
    fn a_replaced_entry_keeps_its_env_and_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(
            &path,
            r#"{"mcpServers":{"mockingbird-studio":{"command":"/old","args":["mcp"],"env":{"A":"1"}}}}"#,
        )
        .unwrap();
        let (r, said) = go(&path, false, "y\n");
        assert!(
            r.is_ok() && said.contains("/old") && said.contains("/x/mockingbird-studio"),
            "{said}"
        );
        let e = &read(&path)["mcpServers"]["mockingbird-studio"];
        assert_eq!(e["command"], "/x/mockingbird-studio");
        assert_eq!(e["env"], json!({"A":"1"}));
    }

    /// Changes the file at the moment the user answers.
    struct Racer(std::path::PathBuf, &'static [u8]);
    impl std::io::Read for Racer {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            unreachable!("only read_line is used")
        }
    }
    impl BufRead for Racer {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            std::fs::write(&self.0, r#"{"added":"while asking"}"#).unwrap();
            Ok(std::mem::take(&mut self.1))
        }
        fn consume(&mut self, _: usize) {}
    }

    #[test]
    fn the_file_is_read_again_after_the_yes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, "{}").unwrap();
        let mut input = Racer(path.clone(), b"y\n");
        apply(&path, entry(), None, false, &mut input, &mut Vec::new()).unwrap();
        assert_eq!(
            read(&path),
            json!({"added":"while asking","mcpServers":{"mockingbird-studio":entry()}})
        );
    }
}
