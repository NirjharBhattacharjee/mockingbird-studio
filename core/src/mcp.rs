//! The MCP server over stdio, from M1-4. Only protocol messages may go to
//! stdout, so anything else goes to stderr.

use std::process::ExitCode;

/// Runs `mockingbird-studio mcp`.
pub fn run() -> ExitCode {
    eprintln!("mockingbird-studio: the MCP server isn't built yet");
    ExitCode::SUCCESS
}
