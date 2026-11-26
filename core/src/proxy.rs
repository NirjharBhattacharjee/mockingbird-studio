//! Preview copies (D4). On `submit`, `ffprobe` reads each audio and video
//! output's length and codecs. A file WebKit can't play gets a `proxy` job:
//! `ffmpeg` writes an H.264 and AAC copy, 720p at most, to
//! `.studio/proxies/<path>.mp4` (`.m4a` for audio). The original is never
//! touched. This is the only thing the app runs ffmpeg for.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::jobs::{self, Job, JobState};
use crate::project::{Kind, Output, Result};
use crate::store;

/// Where to look when config.toml doesn't say. The runner and Claude
/// Desktop have a minimal `PATH`, so `PATH` isn't searched.
const DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/local/bin",
    "/usr/bin",
];

const VIDEO: &[&str] = &[
    "-nostdin",
    "-y",
    "-i",
    "file:{in}",
    // The short side at most 720, never scaled up, even sizes for H.264.
    "-vf",
    "scale=w='if(gt(iw,ih),-2,trunc(min(iw,720)/2)*2)':h='if(gt(iw,ih),trunc(min(ih,720)/2)*2,-2)'",
    "-c:v",
    "libx264",
    "-preset",
    "veryfast",
    "-crf",
    "23",
    "-pix_fmt",
    "yuv420p",
    "-c:a",
    "aac",
    "-b:a",
    "128k",
    "-movflags",
    "+faststart",
    "file:{out}",
];

const AUDIO: &[&str] = &[
    "-nostdin",
    "-y",
    "-i",
    "file:{in}",
    "-vn",
    "-c:a",
    "aac",
    "-b:a",
    "128k",
    "file:{out}",
];

/// `ffmpeg` or `ffprobe`: the path set as `ffmpeg = "…"` or `ffprobe = "…"`
/// in config.toml, or else the first of `DIRS` that has it. A setting that
/// isn't an absolute path to a file is passed over, with a line in `warn`.
pub fn find(name: &str, warn: &mut Vec<String>) -> Option<PathBuf> {
    let config = store::read_config().ok();
    let set = config
        .as_ref()
        .and_then(|c| c.rest.get(name).and_then(|v| v.as_str()));
    pick(name, set, DIRS, warn)
}

fn pick(name: &str, set: Option<&str>, dirs: &[&str], warn: &mut Vec<String>) -> Option<PathBuf> {
    if let Some(set) = set {
        let p = PathBuf::from(set);
        if p.is_absolute() && p.is_file() {
            return Some(p);
        }
        let w = format!(
            "{name} = {set:?} in config.toml isn't an absolute path to a file; looked in {} instead",
            dirs.join(", ")
        );
        if !warn.contains(&w) {
            warn.push(w);
        }
    }
    dirs.iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

/// The preview copy's path for an output, relative to the project folder.
pub fn path_for(o: &Output) -> String {
    let ext = if o.kind == Kind::Audio { "m4a" } else { "mp4" };
    format!(".studio/proxies/{}.{ext}", o.path)
}

/// Whether WebKit plays this file as it is, from ffprobe's streams.
fn plays(o: &Output, probe: &Value) -> bool {
    let streams: Vec<&Value> = probe["streams"]
        .as_array()
        .map(|s| {
            s.iter()
                .filter(|s| s["disposition"]["attached_pic"] != 1)
                .collect()
        })
        .unwrap_or_default();
    let of = |t: &'static str| streams.iter().filter(move |s| s["codec_type"] == t);
    let audio_ok = |s: &&&Value| {
        let c = s["codec_name"].as_str().unwrap_or_default();
        matches!(c, "aac" | "mp3" | "alac" | "flac") || c.starts_with("pcm_")
    };
    // 8-bit 4:2:0 H.264, and HEVC tagged hvc1: what AVFoundation decodes.
    let video_ok = |s: &&&Value| match s["codec_name"].as_str() {
        Some("h264") => matches!(s["pix_fmt"].as_str(), Some("yuv420p" | "yuvj420p")),
        Some("hevc") => s["codec_tag_string"] == "hvc1",
        _ => false,
    };
    match o.kind {
        Kind::Audio => of("audio").next().is_some() && of("audio").all(|s| audio_ok(&s)),
        Kind::Video => {
            let ext = o.path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
            matches!(ext.as_deref(), Some("mp4" | "mov" | "m4v"))
                && of("video").next().is_some()
                && of("video").all(|s| video_ok(&s))
                && of("audio").all(|s| audio_ok(&s))
        }
        _ => true,
    }
}

/// ffprobe's JSON for `file`, an absolute path, or `None` if it fails, says
/// more than `CAP` bytes, or takes longer than `timeout`.
fn ffprobe(bin: &Path, file: &Path, timeout: Duration) -> Option<Value> {
    const CAP: u64 = 1 << 20;
    let mut arg = std::ffi::OsString::from("file:");
    arg.push(file);
    let mut child = Command::new(bin)
        .args(["-v", "error", "-of", "json", "-show_entries"])
        .arg(
            "format=duration:stream=codec_type,codec_name,pix_fmt,codec_tag_string\
             :stream_disposition=attached_pic",
        )
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Read on the side so the clock runs. At the cap the pipe closes, and a
    // writer that's still going fails.
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        stdout.take(CAP).read_to_end(&mut out).map(|_| out)
    });
    let end = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().ok()?.ok()?;
    if !status?.success() || out.len() as u64 >= CAP {
        return None;
    }
    serde_json::from_slice(&out).ok()
}

