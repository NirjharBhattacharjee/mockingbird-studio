//! Two processes add notes to the same project at once. Neither loses a
//! write, because every write holds the lock from its read to its rename.

use std::path::Path;
use std::process::Command;

use serde_json::json;
use studio_core::ops::dispatch;
use studio_core::project::{Project, Via};
use studio_core::store;

const CHILD: &str = "STUDIO_TEST_WRITER";
const NOTES_EACH: u64 = 50;

#[test]
fn two_processes_writing_the_same_project_lose_no_write() {
    // The child: this same test binary, started below with CHILD set.
    if let Ok(dir) = std::env::var(CHILD) {
        for i in 0..NOTES_EACH {
            let args = json!({"project": dir, "step": "script", "text": format!("note {i}")});
            dispatch("note_add", &args, Via::Cli).unwrap();
        }
        return;
    }

    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    let project = Project::new("T", None, "explainer", jiff::Timestamp::now()).unwrap();
    store::create_project(dir, &project).unwrap();

    let exe = std::env::current_exe().unwrap();
    let spawn = || {
        Command::new(&exe)
            .args([
                "--exact",
                "two_processes_writing_the_same_project_lose_no_write",
            ])
            .env(CHILD, dir)
            .env("MOCKINGBIRD_STUDIO_HOME", dir.join("home"))
            .spawn()
            .unwrap()
    };
    let children = [spawn(), spawn()];
    for mut c in children {
        assert!(c.wait().unwrap().success());
    }

    let p: Project = store::read(&Path::new(dir).join(store::PROJECT_FILE)).unwrap();
    let mut ids: Vec<u64> = p.notes.iter().map(|n| n.id).collect();
    ids.sort();
    assert_eq!(ids, (1..=2 * NOTES_EACH).collect::<Vec<_>>());
    assert_eq!(p.next_note, 2 * NOTES_EACH + 1);
}
