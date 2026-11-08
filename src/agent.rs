//! Agent runtime: spawns Claude Code or Codex in headless streaming mode and
//! normalizes both event protocols into one `AgentEvent` stream.
//!
//! * Claude Code: `claude -p --output-format stream-json` — the protocol the
//!   Claude Agent SDK is built on; auth comes from your Claude Code login.
//! * Codex: `codex exec --json` — the protocol the Codex SDK is built on;
//!   auth comes from your "Sign in with ChatGPT" login.

use crate::model::{Mode, Provider, TodoItem};
use crate::resolve;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Session {
        session_id: String,
        model: Option<String>,
    },
    TextStart,
    TextDelta(String),
    TextFinal(String),
    ThinkingStart,
    ThinkingDelta(String),
    ThinkingFinal(String),
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },
    ToolUpdate {
        id: String,
        output: String,
    },
    ToolEnd {
        id: String,
        output: String,
        is_error: bool,
    },
    Todo(Vec<TodoItem>),
    Notice(String),
    Error(String),
    Done {
        session_id: Option<String>,
        cost_usd: Option<f64>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        duration_ms: Option<u64>,
        is_error: bool,
    },
    Exit(Option<i32>),
}

pub struct RunRequest {
    pub run_id: u64,
    pub provider: Provider,
    pub prompt: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Mode,
    pub resume_id: Option<String>,
}

/// Events flowing back to the UI thread, tagged with their run id.
pub type EventTx = Sender<(u64, AgentEvent)>;

#[derive(Default, Clone)]
pub struct Runs(Arc<Mutex<HashMap<u64, Child>>>);

impl Runs {
    pub fn stop(&self, run_id: u64) {
        if let Some(child) = self.0.lock().unwrap().get_mut(&run_id) {
            let _ = child.kill();
        }
    }
}

/// Start a run. `wake` is called after each event so the UI repaints only when needed.
pub fn start(
    req: RunRequest,
    tx: EventTx,
    runs: Runs,
    wake: impl Fn() + Send + Clone + 'static,
) -> Result<(), String> {
    let exe_name = req.provider.exe();
    let exe = resolve::find(exe_name).ok_or_else(|| {
        format!("`{exe_name}` was not found on PATH. Install it and sign in first.")
    })?;

    let mut cmd = resolve::command(&exe);
    let cwd = req.cwd.clone().filter(|c| !c.trim().is_empty());
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }
    let model = req
        .model
        .clone()
        .filter(|m| !m.is_empty() && m != "default");
    let effort = req
        .effort
        .clone()
        .filter(|e| !e.is_empty() && e != "default");
    let resume = req.resume_id.clone().filter(|r| !r.is_empty());

    match req.provider {
        Provider::Claude => {
            cmd.args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]);
            let perm = match req.mode {
                Mode::Plan => "plan",
                Mode::Auto => "bypassPermissions",
                Mode::Build => "acceptEdits",
            };
            cmd.args(["--permission-mode", perm]);
            if let Some(m) = &model {
                cmd.args(["--model", m]);
            }
            if let Some(e) = &effort {
                cmd.args(["--effort", e]);
            }
            if let Some(r) = &resume {
                cmd.args(["--resume", r]);
            }
        }
        Provider::Codex => {
            cmd.args(["exec", "--json", "--skip-git-repo-check"]);
            let sandbox = match req.mode {
                Mode::Plan => "read-only",
                Mode::Auto => "danger-full-access",
                Mode::Build => "workspace-write",
            };
            cmd.args(["-s", sandbox]);
            if let Some(dir) = &cwd {
                cmd.args(["-C", dir]);
            }
            if let Some(m) = &model {
                cmd.args(["-m", m]);
            }
            if let Some(e) = &effort {
                cmd.args(["-c", &format!("model_reasoning_effort=\"{e}\"")]);
            }
            if let Some(r) = &resume {
                cmd.args(["resume", r]);
            }
            cmd.arg("-"); // prompt from stdin
        }
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start {exe_name}: {e}"))?;

    // Prompt goes through stdin: no shell quoting issues, no length limits.
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = req.prompt.clone();
        thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let run_id = req.run_id;
    runs.0.lock().unwrap().insert(run_id, child);

    let stderr_buf = Arc::new(Mutex::new(String::new()));
    {
        let buf = stderr_buf.clone();
        thread::spawn(move || {
            let mut s = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut s);
            *buf.lock().unwrap() = s;
        });
    }

    let provider = req.provider;
    thread::spawn(move || {
        let send = |e: AgentEvent| {
            let _ = tx.send((run_id, e));
            wake();
        };
        let mut saw_done = false;
        let mut parser = Parser::default();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            let events = match provider {
                Provider::Claude => parser.claude(&v),
                Provider::Codex => parser.codex(&v),
            };
            for ev in events {
                saw_done |= matches!(ev, AgentEvent::Done { .. });
                send(ev);
            }
        }

        let child = runs.0.lock().unwrap().remove(&run_id);
        let code = child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code());
        thread::sleep(std::time::Duration::from_millis(60));
        if !saw_done && code.unwrap_or(0) != 0 {
            let err = stderr_buf.lock().unwrap().clone();
            let lines: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
            let tail = lines[lines.len().saturating_sub(6)..].join("\n");
            if !tail.is_empty() {
                send(AgentEvent::Error(tail));
            }
        }
        send(AgentEvent::Exit(code));
    });

    Ok(())
}

