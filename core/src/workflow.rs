//! Workflow templates: the `[[step]]` list of a `project.toml` without the
//! state. `explainer` and `short` are built in; your own are
//! `~/.mockingbird-studio/workflows/<name>.toml`. A project copies its
//! template when it's made, so editing a template doesn't change old ones.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ops::Args;
use crate::project::Result;
use crate::store;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Step {
    pub id: String,
    pub gate: bool,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    step: Vec<Step>,
}

/// (step, gate, after)
type Builtin = &'static [(&'static str, bool, &'static [&'static str])];

/// From the README's steps and the 24 November run. The gates and the order
/// are a first guess for the owner to correct.
const EXPLAINER: Builtin = &[
    ("research", true, &[]),
    ("script", true, &["research"]),
    ("voice", true, &["script"]),
    ("look", true, &["script"]),
    ("sound", true, &["voice"]),
    ("assets", false, &["script"]),
    ("previews", true, &["voice", "look", "assets"]),
    ("shorts", true, &["previews"]),
    ("film", true, &["previews", "sound"]),
    ("thumbnail", true, &["look"]),
    ("description", true, &["film"]),
    ("upload", true, &["film", "thumbnail", "description"]),
];

/// One short as its own project: the explainer without the film. The short
/// is the `shorts` step's output and posts through `upload`.
const SHORT: Builtin = &[
    ("research", true, &[]),
    ("script", true, &["research"]),
    ("voice", true, &["script"]),
    ("look", true, &["script"]),
    ("sound", true, &["voice"]),
    ("assets", false, &["script"]),
    ("previews", true, &["voice", "look", "assets"]),
    ("shorts", true, &["previews", "sound"]),
    ("thumbnail", true, &["look"]),
    ("description", true, &["shorts"]),
    ("upload", true, &["shorts", "thumbnail", "description"]),
];

const BUILTIN: [(&str, Builtin); 2] = [("explainer", EXPLAINER), ("short", SHORT)];

fn dir() -> PathBuf {
    store::home().join("workflows")
}

/// A name is a file name: letters, digits, `-` and `_`. Step ids are folder
/// names in the project, so they follow the same rule.
fn plain(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The steps of workflow `name`: a built-in one, else the file.
pub fn load(name: &str) -> Result<Vec<Step>> {
    if let Some((_, b)) = BUILTIN.iter().find(|(n, _)| *n == name) {
        return Ok(b
            .iter()
            .map(|(id, gate, after)| Step {
                id: id.to_string(),
                gate: *gate,
                after: after.iter().map(|s| s.to_string()).collect(),
            })
            .collect());
    }
    let unknown = || {
        format!(
            "unknown workflow {name:?}; the built-in ones are explainer and short, and your own go in {}",
            dir().display()
        )
    };
    if !plain(name) {
        return Err(unknown());
    }
    let path = dir().join(format!("{name}.toml"));
    if !path.is_file() {
        return Err(unknown());
    }
    let steps = store::read::<File>(&path)?.step;
    check(&steps).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(steps)
}

/// At least one step, plain unique ids, and each `after` names a step
/// listed before it, so there's no loop.
fn check(steps: &[Step]) -> Result<()> {
    if steps.is_empty() {
        return Err("a workflow needs at least one [[step]]".into());
    }
    let mut seen = HashSet::new();
    for s in steps {
        if !plain(&s.id) {
            return Err(format!(
                "step id {:?} should be letters, digits, - and _",
                s.id
            ));
        }
        if let Some(a) = s.after.iter().find(|a| !seen.contains(a.as_str())) {
            return Err(format!(
                "step {:?} comes after {a:?}, which isn't a step listed before it",
                s.id
            ));
        }
        if !seen.insert(s.id.as_str()) {
            return Err(format!("step {:?} is listed twice", s.id));
        }
    }
    Ok(())
}

/// The `workflows` operation: every template with its steps. A file that
/// doesn't load is listed with its error.
pub(crate) fn workflows(_: &Args) -> Result<Value> {
    let mut names: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    let mut own: Vec<String> = fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".toml")?.to_string();
            (!names.contains(&stem)).then_some(stem)
        })
        .collect();
    own.sort();
    names.extend(own);
    let list = names.iter().map(|n| match load(n) {
        Ok(steps) => json!({"name": n, "steps": steps}),
        Err(e) => json!({"name": n, "error": e}),
    });
    Ok(json!({"workflows": list.collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_is_one_video_that_posts() {
        let s = load("short").unwrap();
        let ids: Vec<_> = s.iter().map(|s| s.id.as_str()).collect();
        assert!(!ids.contains(&"film"));
        assert_eq!(ids.last(), Some(&"upload"));
        let upload = s.last().unwrap();
        assert!(upload.gate);
        assert_eq!(upload.after, ["shorts", "thumbnail", "description"]);
        check(&s).unwrap();
        check(&load("explainer").unwrap()).unwrap();
    }

    #[test]
    fn check_refuses_loops_bad_ids_and_repeats() {
        let st = |id: &str, after: &[&str]| Step {
            id: id.into(),
            gate: true,
            after: after.iter().map(|s| s.to_string()).collect(),
        };
        assert!(check(&[]).is_err());
        assert!(check(&[st("a", &["b"]), st("b", &["a"])]).is_err());
        assert!(check(&[st("../x", &[])]).is_err());
        assert!(check(&[st("a", &[]), st("a", &[])]).is_err());
        assert!(check(&[st("a", &["a"])]).is_err());
        check(&[st("a", &[]), st("b", &["a"])]).unwrap();
    }

    #[test]
    fn plain_names_only() {
        assert!(plain("my-flow_2"));
        assert!(!plain("../etc"));
        assert!(!plain("a/b"));
        assert!(!plain(""));
    }
}
