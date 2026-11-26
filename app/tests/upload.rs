//! The `youtube` job against a fake Google on 127.0.0.1 that speaks enough
//! of the token exchange, the resumable upload and `thumbnails.set`. The
//! Keychain is a folder (`MOCKINGBIRD_STUDIO_KEYCHAIN_DIR`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

mod common;

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");
const ACCESS: &str = "ya29.upload-access-token-never-shown";
const REFRESH: &str = "1//upload-refresh-token-never-shown";
/// More than one 8 MiB chunk, and not a multiple of 256 KiB.
const FILM: usize = (10 << 20) + 123;

#[derive(Default)]
struct Fake {
    // How this Google behaves.
    expired: bool,
    quota: bool,
    lock_private: bool,
    refuse_thumbnail: bool,
    break_once: bool,
    /// Every chunk is answered 308 with nothing taken.
    stall: bool,
    /// Made read-only once the film has all arrived, so the post row can't
    /// be written.
    lock_on_done: Option<PathBuf>,
    // What it saw.
    base: String,
    meta: Value,
    got: Vec<u8>,
    breaks: u32,
    thumbnail: Vec<u8>,
    paths: Vec<String>,
    inserts: u32,
    /// Content-Range of each PUT to an earlier job's session, `upload_id=old`.
    old: Vec<String>,
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
}

fn read_head(r: &mut BufReader<&TcpStream>) -> Request {
    let mut first = String::new();
    r.read_line(&mut first).unwrap();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        if h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Request {
        method,
        path,
        headers,
    }
}

