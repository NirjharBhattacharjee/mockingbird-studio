//! The `mockingbird` job kind: `mockingbird-studio mockingbird-call` starts
//! `mockingbird mcp`, calls one tool over stdio and waits for its reply with
//! no time limit (D6). The runner runs it like any job, so cancel stops both
//! processes: they share the job's process group.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::project::Result;
use crate::store;

/// The tools a job may call. The rest are quick enough to call directly.
pub const TOOLS: [&str; 3] = ["motion", "render", "look"];

/// The longest line read from `mockingbird mcp`.
const LINE_LIMIT: u64 = 64 << 20;
/// How long a reply may trail the process that wrote it.
const GRACE: Duration = Duration::from_secs(1);
/// How long `mockingbird mcp` gets to exit once its stdin closes.
const EXIT_WAIT: Duration = Duration::from_secs(3);

/// `mockingbird` in config.toml, or `mockingbird` on the runner's PATH when
/// the key isn't there.
pub fn program() -> Result<String> {
    match store::read_config()?.rest.get("mockingbird") {
        None => Ok("mockingbird".into()),
        Some(toml::Value::String(p)) if !p.is_empty() => Ok(p.clone()),
        Some(_) => Err(format!(
            "mockingbird in {} should be the program's path",
            store::config_path().display()
        )),
    }
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

/// `mockingbird mcp` and its stdin. Dropping it closes stdin, gives the
/// server `EXIT_WAIT` to exit, then kills it.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Server {
    fn send(&mut self, msg: Value) -> Result<()> {
        let stdin = self.stdin.as_mut().expect("open until drop");
        writeln!(stdin, "{msg}").map_err(|e| format!("mockingbird mcp hung up: {e}"))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stdin = None;
        let end = Instant::now() + EXIT_WAIT;
        while Instant::now() < end {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads stdout line by line on its own thread, so the child can be watched
/// while a line is awaited. Ends with an `Err` at EOF or an over-long line.
fn read_lines(stdout: impl Read + Send + 'static) -> mpsc::Receiver<Result<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            let got = (&mut r).take(LINE_LIMIT).read_until(b'\n', &mut line);
            let msg = match got {
                Ok(0) => Err("mockingbird mcp exited without replying".to_string()),
                Ok(n) if n as u64 == LINE_LIMIT && !line.ends_with(b"\n") => {
                    Err("mockingbird mcp sent a line over 64 MB".into())
                }
                Ok(_) => Ok(line),
                Err(e) => Err(format!("can't read mockingbird mcp: {e}")),
            };
            let end = msg.is_err();
            if tx.send(msg).is_err() || end {
                return;
            }
        }
    });
    rx
}

fn call(program: &str, tool: &str, args: Value) -> Result<String> {
    let mut child = Command::new(program)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't start {program} mcp: {e}"))?;
    let lines = read_lines(child.stdout.take().expect("piped"));
    let stdin = child.stdin.take();
    let mut server = Server { child, stdin };
    // The reply to request `id`. Requests from the server (ping, roots) get
    // "not supported": the client offers nothing. A server that exits
    // without replying fails the call even if something it started still
    // holds stdout open.
    let reply = |server: &mut Server, id: u64| -> Result<Value> {
        let mut exited: Option<Instant> = None;
        loop {
            let line = match lines.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => line?,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("mockingbird mcp exited without replying".into());
                }
                Err(RecvTimeoutError::Timeout) => {
                    match exited {
                        Some(t) if t.elapsed() >= GRACE => {
                            return Err("mockingbird mcp exited without replying".into());
                        }
                        None if !matches!(server.child.try_wait(), Ok(None)) => {
                            exited = Some(Instant::now());
                        }
                        _ => {}
                    }
                    continue;
                }
            };
            let Ok(msg) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            let is_request = msg.get("method").is_some();
            if msg["id"] == id && !is_request {
                if let Some(err) = msg.get("error") {
                    return Err(err["message"].as_str().unwrap_or("error").to_string());
                }
                return Ok(msg["result"].clone());
            }
            if is_request && msg.get("id").is_some() {
                server.send(json!({"jsonrpc": "2.0", "id": msg["id"],
                    "error": {"code": -32601, "message": "not supported"}}))?;
            }
        }
    };
    server.send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "mockingbird-studio", "version": env!("CARGO_PKG_VERSION")},
        }}),
    )?;
    reply(&mut server, 1)?;
    server.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
    server.send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": tool, "arguments": args}}))?;
    let result = reply(&mut server, 2)?;
    drop(server);
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
