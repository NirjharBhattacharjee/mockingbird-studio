//! The MCP server over stdio. One tool per operation in `ops::OPS`, so a new
//! operation is a new tool with no code here. Only protocol messages may go
//! to stdout, so anything else goes to stderr.

use std::process::ExitCode;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer, ServerInitializeError};
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::{Map, Value, json};

use crate::ops::{self, ArgKind, OPS};
use crate::project::{NoteState, Project, State, Via};
use crate::store::{self, PROJECT_FILE};

pub const INSTRUCTIONS: &str = "\
mockingbird-studio keeps a video project's steps, files, notes and decisions in project.toml, \
and the person reviews your work in its window or from a terminal.
- In a new chat, call `status` (or `projects` if you don't know the project) before anything else.
- After `submit` on a gate step, stop. Don't start the steps after it until the person approves.
- When the person says they've reviewed something, call `notes`, work through each note, \
and mark it with `note_done` and a reply.
- Call `approve` only when the person said yes in this chat, and put their words in `note`.
Every reply ends with a line saying how many notes are open and which steps are waiting on you.";

/// The input schema for one operation, from its arguments in the table.
fn schema(op: &ops::Op) -> Map<String, Value> {
    let props: Map<String, Value> = op
        .args
        .iter()
        .map(|a| {
            let mut p = match a.kind {
                ArgKind::Text => json!({"type": "string"}),
                ArgKind::Number => json!({"type": "number"}),
                ArgKind::Integer => json!({"type": "integer", "minimum": 0}),
                ArgKind::TextList => json!({"type": "array", "items": {"type": "string"}}),
                ArgKind::Objects => json!({"type": "array", "items": {"type": "object"}}),
                ArgKind::Flag => json!({"type": "boolean"}),
            };
            p["description"] = a.help.into();
            (a.name.to_string(), p)
        })
        .collect();
    let required: Vec<&str> = op
        .args
        .iter()
        .filter(|a| a.required)
        .map(|a| a.name)
        .collect();
    let Value::Object(m) = json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    }) else {
        unreachable!()
    };
    m
}

pub fn tools() -> Vec<Tool> {
    OPS.iter()
        .map(|op| Tool::new(op.name, op.help, Arc::new(schema(op))))
        .collect()
}

/// The line that ends every reply: open notes, and the steps across the
/// listed projects that wait on the agent (sent back for a redo, or ready to
/// start because every step they come after is approved or done).
pub fn waiting_line() -> String {
    let mut open = 0;
    let mut steps = Vec::new();
    let mut broken = Vec::new();
    let dirs = match store::read_config() {
        Ok(c) => c.projects,
        Err(_) => {
            broken.push("config.toml can't be read".to_string());
            vec![]
        }
    };
    // A project's folder name, or its full path when two listed projects
    // share the name.
    let label = |dir: &std::path::Path| {
        let name = dir.file_name().unwrap_or_default();
        if dirs.iter().filter(|d| d.file_name() == Some(name)).count() > 1 {
            dir.display().to_string()
        } else {
            name.to_string_lossy().into_owned()
        }
    };
    for dir in &dirs {
        let name = label(dir);
        let Ok(p) = store::read::<Project>(&dir.join(PROJECT_FILE)) else {
            broken.push(format!("{name}: can't read project.toml"));
            continue;
        };
        open += p
            .notes
            .iter()
            .filter(|n| n.state == NoteState::Open)
            .count();
        let finished = |id: &String| {
            p.steps
                .iter()
                .any(|s| &s.id == id && matches!(s.state, State::Approved | State::Done))
        };
        for s in &p.steps {
            match s.state {
                State::Redo => steps.push(format!("{name}/{} (redo)", s.id)),
                State::Todo if s.after.iter().all(finished) => {
                    steps.push(format!("{name}/{}", s.id))
                }
                _ => {}
            }
        }
    }
    let steps = if steps.is_empty() {
        "none".to_string()
    } else {
        steps.join(", ")
    };
    let mut line = format!("Open notes: {open}. Steps waiting on you: {steps}.");
    for b in broken {
        line += &format!(" {b}.");
    }
    line
}

/// Runs one tool call through `dispatch` as `Via::Mcp`.
pub fn call(name: &str, args: Option<Map<String, Value>>) -> CallToolResult {
    let args = args.map(Value::Object).unwrap_or(Value::Null);
    let (body, ok) = match ops::dispatch(name, &args, Via::Mcp) {
        Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), true),
        Err(e) => (e, false),
    };
    let content = vec![ContentBlock::text(format!("{body}\n{}", waiting_line()))];
    if ok {
        CallToolResult::success(content)
    } else {
        CallToolResult::error(content)
    }
}

struct Server;

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "mockingbird-studio",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // On the blocking pool, so a long call (a sign-in waits for the
        // browser) doesn't stop the server answering anything else.
        tokio::task::spawn_blocking(move || call(&req.name, req.arguments))
            .await
            .map(Into::into)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }
}

/// Runs `mockingbird-studio mcp` until the client closes stdin.
pub fn run() -> ExitCode {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("mockingbird-studio mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = rt.block_on(async {
        match Server.serve(rmcp::transport::stdio()).await {
            Ok(service) => service
                .waiting()
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            // The client hung up before the handshake: a normal stop.
            Err(ServerInitializeError::ConnectionClosed(_)) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mockingbird-studio mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
