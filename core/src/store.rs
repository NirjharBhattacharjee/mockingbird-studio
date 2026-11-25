//! Files on disk. Every write takes an advisory lock on `<file>.lock`, reads,
//! changes, writes a temp file and renames it over the original, so more
//! than one process can write the same file without losing a write.
//! Readers don't lock: the rename means they see the old file or the new.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::project::{Project, Result, SCHEMA};

pub const PROJECT_FILE: &str = "project.toml";

/// `~/.mockingbird-studio`, or `MOCKINGBIRD_STUDIO_HOME`.
pub fn home() -> PathBuf {
    match std::env::var_os("MOCKINGBIRD_STUDIO_HOME") {
        Some(h) if !h.is_empty() => PathBuf::from(h),
        _ => {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".mockingbird-studio")
        }
    }
}

/// `~/.mockingbird-studio/config.toml`. Keys this version doesn't know are
/// kept, so a write here never drops what someone typed in.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    #[serde(default)]
    pub projects: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub niche: Vec<crate::niche::Niche>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channel: Vec<crate::youtube::Channel>,
    #[serde(flatten)]
    pub rest: toml::Table,
}

pub fn config_path() -> PathBuf {
    home().join("config.toml")
}

pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{} is not valid: {e}", path.display()))
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{} is not valid: {e}", path.display()))
}

/// Writes `value` as JSON through a temp file and a rename, so a reader sees
/// the old file or the new one. Makes the folder if it's missing.
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let io = |e: std::io::Error| format!("can't write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, text).map_err(io)?;
    fs::rename(&tmp, path).map_err(io)
}

pub fn read_config() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let mut c: Config = read(&path)?;
        crate::niche::tidy(&mut c.niche);
        Ok(c)
    } else {
        Ok(Config::default())
    }
}

/// Runs `f` while holding the lock on `path`.
fn locked<R>(path: &Path, f: impl FnOnce() -> Result<R>) -> Result<R> {
    let mut lock_path = path.as_os_str().to_owned();
    lock_path.push(".lock");
    let lock = File::create(&lock_path)
        .map_err(|e| format!("can't open {}: {e}", Path::new(&lock_path).display()))?;
    lock.lock()
        .map_err(|e| format!("can't lock {}: {e}", path.display()))?;
    f()
    // The lock is released when `lock` is dropped.
}

pub(crate) fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text =
        toml::to_string(value).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let io = |e: std::io::Error| format!("can't write {}: {e}", path.display());
    let mut file = File::create(&tmp).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)?;
    file.sync_all().map_err(io)?;
    fs::rename(&tmp, path).map_err(io)?;
    // Sync the folder too, so the rename itself survives a crash.
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    File::open(parent.unwrap_or(Path::new(".")))
        .and_then(|d| d.sync_all())
        .map_err(io)
}

/// Writes a new project file. Refused if one is already there.
pub fn create_project(dir: &Path, project: &Project) -> Result<()> {
    let path = dir.join(PROJECT_FILE);
    locked(&path, || {
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        write(&path, project)
    })
}

/// Reads the project in `dir`, lets `f` change it, and writes it back, all
/// under the lock. Nothing is written if `f` fails.
pub fn update_project<R>(dir: &Path, f: impl FnOnce(&mut Project) -> Result<R>) -> Result<R> {
    let path = dir.join(PROJECT_FILE);
    locked(&path, || {
        let mut project: Project = read(&path)?;
        if project.schema != SCHEMA {
            return Err(format!(
                "{} has schema {}; this version only changes schema {SCHEMA}",
                path.display(),
                project.schema
            ));
        }
        let r = f(&mut project)?;
        write(&path, &project)?;
        Ok(r)
    })
}

pub fn update_config<R>(f: impl FnOnce(&mut Config) -> Result<R>) -> Result<R> {
    let path = config_path();
    fs::create_dir_all(home()).map_err(|e| format!("can't make {}: {e}", home().display()))?;
    locked(&path, || {
        let mut config = read_config()?;
        let r = f(&mut config)?;
        write(&path, &config)?;
        Ok(r)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_keeps_keys_it_does_not_know() {
        let text = "projects = [\"/a\"]\n\n[tool.voice]\ncommand = [\"tts\"]\n";
        let mut c: Config = toml::from_str(text).unwrap();
        c.projects.push("/b".into());
        let out = toml::to_string(&c).unwrap();
        let back: Config = toml::from_str(&out).unwrap();
        assert_eq!(
            back.projects,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert_eq!(
            back.rest["tool"]["voice"]["command"][0].as_str(),
            Some("tts")
        );
    }

    #[test]
    fn an_update_keeps_keys_and_rows_it_does_not_know() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::new("T", None, "explainer", jiff::Timestamp::now()).unwrap();
        create_project(dir.path(), &p).unwrap();
        let path = dir.path().join(PROJECT_FILE);
        let mut text = fs::read_to_string(&path).unwrap();
        text = text.replacen("next_note = 1\n", "next_note = 1\ncolour = \"red\"\n", 1);
        text = text.replacen("round = 0\n", "round = 0\nlocked = true\n", 1);
        text.push_str(
            "\n[[post]]\nstep = \"upload\"\nvideo = \"VIDEO_ID\"\nat = 2026-11-25T18:30:00Z\n",
        );
        fs::write(&path, text).unwrap();

        update_project(dir.path(), |p| {
            p.note_add("research", "n", None, None, jiff::Timestamp::now())?;
            Ok(())
        })
        .unwrap();

        let back: Project = read(&path).unwrap();
        assert_eq!(back.notes.len(), 1);
        assert_eq!(back.rest["colour"].as_str(), Some("red"));
        assert_eq!(back.steps[0].rest["locked"].as_bool(), Some(true));
        let post = &back.rest["post"][0];
        assert_eq!(post["video"].as_str(), Some("VIDEO_ID"));
        assert_eq!(
            post["at"].as_datetime().unwrap().to_string(),
            "2026-11-25T18:30:00Z"
        );
    }

    #[test]
    fn a_newer_schema_is_not_changed() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::new("T", None, "explainer", jiff::Timestamp::now()).unwrap();
        p.schema = 2;
        create_project(dir.path(), &p).unwrap();
        let err = update_project(dir.path(), |_| Ok(())).unwrap_err();
        assert!(err.contains("schema 2"), "{err}");
    }

    #[test]
    fn a_failed_change_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::new("T", None, "explainer", jiff::Timestamp::now()).unwrap();
        create_project(dir.path(), &p).unwrap();
        assert!(create_project(dir.path(), &p).is_err());
        let r: Result<()> = update_project(dir.path(), |p| {
            p.title = "changed".into();
            Err("no".into())
        });
        assert!(r.is_err());
        let back: Project = read(&dir.path().join(PROJECT_FILE)).unwrap();
        assert_eq!(back.title, "T");
    }
}