// ─────────────────────────────── normalization ───────────────────────────────

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
    fn claude(&mut self, v: &Value) -> Vec<AgentEvent> {
        use AgentEvent::*;
        let mut out = vec![];
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => {
                self.session_id = v["session_id"].as_str().map(str::to_string);
                out.push(Session {
                    session_id: s(&v["session_id"]),
                    model: v["model"].as_str().map(str::to_string),
                });
            }
            "stream_event" => {
                let ev = &v["event"];
                match ev["type"].as_str().unwrap_or_default() {
                    "content_block_start" => match ev["content_block"]["type"].as_str() {
                        Some("text") => out.push(TextStart),
                        Some("thinking") => out.push(ThinkingStart),
                        _ => {}
                    },
                    "content_block_delta" => {
                        let d = &ev["delta"];
                        match d["type"].as_str() {
                            Some("text_delta") => out.push(TextDelta(s(&d["text"]))),
                            Some("thinking_delta") => out.push(ThinkingDelta(s(&d["thinking"]))),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            "assistant" => {
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => out.push(TextFinal(s(&b["text"]))),
                        Some("tool_use") => out.push(ToolStart {
                            id: s(&b["id"]),
                            name: s(&b["name"]),
                            input: b["input"].clone(),
                        }),
                        _ => {}
                    }
                }
            }
            "user" => {
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_result" {
                        out.push(ToolEnd {
                            id: s(&b["tool_use_id"]),
                            output: tool_result_text(&b["content"]),
                            is_error: b["is_error"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
            "result" => {
                let is_error = v["is_error"].as_bool().unwrap_or(false);
                if is_error {
                    let msg = v["result"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| s(&v["subtype"]));
                    out.push(Error(msg));
                }
                let u = &v["usage"];
                let input = u["input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_read_input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                out.push(Done {
                    session_id: v["session_id"]
                        .as_str()
                        .map(str::to_string)
                        .or(self.session_id.clone()),
                    cost_usd: v["total_cost_usd"].as_f64(),
                    input_tokens: Some(input),
                    output_tokens: u["output_tokens"].as_u64(),
                    duration_ms: v["duration_ms"].as_u64(),
                    is_error,
                });
            }
            _ => {}
        }
        out
    }

    fn codex(&mut self, v: &Value) -> Vec<AgentEvent> {
        use AgentEvent::*;
        let mut out = vec![];
        let ty = v["type"].as_str().unwrap_or_default();
        match ty {
            "thread.started" => {
                self.session_id = v["thread_id"].as_str().map(str::to_string);
                out.push(Session {
                    session_id: s(&v["thread_id"]),
                    model: None,
                });
            }
            "item.started" | "item.updated" | "item.completed" => {
                let item = &v["item"];
                let id = s(&item["id"]);
                let done = ty == "item.completed";
                match item["type"].as_str().unwrap_or_default() {
                    "agent_message" if done => out.push(TextFinal(s(&item["text"]))),
                    "reasoning" if done => out.push(ThinkingFinal(s(&item["text"]))),
                    "command_execution" => {
                        let start = ToolStart {
                            id: id.clone(),
                            name: "shell".into(),
                            input: serde_json::json!({ "command": item["command"] }),
                        };
                        if ty == "item.started" {
                            out.push(start);
                        } else if done {
                            let failed = item["status"] == "failed"
                                || item["exit_code"].as_i64().is_some_and(|c| c != 0);
                            out.push(start);
                            out.push(ToolEnd {
                                id,
                                output: s(&item["aggregated_output"]),
                                is_error: failed,
                            });
                        } else {
                            out.push(ToolUpdate {
                                id,
                                output: s(&item["aggregated_output"]),
                            });
                        }
                    }
                    "file_change" if done => {
                        out.push(ToolStart {
                            id: id.clone(),
                            name: "file_change".into(),
                            input: serde_json::json!({ "changes": item["changes"] }),
                        });
                        out.push(ToolEnd {
                            id,
                            output: String::new(),
                            is_error: item["status"] == "failed",
                        });
                    }
                    "mcp_tool_call" => {
                        out.push(ToolStart {
                            id: id.clone(),
                            name: format!("mcp:{}/{}", s(&item["server"]), s(&item["tool"])),
                            input: item.get("arguments").cloned().unwrap_or(Value::Null),
                        });
                        if done {
                            out.push(ToolEnd {
                                id,
                                output: tool_result_text(&item["result"]["content"]),
                                is_error: item["status"] == "failed",
                            });
                        }
                    }
                    "web_search" => {
                        out.push(ToolStart {
                            id: id.clone(),
                            name: "web_search".into(),
                            input: serde_json::json!({ "query": item["query"] }),
                        });
                        if done {
                            out.push(ToolEnd {
                                id,
                                output: String::new(),
                                is_error: false,
                            });
                        }
                    }
                    "todo_list" => {
                        let items = item["items"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|t| TodoItem {
                                text: s(&t["text"]),
                                done: t["completed"].as_bool().unwrap_or(false),
                                active: false,
                            })
                            .collect();
                        out.push(Todo(items));
                    }
                    // Codex surfaces local hook warnings as error items — show them softly.
                    "error" if done => out.push(Notice(s(&item["message"]))),
                    _ => {}
                }
            }
            "turn.completed" => {
                let u = &v["usage"];
                out.push(Done {
                    session_id: self.session_id.clone(),
                    cost_usd: None,
                    input_tokens: u["input_tokens"].as_u64(),
                    output_tokens: u["output_tokens"].as_u64(),
                    duration_ms: None,
                    is_error: false,
                });
            }
            "turn.failed" => {
                out.push(Error(s(&v["error"]["message"])));
                out.push(Done {
                    session_id: self.session_id.clone(),
                    cost_usd: None,
                    input_tokens: None,
                    output_tokens: None,
                    duration_ms: None,
                    is_error: true,
                });
            }
            "error" => out.push(Error(s(&v["message"]))),
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `OA_FIXTURE=path OA_PROVIDER=claude|codex cargo test -- --nocapture`
    #[test]
    fn parse_fixture() {
        let Ok(path) = std::env::var("OA_FIXTURE") else {
            return;
        };
        let codex = std::env::var("OA_PROVIDER").is_ok_and(|p| p == "codex");
        let mut p = Parser::default();
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            for e in if codex { p.codex(&v) } else { p.claude(&v) } {
                let mut s = format!("{e:?}");
                s.truncate(150);
                println!("{s}");
            }
        }
    }
}
