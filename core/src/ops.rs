//! The one table of operations. Each takes JSON arguments and returns JSON.
//! The CLI, the MCP server and the window are thin layers over `dispatch`,
//! and each passes its own `via`: no operation takes it as an argument.

use std::fs;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::project::{Kind, NoteState, Output, Project, Result, State, Via};
use crate::store::{self, PROJECT_FILE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    Text,
    Number,
    Integer,
    TextList,
}

pub struct Arg {
    pub name: &'static str,
    pub kind: ArgKind,
    pub required: bool,
    pub help: &'static str,
}

pub struct Op {
    pub name: &'static str,
    pub help: &'static str,
    pub args: &'static [Arg],
    run: fn(&Args, Via) -> Result<Value>,
}

const fn req(name: &'static str, kind: ArgKind, help: &'static str) -> Arg {
    Arg {
        name,
        kind,
        required: true,
        help,
    }
}

const fn opt(name: &'static str, kind: ArgKind, help: &'static str) -> Arg {
    Arg {
        name,
        kind,
        required: false,
        help,
    }
}

use ArgKind::*;

const PROJECT: Arg = req(
    "project",
    Text,
    "The project's folder, or its folder name when only one listed project has it.",
);
const STEP: Arg = req("step", Text, "The step's id, such as script or previews.");

pub const OPS: &[Op] = &[
    Op {
        name: "project_new",
        help: "Make a project folder with its step folders and project.toml, and add it to the list.",
        args: &[
            req(
                "dir",
                Text,
                "An absolute path to the folder to make the project in.",
            ),
            req("title", Text, "The video's working title."),
            opt("niche", Text, "The niche this video belongs to."),
            opt(
                "workflow",
                Text,
                "The workflow to copy in. Default: explainer.",
            ),
        ],
        run: project_new,
    },
    Op {
        name: "project_add",
        help: "Add an existing project folder to the list.",
        args: &[req("dir", Text, "The project folder.")],
        run: project_add,
    },
    Op {
        name: "projects",
        help: "Every listed project with each step's state.",
        args: &[],
        run: projects,
    },
    Op {
        name: "status",
        help: "A project's steps, states, outputs, the last decision on each step, and open notes.",
        args: &[PROJECT],
        run: status,
    },
    Op {
        name: "submit",
        help: "Record a step's files for a new round. Refused until every step it comes after is approved or done.",
        args: &[
            PROJECT,
            STEP,
            req(
                "files",
                TextList,
                "The files, relative to the project folder.",
            ),
            opt(
                "summary",
                Text,
                "What this round is, or what changed since the last.",
            ),
        ],
        run: submit,
    },
    Op {
        name: "notes",
        help: "Open notes, and the note on every step waiting for a redo.",
        args: &[PROJECT, opt("step", Text, "Only this step's notes.")],
        run: notes,
    },
    Op {
        name: "note_add",
        help: "Add a note on a step, a file, or a moment in a file.",
        args: &[
            PROJECT,
            STEP,
            req("text", Text, "The note."),
            opt(
                "path",
                Text,
                "The file it's about, relative to the project folder.",
            ),
            opt("at", Number, "Seconds into the file."),
        ],
        run: note_add,
    },
    Op {
        name: "note_done",
        help: "Mark a note done.",
        args: &[
            PROJECT,
            req("id", Integer, "The note's id."),
            opt("reply", Text, "What was done about it."),
        ],
        run: note_done,
    },
    Op {
        name: "approve",
        help: "Approve a step in review. Written to project.toml with the time.",
        args: &[PROJECT, STEP, opt("note", Text, "The words of the yes.")],
        run: approve,
    },
    Op {
        name: "redo",
        help: "Send a step back with a note saying what to change.",
        args: &[PROJECT, STEP, req("note", Text, "What to change.")],
        run: redo,
    },
];

