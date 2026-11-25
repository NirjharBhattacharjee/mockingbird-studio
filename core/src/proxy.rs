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

use serde_json::Value;

use crate::jobs::{self, Job};
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
    "scale=w='if(gt(iw,ih),-2,min(iw,720))':h='if(gt(iw,ih),min(ih,720),-2)'",
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
/// in config.toml, or else the first of `DIRS` that has it.
pub fn find(name: &str) -> Option<PathBuf> {
    let set = store::read_config()
        .ok()
        .and_then(|c| c.rest.get(name).and_then(|v| v.as_str()).map(PathBuf::from));
    match set {
        Some(p) => (p.is_absolute() && p.is_file()).then_some(p),
        None => DIRS
            .iter()
            .map(|d| Path::new(d).join(name))
            .find(|p| p.is_file()),
    }
}

/// The preview copy's path for an output, relative to the project folder.
pub fn path_for(o: &Output) -> String {
    let ext = if o.kind == Kind::Audio { "m4a" } else { "mp4" };
    format!(".studio/proxies/{}.{ext}", o.path)
}

/// Whether WebKit plays this file as it is, from ffprobe's streams.
fn plays(o: &Output, probe: &Value) -> bool {
    let streams = probe["streams"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let codecs = |t: &str| -> Vec<&str> {
        streams
            .iter()
            .filter(|s| s["codec_type"] == t && s["disposition"]["attached_pic"] != 1)
            .filter_map(|s| s["codec_name"].as_str())
            .collect()
    };
    let audio_ok =
        |c: &&str| matches!(*c, "aac" | "mp3" | "alac" | "flac") || c.starts_with("pcm_");
    let audio = codecs("audio");
    match o.kind {
        Kind::Audio => !audio.is_empty() && audio.iter().all(audio_ok),
        Kind::Video => {
            let ext = o.path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
            let video = codecs("video");
            matches!(ext.as_deref(), Some("mp4" | "mov" | "m4v"))
                && !video.is_empty()
                && video.iter().all(|c| matches!(*c, "h264" | "hevc"))
                && audio.iter().all(audio_ok)
        }
        _ => true,
    }
}

/// ffprobe's JSON for `file`, an absolute path. At most `CAP` bytes are
/// read; a reply that big isn't one this asks for.
fn ffprobe(bin: &Path, file: &Path) -> Option<Value> {
    const CAP: u64 = 1 << 20;
    let mut arg = std::ffi::OsString::from("file:");
    arg.push(file);
    let mut child = Command::new(bin)
        .args(["-v", "error", "-of", "json", "-show_entries"])
        .arg("format=duration:stream=codec_type,codec_name:stream_disposition=attached_pic")
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = Vec::new();
    let read = child.stdout.take()?.take(CAP).read_to_end(&mut out);
    // At the cap it may still be writing: it's stopped, and fails below.
    if read.is_err() || out.len() as u64 == CAP {
        let _ = child.kill();
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    (read.is_ok() && ok)
        .then(|| serde_json::from_slice(&out).ok())
        .flatten()
}

/// Records each audio and video output's `duration`, in seconds, and the
/// `proxy` path of each that WebKit can't play. Returns the outputs that
/// need a proxy job. Without ffprobe, or for a file it can't read, nothing
/// is recorded and the submit goes on.
pub fn probe(dir: &Path, outputs: &mut [Output]) -> Vec<Output> {
    let Some(bin) = find("ffprobe") else {
        return vec![];
    };
    let mut need = vec![];
    for o in outputs
        .iter_mut()
        .filter(|o| matches!(o.kind, Kind::Audio | Kind::Video))
    {
        let Some(probe) = ffprobe(&bin, &dir.join(&o.path)) else {
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
            o.rest.insert("proxy".into(), path_for(o).into());
            need.push(o.clone());
        }
    }
    need
}

/// Queues the job that makes `o`'s preview copy. `dir` is canonical.
pub fn start(dir: &Path, o: &Output) -> Result<Job> {
    let ffmpeg =
        find("ffmpeg").ok_or("no ffmpeg; set ffmpeg = \"/path/to/ffmpeg\" in config.toml")?;
    let out = path_for(o);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn out(path: &str) -> Output {
        Output {
            path: path.into(),
            kind: Kind::of(path),
            rest: Default::default(),
        }
    }

    fn streams(s: &[(&str, &str)]) -> Value {
        let s: Vec<Value> = s
            .iter()
            .map(|(t, c)| json!({"codec_type": t, "codec_name": c, "disposition": {"attached_pic": 0}}))
            .collect();
        json!({ "streams": s })
    }

    #[test]
    fn what_webkit_plays() {
        let h264 = streams(&[("video", "h264"), ("audio", "aac")]);
        assert!(plays(&out("a.mp4"), &h264));
        assert!(plays(&out("a.MOV"), &streams(&[("video", "hevc")])));
        assert!(!plays(&out("a.mkv"), &h264));
        assert!(!plays(
            &out("a.webm"),
            &streams(&[("video", "vp9"), ("audio", "opus")])
        ));
        assert!(!plays(&out("a.mp4"), &streams(&[("video", "mpeg4")])));
        assert!(!plays(
            &out("a.mp4"),
            &streams(&[("video", "h264"), ("audio", "opus")])
        ));
        assert!(!plays(&out("a.mp4"), &streams(&[("audio", "aac")])));
        assert!(plays(&out("a.wav"), &streams(&[("audio", "pcm_s16le")])));
        assert!(plays(&out("a.mp3"), &streams(&[("audio", "mp3")])));
        assert!(!plays(&out("a.ogg"), &streams(&[("audio", "vorbis")])));
        // Cover art in an mp3 isn't a video.
        let mut art = streams(&[("audio", "mp3"), ("video", "mjpeg")]);
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
}
