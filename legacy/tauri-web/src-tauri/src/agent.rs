//! Agent runtime: spawns Claude Code or Codex in headless streaming mode and
//! normalizes both event protocols into one shape the UI understands.
//!
//! * Claude Code: `claude -p --output-format stream-json` — the protocol the
//!   Claude Agent SDK is built on; auth comes from your Claude Code login.
//! * Codex: `codex exec --json` — the protocol the Codex SDK is built on;
//!   auth comes from your "Sign in with ChatGPT" login.

use crate::resolve;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::ipc::Channel;
use tauri::State;

#[derive(Default)]
pub struct Runs(pub Arc<Mutex<HashMap<String, Child>>>);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    run_id: String,
    provider: String,
    prompt: String,
    cwd: Option<String>,
    model: Option<String>,
    /// "plan" (read-only) | "build" (edits allowed) | "auto" (full autonomy)
    mode: String,
    resume_id: Option<String>,
    effort: Option<String>,
}

#[tauri::command]
pub fn start_run(
    req: RunRequest,
    on_event: Channel<Value>,
    runs: State<'_, Runs>,
) -> Result<(), String> {
    let exe_name = match req.provider.as_str() {
        "claude" => "claude",
        "codex" => "codex",
        p => return Err(format!("Unknown provider: {p}")),
    };
    let exe = resolve::find(exe_name).ok_or_else(|| {
        format!("`{exe_name}` was not found on PATH. Install it and sign in first.")
    })?;

    let mut cmd = resolve::command(&exe);
    let cwd = req.cwd.clone().filter(|c| !c.trim().is_empty());
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }
    let model = req.model.clone().filter(|m| !m.trim().is_empty() && m != "default");
    let resume = req.resume_id.clone().filter(|r| !r.trim().is_empty());

    if req.provider == "claude" {
        cmd.args([
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
        ]);
        let perm = match req.mode.as_str() {
            "plan" => "plan",
            "auto" => "bypassPermissions",
            _ => "acceptEdits",
        };
        cmd.args(["--permission-mode", perm]);
        if let Some(m) = &model {
            cmd.args(["--model", m]);
        }
        if let Some(e) = req.effort.as_deref().filter(|e| !e.is_empty() && *e != "default") {
            cmd.args(["--effort", e]);
        }
        if let Some(r) = &resume {
            cmd.args(["--resume", r]);
        }
    } else {
        cmd.arg("exec");
        cmd.args(["--json", "--skip-git-repo-check"]);
        let sandbox = match req.mode.as_str() {
            "plan" => "read-only",
            "auto" => "danger-full-access",
            _ => "workspace-write",
        };
        cmd.args(["-s", sandbox]);
        if let Some(dir) = &cwd {
            cmd.args(["-C", dir]);
        }
        if let Some(m) = &model {
            cmd.args(["-m", m]);
        }
        if let Some(e) = req.effort.as_deref().filter(|e| !e.is_empty() && *e != "default") {
            cmd.args(["-c", &format!("model_reasoning_effort=\"{e}\"")]);
        }
        if let Some(r) = &resume {
            cmd.args(["resume", r]);
        }
        cmd.arg("-"); // prompt from stdin
    }

    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Failed to start {exe_name}: {e}"))?;

    // Prompt goes through stdin: no shell quoting issues, no length limits.
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = req.prompt.clone();
        thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let run_id = req.run_id.clone();
    runs.0.lock().unwrap().insert(run_id.clone(), child);

    // Collect stderr for error reporting.
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    {
        let buf = stderr_buf.clone();
        thread::spawn(move || {
            let mut s = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut s);
            *buf.lock().unwrap() = s;
        });
    }

    let runs_map = runs.0.clone();
    let provider = req.provider.clone();
    thread::spawn(move || {
        let mut saw_terminal = false;
        let mut parser = Parser::default();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let events = if provider == "claude" {
                parser.claude(&v)
            } else {
                parser.codex(&v)
            };
            for ev in events {
                if ev["kind"] == "done" {
                    saw_terminal = true;
                }
                let _ = on_event.send(ev);
            }
        }

        // Reap the process (it may already have been killed by stop_run).
        let child = runs_map.lock().unwrap().remove(&run_id);
        let code = child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code());

        // Give the stderr reader a moment to finish.
        thread::sleep(std::time::Duration::from_millis(60));
        if !saw_terminal && code.unwrap_or(0) != 0 {
            let err = stderr_buf.lock().unwrap().clone();
            let tail: String = err
                .lines()
                .filter(|l| !l.trim().is_empty())
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            if !tail.is_empty() {
                let _ = on_event.send(json!({ "kind": "error", "message": tail }));
            }
        }
        let _ = on_event.send(json!({ "kind": "exit", "code": code }));
    });

    Ok(())
}

