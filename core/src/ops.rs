//! The one table of operations. Each takes JSON arguments and returns JSON.
//! The CLI, the MCP server and the window are thin layers over `dispatch`,
//! and each passes its own `via`: no operation takes it as an argument.

use std::fs;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::jobs;
use crate::niche;
use crate::project::{Kind, NoteState, Output, Project, Result, State, Via};
use crate::rounds;
use crate::scan;
use crate::schedule;
use crate::sheet;
use crate::store::{self, PROJECT_FILE};
use crate::upload;
use crate::youtube;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    Text,
    Number,
    Integer,
    TextList,
    /// A list of JSON objects. On the command line, one JSON list.
    Objects,
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
const NICHE: Arg = req("niche", Text, "The niche's id, or its name.");
const NICHE_VISIBILITY: &str = "private, unlisted or public. Private unless set.";
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
            opt(
                "segments",
                Text,
                "For audio and video files: JSON from each file's path to its segments, such as {\"previews/ch1.mp4\": [{\"label\": \"Cold open\", \"from\": 0, \"to\": 21.4}]}, in seconds. The window shows them on the timeline.",
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
    Op {
        name: "job_start",
        help: "Queue a long job. It runs in a separate runner, two at a time, and keeps going after the chat or window that queued it closes. The kind tool runs a tool declared in config.toml with values for its placeholders, never a command line. The kind mockingbird makes one call to mockingbird's motion, render or look with args, and waits however long it takes. The kind youtube posts a project whose upload step is approved, once.",
        args: &[
            req(
                "kind",
                Text,
                "The job's kind: tool, mockingbird or youtube.",
            ),
            opt(
                "tool",
                Text,
                "For kind tool, the tool's name from [tool.<name>] in config.toml. For kind mockingbird: motion, render or look.",
            ),
            opt(
                "args",
                Text,
                "For kind mockingbird: the tool's arguments as a JSON object, with absolute paths. Give out to have the file listed as the job's output.",
            ),
            opt(
                "values",
                TextList,
                "name=value for each {placeholder} in the tool's command. out and out_* are the files it writes, relative to the folder it runs in.",
            ),
            opt(
                "project",
                Text,
                "The project it's for. The job runs in its folder.",
            ),
            opt(
                "step",
                Text,
                "The step it builds. Refused under the same rule as submit.",
            ),
        ],
        run: job_start,
    },
    Op {
        name: "jobs",
        help: "Every job, oldest first, with its state.",
        args: &[opt("project", Text, "Only this project's jobs.")],
        run: jobs,
    },
    Op {
        name: "job",
        help: "One job and the end of its log.",
        args: &[req("id", Text, "The job's id.")],
        run: job,
    },
    Op {
        name: "job_cancel",
        help: "Stop a queued or running job: SIGTERM, then SIGKILL after 5 seconds.",
        args: &[req("id", Text, "The job's id.")],
        run: job_cancel,
    },
    Op {
        name: "sheet",
        help: "The description step's sheet as YouTube lays it out: the description with its chapter lines, and the counts YouTube limits.",
        args: &[PROJECT],
        run: sheet,
    },
    Op {
        name: "round_text",
        help: "A text output as it was submitted in a round, and the lines changed since the round before it.",
        args: &[
            PROJECT,
            STEP,
            req("path", Text, "The file, relative to the project folder."),
            opt("round", Integer, "The round; the current one if left out."),
        ],
        run: round_text,
    },
    Op {
        name: "niches",
        help: "Every niche with its feeds, workflow, visibility and channel.",
        args: &[],
        run: |a, _| niche::niches(a),
    },
    Op {
        name: "niche_add",
        help: "Add a niche. Its id is the name in lower case with dashes. Visibility is private unless you set it.",
        args: &[
            req("name", Text, "The niche's name, such as AI news."),
            opt(
                "feeds",
                TextList,
                "Feed addresses, http or https. The scan fetches them.",
            ),
            opt("workflow", Text, "The default workflow for its projects."),
            opt("visibility", Text, NICHE_VISIBILITY),
            opt("channel", Text, "The label of the channel it posts to."),
        ],
        run: |a, _| niche::niche_add(a),
    },
    Op {
        name: "niche_set",
        help: "Change a niche's name, workflow, visibility or channel. An empty workflow or channel clears it.",
        args: &[
            NICHE,
            opt("name", Text, "The new name. The id stays."),
            opt("workflow", Text, "The default workflow for its projects."),
            opt("visibility", Text, NICHE_VISIBILITY),
            opt("channel", Text, "The label of the channel it posts to."),
        ],
        run: |a, _| niche::niche_set(a),
    },
    Op {
        name: "niche_remove",
        help: "Remove a niche and its feed list. Projects that name it keep the name.",
        args: &[NICHE],
        run: |a, _| niche::niche_remove(a),
    },
    Op {
        name: "feed_add",
        help: "Add a feed address to a niche. Nothing is fetched.",
        args: &[NICHE, req("url", Text, "An http or https address.")],
        run: |a, _| niche::feed_add(a),
    },
    Op {
        name: "feed_remove",
        help: "Remove a feed address from a niche.",
        args: &[NICHE, req("url", Text, "The address, as listed.")],
        run: |a, _| niche::feed_remove(a),
    },
    Op {
        name: "youtube_connect",
        help: "Sign in to your own YouTube channel. Opens Google's consent page in your browser and waits up to five minutes. Asks for the upload scope only. The refresh token goes in the macOS Keychain under the label, and no reply holds it.",
        args: &[
            req(
                "label",
                Text,
                "The name the app knows the channel by, such as main.",
            ),
            req(
                "client_file",
                Text,
                "The JSON file Google gave you for your Desktop app OAuth client (docs/YOUTUBE.md): an absolute path, or one starting with ~/.",
            ),
        ],
        run: |a, _| youtube::connect(&a.text("label"), &a.text("client_file")),
    },
    Op {
        name: "youtube_status",
        help: "Each channel's label, whether it's signed in, and its id and title once an upload has told the app.",
        args: &[],
        run: |_, _| youtube::status(),
    },
    Op {
        name: "youtube_disconnect",
        help: "Revoke a channel's token at Google, delete it from the Keychain, and take the label out of config.toml.",
        args: &[req("label", Text, "The channel's label.")],
        run: |a, _| youtube::disconnect(&a.text("label")),
    },
    Op {
        name: "scan",
        help: "Fetch the feeds of one niche, or of every niche, and write today's scan: the entries from the last window_hours that earlier scans haven't listed. Fetches the feed addresses only, never the pages they link to. From the command line it also runs [scan] command to shortlist the items. Over MCP and from the window it doesn't: rank the items yourself and save your shortlist with topics_set.",
        args: &[opt(
            "niche",
            Text,
            "The niche's id, or its name. Every niche if left out.",
        )],
        run: |a, via| scan::scan(a, via),
    },
    Op {
        name: "topics",
        help: "The latest scan of one niche, or of every niche: its items and its shortlist.",
        args: &[
            opt(
                "niche",
                Text,
                "The niche's id, or its name. Every niche if left out.",
            ),
            opt(
                "day",
                Text,
                "A day, such as 2026-11-26. The latest if left out.",
            ),
        ],
        run: |a, _| scan::topics(a),
    },
    Op {
        name: "topics_set",
        help: "Save your shortlist for a niche's latest scan, or the scan of one day, so the window shows it. Replaces the shortlist, but keeps topics already picked. Each topic is {title, why, items}, where items are links from that scan's items.",
        args: &[
            NICHE,
            req(
                "topics",
                Objects,
                "The topics, best first: objects with title, why, and items (links from the scan).",
            ),
            opt(
                "day",
                Text,
                "The scan's day, such as 2026-11-26. The latest if left out.",
            ),
        ],
        run: |a, _| scan::topics_set(a),
    },
    Op {
        name: "topic_pick",
        help: "Make a project from a topic on a niche's shortlist, with the niche's workflow, and mark the topic picked.",
        args: &[
            NICHE,
            req("topic", Text, "The topic's id, from topics."),
            req("dir", Text, "The new project's folder."),
            opt(
                "day",
                Text,
                "The scan's day, such as 2026-11-26. The latest if left out.",
            ),
        ],
        run: |a, _| scan::topic_pick(a),
    },
    Op {
        name: "scan_time",
        help: "Show, set or clear the time the scan runs each day. A time such as 07:30 writes a launchd agent in ~/Library/LaunchAgents that runs `mockingbird-studio scan` then, and logs to scan.log in the studio's home. An empty time removes the agent. With no time, shows the schedule and who set it. There is one schedule per Mac user: setting it with another MOCKINGBIRD_STUDIO_HOME replaces it.",
        args: &[opt(
            "time",
            Text,
            "HH:MM in 24-hour local time, such as 07:30. Empty clears the schedule.",
        )],
        run: schedule::scan_time,
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
            Objects => value
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_object)),
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
pub(crate) struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    pub(crate) fn opt(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(Value::as_str).map(str::to_string)
    }
    pub(crate) fn text(&self, key: &str) -> String {
        self.opt(key).unwrap_or_default()
    }
    pub(crate) fn objects(&self, key: &str) -> &[Value] {
        self.0
            .get(key)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }
    fn number(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(Value::as_f64)
    }
    pub(crate) fn list(&self, key: &str) -> Vec<String> {
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
    let dir = new_project(
        Path::new(&a.text("dir")),
        &a.text("title"),
        a.opt("niche"),
        a.opt("workflow"),
    )?;
    status_of(&dir)
}

