//! The `youtube` job: posts a project whose upload step is approved.
//! `mockingbird-studio youtube-upload <project>` runs under the job runner:
//!
//! 1. trades the Keychain's refresh token for an access token,
//! 2. claims the post in `project.toml` and opens a resumable upload with
//!    the sheet and the visibility,
//! 3. sends the film in chunks, asking Google how much arrived when a chunk
//!    breaks and carrying on from there,
//! 4. turns the claim into the `[[post]]` row as soon as Google answers,
//! 5. sends the thumbnail and records whether it was set.
//!
//! The claim is what stops a second post: a run that finds a live one stops,
//! and a run that finds a dead job's claim asks that job's session how far
//! it got before starting anything new.
//!
//! It talks only to Google's token address and YouTube's API host, and the
//! upload session must stay on that host. The access token is never
//! printed or written: everything printed here goes to the job's log.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use crate::jobs::{self, Job, JobState};
use crate::niche::Visibility;
use crate::project::{Kind, Output, Post, Project, Result, State, Step, Verdict, Via};
use crate::sheet::{self, Sheet};
use crate::store::{self, Config, PROJECT_FILE};
use crate::youtube;

const STEP: &str = "upload";
const UPLOADING: &str = "uploading";
/// A multiple of 256 KiB, as the resumable protocol asks.
const CHUNK: u64 = 8 << 20;
/// Breaks in a row before the job gives up.
const TRIES: u64 = 5;
/// How long Google keeps a resumable session: a week.
const SESSION_LIFE: SignedDuration = SignedDuration::from_hours(7 * 24);

/// What a post needs, checked when the job is queued and again when it runs.
struct Plan {
    round: u32,
    /// The film, relative to the project, then its full path and size.
    file: String,
    video: PathBuf,
    size: u64,
    sheet: Sheet,
    thumbnail: PathBuf,
    label: String,
    visibility: Visibility,
}

/// Step `id` with its current round approved: the last decision on it is a
/// yes to that round, and nothing has moved it since.
fn approved<'a>(p: &'a Project, id: &str) -> Result<&'a Step> {
    let step = p.step(id)?;
    let yes = p.last_decision(id).filter(|d| {
        d.decision == Verdict::Approved
            && d.round == step.round
            && matches!(step.state, State::Approved | State::Done)
    });
    match yes {
        Some(_) => Ok(step),
        None => Err(format!(
            "the {id} step isn't approved, and nothing is posted without that"
        )),
    }
}

fn the_only(step: &Step, kind: Kind, what: &str) -> Result<Output> {
    match step
        .outputs
        .iter()
        .filter(|o| o.kind == kind)
        .collect::<Vec<_>>()[..]
    {
        [o] => Ok(o.clone()),
        _ => Err(format!("the {} step needs exactly one {what}", step.id)),
    }
}

/// A claim whose job may still be sending: it's recent and its job is
/// queued or running.
fn live(claim: &Post) -> bool {
    Timestamp::now().duration_since(claim.at) < SESSION_LIFE
        && claim
            .job
            .as_deref()
            .and_then(|id| jobs::read(id).ok())
            .is_some_and(|j| matches!(j.state, JobState::Queued | JobState::Running))
}

/// Refuses a project that's posted or being posted. `again` lets a posted
/// project post once more; a live claim stops it all the same.
fn check_unposted(p: &Project, again: bool) -> Result<()> {
    for post in &p.posts {
        if post.state.is_none() && !again {
            return Err(format!(
                "this project is already posted: {}. job_start with again posts it once more",
                post.url
            ));
        }
        if live(post) {
            return Err(format!(
                "job {} is already uploading this project",
                post.job.as_deref().unwrap_or_default()
            ));
        }
    }
    Ok(())
}

/// Refuses a change to the upload step while a job is sending it.
pub fn check_idle(p: &Project) -> Result<()> {
    match p.posts.iter().find(|c| live(c)) {
        Some(c) => Err(format!(
            "job {} is uploading this project now. Wait for it to finish, or cancel it, before changing the upload step",
            c.job.as_deref().unwrap_or_default()
        )),
        None => Ok(()),
    }
}

