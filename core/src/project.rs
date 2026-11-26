//! `project.toml`, schema 1: the types and the rules for changing them.
//! Nothing here touches the disk but loading a workflow template; `store`
//! reads and writes.

use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize};

use crate::niche::Visibility;

pub const SCHEMA: u32 = 1;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    pub schema: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub niche: Option<String>,
    pub workflow: String,
    #[serde(deserialize_with = "time")]
    pub created: Timestamp,
    pub next_note: u64,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
    #[serde(default, rename = "decision")]
    pub decisions: Vec<Decision>,
    #[serde(default, rename = "note")]
    pub notes: Vec<Note>,
    #[serde(default, rename = "post", skip_serializing_if = "Vec::is_empty")]
    pub posts: Vec<Post>,
    /// Keys this version doesn't know, kept so a write never drops them.
    #[serde(flatten)]
    pub rest: toml::Table,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Step {
    pub id: String,
    pub gate: bool,
    #[serde(default)]
    pub after: Vec<String>,
    pub state: State,
    pub round: u32,
    /// What the agent said about the current round, from `submit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The current round's files.
    #[serde(default, rename = "output")]
    pub outputs: Vec<Output>,
    /// Keys this version doesn't know, kept so a write never drops them.
    #[serde(flatten)]
    pub rest: toml::Table,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Todo,
    Review,
    Redo,
    Approved,
    Done,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Output {
    /// Relative to the project folder.
    pub path: String,
    pub kind: Kind,
    /// Keys this version doesn't know, kept so a write never drops them.
    #[serde(flatten)]
    pub rest: toml::Table,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Pdf,
    Image,
    Audio,
    Video,
    Other,
}

impl Kind {
    pub fn of(path: &str) -> Kind {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("md" | "txt" | "toml" | "json" | "srt" | "vtt" | "csv") => Kind::Text,
            Some("pdf") => Kind::Pdf,
            Some("png" | "jpg" | "jpeg" | "gif" | "webp") => Kind::Image,
            Some("wav" | "mp3" | "m4a" | "aac" | "flac" | "ogg") => Kind::Audio,
            Some("mp4" | "mov" | "m4v" | "webm" | "mkv") => Kind::Video,
            _ => Kind::Other,
        }
    }
}

/// Where a decision came from. Set by the layer that ran the operation,
/// never by the caller.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Window,
    Cli,
    Mcp,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Approved,
    Redo,
}

/// Never edited or removed once written.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Decision {
    pub step: String,
    pub round: u32,
    pub decision: Verdict,
    #[serde(deserialize_with = "time")]
    pub at: Timestamp,
    pub via: Via,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Keys this version doesn't know, kept so a write never drops them.
    #[serde(flatten)]
    pub rest: toml::Table,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u64,
    pub step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Seconds into the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
    pub text: String,
    #[serde(deserialize_with = "time")]
    pub created: Timestamp,
    pub round: u32,
    pub state: NoteState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    /// Keys this version doesn't know, kept so a write never drops them.
    #[serde(flatten)]
    pub rest: toml::Table,
}

/// A video the `youtube` job put up, or is putting up. Never removed.
///
/// The job writes the row as a claim (`state = "uploading"`, with the
/// upload session's address) before it sends anything, so a second run
/// stops and a run after a dead one resumes that session instead of posting
/// twice. When Google answers, the same row gets the video and loses
/// `state` and `session`; then `thumbnail` is filled in.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Post {
    pub step: String,
    pub round: u32,
    /// When it was claimed, then when it was posted.
    #[serde(deserialize_with = "time")]
    pub at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    /// `uploading` while it's a claim; absent once posted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The resumable session's address, while it's a claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The file being sent, relative to the project, and its size, while
    /// it's a claim: a later run resumes the session only for the same file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub video: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    pub channel: String,
    pub visibility_asked: Visibility,
    /// What Google's reply said.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub visibility_set: String,
    /// set, refused or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Keys this version doesn't know, kept so a write never drops them.
    #[serde(flatten)]
    pub rest: toml::Table,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NoteState {
    Open,
    Done,
}

