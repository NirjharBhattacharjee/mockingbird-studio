//! The command line. One command per operation, from M1-3.

use std::process::ExitCode;

/// Runs the CLI with the arguments after the program name.
pub fn run(args: &[String]) -> ExitCode {
    eprintln!(
        "mockingbird-studio: no CLI commands yet (got {:?})",
        args.join(" ")
    );
    ExitCode::FAILURE
}
