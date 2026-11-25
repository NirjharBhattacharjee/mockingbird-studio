//! The command line. One command per operation, built from the table in
//! `ops`, so a new operation is a new command with no code here.

use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};
use serde_json::{Map, Value};

use crate::ops::{self, ArgKind, OPS};
use crate::project::Via;

/// `project_new` is the command `project-new`: the underscore is for MCP.
fn command_name(op: &str) -> &'static str {
    Box::leak(op.replace('_', "-").into_boxed_str())
}

fn build() -> Command {
    let mut cli = Command::new("mockingbird-studio")
        .about("Run the studio's operations from a terminal. With no command, open the window.")
        .subcommand_required(true);
    for op in OPS {
        let mut cmd = Command::new(command_name(op.name)).about(op.help).arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Print one line of JSON."),
        );
        for a in op.args {
            let mut arg = Arg::new(a.name)
                .long(a.name)
                .help(a.help)
                .required(a.required);
            if a.kind == ArgKind::TextList {
                arg = arg.action(ArgAction::Append);
            }
            cmd = cmd.arg(arg);
        }
        cli = cli.subcommand(cmd);
    }
    cli
}

/// Runs the CLI with the arguments after the program name.
pub fn run(args: &[String]) -> ExitCode {
    let argv = std::iter::once("mockingbird-studio".to_string()).chain(args.iter().cloned());
    let matches = match build().try_get_matches_from(argv) {
        Ok(m) => m,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let (cmd, m) = matches.subcommand().expect("a command is required");
    let op = OPS
        .iter()
        .find(|o| command_name(o.name) == cmd)
        .expect("clap only matches commands built from OPS");
    let mut json = Map::new();
    for a in op.args {
        let value = match a.kind {
            ArgKind::TextList => m
                .get_many::<String>(a.name)
                .map(|v| Value::from(v.cloned().collect::<Vec<_>>())),
            ArgKind::Text => m.get_one::<String>(a.name).map(|s| Value::from(s.as_str())),
            // Parsed here, not by clap, so a bad number is the dispatch error.
            ArgKind::Number | ArgKind::Integer => m.get_one::<String>(a.name).map(|s| {
                s.parse::<serde_json::Number>()
                    .map_or_else(|_| Value::from(s.as_str()), Value::Number)
            }),
        };
        if let Some(v) = value {
            json.insert(a.name.into(), v);
        }
    }
    match ops::dispatch(op.name, &Value::Object(json), Via::Cli) {
        Ok(v) => {
            // ponytail: both forms are JSON; --json is the compact one. A
            // table view per operation if people read this by eye.
            let out = if m.get_flag("json") {
                v.to_string()
            } else {
                serde_json::to_string_pretty(&v).expect("a Value always serialises")
            };
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mockingbird-studio: {e}");
            ExitCode::FAILURE
        }
    }
}
