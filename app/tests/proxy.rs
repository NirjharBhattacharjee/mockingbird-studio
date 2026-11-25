//! Preview copies: submit records each audio and video file's length, and a
//! file WebKit can't play gets a `proxy` job that writes an H.264 copy.
//! Needs ffmpeg: `cargo test -- --ignored`. The media is made here.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

fn tool(name: &str) -> String {
    ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
        .iter()
        .map(|d| format!("{d}/{name}"))
        .find(|p| Path::new(p).is_file())
        .unwrap_or_else(|| panic!("needs {name}"))
}

fn ffmpeg(args: &[&str]) {
    let ok = Command::new(tool("ffmpeg"))
        .args(["-v", "error", "-y"])
        .args(args)
        .status()
        .unwrap();
    assert!(ok.success(), "ffmpeg {args:?}");
}

fn cli(home: &Path, args: &[&str]) -> Value {
    let out = Command::new(BIN)
        .args(args)
        .arg("--json")
        .env("MOCKINGBIRD_STUDIO_HOME", home)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
#[ignore = "needs ffmpeg"]
fn submit_records_lengths_and_queues_a_preview_copy() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("proxy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    let dir = root.join("video");
    let p = dir.to_str().unwrap();
    cli(&home, &["project-new", "--dir", p, "--title", "Example"]);

    // A WAV plays as it is. An MPEG-4 Part 2 video in Matroska doesn't.
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "sine=duration=1.5",
        dir.join("research/voice.wav").to_str().unwrap(),
    ]);
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=1280x960:rate=25:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=duration=2",
        "-c:v",
        "mpeg4",
        "-c:a",
        "mp2",
        dir.join("research/clip.mkv").to_str().unwrap(),
    ]);

    let began = Instant::now();
    let step = cli(
        &home,
        &[
            "submit",
            "--project",
            p,
            "--step",
            "research",
            "--files",
            "research/voice.wav",
            "research/clip.mkv",
        ],
    );
    eprintln!("submit with ffprobe took {:?}", began.elapsed());
    let outs = &step["output"];
    assert_eq!(outs[0]["duration"], 1.5, "{step}");
    assert!(outs[0].get("proxy").is_none(), "{step}");
    // Matroska with MP2 audio runs a few milliseconds past 2 s.
    let d = outs[1]["duration"].as_f64().unwrap();
    assert!((2.0..2.05).contains(&d), "{step}");
    let proxy = ".studio/proxies/research/clip.mkv.mp4";
    assert_eq!(outs[1]["proxy"], proxy, "{step}");
    let toml = fs::read_to_string(dir.join("project.toml")).unwrap();
    assert!(
        toml.contains("duration = 1.5") && toml.contains("duration = 2."),
        "{toml}"
    );

    let jobs = step["proxy_jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1, "{step}");
    assert_eq!(jobs[0]["kind"], "proxy");
    let id = jobs[0]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(60);
    let job = loop {
        let j = cli(&home, &["job", "--id", id]);
        if !matches!(j["state"].as_str(), Some("queued" | "running")) {
            break j;
        }
        assert!(Instant::now() < end, "proxy job still {}", j["state"]);
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["outputs"], serde_json::json!([proxy]));
    eprintln!("proxy job took {:?} after submit", began.elapsed());

    // H.264 and AAC, the short side down to 720.
    let probe = Command::new(tool("ffprobe"))
        .args([
            "-v",
            "error",
            "-of",
            "csv=p=0",
            "-show_entries",
            "stream=codec_name,width,height",
        ])
        .arg(dir.join(proxy))
        .output()
        .unwrap();
    let probe = String::from_utf8(probe.stdout).unwrap();
    assert_eq!(
        probe.lines().collect::<Vec<_>>(),
        ["h264,960,720", "aac"],
        "{probe}"
    );
}