/// Makes the project folder, its `project.toml` and its place in the list.
/// Returns the folder, canonical.
pub(crate) fn new_project(
    dir: &Path,
    title: &str,
    niche: Option<String>,
    workflow: Option<String>,
) -> Result<PathBuf> {
    let shown = dir.display();
    // Nothing here expands ~ or $HOME: they'd make a folder named "~".
    if dir.to_string_lossy().starts_with(['~', '$']) {
        return Err(format!(
            "{shown}: write the folder's full path; ~ and $ aren't expanded here"
        ));
    }
    if dir.join(PROJECT_FILE).exists() {
        return Err(format!("{shown} is already a project"));
    }
    if dir.read_dir().is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!(
            "{shown} already has files in it; pick a new or empty folder, or use project_add"
        ));
    }
    let workflow = workflow.unwrap_or_else(|| "explainer".into());
    let project = Project::new(title, niche, &workflow, Timestamp::now())?;
    let io = |e: std::io::Error| format!("can't make {}: {e}", dir.display());
    fs::create_dir_all(dir).map_err(io)?;
    let dir = canonical(dir)?;
    for step in &project.steps {
        fs::create_dir_all(dir.join(&step.id)).map_err(io)?;
    }
    store::create_project(&dir, &project)?;
    add_to_list(&dir)?;
    Ok(dir)
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