/// Records each audio and video output's `duration`, in seconds. Returns
/// the paths WebKit can't play. Without ffprobe, or for a file it can't
/// read in 10 seconds, nothing is recorded and the submit goes on.
pub fn probe(dir: &Path, outputs: &mut [Output], warn: &mut Vec<String>) -> Vec<String> {
    let Some(bin) = find("ffprobe", warn) else {
        return vec![];
    };
    let mut need = vec![];
    for o in outputs
        .iter_mut()
        .filter(|o| matches!(o.kind, Kind::Audio | Kind::Video))
    {
        let Some(probe) = ffprobe(&bin, &dir.join(&o.path), Duration::from_secs(10)) else {
            continue;
        };
        let duration = probe["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse::<f64>().ok());
        if let Some(d) = duration {
            o.rest
                .insert("duration".into(), ((d * 1000.0).round() / 1000.0).into());
        }
        if !plays(o, &probe) {
            need.push(o.path.clone());
        }
    }
    need
}

/// Makes sure each output in `need` has a preview copy on its way, and
/// records its `proxy` path once it has. Returns a line per output for the
/// reply: its job, or why there's none.
pub fn queue_all(
    dir: &Path,
    outputs: &mut [Output],
    need: &[String],
    warn: &mut Vec<String>,
) -> Vec<Value> {
    let mut lines = vec![];
    for o in outputs.iter_mut().filter(|o| need.contains(&o.path)) {
        let line = match start(dir, o, warn) {
            Ok(Some(job)) => json!(job),
            Ok(None) => json!({"path": o.path, "proxy": "up to date"}),
            Err(e) => {
                // Kept on the output so the window can say why there's no copy.
                o.rest.insert("proxy_error".into(), e.clone().into());
                lines.push(json!({"path": o.path, "error": e}));
                continue;
            }
        };
        o.rest.insert("proxy".into(), path_for(o).into());
        lines.push(line);
    }
    lines
}

/// The job making `o`'s copy: one already queued or running for the same
/// file, or a new one. `None` when the copy is newer than the original.
/// `dir` is canonical.
fn start(dir: &Path, o: &Output, warn: &mut Vec<String>) -> Result<Option<Job>> {
    let out = path_for(o);
    let busy = jobs::list(Some(dir)).into_iter().find(|j| {
        j.kind == "proxy"
            && matches!(j.state, JobState::Queued | JobState::Running)
            && j.values.get("out") == Some(&out)
    });
    if busy.is_some() {
        return Ok(busy);
    }
    let modified = |p: &str| fs::metadata(dir.join(p)).and_then(|m| m.modified()).ok();
    if let (Some(copy), Some(original)) = (modified(&out), modified(&o.path))
        && copy > original
    {
        return Ok(None);
    }
    let ffmpeg =
        find("ffmpeg", warn).ok_or("no ffmpeg; set ffmpeg = \"/path/to/ffmpeg\" in config.toml")?;
    let values = BTreeMap::from([
        ("in".to_string(), o.path.clone()),
        ("out".to_string(), out.clone()),
    ]);
    let template: Vec<String> = if o.kind == Kind::Audio { AUDIO } else { VIDEO }
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Checked before the folder is made, so a symlinked `.studio` is refused.
    let args = jobs::fill(&template, &values, Some(dir))?;
    let parent = dir.join(&out);
    let parent = parent.parent().expect("a joined path has a parent");
    fs::create_dir_all(parent).map_err(|e| format!("can't make {}: {e}", parent.display()))?;
    let mut command = vec![ffmpeg.to_string_lossy().into_owned()];
    command.extend(args);
    jobs::queue(
        "proxy",
        "ffmpeg",
        command,
        Some(dir.to_path_buf()),
        None,
        values,
        None,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(path: &str) -> Output {
        Output {
            path: path.into(),
            kind: Kind::of(path),
            rest: Default::default(),
        }
    }

    /// Streams as `type:codec`, with `:pix_fmt` or `:tag` for video.
    fn streams(s: &[&str]) -> Value {
        let s: Vec<Value> = s
            .iter()
            .map(|s| {
                let p: Vec<&str> = s.split(':').collect();
                let extra = p.get(2).copied().unwrap_or_default();
                json!({"codec_type": p[0], "codec_name": p[1], "pix_fmt": extra,
                       "codec_tag_string": extra, "disposition": {"attached_pic": 0}})
            })
            .collect();
        json!({ "streams": s })
    }

    #[test]
    fn what_webkit_plays() {
        let h264 = streams(&["video:h264:yuv420p", "audio:aac"]);
        assert!(plays(&out("a.mp4"), &h264));
        assert!(plays(&out("a.m4v"), &streams(&["video:h264:yuvj420p"])));
        assert!(plays(&out("a.MOV"), &streams(&["video:hevc:hvc1"])));
        assert!(!plays(&out("a.mov"), &streams(&["video:hevc:hev1"])));
        assert!(!plays(&out("a.mp4"), &streams(&["video:h264:yuv422p10le"])));
        assert!(!plays(&out("a.mkv"), &h264));
        assert!(!plays(
            &out("a.webm"),
            &streams(&["video:vp9", "audio:opus"])
        ));
        assert!(!plays(&out("a.mp4"), &streams(&["video:mpeg4:yuv420p"])));
        assert!(!plays(
            &out("a.mp4"),
            &streams(&["video:h264:yuv420p", "audio:opus"])
        ));
        assert!(!plays(&out("a.mp4"), &streams(&["audio:aac"])));
        assert!(plays(&out("a.wav"), &streams(&["audio:pcm_s16le"])));
        assert!(plays(&out("a.mp3"), &streams(&["audio:mp3"])));
        assert!(!plays(&out("a.ogg"), &streams(&["audio:vorbis"])));
        // Cover art in an mp3 isn't a video.
        let mut art = streams(&["audio:mp3", "video:mjpeg"]);
        art["streams"][1]["disposition"]["attached_pic"] = 1.into();
        assert!(plays(&out("a.mp3"), &art));
        assert_eq!(
            path_for(&out("previews/a.mkv")),
            ".studio/proxies/previews/a.mkv.mp4"
        );
        assert_eq!(
            path_for(&out("voice/a.ogg")),
            ".studio/proxies/voice/a.ogg.m4a"
        );
    }

    #[test]
    fn a_bad_setting_falls_back_and_says_so() {
        let bin = tempfile::tempdir().unwrap();
        let ffprobe = bin.path().join("ffprobe");
        fs::write(&ffprobe, "").unwrap();
        let dirs = [bin.path().to_str().unwrap()];
        let mut warn = vec![];
        let set = ffprobe.to_str().unwrap();
        assert_eq!(
            pick("ffprobe", Some(set), &dirs, &mut warn),
            Some(ffprobe.clone())
        );
        assert!(warn.is_empty());
        for bad in ["ffprobe", "/no/such/ffprobe"] {
            assert_eq!(
                pick("ffprobe", Some(bad), &dirs, &mut warn),
                Some(ffprobe.clone())
            );
        }
        assert_eq!(warn.len(), 2, "{warn:?}");
        assert!(warn[0].contains("isn't an absolute path"), "{warn:?}");
        assert_eq!(pick("ffprobe", None, &["/no/such"], &mut warn), None);
    }

    #[test]
    fn a_stuck_ffprobe_is_stopped() {
        let bin = tempfile::tempdir().unwrap();
        let stuck = bin.path().join("ffprobe");
        // exec, so the kill reaches the sleep and the pipe closes.
        fs::write(&stuck, "#!/bin/sh\nexec sleep 30\n").unwrap();
        fs::set_permissions(&stuck, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let began = Instant::now();
        assert!(ffprobe(&stuck, Path::new("/x.mp4"), Duration::from_millis(300)).is_none());
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{:?}",
            began.elapsed()
        );
    }
}