#[tauri::command]
pub fn stop_run(run_id: String, runs: State<'_, Runs>) -> Result<(), String> {
    if let Some(child) = runs.0.lock().unwrap().get_mut(&run_id) {
        child.kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ─────────────────────────────── normalization ───────────────────────────────
//
// Normalized events (all carry `kind`):
//   session      { sessionId, model? }
//   text_start / text_delta { text } / text_final { text }
//   thinking_start / thinking_delta { text } / thinking_final { text }
//   tool_start   { id, name, input }
//   tool_update  { id, output }
//   tool_end     { id, output, isError }
//   todo         { items: [{ text, done, active }] }
//   notice       { text }
//   error        { message }
//   done         { sessionId?, costUsd?, inputTokens?, outputTokens?, durationMs?, isError }
//   exit         { code }   (always last, emitted by the runtime)

#[derive(Default)]
struct Parser {
    session_id: Option<String>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(t) => t.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("text") => p["text"].as_str().map(str::to_string),
                Some("image") => Some("[image]".into()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

impl Parser {
    fn claude(&mut self, v: &Value) -> Vec<Value> {
        let mut out = vec![];
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => {
                self.session_id = v["session_id"].as_str().map(str::to_string);
                out.push(json!({
                    "kind": "session",
                    "sessionId": v["session_id"],
                    "model": v["model"],
                }));
            }
            "stream_event" => {
                let ev = &v["event"];
                match ev["type"].as_str().unwrap_or_default() {
                    "content_block_start" => match ev["content_block"]["type"].as_str() {
                        Some("text") => out.push(json!({ "kind": "text_start" })),
                        Some("thinking") => out.push(json!({ "kind": "thinking_start" })),
                        _ => {}
                    },
                    "content_block_delta" => {
                        let d = &ev["delta"];
                        match d["type"].as_str() {
                            Some("text_delta") => {
                                out.push(json!({ "kind": "text_delta", "text": d["text"] }))
                            }
                            Some("thinking_delta") => out.push(
                                json!({ "kind": "thinking_delta", "text": d["thinking"] }),
                            ),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            "assistant" => {
                if let Some(blocks) = v["message"]["content"].as_array() {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => {
                                out.push(json!({ "kind": "text_final", "text": b["text"] }))
                            }
                            Some("tool_use") => out.push(json!({
                                "kind": "tool_start",
                                "id": b["id"],
                                "name": b["name"],
                                "input": b["input"],
                            })),
                            _ => {}
                        }
                    }
                }
            }
            "user" => {
                if let Some(blocks) = v["message"]["content"].as_array() {
                    for b in blocks {
                        if b["type"] == "tool_result" {
                            out.push(json!({
                                "kind": "tool_end",
                                "id": b["tool_use_id"],
                                "output": tool_result_text(&b["content"]),
                                "isError": b["is_error"].as_bool().unwrap_or(false),
                            }));
                        }
                    }
                }
            }
            "result" => {
                let is_error = v["is_error"].as_bool().unwrap_or(false);
                if is_error {
                    let msg = v["result"]
                        .as_str()
                        .map(str::to_string)
                        .or_else(|| v["errors"].as_array().map(|e| {
                            e.iter().map(|x| x.as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n")
                        }))
                        .unwrap_or_else(|| s(&v["subtype"]));
                    out.push(json!({ "kind": "error", "message": msg }));
                }
                let u = &v["usage"];
                let input = u["input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_read_input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                out.push(json!({
                    "kind": "done",
                    "sessionId": v["session_id"].as_str().map(str::to_string).or(self.session_id.clone()),
                    "costUsd": v["total_cost_usd"],
                    "inputTokens": input,
                    "outputTokens": u["output_tokens"],
                    "durationMs": v["duration_ms"],
                    "isError": is_error,
                }));
            }
            _ => {}
        }
        out
    }

    fn codex(&mut self, v: &Value) -> Vec<Value> {
        let mut out = vec![];
        let ty = v["type"].as_str().unwrap_or_default();
        match ty {
            "thread.started" => {
                self.session_id = v["thread_id"].as_str().map(str::to_string);
                out.push(json!({ "kind": "session", "sessionId": v["thread_id"] }));
            }
            "item.started" | "item.updated" | "item.completed" => {
                let item = &v["item"];
                let id = item["id"].clone();
                let done = ty == "item.completed";
                match item["type"].as_str().unwrap_or_default() {
                    "agent_message" if done => {
                        out.push(json!({ "kind": "text_final", "text": item["text"] }))
                    }
                    "reasoning" if done => {
                        out.push(json!({ "kind": "thinking_final", "text": item["text"] }))
                    }
                    "command_execution" => {
                        if ty == "item.started" {
                            out.push(json!({
                                "kind": "tool_start", "id": id, "name": "shell",
                                "input": { "command": item["command"] },
                            }));
                        } else if done {
                            let failed = item["status"] == "failed"
                                || item["exit_code"].as_i64().map(|c| c != 0).unwrap_or(false);
                            out.push(json!({
                                "kind": "tool_start", "id": id, "name": "shell",
                                "input": { "command": item["command"] },
                            }));
                            out.push(json!({
                                "kind": "tool_end", "id": id,
                                "output": item["aggregated_output"], "isError": failed,
                            }));
                        } else {
                            out.push(json!({
                                "kind": "tool_update", "id": id, "output": item["aggregated_output"],
                            }));
                        }
                    }
                    "file_change" if done => {
                        out.push(json!({
                            "kind": "tool_start", "id": id, "name": "file_change",
                            "input": { "changes": item["changes"] },
                        }));
                        out.push(json!({
                            "kind": "tool_end", "id": id, "output": "",
                            "isError": item["status"] == "failed",
                        }));
                    }
                    "mcp_tool_call" => {
                        let name = format!("mcp:{}/{}", s(&item["server"]), s(&item["tool"]));
                        out.push(json!({
                            "kind": "tool_start", "id": id, "name": name,
                            "input": item.get("arguments").cloned().unwrap_or(Value::Null),
                        }));
                        if done {
                            let output = item["result"]["content"].clone();
                            out.push(json!({
                                "kind": "tool_end", "id": id,
                                "output": tool_result_text(&output),
                                "isError": item["status"] == "failed",
                            }));
                        }
                    }
                    "web_search" => {
                        out.push(json!({
                            "kind": "tool_start", "id": id, "name": "web_search",
                            "input": { "query": item["query"] },
                        }));
                        if done {
                            out.push(json!({ "kind": "tool_end", "id": id, "output": "", "isError": false }));
                        }
                    }
                    "todo_list" => {
                        let items: Vec<Value> = item["items"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|t| json!({ "text": t["text"], "done": t["completed"].as_bool().unwrap_or(false), "active": false }))
                                    .collect()
                            })
                            .unwrap_or_default();
                        out.push(json!({ "kind": "todo", "items": items }));
                    }
                    "error" if done => {
                        let msg = s(&item["message"]);
                        // Codex surfaces local hook warnings as error items — show them softly.
                        out.push(json!({ "kind": "notice", "text": msg }));
                    }
                    _ => {}
                }
            }
            "turn.completed" => {
                let u = &v["usage"];
                out.push(json!({
                    "kind": "done",
                    "sessionId": self.session_id,
                    "inputTokens": u["input_tokens"],
                    "outputTokens": u["output_tokens"],
                    "isError": false,
                }));
            }
            "turn.failed" => {
                out.push(json!({ "kind": "error", "message": v["error"]["message"] }));
                out.push(json!({ "kind": "done", "sessionId": self.session_id, "isError": true }));
            }
            "error" => {
                // A turn.failed usually follows with the same message; the UI de-dupes.
                out.push(json!({ "kind": "error", "message": v["message"] }));
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a captured stream through the parser: `OA_FIXTURE=path OA_PROVIDER=claude cargo test -- --nocapture`
    #[test]
    fn parse_fixture() {
        let Ok(path) = std::env::var("OA_FIXTURE") else { return };
        let provider = std::env::var("OA_PROVIDER").unwrap_or_else(|_| "claude".into());
        let mut p = Parser::default();
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let evs = if provider == "claude" { p.claude(&v) } else { p.codex(&v) };
            for e in evs {
                let mut s = e.to_string();
                s.truncate(160);
                println!("{s}");
            }
        }
    }
}
