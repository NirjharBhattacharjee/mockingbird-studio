//! Files on disk. Every write takes an advisory lock on `<file>.lock`, reads,
//! changes, writes a temp file and renames it over the original, so more
//! than one process can write the same file without losing a write.
//! Readers don't lock: the rename means they see the old file or the new.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::project::{Project, Result};

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

pub fn read_config() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        read(&path)
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

fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text =
        toml::to_string(value).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let io = |e: std::io::Error| format!("can't write {}: {e}", path.display());
    let mut file = File::create(&tmp).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)?;
    file.sync_all().map_err(io)?;
    fs::rename(&tmp, path).map_err(io)
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