/// The channel label a project posts to and the visibility it asks for.
/// `private` is a second post asked for through MCP.
fn target(p: &Project, config: &Config, private: bool) -> Result<(String, Visibility)> {
    let niche = p
        .niche
        .as_ref()
        .and_then(|id| config.niche.iter().find(|n| &n.id == id));
    let label = match (niche.and_then(|n| n.channel.clone()), &config.channel[..]) {
        (Some(label), _) => label,
        (None, [only]) => only.label.clone(),
        (None, []) => return Err("no channel is signed in. Sign in with youtube_connect".into()),
        (None, _) => {
            return Err("there's more than one channel; set the niche's channel".into());
        }
    };
    // PRD F5: an approval of this round that came through MCP posts private.
    let round = p.step(STEP)?.round;
    let by_mcp = p
        .last_decision(STEP)
        .is_some_and(|d| d.via == Via::Mcp && d.decision == Verdict::Approved && d.round == round);
    let visibility = match niche {
        Some(n) if !by_mcp && !private => n.visibility,
        _ => Visibility::Private,
    };
    Ok((label, visibility))
}

fn not_signed_in(label: &str) -> String {
    format!("{label} isn't signed in. Sign in with youtube_connect")
}

fn plan(dir: &Path, again: bool, private: bool) -> Result<Plan> {
    let p: Project = store::read(&dir.join(PROJECT_FILE))?;
    check_unposted(&p, again)?;
    let step = approved(&p, STEP)?;
    let video = the_only(step, Kind::Video, "video")?;
    let thumbnail = the_only(approved(&p, "thumbnail")?, Kind::Image, "image")?;
    let (sheet, _) = sheet::submitted(dir, approved(&p, "description")?.round)?;
    let (label, visibility) = target(&p, &store::read_config()?, private)?;
    if !youtube::signed_in(&label)? {
        return Err(not_signed_in(&label));
    }
    let path = dir.join(&video.path);
    let size = fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if size == 0 {
        return Err(format!("{} is empty", path.display()));
    }
    Ok(Plan {
        round: step.round,
        file: video.path,
        video: path,
        size,
        sheet,
        thumbnail: dir.join(&thumbnail.path),
        label,
        visibility,
    })
}

/// The upload step's part of `status`: the posts, where the next one goes
/// and as what, and why an approved step has nothing posting.
/// It reads config.toml, not the Keychain: a channel listed there is one
/// youtube_connect signed in and youtube_disconnect hasn't taken out.
pub fn status(p: &Project, jobs: &[Job]) -> Value {
    let posts: Vec<Value> = p
        .posts
        .iter()
        .map(|post| {
            let mut v = json!(post);
            v["live"] = live(post).into();
            v
        })
        .collect();
    let mut v = json!({"posts": posts});
    let error = match store::read_config().and_then(|c| Ok((target(p, &c, false)?, c))) {
        Ok(((label, visibility), config)) => {
            let signed_in = config.channel.iter().any(|c| c.label == label);
            v["visibility"] = json!(visibility);
            v["signed_in"] = signed_in.into();
            v["channel"] = label.as_str().into();
            (!signed_in).then(|| not_signed_in(&label))
        }
        Err(e) => Some(e),
    };
    if let Some(e) = &error {
        v["error"] = e.as_str().into();
    }
    let busy = jobs
        .iter()
        .any(|j| j.kind == "youtube" && matches!(j.state, JobState::Queued | JobState::Running))
        || p.posts.iter().any(live);
    let posted = p.posts.iter().any(|x| x.state.is_none());
    let stopped = p.posts.iter().any(|x| x.state.is_some());
    let waiting = if busy {
        None
    } else if stopped {
        let again = if posted { " with again" } else { "" };
        Some(format!(
            "An upload stopped before it finished. Start the youtube job{again}: it asks YouTube how far that upload got before sending anything."
        ))
    } else if !posted && approved(p, STEP).is_ok() {
        Some(
            error.unwrap_or_else(|| {
                "Nothing is posting. Start it with job_start, kind youtube.".into()
            }),
        )
    } else {
        None
    };
    if let Some(w) = waiting {
        v["waiting"] = w.into();
    }
    v
}