/// Times are written as RFC 3339 strings. Reading also takes a bare TOML
/// datetime, in case someone typed one by hand.
fn time<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Timestamp, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Either {
        Text(String),
        Toml(toml::value::Datetime),
    }
    let text = match Either::deserialize(d)? {
        Either::Text(s) => s,
        Either::Toml(t) => t.to_string(),
    };
    text.parse().map_err(serde::de::Error::custom)
}

impl Project {
    pub fn new(title: &str, niche: Option<String>, workflow: &str, now: Timestamp) -> Result<Self> {
        if title.trim().is_empty() {
            return Err("a project needs a title".into());
        }
        let steps = crate::workflow::load(workflow)?
            .into_iter()
            .map(|t| Step {
                id: t.id,
                gate: t.gate,
                after: t.after,
                state: State::Todo,
                round: 0,
                summary: None,
                outputs: vec![],
                rest: Default::default(),
            })
            .collect();
        Ok(Project {
            schema: SCHEMA,
            title: title.to_string(),
            niche,
            workflow: workflow.to_string(),
            created: now,
            next_note: 1,
            steps,
            decisions: vec![],
            notes: vec![],
            posts: vec![],
            rest: Default::default(),
        })
    }

    pub fn step(&self, id: &str) -> Result<&Step> {
        self.steps
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("no step {id:?} in this project"))
    }

    fn step_mut(&mut self, id: &str) -> Result<&mut Step> {
        self.steps
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("no step {id:?} in this project"))
    }

    /// Whether step `id` takes a new round: it's in todo, redo or approved,
    /// and every step in `after` is approved or done. `submit` and a job
    /// tied to a step both go through this.
    pub fn check_submit(&self, id: &str) -> Result<()> {
        let step = self.step(id)?;
        if !matches!(step.state, State::Todo | State::Redo | State::Approved) {
            return Err(format!(
                "step {id:?} is {:?}; submit takes a step in todo, redo or approved",
                step.state
            ));
        }
        let waiting: Vec<&str> = step
            .after
            .iter()
            .filter(|a| {
                !matches!(
                    self.step(a).map(|s| s.state),
                    Ok(State::Approved | State::Done)
                )
            })
            .map(String::as_str)
            .collect();
        if !waiting.is_empty() {
            return Err(format!(
                "step {id:?} waits for {} to be approved or done",
                waiting.join(", ")
            ));
        }
        Ok(())
    }

    /// Records a new round of files. Refused until every step in `after`
    /// is approved or done: this is what makes a gate hold.
    pub fn submit(
        &mut self,
        id: &str,
        outputs: Vec<Output>,
        summary: Option<String>,
        via: Via,
        now: Timestamp,
    ) -> Result<()> {
        self.check_submit(id)?;
        let was_approved = self.step(id)?.state == State::Approved;
        let step = self.step_mut(id)?;
        step.round += 1;
        step.outputs = outputs;
        step.summary = summary;
        step.state = if step.gate {
            State::Review
        } else {
            State::Done
        };
        if was_approved {
            self.cascade(id, "was submitted again", via, now);
        }
        Ok(())
    }

    pub fn approve(
        &mut self,
        id: &str,
        note: Option<String>,
        via: Via,
        now: Timestamp,
    ) -> Result<()> {
        let step = self.step_mut(id)?;
        if step.state != State::Review {
            return Err(format!(
                "step {id:?} is {:?}; only a step in review can be approved",
                step.state
            ));
        }
        step.state = State::Approved;
        let round = step.round;
        self.decide(id, round, Verdict::Approved, note, via, now);
        Ok(())
    }

    pub fn redo(&mut self, id: &str, note: &str, via: Via, now: Timestamp) -> Result<()> {
        if note.trim().is_empty() {
            return Err("redo needs a note saying what to change".into());
        }
        let step = self.step_mut(id)?;
        if !matches!(step.state, State::Review | State::Approved | State::Done) {
            return Err(format!(
                "step {id:?} is {:?}; redo takes a step in review, approved or done",
                step.state
            ));
        }
        step.state = State::Redo;
        let round = step.round;
        self.decide(id, round, Verdict::Redo, Some(note.to_string()), via, now);
        self.cascade(id, "was sent back for a redo", via, now);
        Ok(())
    }

    /// Sends every step that comes after `id`, directly or through other
    /// steps, and is past todo back to redo, with a decision saying why.
    fn cascade(&mut self, id: &str, why: &str, via: Via, now: Timestamp) {
        let mut later = vec![id.to_string()];
        let mut grew = true;
        while grew {
            grew = false;
            for s in &self.steps {
                if !later.contains(&s.id) && s.after.iter().any(|a| later.contains(a)) {
                    later.push(s.id.clone());
                    grew = true;
                }
            }
        }
        let note = format!("{id} {why}, and this step comes after it");
        for s in self.steps.iter_mut() {
            if later[1..].contains(&s.id) && !matches!(s.state, State::Todo | State::Redo) {
                s.state = State::Redo;
                self.decisions.push(Decision {
                    step: s.id.clone(),
                    round: s.round,
                    decision: Verdict::Redo,
                    at: now,
                    via,
                    note: Some(note.clone()),
                    rest: Default::default(),
                });
            }
        }
    }

    fn decide(
        &mut self,
        step: &str,
        round: u32,
        decision: Verdict,
        note: Option<String>,
        via: Via,
        at: Timestamp,
    ) {
        self.decisions.push(Decision {
            step: step.to_string(),
            round,
            decision,
            at,
            via,
            note,
            rest: Default::default(),
        });
    }

    pub fn note_add(
        &mut self,
        step: &str,
        text: &str,
        path: Option<String>,
        at: Option<f64>,
        now: Timestamp,
    ) -> Result<&Note> {
        if text.trim().is_empty() {
            return Err("a note needs text".into());
        }
        if at.is_some_and(|t| !t.is_finite() || t < 0.0) {
            return Err("at must be a number of seconds, 0 or more".into());
        }
        if at.is_some() && path.is_none() {
            return Err("at is a time in a file, so it needs a path".into());
        }
        let round = self.step(step)?.round;
        let id = self.next_note;
        self.next_note += 1;
        self.notes.push(Note {
            id,
            step: step.to_string(),
            path,
            at,
            text: text.to_string(),
            created: now,
            round,
            state: NoteState::Open,
            reply: None,
            rest: Default::default(),
        });
        Ok(self.notes.last().unwrap())
    }

    pub fn note_done(&mut self, id: u64, reply: Option<String>) -> Result<&Note> {
        let note = self
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("no note {id}"))?;
        note.state = NoteState::Done;
        if reply.is_some() {
            note.reply = reply;
        }
        Ok(note)
    }

    pub fn last_decision(&self, step: &str) -> Option<&Decision> {
        self.decisions.iter().rev().find(|d| d.step == step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use State::*;

    fn now() -> Timestamp {
        Timestamp::now()
    }

    fn out() -> Vec<Output> {
        vec![Output {
            path: "x/x.md".into(),
            kind: Kind::Text,
            rest: Default::default(),
        }]
    }

    fn state(p: &Project, id: &str) -> State {
        p.step(id).unwrap().state
    }

    /// A project with research approved, so script and the steps after it
    /// can be submitted.
    fn project() -> Project {
        let mut p = Project::new("T", None, "explainer", now()).unwrap();
        p.submit("research", out(), None, Via::Cli, now()).unwrap();
        p.approve("research", None, Via::Cli, now()).unwrap();
        p
    }

    #[test]
    fn new_project_starts_every_step_in_todo() {
        let p = Project::new("T", None, "explainer", now()).unwrap();
        assert_eq!(p.steps.len(), 12);
        assert!(p.steps.iter().all(|s| s.state == Todo && s.round == 0));
        assert!(p.step("thumbnail").is_ok() && p.step("description").is_ok());
        assert!(Project::new("T", None, "nope", now()).is_err());
        assert!(Project::new(" ", None, "explainer", now()).is_err());
    }

    #[test]
    fn todo_to_review_on_submit_with_a_gate() {
        let mut p = project();
        p.submit("script", out(), Some("first draft".into()), Via::Cli, now())
            .unwrap();
        let s = p.step("script").unwrap();
        assert_eq!((s.state, s.round), (Review, 1));
        assert_eq!(s.summary.as_deref(), Some("first draft"));
    }

    #[test]
    fn todo_to_done_on_submit_without_a_gate() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.approve("script", None, Via::Cli, now()).unwrap();
        p.submit("assets", out(), None, Via::Cli, now()).unwrap();
        assert_eq!(state(&p, "assets"), Done);
    }

    #[test]
    fn review_to_approved() {
        let p = project();
        assert_eq!(state(&p, "research"), Approved);
        let d = p.last_decision("research").unwrap();
        assert_eq!(
            (d.decision, d.via, d.round),
            (Verdict::Approved, Via::Cli, 1)
        );
    }

    #[test]
    fn review_to_redo_needs_a_note() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        assert!(p.redo("script", "  ", Via::Mcp, now()).is_err());
        p.redo("script", "slow open", Via::Mcp, now()).unwrap();
        assert_eq!(state(&p, "script"), Redo);
        let d = p.last_decision("script").unwrap();
        assert_eq!(
            (d.decision, d.via, d.note.as_deref()),
            (Verdict::Redo, Via::Mcp, Some("slow open"))
        );
    }

    #[test]
    fn approved_to_redo() {
        let mut p = project();
        p.redo("research", "wrong topic", Via::Window, now())
            .unwrap();
        assert_eq!(state(&p, "research"), Redo);
    }

    #[test]
    fn done_to_redo() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.approve("script", None, Via::Cli, now()).unwrap();
        p.submit("assets", out(), None, Via::Cli, now()).unwrap();
        p.redo("assets", "missing logo", Via::Cli, now()).unwrap();
        assert_eq!(state(&p, "assets"), Redo);
    }

    #[test]
    fn redo_to_review_on_submit_with_a_gate() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.redo("script", "again", Via::Cli, now()).unwrap();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        let s = p.step("script").unwrap();
        assert_eq!((s.state, s.round), (Review, 2));
    }

    #[test]
    fn redo_to_done_on_submit_without_a_gate() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.approve("script", None, Via::Cli, now()).unwrap();
        p.submit("assets", out(), None, Via::Cli, now()).unwrap();
        p.redo("assets", "again", Via::Cli, now()).unwrap();
        p.submit("assets", out(), None, Via::Cli, now()).unwrap();
        let s = p.step("assets").unwrap();
        assert_eq!((s.state, s.round), (Done, 2));
    }

    #[test]
    fn approved_to_review_on_submit_again() {
        let mut p = project();
        p.submit("research", out(), None, Via::Cli, now()).unwrap();
        let s = p.step("research").unwrap();
        assert_eq!((s.state, s.round), (Review, 2));
    }

    #[test]
    fn a_redo_sends_every_later_step_past_todo_back_to_redo() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.approve("script", None, Via::Cli, now()).unwrap();
        p.submit("assets", out(), None, Via::Cli, now()).unwrap(); // done
        p.submit("voice", out(), None, Via::Cli, now()).unwrap(); // review
        let before = p.decisions.len();
        p.redo("research", "wrong topic", Via::Window, now())
            .unwrap();
        for id in ["research", "script", "assets", "voice"] {
            assert_eq!(state(&p, id), Redo, "{id}");
        }
        assert_eq!(state(&p, "look"), Todo);
        // research's own decision, then one for each later step.
        assert_eq!(p.decisions.len(), before + 4);
        let d = p.last_decision("voice").unwrap();
        assert_eq!(
            (d.decision, d.via, d.round),
            (Verdict::Redo, Via::Window, 1)
        );
        assert!(
            d.note
                .as_deref()
                .unwrap()
                .contains("research was sent back")
        );
    }

    #[test]
    fn resubmitting_an_approved_step_sends_later_steps_back_to_redo() {
        let mut p = project();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.submit("research", out(), None, Via::Mcp, now()).unwrap();
        assert_eq!(state(&p, "research"), Review);
        assert_eq!(state(&p, "script"), Redo);
        let d = p.last_decision("script").unwrap();
        assert_eq!(d.via, Via::Mcp);
        assert!(
            d.note
                .as_deref()
                .unwrap()
                .contains("research was submitted again")
        );
        // A step submitted from redo sends nothing back.
        let n = p.decisions.len();
        p.approve("research", None, Via::Cli, now()).unwrap();
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        assert_eq!(p.decisions.len(), n + 1);
    }

    #[test]
    fn submit_past_an_open_gate_is_refused() {
        let mut p = Project::new("T", None, "explainer", now()).unwrap();
        p.submit("research", out(), None, Via::Cli, now()).unwrap();
        // research is in review, not approved, so script waits.
        let err = p
            .submit("script", out(), None, Via::Cli, now())
            .unwrap_err();
        assert!(err.contains("waits for research"), "{err}");
        assert_eq!(state(&p, "script"), Todo);
        // upload waits for three steps.
        let err = p
            .submit("upload", out(), None, Via::Cli, now())
            .unwrap_err();
        assert!(err.contains("film, thumbnail, description"), "{err}");
    }

    #[test]
    fn moves_off_the_diagram_are_refused() {
        let mut p = project();
        assert!(p.approve("script", None, Via::Cli, now()).is_err()); // todo
        assert!(p.redo("script", "x", Via::Cli, now()).is_err()); // todo
        assert!(p.approve("research", None, Via::Cli, now()).is_err()); // approved
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        assert!(p.submit("script", out(), None, Via::Cli, now()).is_err()); // review
        p.redo("script", "x", Via::Cli, now()).unwrap();
        assert!(p.approve("script", None, Via::Cli, now()).is_err()); // redo
        assert!(p.redo("script", "x", Via::Cli, now()).is_err()); // redo
        p.submit("script", out(), None, Via::Cli, now()).unwrap();
        p.approve("script", None, Via::Cli, now()).unwrap();
        p.submit("assets", out(), None, Via::Cli, now()).unwrap();
        assert!(p.submit("assets", out(), None, Via::Cli, now()).is_err()); // done
        assert!(p.approve("assets", None, Via::Cli, now()).is_err()); // done
        assert!(p.submit("nope", out(), None, Via::Cli, now()).is_err());
    }

    #[test]
    fn notes_take_ids_in_order_and_close_with_a_reply() {
        let mut p = project();
        assert_eq!(
            p.note_add("research", "a", None, None, now()).unwrap().id,
            1
        );
        let n = p
            .note_add(
                "research",
                "b",
                Some("research/r.md".into()),
                Some(4.0),
                now(),
            )
            .unwrap();
        assert_eq!((n.id, n.round, n.at), (2, 1, Some(4.0)));
        assert!(
            p.note_add("research", "c", Some("r".into()), Some(-1.0), now())
                .is_err()
        );
        assert!(p.note_add("research", "c", None, Some(1.0), now()).is_err());
        assert!(p.note_add("nope", "c", None, None, now()).is_err());
        let n = p.note_done(2, Some("fixed".into())).unwrap();
        assert_eq!(
            (n.state, n.reply.as_deref()),
            (NoteState::Done, Some("fixed"))
        );
        assert!(p.note_done(9, None).is_err());
        assert_eq!(p.next_note, 3);
    }

    #[test]
    fn toml_round_trips_and_reads_a_bare_datetime() {
        let mut p = project();
        p.note_add("research", "n", Some("r.wav".into()), Some(1.5), now())
            .unwrap();
        let text = toml::to_string(&p).unwrap();
        assert_eq!(toml::from_str::<Project>(&text).unwrap(), p);

        let typed = "schema = 1\ntitle = \"T\"\nworkflow = \"explainer\"\n\
                     created = 2026-11-25T09:00:00Z\nnext_note = 1\n";
        let p: Project = toml::from_str(typed).unwrap();
        assert_eq!(p.created.to_string(), "2026-11-25T09:00:00Z");
    }
}