/// Runs the operation `name` with JSON `args`. `via` says which layer called.
pub fn dispatch(name: &str, args: &Value, via: Via) -> Result<Value> {
    let op = OPS
        .iter()
        .find(|o| o.name == name)
        .ok_or_else(|| format!("no operation {name:?}"))?;
    let empty = Map::new();
    let map = match args {
        Value::Object(m) => m,
        Value::Null => &empty,
        _ => return Err(format!("{name} takes an object of arguments")),
    };
    for (key, value) in map {
        let arg = op
            .args
            .iter()
            .find(|a| a.name == key)
            .ok_or_else(|| format!("{name} takes no argument {key:?}"))?;
        let ok = match arg.kind {
            _ if value.is_null() => true,
            Text => value.is_string(),
            Number => value.is_number(),
            Integer => value.is_u64(),
            TextList => value
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string)),
        };
        if !ok {
            return Err(format!("{name}: {key} should be {:?}", arg.kind));
        }
    }
    if let Some(a) = op.args.iter().find(|a| {
        let v = map.get(a.name);
        a.required
            && (v.is_none_or(Value::is_null)
                || v.and_then(Value::as_str)
                    .is_some_and(|s| s.trim().is_empty()))
    }) {
        return Err(format!("{name} needs {}", a.name));
    }
    (op.run)(&Args(map), via)
}

/// Arguments already checked against the table by `dispatch`.
struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn opt(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(Value::as_str).map(str::to_string)
    }
    fn text(&self, key: &str) -> String {
        self.opt(key).unwrap_or_default()
    }
    fn number(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(Value::as_f64)
    }
    fn list(&self, key: &str) -> Vec<String> {
        let items = self.0.get(key).and_then(Value::as_array);
        items
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    }
    fn project(&self) -> Result<PathBuf> {
        project_dir(&self.text("project"))
    }
}

