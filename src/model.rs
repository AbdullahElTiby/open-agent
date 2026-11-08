//! Sessions, messages, the event reducer, cross-agent relay and persistence.

use crate::agent::AgentEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn exe(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
        }
    }
    pub fn other(self) -> Self {
        match self {
            Provider::Claude => Provider::Codex,
            Provider::Codex => Provider::Claude,
        }
    }
    pub fn efforts(self) -> &'static [&'static str] {
        match self {
            Provider::Claude => &["default", "low", "medium", "high", "xhigh", "max"],
            Provider::Codex => &["default", "low", "medium", "high", "xhigh"],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Plan,
    Build,
    Auto,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Plan, Mode::Build, Mode::Auto];
    pub fn label(self) -> &'static str {
        match self {
            Mode::Plan => "Read only",
            Mode::Build => "Auto",
            Mode::Auto => "Full access",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Mode::Plan => "Reads and plans. Never edits files or runs commands that change things.",
            Mode::Build => "Edits files in the workspace. Codex runs commands in a sandbox; Claude can't run shell.",
            Mode::Auto => "No approvals, no sandbox. Only use on code you can roll back.",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Running,
    Ok,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub active: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Info,
    Error,
    Relay,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Part {
    Text {
        text: String,
        #[serde(default)]
        open: bool,
    },
    Thinking {
        text: String,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        started: u64,
        #[serde(default)]
        ended: u64,
    },
    Tool {
        id: String,
        name: String,
        input: Value,
        #[serde(default)]
        output: String,
        status: ToolStatus,
    },
    Todo {
        items: Vec<TodoItem>,
    },
    Notice {
        text: String,
        tone: Tone,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum MsgStatus {
    Streaming,
    Done,
    Error,
    Stopped,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Meta {
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub id: u64,
    pub role: Role,
    pub provider: Provider,
    pub parts: Vec<Part>,
    pub created: u64,
    pub status: MsgStatus,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub id: u64,
    pub title: String,
    pub cwd: String,
    pub provider: Provider,
    pub mode: Mode,
    pub model_claude: String,
    pub model_codex: String,
    pub effort_claude: String,
    pub effort_codex: String,
    #[serde(default)]
    pub thread_claude: Option<String>,
    #[serde(default)]
    pub thread_codex: Option<String>,
    pub messages: Vec<Message>,
    pub created: u64,
    pub updated: u64,
}

impl Session {
    pub fn new(id: u64, prev: Option<&Session>) -> Self {
        Session {
            id,
            title: "New thread".into(),
            cwd: prev.map(|p| p.cwd.clone()).unwrap_or_default(),
            provider: prev.map(|p| p.provider).unwrap_or(Provider::Claude),
            mode: prev.map(|p| p.mode).unwrap_or(Mode::Build),
            model_claude: prev
                .map(|p| p.model_claude.clone())
                .unwrap_or_else(|| "default".into()),
            model_codex: prev
                .map(|p| p.model_codex.clone())
                .unwrap_or_else(|| "default".into()),
            effort_claude: prev
                .map(|p| p.effort_claude.clone())
                .unwrap_or_else(|| "default".into()),
            effort_codex: prev
                .map(|p| p.effort_codex.clone())
                .unwrap_or_else(|| "default".into()),
            thread_claude: None,
            thread_codex: None,
            messages: vec![],
            created: now_ms(),
            updated: now_ms(),
        }
    }
    pub fn model(&self, p: Provider) -> &str {
        match p {
            Provider::Claude => &self.model_claude,
            Provider::Codex => &self.model_codex,
        }
    }
    pub fn model_mut(&mut self, p: Provider) -> &mut String {
        match p {
            Provider::Claude => &mut self.model_claude,
            Provider::Codex => &mut self.model_codex,
        }
    }
    pub fn effort(&self, p: Provider) -> &str {
        match p {
            Provider::Claude => &self.effort_claude,
            Provider::Codex => &self.effort_codex,
        }
    }
    pub fn effort_mut(&mut self, p: Provider) -> &mut String {
        match p {
            Provider::Claude => &mut self.effort_claude,
            Provider::Codex => &mut self.effort_codex,
        }
    }
    pub fn thread(&self, p: Provider) -> Option<&String> {
        match p {
            Provider::Claude => self.thread_claude.as_ref(),
            Provider::Codex => self.thread_codex.as_ref(),
        }
    }
    pub fn set_thread(&mut self, p: Provider, id: String) {
        match p {
            Provider::Claude => self.thread_claude = Some(id),
            Provider::Codex => self.thread_codex = Some(id),
        }
    }
    pub fn clear_threads(&mut self) {
        self.thread_claude = None;
        self.thread_codex = None;
    }
}

/// The short "target" of a tool call: a path, pattern, command or query.
pub fn tool_target(input: &Value) -> String {
    for k in [
        "file_path",
        "path",
        "notebook_path",
        "pattern",
        "command",
        "query",
        "url",
        "description",
        "skill",
    ] {
        if let Some(v) = input.get(k).and_then(|v| v.as_str()) {
            return v.to_string();
        }
    }
    String::new()
}

/// When a session switches to an agent that has no thread yet, hand it the conversation so far.
pub fn relay_context(s: &Session, to: Provider) -> Option<String> {
    if s.thread(to).is_some() || s.messages.is_empty() {
        return None;
    }
    let mut lines = vec![];
    for m in s.messages.iter().rev().take(14).rev() {
        let text = m
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text, .. } => Some(text.clone()),
                Part::Tool { name, input, .. } if name != "__hidden" => {
                    let t = tool_target(input);
                    Some(if t.is_empty() {
                        format!("[{name}]")
                    } else {
                        format!("[{name} {t}]")
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            let who = match m.role {
                Role::User => "USER".to_string(),
                Role::Agent => format!("ASSISTANT ({})", m.provider.label()),
            };
            lines.push(format!("{who}: {}", text.trim()));
        }
    }
    let mut ctx = lines.join("\n\n");
    if ctx.len() > 8000 {
        let mut cut = ctx.len() - 8000;
        while !ctx.is_char_boundary(cut) {
            cut += 1;
        }
        ctx = format!("…{}", &ctx[cut..]);
    }
    Some(format!(
        "<previous_conversation note=\"This thread was handed over to you from another coding agent working in the same workspace. Continue from here.\">\n{ctx}\n</previous_conversation>\n\n"
    ))
}

fn close_open(parts: &mut [Part]) {
    if let Some(last) = parts.last_mut() {
        match last {
            Part::Text { open, .. } => *open = false,
            Part::Thinking { open, ended, .. } if *open => {
                *open = false;
                *ended = now_ms();
            }
            _ => {}
        }
    }
}

fn finish_tools(parts: &mut [Part], status: ToolStatus) {
    for p in parts.iter_mut() {
        if let Part::Tool {
            status: st @ ToolStatus::Running,
            ..
        } = p
        {
            *st = status;
        }
    }
}

/// Fold one agent event into the streaming agent message.
pub fn apply_event(msg: &mut Message, e: &AgentEvent) {
    let parts = &mut msg.parts;
    match e {
        AgentEvent::TextStart => {
            close_open(parts);
            parts.push(Part::Text {
                text: String::new(),
                open: true,
            });
        }
        AgentEvent::TextDelta(t) => match parts.last_mut() {
            Some(Part::Text { text, open: true }) => text.push_str(t),
            _ => {
                close_open(parts);
                parts.push(Part::Text {
                    text: t.clone(),
                    open: true,
                });
            }
        },
        AgentEvent::TextFinal(t) => match parts.last_mut() {
            Some(Part::Text { text, open }) if *open => {
                *text = t.clone();
                *open = false;
            }
            _ if !t.trim().is_empty() => {
                close_open(parts);
                parts.push(Part::Text {
                    text: t.clone(),
                    open: false,
                });
            }
            _ => {}
        },
        AgentEvent::ThinkingStart => {
            close_open(parts);
            parts.push(Part::Thinking {
                text: String::new(),
                open: true,
                started: now_ms(),
                ended: 0,
            });
        }
        AgentEvent::ThinkingDelta(t) => match parts.last_mut() {
            Some(Part::Thinking {
                text, open: true, ..
            }) => text.push_str(t),
            _ => {
                close_open(parts);
                parts.push(Part::Thinking {
                    text: t.clone(),
                    open: true,
                    started: now_ms(),
                    ended: 0,
                });
            }
        },
        AgentEvent::ThinkingFinal(t) => match parts.last_mut() {
            Some(Part::Thinking {
                text, open, ended, ..
            }) if *open => {
                if !t.is_empty() {
                    *text = t.clone();
                }
                *open = false;
                *ended = now_ms();
            }
            _ if !t.trim().is_empty() => {
                close_open(parts);
                parts.push(Part::Thinking {
                    text: t.clone(),
                    open: false,
                    started: 0,
                    ended: 0,
                });
            }
            _ => {}
        },
        AgentEvent::ToolStart { id, name, input } => {
            if parts
                .iter()
                .any(|p| matches!(p, Part::Tool { id: i, .. } if i == id))
            {
                return;
            }
            close_open(parts);
            // Claude's TodoWrite becomes a live checklist instead of a tool row.
            if name == "TodoWrite" {
                if let Some(todos) = input.get("todos").and_then(|t| t.as_array()) {
                    let items = todos
                        .iter()
                        .map(|t| {
                            let status = t["status"].as_str().unwrap_or_default();
                            let text = if status == "in_progress" {
                                t["activeForm"].as_str().or(t["content"].as_str())
                            } else {
                                t["content"].as_str()
                            };
                            TodoItem {
                                text: text.unwrap_or_default().to_string(),
                                done: status == "completed",
                                active: status == "in_progress",
                            }
                        })
                        .collect();
                    parts.retain(|p| !matches!(p, Part::Todo { .. }));
                    parts.push(Part::Todo { items });
                    parts.push(Part::Tool {
                        id: id.clone(),
                        name: "__hidden".into(),
                        input: Value::Null,
                        output: String::new(),
                        status: ToolStatus::Running,
                    });
                    return;
                }
            }
            parts.push(Part::Tool {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                output: String::new(),
                status: ToolStatus::Running,
            });
        }
        AgentEvent::ToolUpdate { id, output } => {
            for p in parts.iter_mut() {
                if let Part::Tool {
                    id: i, output: o, ..
                } = p
                {
                    if i == id {
                        *o = output.clone();
                    }
                }
            }
        }
        AgentEvent::ToolEnd {
            id,
            output,
            is_error,
        } => {
            for p in parts.iter_mut() {
                if let Part::Tool {
                    id: i,
                    output: o,
                    status,
                    ..
                } = p
                {
                    if i == id {
                        *o = output.clone();
                        *status = if *is_error {
                            ToolStatus::Error
                        } else {
                            ToolStatus::Ok
                        };
                    }
                }
            }
        }
        AgentEvent::Todo(items) => {
            if let Some(Part::Todo { items: it }) =
                parts.iter_mut().find(|p| matches!(p, Part::Todo { .. }))
            {
                *it = items.clone();
            } else {
                parts.push(Part::Todo {
                    items: items.clone(),
                });
            }
        }
        AgentEvent::Notice(t) => {
            if !t.to_lowercase().contains("hook") {
                parts.push(Part::Notice {
                    text: t.clone(),
                    tone: Tone::Info,
                });
            }
        }
        AgentEvent::Error(m) => {
            let dup = parts
                .iter()
                .any(|p| matches!(p, Part::Notice { text, tone: Tone::Error } if text == m));
            if !dup && !m.is_empty() {
                close_open(parts);
                parts.push(Part::Notice {
                    text: m.clone(),
                    tone: Tone::Error,
                });
            }
            msg.status = MsgStatus::Error;
        }
        AgentEvent::Done {
            cost_usd,
            input_tokens,
            output_tokens,
            duration_ms,
            is_error,
            ..
        } => {
            close_open(parts);
            finish_tools(parts, ToolStatus::Ok);
            if *is_error {
                msg.status = MsgStatus::Error;
            } else if msg.status == MsgStatus::Streaming {
                msg.status = MsgStatus::Done;
            }
            msg.meta.cost_usd = *cost_usd;
            msg.meta.input_tokens = *input_tokens;
            msg.meta.output_tokens = *output_tokens;
            msg.meta.duration_ms = duration_ms.or(Some(now_ms().saturating_sub(msg.created)));
        }
        AgentEvent::Session { model, .. } => {
            if model.is_some() {
                msg.meta.model = model.clone();
            }
        }
        AgentEvent::Exit(code) => {
            close_open(parts);
            let stopped = msg.status == MsgStatus::Stopped;
            finish_tools(
                parts,
                if stopped {
                    ToolStatus::Error
                } else {
                    ToolStatus::Ok
                },
            );
            if msg.status == MsgStatus::Streaming {
                msg.status = if *code == Some(0) {
                    MsgStatus::Done
                } else {
                    MsgStatus::Error
                };
            }
            if msg.meta.duration_ms.is_none() {
                msg.meta.duration_ms = Some(now_ms().saturating_sub(msg.created));
            }
        }
    }
}

// ─────────────────────────────── persistence ───────────────────────────────

#[derive(Serialize, Deserialize, Default)]
pub struct Saved {
    pub sessions: Vec<Session>,
    pub active: u64,
    #[serde(default = "yes")]
    pub sidebar: bool,
    #[serde(default = "yes")]
    pub changes: bool,
}

fn yes() -> bool {
    true
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Open Agent")
}

pub fn load() -> Option<Saved> {
    let raw = std::fs::read_to_string(data_dir().join("sessions.json")).ok()?;
    let mut saved: Saved = serde_json::from_str(&raw).ok()?;
    // Any run that was live when the app closed is over now.
    for s in &mut saved.sessions {
        for m in &mut s.messages {
            if m.status == MsgStatus::Streaming {
                m.status = MsgStatus::Stopped;
                close_open(&mut m.parts);
                finish_tools(&mut m.parts, ToolStatus::Error);
            }
        }
    }
    Some(saved)
}

pub fn save(saved: &Saved) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string(saved) {
        let tmp = dir.join("sessions.json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, dir.join("sessions.json"));
        }
    }
}
