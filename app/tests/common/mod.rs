//! An MCP client over the binary's stdio, shared by the MCP and loop tests.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

pub const BIN: &str = env!("CARGO_BIN_EXE_mockingbird-studio");

pub struct Client {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Client {
    // Not every test file that shares this module calls it.
    #[allow(dead_code)]
    pub fn start(home: &std::path::Path) -> Client {
        Client::start_env(home, &[])
    }

    /// Starts the server with extra environment variables.
    pub fn start_env(home: &std::path::Path, env: &[(&str, &str)]) -> Client {
        let mut child = Command::new(BIN)
            .arg("mcp")
            .env("MOCKINGBIRD_STUDIO_HOME", home)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin: Some(stdin),
            stdout,
            id: 0,
        };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"},
            }),
        );
        assert!(init["instructions"].as_str().unwrap().contains("status"));
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    pub fn send(&mut self, msg: Value) {
        writeln!(self.stdin.as_mut().unwrap(), "{msg}").unwrap();
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server hung up"
            );
            let msg: Value = serde_json::from_str(&line).expect("stdout is JSON-RPC only");
            if msg["id"] == id {
                assert!(msg["error"].is_null(), "{msg}");
                return msg["result"].clone();
            }
        }
    }

    /// Calls a tool. Returns the JSON reply and the waiting line after it.
    pub fn call(&mut self, name: &str, args: Value) -> (Value, String) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let text = r["content"][0]["text"].as_str().unwrap();
        let (body, line) = text.rsplit_once('\n').unwrap();
        assert_eq!(r["isError"], false, "{name}: {body}");
        (serde_json::from_str(body).unwrap(), line.to_string())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