/// A folder path, or a folder name when exactly one listed project has it.
/// A name without a `/` is looked up in the list first.
fn project_dir(arg: &str) -> Result<PathBuf> {
    if !arg.contains('/') {
        let config = store::read_config()?;
        let hits: Vec<&PathBuf> = config
            .projects
            .iter()
            .filter(|d| d.file_name().is_some_and(|n| n == arg))
            .collect();
        match hits.as_slice() {
            [one] => return Ok(one.to_path_buf()),
            [] => {}
            many => {
                return Err(format!(
                    "{} projects are named {arg:?}; pass the path: {}",
                    many.len(),
                    many.iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    let path = Path::new(arg);
    if path.join(PROJECT_FILE).is_file() {
        return canonical(path);
    }
    Err(format!("no project at or named {arg:?}"))
}

fn canonical(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// `rel` resolved inside the project folder `dir`, as a path relative to it.
/// Refused if it doesn't exist or leaves the folder through `..`, an
/// absolute path or a symlink.
pub fn inside(dir: &Path, rel: &str) -> Result<String> {
    let full = canonical(&dir.join(rel))?;
    let path = full
        .strip_prefix(canonical(dir)?)
        .map_err(|_| format!("{rel} is outside the project folder"))?;
    if !full.is_file() {
        return Err(format!("{rel} is not a file"));
    }
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{rel} is not a UTF-8 path"))
}

fn add_to_list(dir: &Path) -> Result<()> {
    store::update_config(|c| {
        if !c.projects.iter().any(|p| p == dir) {
            c.projects.push(dir.to_path_buf());
        }
        Ok(())
    })
}

fn project_new(a: &Args, _: Via) -> Result<Value> {
    let dir = PathBuf::from(a.text("dir"));
    if dir.join(PROJECT_FILE).exists() {
        return Err(format!("{} is already a project", dir.display()));
    }
    let workflow = a.opt("workflow").unwrap_or_else(|| "explainer".into());
    let project = Project::new(
        &a.text("title"),
        a.opt("niche"),
        &workflow,
        Timestamp::now(),
    )?;
    let io = |e: std::io::Error| format!("can't make {}: {e}", dir.display());
    fs::create_dir_all(&dir).map_err(io)?;
    let dir = canonical(&dir)?;
    for step in &project.steps {
        fs::create_dir_all(dir.join(&step.id)).map_err(io)?;
    }
    store::create_project(&dir, &project)?;
    add_to_list(&dir)?;
    status_of(&dir)
}

fn project_add(a: &Args, _: Via) -> Result<Value> {
    let dir = canonical(Path::new(&a.text("dir")))?;
    store::read::<Project>(&dir.join(PROJECT_FILE))?;
    add_to_list(&dir)?;
    status_of(&dir)
}

fn projects(_: &Args, _: Via) -> Result<Value> {
    let list = store::read_config()?.projects.into_iter().map(|dir| {
        match store::read::<Project>(&dir.join(PROJECT_FILE)) {
            Ok(p) => json!({
                "project": dir,
                "title": p.title,
                "steps": p.steps.iter().map(|s| json!({"id": s.id, "state": s.state})).collect::<Vec<_>>(),
            }),
            Err(e) => json!({"project": dir, "error": e}),
        }
    });
    Ok(json!({"projects": list.collect::<Vec<_>>()}))
}

fn status(a: &Args, _: Via) -> Result<Value> {
    status_of(&a.project()?)
}

fn status_of(dir: &Path) -> Result<Value> {
    let p: Project = store::read(&dir.join(PROJECT_FILE))?;
    let steps: Vec<Value> = p
        .steps
        .iter()
        .map(|s| {
            let mut v = json!(s);
            v["last_decision"] = json!(p.last_decision(&s.id).filter(|d| d.round == s.round));
            v
        })
        .collect();
    let open: Vec<_> = p
        .notes
        .iter()
        .filter(|n| n.state == NoteState::Open)
        .collect();
    Ok(json!({
        "project": dir,
        "title": p.title,
        "niche": p.niche,
        "workflow": p.workflow,
        "created": p.created,
        "steps": steps,
        "open_notes": open,
    }))
}

fn submit(a: &Args, via: Via) -> Result<Value> {
    let dir = a.project()?;
    let files = a.list("files");
    if files.is_empty() {
        return Err("submit needs at least one file".into());
    }
    let outputs = files
        .iter()
        .map(|f| {
            inside(&dir, f).map(|path| Output {
                kind: Kind::of(&path),
                path,
                rest: Default::default(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let step = a.text("step");
    let paths: Vec<String> = outputs.iter().map(|o| o.path.clone()).collect();
    store::update_project(&dir, |p| {
        p.submit(&step, outputs, a.opt("summary"), via, Timestamp::now())?;
        // After the gate check, so a gate refusal comes first. An Err here
        // writes nothing. A match so the next step's checks are one more arm.
        #[allow(clippy::single_match)]
        match step.as_str() {
            "thumbnail" => crate::thumbnail::check(&dir, &paths)?,
            _ => {}
        }
        Ok(json!(p.step(&step)?))
    })
}

fn notes(a: &Args, _: Via) -> Result<Value> {
    let p: Project = store::read(&a.project()?.join(PROJECT_FILE))?;
    let only = a.opt("step");
    if let Some(s) = &only {
        p.step(s)?;
    }
    let wanted = |step: &str| only.as_deref().is_none_or(|s| s == step);
    let open: Vec<_> = p
        .notes
        .iter()
        .filter(|n| n.state == NoteState::Open && wanted(&n.step))
        .collect();
    let redo: Vec<_> = p
        .steps
        .iter()
        .filter(|s| s.state == State::Redo && wanted(&s.id))
        .map(|s| {
            let note = p
                .last_decision(&s.id)
                .filter(|d| d.round == s.round)
                .and_then(|d| d.note.clone());
            json!({"step": s.id, "round": s.round, "note": note})
        })
        .collect();
    Ok(json!({"notes": open, "redo": redo}))
}

fn note_add(a: &Args, _: Via) -> Result<Value> {
    let dir = a.project()?;
    let path = a.opt("path").map(|p| inside(&dir, &p)).transpose()?;
    store::update_project(&dir, |p| {
        let note = p.note_add(
            &a.text("step"),
            &a.text("text"),
            path,
            a.number("at"),
            Timestamp::now(),
        )?;
        Ok(json!(note))
    })
}

fn note_done(a: &Args, _: Via) -> Result<Value> {
    let id = a.0.get("id").and_then(Value::as_u64).unwrap_or_default();
    store::update_project(&a.project()?, |p| {
        Ok(json!(p.note_done(id, a.opt("reply"))?))
    })
}

fn approve(a: &Args, via: Via) -> Result<Value> {
    let step = a.text("step");
    store::update_project(&a.project()?, |p| {
        p.approve(&step, a.opt("note"), via, Timestamp::now())?;
        Ok(json!(p.last_decision(&step)))
    })
}

fn redo(a: &Args, via: Via) -> Result<Value> {
    let step = a.text("step");
    store::update_project(&a.project()?, |p| {
        p.redo(&step, &a.text("note"), via, Timestamp::now())?;
        Ok(json!(p.last_decision(&step)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    /// One temp home for every test in this binary, set before any test
    /// reads it.
    fn setup() -> tempfile::TempDir {
        static HOME: Once = Once::new();
        HOME.call_once(|| {
            let home = tempfile::tempdir().unwrap().keep();
            // SAFETY: every test calls this before it reads the environment,
            // and `Once` makes the others wait until it's set.
            unsafe { std::env::set_var("MOCKINGBIRD_STUDIO_HOME", home) };
        });
        tempfile::tempdir().unwrap()
    }

    fn run(name: &str, args: Value) -> Result<Value> {
        dispatch(name, &args, Via::Cli)
    }

    fn new_project(root: &Path, name: &str) -> String {
        let dir = root.join(name);
        run("project_new", json!({"dir": dir, "title": "Example"})).unwrap();
        dir.to_str().unwrap().to_string()
    }

    #[test]
    fn the_table_has_the_ten_operations() {
        let names: Vec<_> = OPS.iter().map(|o| o.name).collect();
        assert_eq!(
            names,
            [
                "project_new",
                "project_add",
                "projects",
                "status",
                "submit",
                "notes",
                "note_add",
                "note_done",
                "approve",
                "redo"
            ]
        );
        assert!(OPS.iter().all(|o| o.args.iter().all(|a| a.name != "via")));
    }

    #[test]
    fn dispatch_checks_arguments_against_the_table() {
        let _t = setup();
        let err = run(
            "approve",
            json!({"project": "x", "step": "script", "via": "window"}),
        )
        .unwrap_err();
        assert!(err.contains("no argument \"via\""), "{err}");
        assert!(
            run("redo", json!({"project": "x", "step": "script"}))
                .unwrap_err()
                .contains("needs note")
        );
        assert!(
            run("note_done", json!({"project": "x", "id": "1"}))
                .unwrap_err()
                .contains("id should be")
        );
        assert!(run("nope", Value::Null).is_err());
        let err = run("status", json!({"project": ""})).unwrap_err();
        assert!(err.contains("needs project"), "{err}");
        assert!(run("project_new", json!({"dir": "x", "title": " "})).is_err());
    }

    #[test]
    fn new_project_makes_folders_and_is_listed_by_name() {
        let t = setup();
        let dir = new_project(t.path(), "listed-by-name");
        for step in ["research", "thumbnail", "description", "upload"] {
            assert!(Path::new(&dir).join(step).is_dir());
        }
        let status = run("status", json!({"project": "listed-by-name"})).unwrap();
        assert_eq!(status["title"], "Example");
        assert_eq!(status["steps"][0]["state"], "todo");
        let all = run("projects", Value::Null).unwrap();
        assert!(
            all["projects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["project"] == status["project"])
        );
        assert!(run("project_new", json!({"dir": dir, "title": "Again"})).is_err());

        // Taken off the list by hand, then added back.
        store::update_config(|c| {
            c.projects.retain(|p| !p.ends_with("listed-by-name"));
            Ok(())
        })
        .unwrap();
        assert!(run("status", json!({"project": "listed-by-name"})).is_err());
        run("project_add", json!({"dir": dir})).unwrap();
        assert!(run("status", json!({"project": "listed-by-name"})).is_ok());
    }

    #[test]
    fn a_path_that_leaves_the_folder_is_refused() {
        let t = setup();
        let dir = new_project(t.path(), "paths");
        fs::write(t.path().join("outside.md"), "x").unwrap();
        std::os::unix::fs::symlink(
            t.path().join("outside.md"),
            Path::new(&dir).join("research/link.md"),
        )
        .unwrap();
        let outside = t.path().join("outside.md");
        for bad in [
            "../outside.md",
            "research/../../outside.md",
            outside.to_str().unwrap(),
            "research/link.md",
        ] {
            let err = run(
                "submit",
                json!({"project": dir, "step": "research", "files": [bad]}),
            )
            .unwrap_err();
            assert!(err.contains("outside the project folder"), "{bad}: {err}");
            let err = run(
                "note_add",
                json!({"project": dir, "step": "research", "text": "n", "path": bad}),
            );
            assert!(err.is_err(), "{bad}");
        }
        assert!(
            run(
                "submit",
                json!({"project": dir, "step": "research", "files": ["research/none.md"]})
            )
            .is_err()
        );
        assert!(
            run(
                "submit",
                json!({"project": dir, "step": "research", "files": ["research"]})
            )
            .is_err()
        );

        // Inside the folder is fine, given relative or absolute.
        fs::write(Path::new(&dir).join("research/topic.md"), "x").unwrap();
        let abs = Path::new(&dir).join("research/topic.md");
        let step = run(
            "submit",
            json!({"project": dir, "step": "research", "files": [abs]}),
        )
        .unwrap();
        assert_eq!(step["output"][0]["path"], "research/topic.md");
        assert_eq!(step["output"][0]["kind"], "text");
    }

    #[test]
    fn submit_on_the_thumbnail_step_checks_the_image() {
        let t = setup();
        let dir = new_project(t.path(), "thumb");
        let d = Path::new(&dir);
        for id in ["research", "script", "look"] {
            let f = format!("{id}/x.md");
            fs::write(d.join(&f), "x").unwrap();
            run("submit", json!({"project": dir, "step": id, "files": [f]})).unwrap();
            run("approve", json!({"project": dir, "step": id})).unwrap();
        }
        let submit = |f: &str| {
            run(
                "submit",
                json!({"project": dir, "step": "thumbnail", "files": [f]}),
            )
        };
        fs::write(d.join("thumbnail/a.png"), b"not an image").unwrap();
        assert!(
            submit("thumbnail/a.png")
                .unwrap_err()
                .contains("not a PNG or JPEG")
        );
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend([0, 0, 5, 0, 0, 0, 2, 0xD0, 8, 2, 0, 0, 0]);
        fs::write(d.join("thumbnail/a.png"), png).unwrap();
        assert_eq!(submit("thumbnail/a.png").unwrap()["state"], "review");
    }

    #[test]
    fn the_notes_round_trip() {
        let t = setup();
        let dir = new_project(t.path(), "round-trip");
        let p = |extra: Value| {
            let mut v = json!({"project": dir});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            v
        };
        fs::write(Path::new(&dir).join("research/topic.md"), "x").unwrap();
        run(
            "submit",
            p(json!({"step": "research", "files": ["research/topic.md"]})),
        )
        .unwrap();
        run(
            "note_add",
            p(json!({"step": "research", "text": "one", "path": "research/topic.md"})),
        )
        .unwrap();
        run("note_add", p(json!({"step": "research", "text": "two"}))).unwrap();
        let d = dispatch(
            "redo",
            &p(json!({"step": "research", "note": "tighten it"})),
            Via::Window,
        )
        .unwrap();
        assert_eq!(
            (d["via"].as_str(), d["decision"].as_str()),
            (Some("window"), Some("redo"))
        );

        let n = run("notes", p(json!({"step": "research"}))).unwrap();
        assert_eq!(n["notes"].as_array().unwrap().len(), 2);
        assert_eq!(n["redo"][0]["note"], "tighten it");
        assert!(run("notes", p(json!({"step": "nope"}))).is_err());

        run(
            "submit",
            p(json!({"step": "research", "files": ["research/topic.md"]})),
        )
        .unwrap();
        // The redo was on round 1, so round 2 shows no decision yet.
        let status = run("status", p(json!({}))).unwrap();
        assert_eq!(status["steps"][0]["last_decision"], Value::Null);
        run("note_done", p(json!({"id": 1, "reply": "done"}))).unwrap();
        run("note_done", p(json!({"id": 2}))).unwrap();
        let d = dispatch("approve", &p(json!({"step": "research"})), Via::Mcp).unwrap();
        assert_eq!(
            (d["via"].as_str(), d["round"].as_u64()),
            (Some("mcp"), Some(2))
        );

        let status = run("status", p(json!({}))).unwrap();
        assert_eq!(status["open_notes"].as_array().unwrap().len(), 0);
        assert_eq!(status["steps"][0]["state"], "approved");
        assert_eq!(status["steps"][0]["last_decision"]["decision"], "approved");
        let n = run("notes", p(json!({}))).unwrap();
        assert_eq!(n, json!({"notes": [], "redo": []}));
    }
}