/// Queues a `youtube` job for the project in `dir`, which is canonical.
/// `again` posts a project that's already posted; only a person asks for it.
/// `private` asks for private whatever the niche says.
pub fn start(dir: PathBuf, again: bool, private: bool) -> Result<Job> {
    plan(&dir, again, private)?;
    let busy = jobs::list(Some(&dir))
        .into_iter()
        .any(|j| j.kind == "youtube" && matches!(j.state, JobState::Queued | JobState::Running));
    if busy {
        return Err("a youtube job for this project is already queued or running".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("can't find this program: {e}"))?;
    let mut command = vec![
        exe.to_string_lossy().into_owned(),
        "youtube-upload".into(),
        dir.to_string_lossy().into_owned(),
    ];
    if again {
        command.push("--again".into());
    }
    if private {
        command.push("--private".into());
    }
    jobs::queue(
        "youtube",
        "upload",
        command,
        Some(dir),
        None,
        Default::default(),
        None,
    )
}

/// `mockingbird-studio youtube-upload <project> [--again] [--private]`.
pub fn run(args: &[String]) -> ExitCode {
    let flags = args.get(1..).unwrap_or_default();
    let (Some(dir), true) = (
        args.first(),
        flags.iter().all(|f| f == "--again" || f == "--private"),
    ) else {
        eprintln!("usage: mockingbird-studio youtube-upload <project> [--again] [--private]");
        return ExitCode::from(2);
    };
    let has = |f: &str| flags.iter().any(|a| a == f);
    match upload(Path::new(dir), has("--again"), has("--private")) {
        Ok(lines) => {
            println!("{lines}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("youtube: {e}");
            ExitCode::FAILURE
        }
    }
}

fn name(v: Visibility) -> String {
    json!(v).as_str().unwrap_or_default().to_string()
}

/// What a run does about a dead job's claim.
enum Prior {
    /// Nothing to ask about: open a new session.
    Fresh,
    /// The same file for the same round: carry on in its session.
    Resume(String),
    /// Another file or round was being sent: ask that session once whether
    /// it finished, with the size it was sending and the visibility it asked.
    Ask(String, Option<u64>, Visibility),
}

fn fresh(plan: &Plan, job: &Option<String>) -> Post {
    Post {
        step: STEP.into(),
        round: plan.round,
        at: Timestamp::now(),
        job: job.clone(),
        state: Some(UPLOADING.into()),
        session: None,
        file: Some(plan.file.clone()),
        size: Some(plan.size),
        video: String::new(),
        url: String::new(),
        channel: plan.label.clone(),
        visibility_asked: plan.visibility,
        visibility_set: String::new(),
        thumbnail: None,
        rest: Default::default(),
    }
}

/// Claims the post under the project's lock. A dead job's claim is taken
/// over; its session is resumed only for the same round and file.
fn claim(dir: &Path, plan: &Plan, job: &Option<String>, again: bool) -> Result<Prior> {
    store::update_project(dir, |p| {
        check_unposted(p, again)?;
        let Some(i) = p.posts.iter().position(|c| c.state.is_some()) else {
            p.posts.push(fresh(plan, job));
            return Ok(Prior::Fresh);
        };
        let c = &mut p.posts[i];
        let same = c.round == plan.round
            && c.size == Some(plan.size)
            && c.file.as_deref() == Some(plan.file.as_str());
        // The token goes only to YouTube's host, whatever project.toml says.
        let ours = |s: &String| s.starts_with(&format!("{}/", youtube::api()));
        Ok(match c.session.clone().filter(ours) {
            Some(s) if same => {
                c.job = job.clone();
                c.at = Timestamp::now();
                Prior::Resume(s)
            }
            Some(s) => {
                c.job = job.clone();
                c.at = Timestamp::now();
                Prior::Ask(s, c.size, c.visibility_asked)
            }
            None => {
                *c = fresh(plan, job);
                Prior::Fresh
            }
        })
    })
}

/// One question to an earlier session: did its upload finish? Returns the
/// video Google made if it did.
fn ask(agent: &ureq::Agent, auth: &str, session: &str, size: Option<u64>) -> Option<Value> {
    let total = size.map_or("*".to_string(), |s| s.to_string());
    let r = agent
        .put(session)
        .header("Authorization", auth)
        .header("Content-Range", format!("bytes */{total}"))
        .send(&[][..])
        .ok()?;
    match read_reply(r) {
        (200 | 201, _, body) => serde_json::from_str(&body).ok(),
        _ => None,
    }
}

fn with_claim(dir: &Path, f: impl FnOnce(&mut Post)) -> Result<()> {
    store::update_project(dir, |p| {
        let c = p
            .posts
            .iter_mut()
            .find(|c| c.state.is_some())
            .ok_or("the claim on this post is gone from project.toml")?;
        f(c);
        Ok(())
    })
}

fn upload(dir: &Path, again: bool, private: bool) -> Result<String> {
    let plan = plan(dir, again, private)?;
    let size = plan.size;
    let token = youtube::access_token(&plan.label)?;
    let auth = format!("Bearer {token}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        // Google's "308 Resume Incomplete" is an answer, not a redirect.
        .max_redirects(0)
        // ponytail: one chunk must finish within this; a smaller CHUNK if
        // slow uplinks time out.
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let job = std::env::var("MOCKINGBIRD_STUDIO_JOB").ok();
    let mut asked = plan.visibility;
    let (mut session, mut found) = (None, None);
    match claim(dir, &plan, &job, again)? {
        Prior::Fresh => {}
        Prior::Resume(s) => {
            eprintln!("an earlier job left this upload unfinished; asking YouTube how far it got");
            session = Some(s);
        }
        Prior::Ask(s, old, visibility) => {
            eprintln!(
                "an earlier job was sending another round; asking YouTube whether that upload finished"
            );
            found = ask(&agent, &auth, &s, old);
            if found.is_some() {
                eprintln!("it did, so that video is the post and nothing more is sent");
                asked = visibility;
            } else {
                with_claim(dir, |c| *c = fresh(&plan, &job))?;
            }
        }
    }
    let mut resuming = session.is_some();
    let video = if let Some(v) = found {
        v
    } else {
        loop {
            let at = match session.take() {
                Some(s) => s,
                None => {
                    let s = open(&agent, &auth, &plan, size)?;
                    with_claim(dir, |c| c.session = Some(s.clone()))?;
                    s
                }
            };
            match send(&agent, &auth, &at, &plan.video, size, resuming)? {
                Some(v) => break v,
                None if resuming => {
                    eprintln!("the earlier upload session is gone; starting over");
                    resuming = false;
                }
                None => return Err("YouTube dropped the upload session. Run the job again".into()),
            }
        }
    };

    let id = video["id"].as_str().unwrap_or_default().to_string();
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("the upload finished but Google's reply has no video id".into());
    }
    let url = format!("https://youtu.be/{id}");
    let set = video["status"]["privacyStatus"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    with_claim(dir, |c| {
        c.at = Timestamp::now();
        c.job = job.clone();
        c.state = None;
        c.session = None;
        c.file = None;
        c.size = None;
        c.video = id.clone();
        c.url = url.clone();
        c.visibility_set = set.clone();
    })
    .map_err(|e| {
        format!(
            "{url} is up, but its post row couldn't be written: {e}. Run the job again to record it"
        )
    })?;
    let mut lines = vec![format!("Posted {url} to {} as {set}.", plan.label)];
    if set != name(asked) {
        lines.push(format!(
            "Google set it to {set}, not {}. YouTube locks videos from an API project that hasn't passed its audit to private.",
            name(asked)
        ));
    }
    remember_channel(&plan.label, &video);

    let (state, why) = thumbnail(&agent, &auth, &id, &plan.thumbnail);
    match (state, why) {
        ("refused", Some(why)) => lines.push(format!(
            "YouTube refused the thumbnail: {why}. Custom thumbnails need a verified channel."
        )),
        (_, Some(why)) => lines.push(format!("The thumbnail wasn't set: {why}")),
        _ => {}
    }
    store::update_project(dir, |p| {
        if let Some(post) = p.posts.iter_mut().find(|p| p.video == id) {
            post.thumbnail = Some(state.into());
        }
        Ok(())
    })
    .map_err(|e| format!("{url} is up, but the thumbnail's result couldn't be written: {e}"))?;
    Ok(lines.join("\n"))
}

/// Fills in the channel's id and title from the reply, the one place the
/// upload scope learns them.
fn remember_channel(label: &str, video: &Value) {
    let snippet = &video["snippet"];
    let (Some(id), title) = (
        snippet["channelId"].as_str(),
        snippet["channelTitle"].as_str(),
    ) else {
        return;
    };
    let r = store::update_config(|c| {
        if let Some(ch) = c.channel.iter_mut().find(|ch| ch.label == label) {
            ch.id = Some(id.to_string());
            ch.title = title.map(str::to_string).or(ch.title.take());
        }
        Ok(())
    });
    if let Err(e) = r {
        eprintln!("can't note the channel's name in config.toml: {e}");
    }
}

/// Google's words for a refusal: its message and reason, never the body.
fn refusal(status: u16, body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let message = v["error"]["message"].as_str().unwrap_or_default();
    let reason = v["error"]["errors"][0]["reason"]
        .as_str()
        .unwrap_or_default();
    match reason {
        "quotaExceeded" | "dailyLimitExceeded" => format!(
            "YouTube's daily quota for your API project is spent ({reason}: {message}). It resets at midnight Pacific time; run the job again after that"
        ),
        "uploadLimitExceeded" => format!(
            "the channel has reached YouTube's upload limit ({message}). Run the job again later"
        ),
        _ if status == 401 => format!(
            "Google turned the sign-in down (401: {message}). Sign in again with youtube_connect, then run the job again"
        ),
        _ => format!("YouTube answered {status}: {message} {reason}")
            .trim_end()
            .to_string(),
    }
}

fn read_reply(mut r: ureq::http::Response<ureq::Body>) -> (u16, Option<String>, String) {
    let range = r
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = r.body_mut().read_to_string().unwrap_or_default();
    (r.status().as_u16(), range, body)
}

/// Opens the resumable session and returns its address.
fn open(agent: &ureq::Agent, auth: &str, plan: &Plan, size: u64) -> Result<String> {
    let s = &plan.sheet;
    let meta = json!({
        "snippet": {
            "title": s.title,
            "description": s.full_description(),
            "tags": s.tags,
            "categoryId": s.category.to_string(),
        },
        "status": {
            "privacyStatus": plan.visibility,
            "selfDeclaredMadeForKids": s.made_for_kids,
            "containsSyntheticMedia": s.altered_content,
        },
    });
    let base = youtube::api();
    let r = agent
        .post(format!(
            "{base}/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status"
        ))
        .header("Authorization", auth)
        .header("Content-Type", "application/json; charset=UTF-8")
        .header("X-Upload-Content-Length", size.to_string())
        .header("X-Upload-Content-Type", "video/*")
        .send(meta.to_string())
        .map_err(|e| format!("can't reach YouTube: {e}"))?;
    let session = r
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let (status, _, body) = read_reply(r);
    if !(200..300).contains(&status) {
        return Err(refusal(status, &body));
    }
    let session = session.ok_or("YouTube opened no upload session")?;
    // The film goes to Google's host and nowhere else.
    if !session.starts_with(&format!("{base}/")) {
        return Err("YouTube's upload session is on another host, so nothing was sent".into());
    }
    Ok(session)
}

/// Sends the file from wherever Google says it has got to, and returns the
/// video Google made, or `None` when the session is gone (404 or 410).
/// `asking` starts by asking how much arrived, to resume an earlier job's
/// session. A chunk that breaks (no answer, a 5xx, or a 308 that took none
/// of it) is followed by asking; `TRIES` breaks without progress give up.
fn send(
    agent: &ureq::Agent,
    auth: &str,
    session: &str,
    path: &Path,
    size: u64,
    mut asking: bool,
) -> Result<Option<Value>> {
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = File::open(path).map_err(io)?;
    let (mut from, mut breaks) = (0u64, 0u64);
    loop {
        let (range, chunk) = if asking {
            (format!("bytes */{size}"), vec![])
        } else {
            let len = CHUNK.min(size - from);
            let mut chunk = vec![0; len as usize];
            file.seek(SeekFrom::Start(from))
                .and_then(|_| file.read_exact(&mut chunk))
                .map_err(io)?;
            (format!("bytes {from}-{}/{size}", from + len - 1), chunk)
        };
        let reply = agent
            .put(session)
            .header("Authorization", auth)
            .header("Content-Range", &range)
            .send(&chunk[..])
            .map(read_reply);
        let why = match reply {
            Ok((200 | 201, _, body)) => {
                return serde_json::from_str(&body)
                    .map(Some)
                    .map_err(|_| "the upload finished but Google's reply isn't JSON".into());
            }
            Ok((404 | 410, _, _)) => return Ok(None),
            Ok((308, range, _)) => {
                // `bytes=0-N` is what has arrived; none means nothing has.
                let at = range
                    .and_then(|r| r.rsplit('-').next()?.parse::<u64>().ok())
                    .map_or(0, |end| end + 1)
                    .min(size);
                let progressed = at > from;
                let was_asking = std::mem::replace(&mut asking, false);
                from = at;
                if progressed {
                    breaks = 0;
                }
                if progressed || was_asking {
                    continue;
                }
                "YouTube took none of the chunk".to_string()
            }
            // ponytail: an access token lasts an hour, so an upload longer
            // than that ends here with a 401. Refresh it here if films get that big.
            Ok((status, _, body)) if status < 500 => return Err(refusal(status, &body)),
            Ok((status, _, _)) => format!("YouTube answered {status}"),
            Err(e) => e.to_string(),
        };
        breaks += 1;
        if breaks >= TRIES {
            return Err(format!(
                "the upload broke {TRIES} times in a row at byte {from} of {size} ({why}). Run the job again"
            ));
        }
        eprintln!(
            "the upload broke at byte {from} of {size} ({why}); asking YouTube how much arrived"
        );
        std::thread::sleep(Duration::from_secs(breaks));
        asking = true;
    }
}

/// `thumbnails.set`. Returns set, refused (403: the channel isn't verified)
/// or failed, and why when it isn't set.
fn thumbnail(
    agent: &ureq::Agent,
    auth: &str,
    id: &str,
    path: &Path,
) -> (&'static str, Option<String>) {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => return ("failed", Some(format!("{}: {e}", path.display()))),
    };
    let png = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    let r = agent
        .post(format!(
            "{}/upload/youtube/v3/thumbnails/set?videoId={id}",
            youtube::api()
        ))
        .header("Authorization", auth)
        .header("Content-Type", if png { "image/png" } else { "image/jpeg" })
        .send(&bytes[..])
        .map(read_reply);
    match r {
        Ok((200..=299, _, _)) => ("set", None),
        Ok((403, _, body)) => ("refused", Some(refusal(403, &body))),
        Ok((status, _, body)) => ("failed", Some(refusal(status, &body))),
        Err(e) => ("failed", Some(format!("can't reach YouTube: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_refusals_read_as_what_to_do() {
        let quota = r#"{"error":{"code":403,"message":"The request cannot be completed because you have exceeded your quota.","errors":[{"reason":"quotaExceeded","domain":"youtube.quota"}]}}"#;
        let e = refusal(403, quota);
        assert!(
            e.contains("quota") && e.contains("run the job again"),
            "{e}"
        );
        assert!(refusal(401, "{}").contains("youtube_connect"));
        assert_eq!(refusal(404, "not json"), "YouTube answered 404:");
    }
}
