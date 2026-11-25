use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Both branches return before any Tauri code runs, so no window or dock
    // icon appears.
    match args.first().map(String::as_str) {
        Some("mcp") => return studio_core::mcp::run(),
        Some(_) => return studio_core::cli::run(&args),
        None => {}
    }
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the window");
    ExitCode::SUCCESS
}
