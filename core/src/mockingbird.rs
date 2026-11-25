//! The `mockingbird` job kind: `mockingbird-studio mockingbird-call` starts
//! `mockingbird mcp`, calls one tool over stdio and waits for its reply with
//! no time limit (D6). The runner runs it like any job, so cancel stops both
//! processes: they share the job's process group.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitCode, Stdio};

use serde_json::{Value, json};

use crate::project::Result;
use crate::store;

/// The tools a job may call. The rest are quick enough to call directly.
pub const TOOLS: [&str; 3] = ["motion", "render", "look"];

/// `mockingbird` in config.toml, or `mockingbird` on the runner's PATH.
pub fn program() -> String {
    store::read_config()
        .ok()
        .and_then(|c| c.rest.get("mockingbird")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "mockingbird".into())
}

/// `mockingbird-studio mockingbird-call <program> <tool> <args json>`. The
/// reply goes to stdout, which is the job's log.
pub fn run_call(args: &[String]) -> ExitCode {
    let [program, tool, json] = args else {
        eprintln!("usage: mockingbird-studio mockingbird-call <program> <tool> <args json>");
        return ExitCode::from(2);
    };
    let result = serde_json::from_str(json)
        .map_err(|e| format!("args: {e}"))
        .and_then(|a| call(program, tool, a));
    match result {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("mockingbird {tool}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn call(program: &str, tool: &str, args: Value) -> Result<String> {
    let mut child = Command::new(program)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't start {program} mcp: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut stdout = BufReader::new(child.stdout.take().expect("piped"));
    let mut send =
        |msg: Value| writeln!(stdin, "{msg}").map_err(|e| format!("{program} mcp hung up: {e}"));
    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "mockingbird-studio", "version": env!("CARGO_PKG_VERSION")},
        }}),
    )?;
    let mut reply = |id: u64, send: &mut dyn FnMut(Value) -> Result<()>| -> Result<Value> {
        let mut line = String::new();
        loop {
            line.clear();
            match stdout.read_line(&mut line) {
                Ok(0) => return Err(format!("{program} mcp exited without replying")),
                Err(e) => return Err(format!("can't read {program} mcp: {e}")),
                Ok(_) => {}
            }
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg["id"] == id && msg.get("method").is_none() {
                if let Some(err) = msg.get("error") {
                    return Err(err["message"].as_str().unwrap_or("error").to_string());
                }
                return Ok(msg["result"].clone());
            }
            // A request from the server (ping, roots): the client offers none.
            if msg.get("method").is_some() && msg.get("id").is_some() {
                send(json!({"jsonrpc": "2.0", "id": msg["id"],
                    "error": {"code": -32601, "message": "not supported"}}))?;
            }
        }
    };
    reply(1, &mut send)?;
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": tool, "arguments": args}}))?;
    let result = reply(2, &mut send);
    let _ = child.kill();
    let _ = child.wait();
    let result = result?;
    let text = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if result["isError"] == true {
        return Err(text);
    }
    Ok(text)
}