fn reply(mut s: &TcpStream, status: &str, extra: &str, body: &str) {
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn handle(s: TcpStream, fake: &Mutex<Fake>) {
    let mut r = BufReader::new(&s);
    let req = read_head(&mut r);
    let len: usize = req
        .headers
        .get("content-length")
        .map_or(0, |v| v.parse().unwrap());
    let mut f = fake.lock().unwrap();
    f.paths.push(format!("{} {}", req.method, req.path));
    let path = req.path.split('?').next().unwrap().to_string();
    if path != "/token" {
        assert_eq!(req.headers["authorization"], format!("Bearer {ACCESS}"));
    }
    // A chunk that breaks: half of it arrives, then the connection drops.
    if req.method == "PUT"
        && f.break_once
        && f.breaks == 0
        && !req.headers["content-range"].contains('*')
    {
        let mut half = vec![0; len / 2];
        r.read_exact(&mut half).unwrap();
        f.got.extend_from_slice(&half);
        f.breaks += 1;
        let _ = s.shutdown(Shutdown::Both);
        return;
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    // An earlier job's session that never finished.
    if req.path.contains("upload_id=old") {
        f.old.push(req.headers["content-range"].clone());
        reply(&s, "308 Resume Incomplete", "", "");
        return;
    }
    match (req.method.as_str(), path.as_str()) {
        ("POST", "/token") => {
            let form: HashMap<String, String> =
                form_urlencoded::parse(&body).into_owned().collect();
            assert_eq!(form["grant_type"], "refresh_token");
            assert_eq!(form["refresh_token"], REFRESH);
            if f.expired {
                let e = json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."});
                reply(&s, "400 Bad Request", "", &e.to_string());
            } else {
                let t = json!({"access_token": ACCESS, "expires_in": 3599, "token_type": "Bearer"});
                reply(&s, "200 OK", "", &t.to_string());
            }
        }
        ("POST", "/upload/youtube/v3/videos") => {
            assert!(req.path.contains("uploadType=resumable"), "{}", req.path);
            assert_eq!(req.headers["x-upload-content-length"], FILM.to_string());
            if f.quota {
                let e = json!({"error": {"code": 403, "message": "The request cannot be completed because you have exceeded your quota.", "errors": [{"reason": "quotaExceeded", "domain": "youtube.quota"}]}});
                reply(&s, "403 Forbidden", "", &e.to_string());
                return;
            }
            f.inserts += 1;
            f.got.clear();
            f.meta = serde_json::from_slice(&body).unwrap();
            let at = format!(
                "Location: {}/upload/youtube/v3/videos?uploadType=resumable&upload_id=s1\r\n",
                f.base
            );
            reply(&s, "200 OK", &at, "");
        }
        ("PUT", "/upload/youtube/v3/videos") => {
            let range = req.headers["content-range"].clone();
            let (span, total) = range.trim_start_matches("bytes ").split_once('/').unwrap();
            assert_eq!(total, FILM.to_string());
            if span != "*" && !f.stall {
                let start: usize = span.split('-').next().unwrap().parse().unwrap();
                assert_eq!(start, f.got.len(), "a chunk starts where Google left off");
                f.got.extend_from_slice(&body);
                if f.got.len() == FILM
                    && let Some(dir) = f.lock_on_done.take()
                {
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
                }
            }
            if f.got.len() == FILM {
                let asked = f.meta["status"]["privacyStatus"].as_str().unwrap();
                let set = if f.lock_private { "private" } else { asked };
                let v = json!({"id": "vid_123-A",
                    "snippet": {"channelId": "UCtest", "channelTitle": "Test channel"},
                    "status": {"privacyStatus": set}});
                reply(&s, "200 OK", "", &v.to_string());
            } else {
                let has = match f.got.len() {
                    0 => String::new(),
                    n => format!("Range: bytes=0-{}\r\n", n - 1),
                };
                reply(&s, "308 Resume Incomplete", &has, "");
            }
        }
        ("POST", "/upload/youtube/v3/thumbnails/set") => {
            assert!(req.path.contains("videoId=vid_123-A"), "{}", req.path);
            if f.refuse_thumbnail {
                let e = json!({"error": {"code": 403, "message": "The authenticated user doesn't have permissions to upload and set custom video thumbnails.", "errors": [{"reason": "forbidden"}]}});
                reply(&s, "403 Forbidden", "", &e.to_string());
            } else {
                f.thumbnail = body;
                reply(&s, "200 OK", "", "{}");
            }
        }
        _ => reply(&s, "404 Not Found", "", "{}"),
    }
}

fn film() -> Vec<u8> {
    (0..FILM).map(|i| (i * 7 % 251) as u8).collect()
}

struct World {
    home: tempfile::TempDir,
    keychain: tempfile::TempDir,
    project: PathBuf,
    fake: Arc<Mutex<Fake>>,
    _root: tempfile::TempDir,
}

impl World {
    fn new(setup: impl FnOnce(&mut Fake)) -> World {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut f = Fake {
            base: format!("http://{}", listener.local_addr().unwrap()),
            ..Fake::default()
        };
        setup(&mut f);
        let fake = Arc::new(Mutex::new(f));
        let fake2 = fake.clone();
        std::thread::spawn(move || {
            for s in listener.incoming() {
                handle(s.unwrap(), &fake2);
            }
        });

        let home = tempfile::tempdir().unwrap();
        let keychain = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let project = root.path().canonicalize().unwrap().join("film");
        std::fs::write(
            home.path().join("config.toml"),
            "projects = []\n\n[[niche]]\nid = \"n\"\nname = \"N\"\nvisibility = \"public\"\nchannel = \"main\"\n\n[[channel]]\nlabel = \"main\"\n",
        )
        .unwrap();
        let secret =
            json!({"client_id": "id", "client_secret": "secret", "refresh_token": REFRESH});
        std::fs::write(keychain.path().join("main"), secret.to_string()).unwrap();
        for d in ["thumbnail", "description", "upload"] {
            std::fs::create_dir_all(project.join(d)).unwrap();
        }
        std::fs::write(project.join("upload/film.mp4"), film()).unwrap();
        std::fs::write(project.join("thumbnail/thumbnail.png"), b"\x89PNG stand-in").unwrap();
        std::fs::write(
            project.join("description/sheet.toml"),
            r#"title = "A synthetic film"
description = "Made by a test."
tags = ["test", "two words"]
category = 28
made_for_kids = false
altered_content = true

[[chapter]]
at = 0
title = "Start"

[[chapter]]
at = 10
title = "Middle"

[[chapter]]
at = 20
title = "End"
"#,
        )
        .unwrap();
        std::fs::write(
            project.join("project.toml"),
            r#"schema = 1
title = "A synthetic film"
niche = "n"
workflow = "explainer"
created = "2026-11-26T09:00:00Z"
next_note = 1

[[step]]
id = "thumbnail"
gate = true
state = "approved"
round = 1
[[step.output]]
path = "thumbnail/thumbnail.png"
kind = "image"

[[step]]
id = "description"
gate = true
state = "approved"
round = 1
[[step.output]]
path = "description/sheet.toml"
kind = "text"

[[step]]
id = "upload"
gate = true
after = ["thumbnail", "description"]
state = "approved"
round = 1
[[step.output]]
path = "upload/film.mp4"
kind = "video"

[[decision]]
step = "thumbnail"
round = 1
decision = "approved"
at = "2026-11-26T09:30:00Z"
via = "window"

[[decision]]
step = "description"
round = 1
decision = "approved"
at = "2026-11-26T09:40:00Z"
via = "window"

[[decision]]
step = "upload"
round = 1
decision = "approved"
at = "2026-11-26T10:00:00Z"
via = "window"
"#,
        )
        .unwrap();
        World {
            home,
            keychain,
            project,
            fake,
            _root: root,
        }
    }

    fn command(&self) -> Command {
        let mut c = Command::new(BIN);
        c.env("MOCKINGBIRD_STUDIO_HOME", self.home.path())
            .env("MOCKINGBIRD_STUDIO_KEYCHAIN_DIR", self.keychain.path())
            .env("MOCKINGBIRD_STUDIO_GOOGLE", &self.fake.lock().unwrap().base);
        c
    }

    /// Runs the job's program straight, as the runner would. Returns
    /// whether it worked and everything it printed.
    fn upload(&self) -> (bool, String) {
        let out = self
            .command()
            .arg("youtube-upload")
            .arg(&self.project)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    }

    fn cli(&self, args: &[&str]) -> (bool, String) {
        let out = self.command().args(args).arg("--json").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    }

    /// Waits until no job is queued or running and returns them all.
    fn jobs_end(&self) -> Vec<Value> {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let (_, out) = self.cli(&["jobs"]);
            let jobs = serde_json::from_str::<Value>(&out).unwrap()["jobs"]
                .as_array()
                .unwrap()
                .clone();
            if !jobs
                .iter()
                .any(|j| matches!(j["state"].as_str(), Some("queued" | "running")))
            {
                return jobs;
            }
            assert!(Instant::now() < end, "jobs didn't finish: {jobs:?}");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn project_toml(&self) -> String {
        std::fs::read_to_string(self.project.join("project.toml")).unwrap()
    }
}

/// Every file under `dir`, as text.
fn files(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else {
            let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).to_string();
            out.push((p.display().to_string(), text));
        }
    }
}

#[test]
fn a_clean_upload_through_the_runner_writes_one_post_and_no_token() {
    let w = World::new(|_| {});
    let dir = w.project.to_str().unwrap();
    let (ok, out) = w.cli(&["job-start", "--kind", "youtube", "--project", dir]);
    assert!(ok, "{out}");
    let mut seen = out.clone();
    let id = serde_json::from_str::<Value>(&out).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let end = Instant::now() + Duration::from_secs(30);
    let job = loop {
        let (_, out) = w.cli(&["job", "--id", &id]);
        let j: Value = serde_json::from_str(&out).unwrap();
        if !matches!(j["state"].as_str(), Some("queued" | "running")) {
            seen += &out;
            break j;
        }
        assert!(Instant::now() < end, "the job didn't finish: {j}");
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(job["state"], "done", "{job}");
    assert!(
        job["log"]
            .as_str()
            .unwrap()
            .contains("Posted https://youtu.be/vid_123-A")
    );

    let f = w.fake.lock().unwrap();
    assert!(f.got == film(), "the film arrived whole");
    assert_eq!(f.thumbnail, b"\x89PNG stand-in");
    assert_eq!(f.meta["snippet"]["title"], "A synthetic film");
    assert_eq!(
        f.meta["snippet"]["description"],
        "Made by a test.\n\n0:00 Start\n0:10 Middle\n0:20 End"
    );
    assert_eq!(f.meta["snippet"]["tags"], json!(["test", "two words"]));
    assert_eq!(f.meta["snippet"]["categoryId"], "28");
    assert_eq!(f.meta["status"]["privacyStatus"], "public");
    assert_eq!(f.meta["status"]["selfDeclaredMadeForKids"], false);
    assert_eq!(f.meta["status"]["containsSyntheticMedia"], true);
    drop(f);

    let p = w.project_toml();
    for line in [
        "[[post]]",
        "step = \"upload\"",
        &format!("job = \"{id}\""),
        "video = \"vid_123-A\"",
        "url = \"https://youtu.be/vid_123-A\"",
        "channel = \"main\"",
        "visibility_asked = \"public\"",
        "visibility_set = \"public\"",
        "thumbnail = \"set\"",
    ] {
        assert!(p.contains(line), "{line} in\n{p}");
    }
    let config = std::fs::read_to_string(w.home.path().join("config.toml")).unwrap();
    assert!(
        config.contains("id = \"UCtest\"") && config.contains("Test channel"),
        "{config}"
    );

    // A project posts once.
    let (ok, out) = w.cli(&["job-start", "--kind", "youtube", "--project", dir]);
    assert!(!ok && out.contains("already posted"), "{out}");
    seen += &out;

    let mut all = vec![];
    files(w.home.path(), &mut all);
    files(&w.project, &mut all);
    for token in [ACCESS, REFRESH] {
        assert!(!seen.contains(token), "a reply holds {token}");
        for (path, text) in &all {
            assert!(!text.contains(token), "{path} holds {token}");
        }
    }
}

#[test]
fn a_chunk_that_breaks_carries_on_from_what_arrived() {
    let w = World::new(|f| f.break_once = true);
    let (ok, out) = w.upload();
    assert!(ok, "{out}");
    assert!(out.contains("asking YouTube how much arrived"), "{out}");
    let f = w.fake.lock().unwrap();
    assert_eq!(f.breaks, 1);
    assert!(f.got == film(), "every byte once, in order");
    assert!(
        f.paths.iter().any(|p| p.starts_with("PUT")),
        "{:?}",
        f.paths
    );
}

#[test]
fn a_visibility_google_changed_is_recorded_not_failed() {
    let w = World::new(|f| f.lock_private = true);
    let (ok, out) = w.upload();
    assert!(ok, "{out}");
    assert!(
        out.contains("Google set it to private, not public"),
        "{out}"
    );
    let p = w.project_toml();
    assert!(p.contains("visibility_asked = \"public\""), "{p}");
    assert!(p.contains("visibility_set = \"private\""), "{p}");
}

#[test]
fn a_refused_thumbnail_leaves_the_video_up() {
    let w = World::new(|f| f.refuse_thumbnail = true);
    let (ok, out) = w.upload();
    assert!(ok, "{out}");
    assert!(out.contains("verified channel"), "{out}");
    let p = w.project_toml();
    assert!(p.contains("video = \"vid_123-A\""), "{p}");
    assert!(p.contains("thumbnail = \"refused\""), "{p}");
}

#[test]
fn an_expired_sign_in_fails_before_anything_is_sent() {
    let w = World::new(|f| f.expired = true);
    let (ok, out) = w.upload();
    assert!(!ok);
    assert!(
        out.contains("expired or was revoked") && out.contains("youtube_connect"),
        "{out}"
    );
    assert!(!w.project_toml().contains("[[post]]"));
    assert_eq!(w.fake.lock().unwrap().paths, vec!["POST /token"]);
}

#[test]
fn a_spent_quota_fails_with_googles_reason() {
    let w = World::new(|f| f.quota = true);
    let (ok, out) = w.upload();
    assert!(!ok);
    assert!(
        out.contains("quotaExceeded") && out.contains("run the job again"),
        "{out}"
    );
    // The claim stays, with no session, for the next run to take over.
    let p = w.project_toml();
    assert!(
        p.contains("state = \"uploading\"") && !p.contains("video ="),
        "{p}"
    );
    assert!(w.fake.lock().unwrap().got.is_empty());
}

#[test]
fn nothing_is_posted_without_the_upload_approval() {
    let w = World::new(|_| {});
    let p = w
        .project_toml()
        .replace("decision = \"approved\"", "decision = \"redo\"");
    std::fs::write(w.project.join("project.toml"), p).unwrap();
    let (ok, out) = w.cli(&[
        "job-start",
        "--kind",
        "youtube",
        "--project",
        w.project.to_str().unwrap(),
    ]);
    assert!(!ok && out.contains("isn't approved"), "{out}");
    let (ok, out) = w.upload();
    assert!(!ok && out.contains("isn't approved"), "{out}");
    assert!(w.fake.lock().unwrap().paths.is_empty());
}

#[test]
fn a_rerun_after_a_lost_post_row_records_the_video_without_a_second_upload() {
    let w = World::new(|_| {});
    w.fake.lock().unwrap().lock_on_done = Some(w.project.clone());
    let (ok, out) = w.upload();
    std::fs::set_permissions(&w.project, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!ok, "{out}");
    assert!(out.contains("https://youtu.be/vid_123-A is up"), "{out}");
    let p = w.project_toml();
    assert!(
        p.contains("state = \"uploading\"") && p.contains("session = "),
        "{p}"
    );

    let (ok, out) = w.upload();
    assert!(ok, "{out}");
    assert!(out.contains("asking YouTube how far it got"), "{out}");
    let f = w.fake.lock().unwrap();
    assert_eq!(f.inserts, 1, "one video, not two");
    assert!(f.got == film());
    let p = w.project_toml();
    assert_eq!(p.matches("[[post]]").count(), 1, "{p}");
    assert!(
        p.contains("video = \"vid_123-A\"") && p.contains("thumbnail = \"set\""),
        "{p}"
    );
    assert!(!p.contains("uploading") && !p.contains("session = "), "{p}");
}

#[test]
fn two_job_starts_at_once_make_one_upload() {
    let w = World::new(|_| {});
    let dir = w.project.to_str().unwrap();
    let starts: Vec<_> = (0..2)
        .map(|_| {
            w.command()
                .args(["job-start", "--kind", "youtube", "--project", dir, "--json"])
                .output()
        })
        .collect();
    assert!(starts.iter().any(|o| o.as_ref().unwrap().status.success()));
    let jobs = w.jobs_end();
    assert_eq!(
        jobs.iter().filter(|j| j["state"] == "done").count(),
        1,
        "{jobs:?}"
    );
    assert_eq!(w.fake.lock().unwrap().inserts, 1);
    assert_eq!(w.project_toml().matches("[[post]]").count(), 1);
}

#[test]
fn a_live_claim_stops_a_second_run() {
    let w = World::new(|_| {});
    let id = "20261126-100000-beef";
    let job_dir = w.home.path().join("jobs").join(id);
    std::fs::create_dir_all(&job_dir).unwrap();
    std::fs::write(
        job_dir.join("job.toml"),
        format!("id = \"{id}\"\nkind = \"youtube\"\ntool = \"upload\"\ncommand = []\nstate = \"running\"\ncreated = \"2026-11-26T10:00:00Z\"\n"),
    )
    .unwrap();
    let claim = format!(
        "\n[[post]]\nstep = \"upload\"\nround = 1\nat = \"{}\"\njob = \"{id}\"\nstate = \"uploading\"\nchannel = \"main\"\nvisibility_asked = \"public\"\n",
        jiff::Timestamp::now()
    );
    std::fs::write(w.project.join("project.toml"), w.project_toml() + &claim).unwrap();
    let dir = w.project.to_str().unwrap();
    let (ok, out) = w.cli(&["job-start", "--kind", "youtube", "--project", dir]);
    assert!(!ok && out.contains("already uploading"), "{out}");
    let (ok, out) = w.upload();
    assert!(!ok && out.contains("already uploading"), "{out}");
    // Nor can the step change under it.
    let (ok, out) = w.cli(&["redo", "--project", dir, "--step", "upload", "--note", "x"]);
    assert!(!ok && out.contains("uploading this project now"), "{out}");
    let (ok, out) = w.cli(&[
        "submit",
        "--project",
        dir,
        "--step",
        "upload",
        "--files",
        "upload/film.mp4",
    ]);
    assert!(!ok && out.contains("uploading this project now"), "{out}");
    assert!(w.fake.lock().unwrap().paths.is_empty());
}

#[test]
fn chunks_that_google_never_takes_give_up_after_five() {
    let w = World::new(|f| f.stall = true);
    let (ok, out) = w.upload();
    assert!(!ok);
    assert!(out.contains("broke 5 times in a row at byte 0"), "{out}");
    assert_eq!(out.matches("the upload broke at byte").count(), 4, "{out}");
}

impl World {
    /// The film, thumbnail and description approved, and the upload step
    /// waiting for its review.
    fn in_review(&self) {
        let p = self
            .project_toml()
            .replace(
                "[[step]]\nid = \"thumbnail\"",
                "[[step]]\nid = \"film\"\ngate = true\nstate = \"approved\"\nround = 1\n[[step.output]]\npath = \"upload/film.mp4\"\nkind = \"video\"\n\n[[step]]\nid = \"thumbnail\"",
            )
            .replace(
                "after = [\"thumbnail\", \"description\"]\nstate = \"approved\"",
                "after = [\"film\", \"thumbnail\", \"description\"]\nstate = \"review\"",
            )
            .replace(
                "step = \"upload\"\nround = 1\ndecision = \"approved\"\nat = \"2026-11-26T10:00:00Z\"",
                "step = \"film\"\nround = 1\ndecision = \"approved\"\nat = \"2026-11-26T09:20:00Z\"",
            );
        assert!(
            p.contains("id = \"film\"") && p.contains("\"review\""),
            "{p}"
        );
        std::fs::write(self.project.join("project.toml"), p).unwrap();
    }

    fn status(&self) -> Value {
        let (ok, out) = self.cli(&["status", "--project", self.project.to_str().unwrap()]);
        assert!(ok, "{out}");
        serde_json::from_str(&out).unwrap()
    }
}

#[test]
fn approving_the_upload_step_posts_once_and_again_only_when_asked() {
    let w = World::new(|_| {});
    w.in_review();
    let dir = w.project.to_str().unwrap();
    let u = &w.status()["upload"];
    assert_eq!(u["channel"], "main");
    assert_eq!(u["visibility"], "public");
    assert_eq!(u["signed_in"], true);

    let (ok, out) = w.cli(&["approve", "--project", dir, "--step", "upload"]);
    assert!(ok, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["youtube_job"]["kind"], "youtube", "{v}");
    let jobs = w.jobs_end();
    assert_eq!(jobs[0]["state"], "done", "{jobs:?}");
    let p = w.project_toml();
    assert_eq!(p.matches("[[post]]").count(), 1, "{p}");
    assert!(p.contains("visibility_asked = \"public\""), "{p}");
    let u = &w.status()["upload"];
    assert_eq!(u["posts"][0]["url"], "https://youtu.be/vid_123-A", "{u}");
    assert!(u["waiting"].is_null(), "{u}");

    // Two files, or no video, are refused; one video is a new round.
    let (ok, out) = w.cli(&[
        "submit",
        "--project",
        dir,
        "--step",
        "upload",
        "--files",
        "upload/film.mp4",
        "thumbnail/thumbnail.png",
    ]);
    assert!(!ok && out.contains("exactly one file"), "{out}");
    let (ok, out) = w.cli(&[
        "submit",
        "--project",
        dir,
        "--step",
        "upload",
        "--files",
        "upload/film.mp4",
    ]);
    assert!(ok, "{out}");
    let (ok, out) = w.cli(&["approve", "--project", dir, "--step", "upload"]);
    assert!(ok, "{out}");
    assert!(
        !out.contains("youtube_job"),
        "a second approval queues nothing: {out}"
    );
    w.jobs_end();
    assert_eq!(w.fake.lock().unwrap().inserts, 1, "no second upload");

    let (ok, out) = w.cli(&["job-start", "--kind", "youtube", "--project", dir]);
    assert!(!ok && out.contains("already posted"), "{out}");
    let (ok, out) = w.cli(&[
        "job-start",
        "--kind",
        "youtube",
        "--project",
        dir,
        "--again",
    ]);
    assert!(ok, "{out}");
    assert!(out.contains("--again"), "{out}");
    w.jobs_end();
    assert_eq!(w.fake.lock().unwrap().inserts, 2);
    assert_eq!(w.project_toml().matches("[[post]]").count(), 2);

    // Through MCP, a second post goes up private.
    let (v, _) = w.mcp().call(
        "job_start",
        json!({"kind": "youtube", "project": w.project, "again": true}),
    );
    assert!(v["command"].to_string().contains("--private"), "{v}");
    w.jobs_end();
    let f = w.fake.lock().unwrap();
    assert_eq!(f.inserts, 3);
    assert_eq!(f.meta["status"]["privacyStatus"], "private");
}

#[test]
fn an_mcp_approval_on_a_public_niche_asks_for_private() {
    let w = World::new(|_| {});
    w.in_review();
    let (v, _) = w.mcp().call(
        "approve",
        json!({"project": w.project, "step": "upload", "note": "yes, post it"}),
    );
    assert_eq!(v["via"], "mcp", "{v}");
    assert_eq!(v["youtube_job"]["kind"], "youtube", "{v}");
    let jobs = w.jobs_end();
    assert_eq!(jobs[0]["state"], "done", "{jobs:?}");
    assert_eq!(
        w.fake.lock().unwrap().meta["status"]["privacyStatus"],
        "private"
    );
    let p = w.project_toml();
    assert!(p.contains("visibility_asked = \"private\""), "{p}");
}

#[test]
fn an_approval_with_no_channel_signed_in_posts_nothing_and_status_says_why() {
    let w = World::new(|_| {});
    w.in_review();
    // Signed out: no Keychain item, and no [[channel]] in config.toml.
    std::fs::remove_file(w.keychain.path().join("main")).unwrap();
    let config = w.home.path().join("config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, text.replace("[[channel]]\nlabel = \"main\"\n", "")).unwrap();
    let dir = w.project.to_str().unwrap();
    let (ok, out) = w.cli(&["approve", "--project", dir, "--step", "upload"]);
    assert!(ok, "{out}");
    assert!(out.contains("main isn't signed in"), "{out}");
    assert!(w.jobs_end().is_empty());
    let u = &w.status()["upload"];
    assert!(
        u["waiting"].as_str().unwrap().contains("youtube_connect"),
        "{u}"
    );
    assert!(w.fake.lock().unwrap().paths.is_empty());
}

impl World {
    /// An MCP server for this world.
    fn mcp(&self) -> common::Client {
        let base = self.fake.lock().unwrap().base.clone();
        common::Client::start_env(
            self.home.path(),
            &[
                (
                    "MOCKINGBIRD_STUDIO_KEYCHAIN_DIR",
                    self.keychain.path().to_str().unwrap(),
                ),
                ("MOCKINGBIRD_STUDIO_GOOGLE", &base),
            ],
        )
    }
}

#[test]
fn a_dead_claim_from_an_earlier_round_is_asked_once_and_never_sent_to() {
    let w = World::new(|_| {});
    let p = w
        .project_toml()
        .replace(
            "state = \"approved\"\nround = 1\n[[step.output]]\npath = \"upload/film.mp4\"",
            "state = \"approved\"\nround = 2\n[[step.output]]\npath = \"upload/film.mp4\"",
        )
        .replace(
            "step = \"upload\"\nround = 1\ndecision = \"approved\"\nat = \"2026-11-26T10:00:00Z\"\nvia = \"window\"",
            "step = \"upload\"\nround = 2\ndecision = \"approved\"\nat = \"2026-11-26T10:00:00Z\"\nvia = \"mcp\"",
        );
    assert!(
        p.contains("round = 2") && p.contains("via = \"mcp\""),
        "{p}"
    );
    let base = w.fake.lock().unwrap().base.clone();
    let claim = format!(
        "\n[[post]]\nstep = \"upload\"\nround = 1\nat = \"{}\"\njob = \"20261126-090000-dead\"\nstate = \"uploading\"\nsession = \"{base}/upload/youtube/v3/videos?uploadType=resumable&upload_id=old\"\nfile = \"upload/film.mp4\"\nsize = {FILM}\nchannel = \"main\"\nvisibility_asked = \"public\"\n",
        jiff::Timestamp::now()
    );
    std::fs::write(w.project.join("project.toml"), p + &claim).unwrap();

    let (ok, out) = w.upload();
    assert!(ok, "{out}");
    assert!(out.contains("whether that upload finished"), "{out}");
    let f = w.fake.lock().unwrap();
    assert_eq!(
        f.old,
        vec![format!("bytes */{FILM}")],
        "one question, no bytes"
    );
    assert_eq!(f.meta["status"]["privacyStatus"], "private");
    assert!(f.got == film());
    drop(f);
    let p = w.project_toml();
    assert_eq!(p.matches("[[post]]").count(), 1, "{p}");
    assert!(
        p.contains("round = 2")
            && p.contains("visibility_asked = \"private\"")
            && !p.contains("upload_id=old"),
        "{p}"
    );
}
