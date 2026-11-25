use std::process::ExitCode;

use serde_json::Value;
use studio_core::{ops, project::Via, store};
use tauri::{AppHandle, Manager};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The early returns happen before any Tauri code runs, so no window
    // or dock icon appears.
    match args.first().map(String::as_str) {
        Some("mcp") => return studio_core::mcp::run(),
        Some("runner") => return studio_core::jobs::run_runner(),
        Some("setup") => return studio_core::setup::run(&args[1..]),
        Some("mockingbird-call") => return studio_core::mockingbird::run_call(&args[1..]),
        Some("youtube-upload") => return studio_core::upload::run(&args[1..]),
        Some(_) => return studio_core::cli::run(&args),
        None => {}
    }
    tauri::Builder::default()
        .setup(|app| {
            allow_listed(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![op])
        .run(tauri::generate_context!())
        .expect("error while running the window");
    ExitCode::SUCCESS
}

/// The window's one command: an operation from the core table, recorded as
/// coming from the window. It runs on the blocking pool, so a sign-in
/// waiting on the browser freezes neither the window nor the async runtime.
#[tauri::command]
async fn op(app: AppHandle, name: String, args: Value) -> Result<Value, String> {
    let lists = matches!(name.as_str(), "projects" | "project_new" | "project_add");
    let out =
        tauri::async_runtime::spawn_blocking(move || ops::dispatch(&name, &args, Via::Window))
            .await
            .map_err(|e| e.to_string())??;
    if lists {
        allow_listed(&app);
    }
    Ok(out)
}

/// Lets the asset protocol read inside the listed project folders, nothing
/// wider. The scope starts empty in tauri.conf.json.
// ponytail: a folder taken off the list stays readable until the app restarts.
fn allow_listed(app: &AppHandle) {
    let Ok(config) = store::read_config() else {
        return;
    };
    let scope = app.asset_protocol_scope();
    for dir in config.projects {
        // The protocol canonicalizes each request, so the scope must be canonical too.
        match dir.canonicalize() {
            Ok(dir) => {
                // `**` skips dot-folders, so preview copies are allowed by name.
                for d in [dir.clone(), dir.join(".studio/proxies")] {
                    if let Err(e) = scope.allow_directory(&d, true) {
                        eprintln!("can't allow {}: {e}", d.display());
                    }
                }
            }
            Err(e) => eprintln!("can't allow {}: {e}", dir.display()),
        }
    }
}
