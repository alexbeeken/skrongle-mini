//! The MCP server's protocol (#168, `docs/api.md`): JSON-RPC 2.0 as MCP's
//! streamable HTTP transport carries it, answered as one JSON body per POST.
//! No session (`Mcp-Session-Id`), no SSE stream: nothing stays open on the
//! Pi. `initialize`, `ping`, `tools/list` and `tools/call`; a notification
//! (or a response a client posts) gets 202. Written by hand over
//! `serde_json`: no MCP crate.
//!
//! The HTTP gate (switch, `Origin`, method, content type, bearer token, the
//! per-grant limit, the body cap, scopes) is `routes/mcp.rs`; the tools are
//! `mcp/tools.rs` and `mcp/format.rs`.
//!
//! What a tool returns is data: course text travels as JSON values in the
//! result, never in `instructions` or a description, which are fixed strings.

use serde_json::{json, Value};

use crate::auth::api::Scope;

pub mod format;
pub mod tools;

/// The protocol versions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: [&str; 2] = [LATEST, "2025-03-26"];
const LATEST: &str = "2025-06-18";

/// The most messages one batch may hold (2025-03-26 allows batches).
pub const MAX_BATCH: usize = 10;

/// How deeply a request's JSON may nest. A request is an envelope around
/// strings (or, at most, a course file sent as JSON): far below this.
pub const MAX_DEPTH: usize = 32;

/// What `initialize` tells the agent before any tool call. Fixed: nothing a
/// course or a person wrote goes in here.
pub const INSTRUCTIONS: &str = "Skrongle runs courses written as JSON files. \
Read get_course_format first: with no section it gives the overview and the list of sections; then ask for the sections you need, \
and \"example\" for a worked course. \
Courses are named by their id, not their title: list_courses gives the ids of the courses you may work on. \
Check files with validate_course before create_course or put_course_files; \
a write that would leave the course broken is refused and changes nothing. \
Give the person the link from the result so they can open the course.";

// JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// Server-defined: the per-grant request limit.
pub const TOO_MANY_REQUESTS: i64 = -32000;

/// A tool's answer: its JSON result, and whether it is a refusal the agent
/// should read and fix (`isError`), which is not a protocol error.
#[derive(Debug)]
pub struct Reply {
    pub value: Value,
    pub is_error: bool,
}

impl Reply {
    pub fn ok(value: Value) -> Reply {
        Reply { value, is_error: false }
    }

    pub fn refused(message: impl Into<String>) -> Reply {
        Reply { value: json!({ "error": message.into() }), is_error: true }
    }

    /// A refusal with more detail (each file's error, say) beside the message.
    pub fn refused_with(message: impl Into<String>, detail: Value) -> Reply {
        let mut value = json!({ "error": message.into() });
        if let (Some(obj), Value::Object(more)) = (value.as_object_mut(), detail) {
            obj.extend(more);
        }
        Reply { value, is_error: true }
    }

    /// As MCP's `CallToolResult`: the JSON as text, and as structured content.
    fn into_result(self) -> Value {
        let text = serde_json::to_string_pretty(&self.value).unwrap_or_default();
        json!({ "content": [{ "type": "text", "text": text }], "structuredContent": self.value, "isError": self.is_error })
    }
}

/// One tool: its name, what it is for, its input schema and the scope it needs.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub scope: Scope,
    pub read_only: bool,
    schema: fn() -> Value,
}

const PATH_HELP: &str = "course.json, placement.json, lessons/<name>.json (folders allowed) or pictures/<name>.svg";

fn files_schema() -> Value {
    json!({
        "type": "array",
        "description": "Course files, each at its full path in the course.",
        "items": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": PATH_HELP },
                "content": { "type": "string", "description": "The file's text: JSON (SVG for a picture). A JSON object is taken too." },
            },
            "required": ["path", "content"],
        },
    })
}

fn course_id() -> Value {
    json!({ "type": "string", "description": "The course's id, from list_courses." })
}

/// The tools, in the order `tools/list` gives them.
pub const TOOLS: [Tool; 6] = [
    Tool {
        name: "get_course_format",
        description: "The course format contract: how course.json, lesson files and every question type are written. \
                      With no section, the overview and the list of sections; with a section, only that part. \
                      Section \"example\" lists a worked course that uses every feature; \"file\" returns one of its files.",
        scope: Scope::Read,
        read_only: true,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "section": { "type": "string", "description": "A section id from the list, or \"example\"." },
                    "file": { "type": "string", "description": "A file of the example course, such as course.json." },
                },
            })
        },
    },
    Tool {
        name: "list_courses",
        description: "The courses you may work on: the person's own (never built-in courses or courses shared with them). \
                      For each: id, title, language, lesson count, whether it loads, and its link. A link is a full address \
                      when the server has a public address set, else a path on the server the person uses.",
        scope: Scope::Read,
        read_only: true,
        schema: || json!({ "type": "object", "properties": {} }),
    },
    Tool {
        name: "get_course_files",
        description: "The files of one of the person's courses: path and content. Give \"paths\" to get only those files.",
        scope: Scope::Read,
        read_only: true,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "course_id": course_id(),
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "Only these files (optional)." },
                },
                "required": ["course_id"],
            })
        },
    },
    Tool {
        name: "validate_course",
        description: "Check course files and store nothing. \"files\" alone checks a whole new course; \"course_id\" and \"files\" \
                      check those files laid over that course; \"course_id\" alone checks the course as stored. \
                      Returns the errors the course editor shows, each naming the file and field.",
        scope: Scope::Read,
        read_only: true,
        schema: || json!({ "type": "object", "properties": { "course_id": course_id(), "files": files_schema() } }),
    },
    Tool {
        name: "create_course",
        description: "Make a new course from a whole set of files (course.json and the lessons). It must validate; nothing is \
                      stored otherwise. The course gets a new id, which the result gives with the link that opens it.",
        scope: Scope::Write,
        read_only: false,
        schema: || json!({ "type": "object", "properties": { "files": files_schema() }, "required": ["files"] }),
    },
    Tool {
        name: "put_course_files",
        description: "Replace or add files of one of the person's courses. The whole course is validated with the new files \
                      first; if any file is refused or the course would not load, nothing changes.",
        scope: Scope::Write,
        read_only: false,
        schema: || json!({ "type": "object", "properties": { "course_id": course_id(), "files": files_schema() }, "required": ["course_id", "files"] }),
    },
];

pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

fn tools_list() -> Value {
    let list: Vec<Value> = TOOLS
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": { "readOnlyHint": t.read_only, "destructiveHint": false, "openWorldHint": false },
            })
        })
        .collect();
    json!({ "tools": list })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(LATEST);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "skrongle", "title": "Skrongle", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

/// One JSON-RPC request (it has an id, so it gets an answer).
pub struct Message {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

/// One element of what was posted.
pub enum Item {
    Request(Message),
    /// A notification, or a response to a request this server never makes:
    /// nothing to answer.
    Quiet,
    /// Not a JSON-RPC message: the error answer to send for it.
    Invalid(Value),
}

/// What was posted: one message, or a batch of at most `MAX_BATCH`.
pub struct Incoming {
    pub batch: bool,
    pub items: Vec<Item>,
}

/// Does `body` nest deeper than `max` (arrays and objects, outside
/// strings)? Asked before parsing, so a deep body costs a byte scan only.
pub fn too_deep(body: &[u8], max: usize) -> bool {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &b in body {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > max {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

fn item(v: Value) -> Item {
    let Some(obj) = v.as_object() else { return Item::Invalid(error(&Value::Null, INVALID_REQUEST, "Each message must be a JSON object.")) };
    let id = obj.get("id").cloned();
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Item::Invalid(error(&id.unwrap_or(Value::Null), INVALID_REQUEST, "jsonrpc must be \"2.0\"."));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        if obj.contains_key("result") || obj.contains_key("error") {
            return Item::Quiet;
        }
        return Item::Invalid(error(&id.unwrap_or(Value::Null), INVALID_REQUEST, "No method."));
    };
    match id {
        None => Item::Quiet,
        Some(id) => Item::Request(Message { id, method: method.to_string(), params: obj.get("params").cloned().unwrap_or(Value::Null) }),
    }
}

/// Parse a POST body. `Err` is the one JSON-RPC error answer to send: not
/// JSON, nested past `MAX_DEPTH`, an empty batch or one over `MAX_BATCH`.
pub fn parse(body: &[u8]) -> Result<Incoming, Value> {
    if too_deep(body, MAX_DEPTH) {
        return Err(error(&Value::Null, PARSE_ERROR, "The JSON is nested too deeply."));
    }
    let v: Value = serde_json::from_slice(body).map_err(|_| error(&Value::Null, PARSE_ERROR, "The body is not valid JSON."))?;
    match v {
        Value::Array(list) if list.is_empty() => Err(error(&Value::Null, INVALID_REQUEST, "An empty batch.")),
        Value::Array(list) if list.len() > MAX_BATCH => {
            Err(error(&Value::Null, INVALID_REQUEST, &format!("A batch may hold at most {MAX_BATCH} messages.")))
        }
        Value::Array(list) => Ok(Incoming { batch: true, items: list.into_iter().map(item).collect() }),
        other => Ok(Incoming { batch: false, items: vec![item(other)] }),
    }
}

pub fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: &Value, result: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The scope a message needs before it runs: a `tools/call`'s tool's.
pub fn needed_scope(m: &Message) -> Option<Scope> {
    if m.method != "tools/call" {
        return None;
    }
    m.params.get("name").and_then(Value::as_str).and_then(tool).map(|t| t.scope)
}

/// Answer one request. A failure inside a tool (the database, say) is a
/// fixed message here and the detail goes to the log only.
pub async fn answer(ctx: &tools::Ctx<'_>, m: Message) -> Value {
    let id = m.id;
    match m.method.as_str() {
        "initialize" => result(&id, &initialize(&m.params)),
        "ping" => result(&id, &json!({})),
        "tools/list" => result(&id, &tools_list()),
        "tools/call" => {
            let Some(name) = m.params.get("name").and_then(Value::as_str) else {
                return error(&id, INVALID_PARAMS, "tools/call needs a tool name.");
            };
            let args = match m.params.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(v @ Value::Object(_)) => v.clone(),
                Some(_) => return error(&id, INVALID_PARAMS, "arguments must be an object."),
            };
            match tools::call(ctx, name, &args).await {
                Ok(Some(reply)) => result(&id, &reply.into_result()),
                Ok(None) => error(&id, INVALID_PARAMS, "Unknown tool. tools/list names them."),
                Err(e) => {
                    tracing::error!("mcp: {name} for account {} failed: {e}", ctx.user.id);
                    error(&id, INTERNAL_ERROR, "Internal error.")
                }
            }
        }
        _ => error(&id, METHOD_NOT_FOUND, "Method not found."),
    }
}

#[cfg(test)]
mod tests;
