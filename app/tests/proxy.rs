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

/// A 2 s 1280×960 MPEG-4 Part 2 video with MP2 audio, in Matroska.
fn make_clip(dir: &Path) {
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
    make_clip(&dir);

    // A relative path in config.toml is passed over, and the reply says so.
    let config = home.join("config.toml");
    let mut text = fs::read_to_string(&config).unwrap();
    text.push_str("ffprobe = \"ffprobe\"\n");
    fs::write(&config, text).unwrap();

    let submit = || {
        cli(
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
        )
    };
    // The step has a gate: approve the last round to send a new one.
    let resubmit = || {
        cli(&home, &["approve", "--project", p, "--step", "research"]);
        submit()
    };
    let wait = |id: &str| {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let j = cli(&home, &["job", "--id", id]);
            if !matches!(j["state"].as_str(), Some("queued" | "running")) {
                break j;
            }
            assert!(Instant::now() < end, "proxy job still {}", j["state"]);
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let proxy_jobs = || {
        let all = cli(&home, &["jobs", "--project", p]);
        all["jobs"].as_array().unwrap().len()
    };

    let began = Instant::now();
    let step = submit();
    eprintln!("submit with ffprobe took {:?}", began.elapsed());
    assert!(
        step["warnings"][0]
            .as_str()
            .unwrap()
            .contains("ffprobe = \"ffprobe\""),
        "{step}"
    );
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
    let id = jobs[0]["id"].as_str().unwrap().to_string();
    // Submitted again at once: the job on its way, or the finished copy,
    // stands. Either way there's still one job.
    resubmit();
    assert_eq!(proxy_jobs(), 1);
    let job = wait(&id);
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["outputs"], serde_json::json!([proxy]));
    eprintln!("proxy job took {:?} after submit", began.elapsed());

    // The copy is newer than the original: nothing to do.
    let again = resubmit();
    assert_eq!(again["proxy_jobs"][0]["proxy"], "up to date", "{again}");
    assert_eq!(again["output"][1]["proxy"], proxy, "{again}");
    assert_eq!(proxy_jobs(), 1);

    // A new render at the same path gets a new copy.
    make_clip(&dir);
    let redone = resubmit();
    let id = redone["proxy_jobs"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(wait(&id)["state"], "done", "{redone}");
    assert_eq!(proxy_jobs(), 2);

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