pub(crate) fn status_of(dir: &Path) -> Result<Value> {
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
    let jobs = jobs::list(Some(dir));
    jobs::wake(&jobs);
    Ok(json!({
        "project": dir,
        "title": p.title,
        "niche": p.niche,
        "workflow": p.workflow,
        "created": p.created,
        "steps": steps,
        "open_notes": open,
        "jobs": jobs,
    }))
}

fn submit(a: &Args, via: Via) -> Result<Value> {
    let dir = a.project()?;
    let files = a.list("files");
    if files.is_empty() {
        return Err("submit needs at least one file".into());
    }
    let mut outputs = files
        .iter()
        .map(|f| {
            inside(&dir, f).map(|path| Output {
                kind: Kind::of(&path),
                path,
                rest: Default::default(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // Before the lock: ffprobe takes a moment per file.
    let mut warnings = vec![];
    let need = crate::proxy::probe(&dir, &mut outputs, &mut warnings);
    // After the probe, so segments are checked against each file's length.
    if let Some(json) = a.opt("segments") {
        add_segments(&dir, &mut outputs, &json)?;
    }
    let mut proxy_jobs = vec![];
    let step = a.text("step");
    let paths: Vec<String> = outputs.iter().map(|o| o.path.clone()).collect();
    let mut v = store::update_project(&dir, |p| {
        p.submit(&step, outputs, a.opt("summary"), via, Timestamp::now())?;
        // After the gate check, so a gate refusal comes first. An Err here
        // writes nothing. A match so the next step's checks are one more arm.
        match step.as_str() {
            "thumbnail" => crate::thumbnail::check(&dir, &paths)?,
            "description" => sheet::check_submit(&dir, &paths)?,
            _ => {}
        }
        let s = p.step(&step)?;
        rounds::keep(&dir, &step, s.round, &s.outputs)?;
        // Last, so a refused submit queues nothing. A copy that can't be
        // queued doesn't refuse the submit: the reply says why.
        if let Some(s) = p.steps.iter_mut().find(|s| s.id == step) {
            proxy_jobs = crate::proxy::queue_all(&dir, &mut s.outputs, &need, &mut warnings);
        }
        Ok(json!(p.step(&step)?))
    })?;
    if !proxy_jobs.is_empty() {
        v["proxy_jobs"] = proxy_jobs.into();
    }
    if !warnings.is_empty() {
        v["warnings"] = warnings.into();
    }
    Ok(v)
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    label: String,
    from: f64,
    to: f64,
}

/// Puts each file's segments on its output, after checking that the file is
/// one of this round's audio or video files, has one list, and each segment
/// runs forward and ends by the file's length when that's known.
fn add_segments(dir: &Path, outputs: &mut [Output], json: &str) -> Result<()> {
    // Slack for a length that ffprobe reads a little short.
    const SLACK: f64 = 0.5;
    let map: std::collections::BTreeMap<String, Vec<Segment>> =
        serde_json::from_str(json).map_err(|e| format!("segments: {e}"))?;
    let mut seen = std::collections::BTreeSet::new();
    for (file, segments) in map {
        let path = inside(dir, &file)?;
        if !seen.insert(path.clone()) {
            return Err(format!("two segment lists for {path:?}"));
        }
        let o = outputs
            .iter_mut()
            .find(|o| o.path == path)
            .ok_or_else(|| format!("segments for {file:?}, which isn't one of the files"))?;
        if !matches!(o.kind, Kind::Audio | Kind::Video) {
            return Err(format!("segments for {file:?}, which isn't audio or video"));
        }
        if let Some(s) = segments
            .iter()
            .find(|s| !(s.from >= 0.0 && s.to > s.from && s.to.is_finite()))
        {
            return Err(format!(
                "segment {:?} in {file:?} should have 0 <= from < to",
                s.label
            ));
        }
        if let Some(d) = o.rest.get("duration").and_then(toml::Value::as_float)
            && let Some(s) = segments.iter().find(|s| s.to > d + SLACK)
        {
            return Err(format!(
                "segment {:?} in {file:?} ends at {} s, after the file's {d} s",
                s.label, s.to
            ));
        }
        let v = toml::Value::try_from(&segments).map_err(|e| e.to_string())?;
        o.rest.insert("segments".into(), v);
    }
    Ok(())
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

fn job_start(a: &Args, _: Via) -> Result<Value> {
    let kind = a.text("kind");
    if kind == "youtube" {
        let dir = a.opt("project").ok_or("a youtube job needs its project")?;
        return Ok(json!(upload::start(project_dir(&dir)?)?));
    }
    if kind != "tool" && kind != "mockingbird" {
        return Err(format!(
            "no job kind {kind:?}; the kinds are: tool, mockingbird, youtube"
        ));
    }
    let tool = a.opt("tool").ok_or(format!("a {kind} job needs tool"))?;
    let values: std::collections::BTreeMap<String, String> = a
        .list("values")
        .into_iter()
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) if !k.is_empty() => Ok((k.to_string(), v.to_string())),
            _ => Err(format!("{kv:?} should be name=value")),
        })
        .collect::<Result<_>>()?;
    let project = a.opt("project").map(|p| project_dir(&p)).transpose()?;
    let step = a.opt("step");
    if let Some(step) = &step {
        let dir = project
            .as_ref()
            .ok_or("a job for a step needs its project")?;
        store::read::<Project>(&dir.join(PROJECT_FILE))?.check_submit(step)?;
    }
    if kind == "mockingbird" {
        if !values.is_empty() {
            return Err("a mockingbird job takes args, not values".into());
        }
        let args = a.opt("args").ok_or("a mockingbird job needs args")?;
        return Ok(json!(jobs::start_mockingbird(&tool, &args, project, step)?));
    }
    if a.opt("args").is_some() {
        return Err("a tool job takes values, not args".into());
    }
    Ok(json!(jobs::start(&tool, values, project, step)?))
}

fn jobs(a: &Args, _: Via) -> Result<Value> {
    let project = a.opt("project").map(|p| project_dir(&p)).transpose()?;
    let list = jobs::list(project.as_deref());
    jobs::wake(&list);
    Ok(json!({"jobs": list}))
}

fn job(a: &Args, _: Via) -> Result<Value> {
    let id = a.text("id");
    let j = jobs::read(&id)?;
    jobs::wake(std::slice::from_ref(&j));
    let mut v = json!(j);
    v["log"] = jobs::log_tail(&id).into();
    Ok(v)
}

fn job_cancel(a: &Args, _: Via) -> Result<Value> {
    jobs::cancel(&a.text("id"))?;
    Ok(json!({"id": a.text("id"), "cancel": "asked"}))
}

/// The sheet as submitted this round, so the person approves what the agent
/// sent.
fn sheet(a: &Args, _: Via) -> Result<Value> {
    let dir = a.project()?;
    let p: Project = store::read(&dir.join(PROJECT_FILE))?;
    let (s, kept) = sheet::submitted(&dir, p.step("description")?.round)?;
    let description = s.full_description();
    Ok(json!({
        "kept": kept,
        "error": s.check().err(),
        "limits": {
            "title": sheet::TITLE_CHARS,
            "description": sheet::DESCRIPTION_BYTES,
            "tags": sheet::TAGS_CHARS,
        },
        "counts": {
            "title": s.title.chars().count(),
            "description": description.len(),
            "tags": s.tags_chars(),
        },
        "description": description,
        "title": s.title,
        "tags": s.tags,
        "category": s.category,
        "made_for_kids": s.made_for_kids,
        "altered_content": s.altered_content,
    }))
}

fn round_text(a: &Args, _: Via) -> Result<Value> {
    let dir = a.project()?;
    let p: Project = store::read(&dir.join(PROJECT_FILE))?;
    let round = match a.0.get("round").and_then(Value::as_u64) {
        Some(r) => Some(u32::try_from(r).map_err(|_| format!("round {r} is too large"))?),
        None => None,
    };
    rounds::text(&dir, p.step(&a.text("step"))?, &a.text("path"), round)
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
    fn the_table_has_every_operation() {
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
                "redo",
                "job_start",
                "jobs",
                "job",
                "job_cancel",
                "sheet",
                "round_text",
                "niches",
                "niche_add",
                "niche_set",
                "niche_remove",
                "feed_add",
                "feed_remove",
                "youtube_connect",
                "youtube_status",
                "youtube_disconnect",
                "scan",
                "topics",
                "topics_set",
                "topic_pick",
                "scan_time"
            ]
        );
        assert!(OPS.iter().all(|o| o.args.iter().all(|a| a.name != "via")));
        // [scan] command runs a program; only the person's own edit sets it.
        assert!(
            OPS.iter()
                .all(|o| o.args.iter().all(|a| a.name != "command"))
        );
    }

    #[test]
    fn scan_and_topics_say_what_is_missing_without_fetching() {
        let _t = setup();
        run("niche_add", json!({"name": "Scan test"})).unwrap();
        let e = run("scan", json!({"niche": "scan-test"})).unwrap_err();
        assert!(e.contains("has no feeds"), "{e}");
        let e = run("topics", json!({"niche": "scan-test"})).unwrap_err();
        assert!(e.contains("no scan of scan-test yet"), "{e}");
        let e = run("topics", json!({"day": "yesterday"})).unwrap_err();
        assert!(e.contains("day should look like"), "{e}");
        assert!(run("scan", json!({"niche": "nope"})).is_err());
        // Asked for every niche, a niche with no feeds is reported, not fatal.
        let r = run("scan", json!({})).unwrap();
        assert!(
            r["errors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["niche"] == "scan-test")
        );
        assert!(run("topics", json!({"niche": "scan-test", "day": "2020-01-01"})).is_err());
        // A scan file that can't be read: an error for that niche, not the call.
        let dir = store::home().join("scans/scan-test");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("2026-11-26.json"), "not json").unwrap();
        let r = run("topics", json!({})).unwrap();
        let errs = r["errors"].as_array().unwrap();
        assert!(errs.iter().any(|e| e["niche"] == "scan-test"), "{r}");
        assert!(run("topics", json!({"niche": "scan-test"})).is_err());
    }

    #[test]
    fn an_agent_sets_the_shortlist_and_a_topic_becomes_a_project() {
        let t = setup();
        run("niche_add", json!({"name": "Pick test"})).unwrap();
        let e = run("topics_set", json!({"niche": "pick-test", "topics": []})).unwrap_err();
        assert!(e.contains("no scan of pick-test"), "{e}");
        let file = store::home().join("scans/pick-test/2026-11-25.json");
        let scan = json!({
            "niche": "pick-test", "day": "2026-11-25", "at": "2026-11-25T07:30:00Z",
            "feeds": [], "error": "the command failed",
            "items": [{"link": "https://a.example/1", "feed": "https://a.example/feed", "title": "One"}],
        });
        store::write_json(&file, &scan).unwrap();
        let topics = json!([
            {"title": "First idea", "why": "Because.", "items": ["https://a.example/1"]},
            {"title": "Second idea"},
        ]);
        let r = run(
            "topics_set",
            json!({"niche": "Pick test", "topics": topics}),
        )
        .unwrap();
        assert_eq!(r["shortlist_by"], "agent");
        assert!(r.get("error").is_none(), "{r}");
        assert_eq!(r["topics"][1]["id"], "2");
        // A link that isn't in the scan refuses the whole list and changes nothing.
        let bad = json!([{"title": "X", "items": ["https://elsewhere.example/"]}]);
        let e = run("topics_set", json!({"niche": "pick-test", "topics": bad})).unwrap_err();
        assert!(e.contains("not a link from this scan"), "{e}");
        assert!(run("topics_set", json!({"niche": "pick-test", "topics": ["x"]})).is_err());

        let dir = t.path().join("first-idea");
        let r = run(
            "topic_pick",
            json!({"niche": "pick-test", "topic": "1", "dir": dir}),
        )
        .unwrap();
        assert_eq!(r["title"], "First idea");
        assert_eq!(r["niche"], "pick-test");
        assert_eq!(r["workflow"], "explainer");
        assert_eq!(r["topic"]["items"][0], "https://a.example/1");
        let dir = canonical(&dir).unwrap();
        assert!(store::read_config().unwrap().projects.contains(&dir));
        assert!(dir.join("research").is_dir());
        let e = run(
            "topic_pick",
            json!({"niche": "pick-test", "topic": "1", "dir": t.path().join("again")}),
        )
        .unwrap_err();
        assert!(e.contains("already the project"), "{e}");
        let e = run(
            "topic_pick",
            json!({"niche": "pick-test", "topic": "9", "dir": t.path().join("nine")}),
        )
        .unwrap_err();
        assert!(e.contains("no topic \"9\""), "{e}");

        // A new shortlist keeps the picked topic and numbers after it.
        let r = run(
            "topics_set",
            json!({"niche": "pick-test", "day": "2026-11-25", "topics": [{"title": "Third"}]}),
        )
        .unwrap();
        let list = r["topics"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            (list[0]["id"].as_str(), list[0]["project"].as_str()),
            (Some("1"), dir.to_str())
        );
        assert_eq!(list[1]["id"], "2");
        let back: scan::Scan = store::read_json(&file).unwrap();
        assert_eq!(back.topics.len(), 2);

        // The project is made before the topic is linked, and a failed link says so.
        let scans = file.parent().unwrap();
        let ro = |mode| {
            fs::set_permissions(scans, std::os::unix::fs::PermissionsExt::from_mode(mode)).unwrap()
        };
        ro(0o555);
        let third = t.path().join("third");
        let e = run(
            "topic_pick",
            json!({"niche": "pick-test", "topic": "2", "dir": third}),
        );
        ro(0o755);
        let e = e.unwrap_err();
        assert!(
            e.contains("made the project") && e.contains("couldn't mark topic 2"),
            "{e}"
        );
        assert!(third.join(PROJECT_FILE).is_file());
    }

    #[test]
    fn project_new_refuses_home_shorthand_and_folders_with_files() {
        let t = setup();
        for dir in ["~/videos/x", "~", "$HOME/videos/x"] {
            let e = run("project_new", json!({"dir": dir, "title": "T"})).unwrap_err();
            assert!(e.contains("full path"), "{e}");
            assert!(!Path::new(dir).exists());
        }
        fs::write(t.path().join("footage.mov"), "x").unwrap();
        let e = run("project_new", json!({"dir": t.path(), "title": "T"})).unwrap_err();
        assert!(e.contains("already has files"), "{e}");
        let empty = t.path().join("empty");
        fs::create_dir(&empty).unwrap();
        run("project_new", json!({"dir": empty, "title": "T"})).unwrap();
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
    fn submit_keeps_segments_on_audio_and_video() {
        let t = setup();
        let dir = new_project(t.path(), "segments");
        for f in ["research/a.mp4", "research/b.md"] {
            fs::write(Path::new(&dir).join(f), "x").unwrap();
        }
        let submit = |segments: &str| {
            run(
                "submit",
                json!({"project": dir, "step": "research",
                    "files": ["research/a.mp4", "research/b.md"], "segments": segments}),
            )
        };
        for (bad, says) in [
            (r#"{"research/b.md": []}"#, "isn't audio or video"),
            (r#"{"research/c.mp4": []}"#, "research/c.mp4"),
            (
                r#"{"research/a.mp4": [{"label": "x", "from": 2, "to": 1}]}"#,
                "from < to",
            ),
            (
                r#"{"research/a.mp4": [{"label": "x", "from": 0}]}"#,
                "missing field",
            ),
            ("[]", "segments:"),
            (
                r#"{"research/a.mp4": [], "./research/a.mp4": []}"#,
                "two segment lists",
            ),
        ] {
            let err = submit(bad).unwrap_err();
            assert!(err.contains(says), "{bad}: {err}");
        }
        let step =
            submit(r#"{"./research/a.mp4": [{"label": "Cold open", "from": 0, "to": 2.5}]}"#)
                .unwrap();
        assert_eq!(
            step["output"][0]["segments"],
            json!([{"label": "Cold open", "from": 0.0, "to": 2.5}])
        );
        assert!(step["output"][1].get("segments").is_none());
    }

    #[test]
    fn segments_end_by_the_files_length() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("a.mp4"), "x").unwrap();
        let mut outs = [Output {
            path: "a.mp4".into(),
            kind: Kind::Video,
            rest: toml::Table::from_iter([("duration".to_string(), 10.0.into())]),
        }];
        let seg = |to: f64| format!(r#"{{"a.mp4": [{{"label": "x", "from": 0, "to": {to}}}]}}"#);
        add_segments(t.path(), &mut outs, &seg(10.4)).unwrap();
        let err = add_segments(t.path(), &mut outs, &seg(10.6)).unwrap_err();
        assert!(err.contains("after the file's 10 s"), "{err}");
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

    #[test]
    fn a_round_keeps_its_text_and_shows_what_changed() {
        let t = setup();
        let dir = new_project(t.path(), "rounds");
        let file = Path::new(&dir).join("research/topic.md");
        let args = json!({"project": dir, "step": "research", "files": ["research/topic.md"]});
        let text = |extra: Value| {
            let mut v = json!({"project": dir, "step": "research", "path": "research/topic.md"});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            run("round_text", v)
        };
        fs::write(&file, "one\ntwo\n").unwrap();
        run("submit", args.clone()).unwrap();
        let r = text(json!({})).unwrap();
        assert_eq!(
            (r["round"].as_u64(), r["changes"].is_null()),
            (Some(1), true)
        );

        run(
            "redo",
            json!({"project": dir, "step": "research", "note": "more"}),
        )
        .unwrap();
        fs::write(&file, "one\nthree\n").unwrap();
        run("submit", args).unwrap();
        // The agent edits the file after submitting: the round still shows what it got.
        fs::write(&file, "later\n").unwrap();
        let r = text(json!({})).unwrap();
        assert_eq!(r["text"], "one\nthree\n");
        assert_eq!(r["previous_round"], 1);
        assert_eq!(
            r["changes"],
            json!([[" ", "one"], ["-", "two"], ["+", "three"]])
        );
        assert_eq!(text(json!({"round": 1})).unwrap()["text"], "one\ntwo\n");
        assert!(text(json!({"round": 3})).is_err());
        assert!(text(json!({"path": "../rounds/research/topic.md"})).is_err());
        let err = text(json!({"round": 1u64 << 32})).unwrap_err();
        assert!(err.contains("too large"), "{err}");
        let r = text(json!({"round": 2})).unwrap();
        assert_eq!(
            (r["kept"].as_bool(), r["diff"].is_null()),
            (Some(true), true)
        );

        // A round with no kept copy falls back to the live file and says so.
        fs::remove_dir_all(Path::new(&dir).join(".studio/rounds/research/2")).unwrap();
        let r = text(json!({})).unwrap();
        assert_eq!(
            (r["kept"].as_bool(), r["text"].as_str()),
            (Some(false), Some("later\n"))
        );
    }

    #[test]
    fn a_symlinked_studio_folder_is_refused() {
        let t = setup();
        let dir = new_project(t.path(), "symlinked");
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, Path::new(&dir).join(".studio")).unwrap();
        fs::write(Path::new(&dir).join("research/topic.md"), "x").unwrap();
        let err = run(
            "submit",
            json!({"project": dir, "step": "research", "files": ["research/topic.md"]}),
        )
        .unwrap_err();
        assert!(err.contains("outside the project folder"), "{err}");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        let status = run("status", json!({"project": dir})).unwrap();
        assert_eq!(status["steps"][0]["round"], 0);
    }

    #[test]
    fn the_description_step_checks_its_sheet() {
        let t = setup();
        let dir = new_project(t.path(), "sheet");
        let root = Path::new(&dir);
        let submit = |files: Value| {
            run(
                "submit",
                json!({"project": dir, "step": "description", "files": files}),
            )
        };
        // The checks run after the gate check, so approve every step before it.
        let steps = run("status", json!({"project": dir})).unwrap()["steps"].clone();
        for s in steps.as_array().unwrap() {
            let id = s["id"].as_str().unwrap();
            if id == "description" {
                break;
            }
            let file = if id == "thumbnail" {
                // A PNG header that says 1280×720, which is all the check reads.
                let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
                png.extend([0, 0, 5, 0, 0, 0, 2, 0xD0, 8, 2, 0, 0, 0]);
                fs::write(root.join("thumbnail/a.png"), png).unwrap();
                "thumbnail/a.png".to_string()
            } else {
                fs::write(root.join(format!("{id}.md")), "x").unwrap();
                format!("{id}.md")
            };
            let files = json!([file]);
            run(
                "submit",
                json!({"project": dir, "step": id, "files": files}),
            )
            .unwrap();
            if s["gate"] == true {
                run("approve", json!({"project": dir, "step": id})).unwrap();
            }
        }

        fs::write(root.join("description/notes.md"), "x").unwrap();
        let err = submit(json!(["description/notes.md"])).unwrap_err();
        assert!(err.contains("needs description/sheet.toml"), "{err}");

        let good = "title = \"Example\"\ndescription = \"About it.\"\ntags = [\"a\"]\n\
                    category = 28\nmade_for_kids = false\naltered_content = false\n";
        fs::write(root.join(sheet::PATH), format!("{good}nope = 1\n")).unwrap();
        let err = submit(json!([sheet::PATH])).unwrap_err();
        assert!(err.contains("unknown field"), "{err}");
        let long = format!("title = \"{}\"\n{}", "x".repeat(101), &good[18..]);
        fs::write(root.join(sheet::PATH), long).unwrap();
        let err = submit(json!([sheet::PATH])).unwrap_err();
        assert!(err.starts_with("title is 101 characters"), "{err}");

        fs::write(root.join(sheet::PATH), good).unwrap();
        submit(json!([sheet::PATH])).unwrap();
        let view = run("sheet", json!({"project": dir})).unwrap();
        assert_eq!(
            view["counts"],
            json!({"title": 7, "description": 9, "tags": 1})
        );
        assert_eq!(view["limits"]["description"], 5000);
        assert_eq!(view["error"], Value::Null);
        // A sheet changed after submit: the view is still the submitted one.
        fs::write(root.join(sheet::PATH), good.replace("[\"a\"]", "[\"a,b\"]")).unwrap();
        let view = run("sheet", json!({"project": dir})).unwrap();
        assert_eq!(
            (view["kept"].as_bool(), &view["error"]),
            (Some(true), &Value::Null)
        );
        assert_eq!(view["tags"], json!(["a"]));
    }
}
