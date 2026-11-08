//! Import existing sessions from Claude Code (CLI and Claude Desktop's Code
//! tab) and Codex (CLI and desktop app) into Open Agent threads.
//!
//! * Claude: `~/.claude/projects/<project>/<session-id>.jsonl` (respects
//!   `CLAUDE_CONFIG_DIR`), plus any `.claude/projects` folder Claude Desktop
//!   keeps under its own app-data directory.
//! * Codex: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` and
//!   `~/.codex/archived_sessions` (respects `CODEX_HOME`).
//!
//! Imported threads keep the agent's real session id, so sending a new message
//! resumes the original conversation (`--resume` / `codex exec resume`).

use crate::agent::AgentEvent;
use crate::model::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Tool output is capped so `sessions.json` stays small enough to save quickly.
const MAX_OUTPUT: usize = 6000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Claude,
    Codex,
    All,
}

impl Source {
    fn claude(self) -> bool {
        matches!(self, Source::Claude | Source::All)
    }
    fn codex(self) -> bool {
        matches!(self, Source::Codex | Source::All)
    }
}

#[derive(Default)]
pub struct Report {
    /// New sessions with ids still 0; the app assigns real ids.
    pub sessions: Vec<Session>,
    pub claude: usize,
    pub codex: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Scan the chosen sources. `known` holds agent session ids already present
/// in Open Agent; those transcripts are skipped.
pub fn run(source: Source, known: &HashSet<String>) -> Report {
    let mut r = Report::default();
    if source.claude() {
        for file in claude_files() {
            let id = stem(&file);
            if known.contains(&id) {
                r.skipped += 1;
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&file) else {
                r.failed += 1;
                continue;
            };
            // Transcripts without a single prompt (aborted starts, summaries only) are ignored.
            if let Some(s) = parse_claude(&raw, &id) {
                r.claude += 1;
                r.sessions.push(s);
            }
        }
    }
    if source.codex() {
        let mut seen = HashSet::new();
        for file in codex_files() {
            let Ok(raw) = std::fs::read_to_string(&file) else {
                r.failed += 1;
                continue;
            };
            let Some(s) = parse_codex(&raw) else { continue };
            let id = s.thread_codex.clone().unwrap_or_default();
            if known.contains(&id) || !seen.insert(id) {
                r.skipped += 1;
                continue;
            }
            r.codex += 1;
            r.sessions.push(s);
        }
    }
    r
}

// ─────────────────────────────── discovery ───────────────────────────────

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn jsonl_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "jsonl"))
        .collect()
}

fn claude_files() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![];
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        roots.push(PathBuf::from(dir).join("projects"));
    }
    if let Some(h) = home() {
        roots.push(h.join(".claude").join("projects"));
    }
    // Claude Desktop runs Claude Code with its own config dirs under its app data.
    let mut desktop = vec![];
    if let Some(a) = std::env::var_os("APPDATA") {
        desktop.push(PathBuf::from(a).join("Claude"));
    }
    if let Some(h) = home() {
        desktop.push(h.join("Library").join("Application Support").join("Claude"));
        desktop.push(h.join(".config").join("Claude"));
    }
    for d in desktop {
        find_claude_projects(&d, 0, &mut roots);
    }

    let mut seen = HashSet::new();
    let mut out = vec![];
    for root in roots {
        let Ok(canon) = root.canonicalize() else {
            continue;
        };
        if !seen.insert(canon) {
            continue;
        }
        for project in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            if !project.path().is_dir() {
                continue;
            }
            // Top level only: subagent transcripts live in subfolders or `agent-*.jsonl`.
            for f in jsonl_in(&project.path()) {
                if !stem(&f).starts_with("agent-") {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// Walk an app-data folder looking for `.claude/projects` directories.
fn find_claude_projects(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    const SKIP: &[&str] = &[
        "Cache",
        "Code Cache",
        "GPUCache",
        "DawnCache",
        "DawnGraphiteCache",
        "DawnWebGPUCache",
        "IndexedDB",
        "Local Storage",
        "Session Storage",
        "blob_storage",
        "Crashpad",
        "logs",
        "vm_bundles",
        "node_modules",
        "Service Worker",
        "Shared Dictionary",
        "WebStorage",
        "extensions",
        "Claude Extensions",
    ];
    if depth > 7 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".claude" {
            let projects = p.join("projects");
            if projects.is_dir() {
                out.push(projects);
            }
            continue;
        }
        if !SKIP.contains(&name.as_str()) {
            find_claude_projects(&p, depth + 1, out);
        }
    }
}

fn codex_files() -> Vec<PathBuf> {
    let base = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".codex")));
    let Some(base) = base else { return vec![] };
    let mut out = vec![];
    for dir in [base.join("sessions"), base.join("archived_sessions")] {
        walk_jsonl(&dir, 0, &mut out);
    }
    out
}

fn walk_jsonl(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 5 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_jsonl(&p, depth + 1, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

// ─────────────────────────────── building ───────────────────────────────

/// Turns a flat transcript into Open Agent messages, reusing the live event reducer.
struct Builder {
    provider: Provider,
    messages: Vec<Message>,
    agent: Option<Message>,
    turn_start: u64,
    last_ts: u64,
    first_ts: u64,
    model: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    stopped: bool,
}

impl Builder {
    fn new(provider: Provider) -> Self {
        Builder {
            provider,
            messages: vec![],
            agent: None,
            turn_start: 0,
            last_ts: 0,
            first_ts: 0,
            model: None,
            input_tokens: None,
            output_tokens: None,
            stopped: false,
        }
    }

    fn tick(&mut self, ts: u64) {
        if ts > 0 {
            if self.first_ts == 0 {
                self.first_ts = ts;
            }
            self.last_ts = self.last_ts.max(ts);
        }
    }

    fn user(&mut self, text: String, ts: u64) {
        self.close();
        self.tick(ts);
        self.turn_start = self.last_ts;
        self.messages.push(Message {
            id: 0,
            role: Role::User,
            provider: self.provider,
            created: self.last_ts,
            status: MsgStatus::Done,
            parts: vec![Part::Text { text, open: false }],
            meta: Meta::default(),
        });
    }

    fn event(&mut self, e: AgentEvent, ts: u64) {
        self.tick(ts);
        let created = if self.turn_start > 0 {
            self.turn_start
        } else {
            self.last_ts
        };
        let provider = self.provider;
        let m = self.agent.get_or_insert_with(|| Message {
            id: 0,
            role: Role::Agent,
            provider,
            created,
            status: MsgStatus::Streaming,
            parts: vec![],
            meta: Meta::default(),
        });
        let e = match e {
            AgentEvent::ToolEnd {
                id,
                output,
                is_error,
            } => AgentEvent::ToolEnd {
                id,
                output: cap(output),
                is_error,
            },
            e => e,
        };
        apply_event(m, &e);
    }

    fn close(&mut self) {
        let Some(mut m) = self.agent.take() else {
            self.stopped = false;
            return;
        };
        if self.stopped {
            m.status = MsgStatus::Stopped;
        }
        let duration = self.last_ts.saturating_sub(m.created);
        apply_event(
            &mut m,
            &AgentEvent::Done {
                session_id: None,
                cost_usd: None,
                input_tokens: self.input_tokens.take(),
                output_tokens: self.output_tokens.take(),
                duration_ms: Some(duration),
                is_error: false,
            },
        );
        m.meta.model = self.model.clone();
        self.stopped = false;
        if !m.parts.is_empty() {
            self.messages.push(m);
        }
    }

    fn finish(mut self, thread: String, cwd: String, title: Option<String>) -> Option<Session> {
        self.close();
        let first_user = self.messages.iter().find(|m| m.role == Role::User)?;
        let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
            match first_user.parts.first() {
                Some(Part::Text { text, .. }) => text.clone(),
                _ => String::new(),
            }
        });
        let title: String = title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(60)
            .collect();
        let mut s = Session::new(0, None);
        s.title = if title.is_empty() {
            "Imported thread".into()
        } else {
            title
        };
        s.cwd = cwd;
        s.provider = self.provider;
        s.set_thread(self.provider, thread);
        s.messages = self.messages;
        s.created = if self.first_ts > 0 {
            self.first_ts
        } else {
            now_ms()
        };
        s.updated = if self.last_ts > 0 {
            self.last_ts
        } else {
            s.created
        };
        Some(s)
    }
}

fn cap(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        let mut cut = MAX_OUTPUT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n… [truncated on import]");
    }
    s
}

fn st(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// `2025-06-01T12:34:56.789Z` → unix ms. Offsets other than `Z` are honoured.
pub fn parse_ts(t: &str) -> u64 {
    let b = t.as_bytes();
    if b.len() < 19 {
        return 0;
    }
    let num = |r: std::ops::Range<usize>| t.get(r).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
    let (y, mo, d, h, mi, s) = (
        num(0..4),
        num(5..7),
        num(8..10),
        num(11..13),
        num(14..16),
        num(17..19),
    );
    let mut ms = 0i64;
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let frac = &t[start..i];
        let digits: String = frac.chars().chain("000".chars()).take(3).collect();
        ms = digits.parse().unwrap_or(0);
    }
    let mut offset = 0i64;
    if let Some(&c) = b.get(i) {
        if c == b'+' || c == b'-' {
            let oh = t
                .get(i + 1..i + 3)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            let om = t
                .get(i + 4..i + 6)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            offset = (oh * 60 + om) * 60 * if c == b'+' { 1 } else { -1 };
        }
    }
    // Days from civil (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + h * 3600 + mi * 60 + s - offset;
    if secs < 0 {
        0
    } else {
        (secs * 1000 + ms) as u64
    }
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(t) => t.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("text") | Some("input_text") | Some("output_text") => {
                    p["text"].as_str().map(str::to_string)
                }
                Some("image") | Some("input_image") => Some("[image]".into()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ─────────────────────────────── Claude Code ───────────────────────────────

/// Clean a Claude user prompt; `None` means it was machine-generated.
fn claude_user_text(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty()
        || t.starts_with("<local-command-stdout>")
        || t.starts_with("<local-command-stderr>")
        || t.starts_with("<local-command-caveat>")
        || t.starts_with("Caveat: The messages below were generated")
        || t.starts_with("<system-reminder>")
        || t.starts_with("[Request interrupted by user")
    {
        return None;
    }
    // Slash commands are stored as XML-ish tags.
    if t.contains("<command-name>") {
        let tag = |name: &str| {
            let open = format!("<{name}>");
            let close = format!("</{name}>");
            t.find(&open)
                .and_then(|a| {
                    t[a + open.len()..]
                        .find(&close)
                        .map(|b| t[a + open.len()..a + open.len() + b].trim().to_string())
                })
                .unwrap_or_default()
        };
        let cmd = tag("command-name");
        let args = tag("command-args");
        let line = if args.is_empty() {
            cmd
        } else {
            format!("{cmd} {args}")
        };
        return if line.trim().is_empty() {
            None
        } else {
            Some(line)
        };
    }
    Some(t.to_string())
}

pub fn parse_claude(raw: &str, session_id: &str) -> Option<Session> {
    let mut b = Builder::new(Provider::Claude);
    let mut cwd = String::new();
    let mut custom_title: Option<String> = None;
    let mut summary: Option<String> = None;
    // message id → (input tokens incl. cache, output tokens); a message spans several lines.
    let mut usage: HashMap<String, (u64, u64)> = HashMap::new();
    let mut turn_ids: Vec<String> = vec![];

    let flush_usage =
        |b: &mut Builder, usage: &HashMap<String, (u64, u64)>, ids: &mut Vec<String>| {
            if ids.is_empty() {
                return;
            }
            let input = ids.last().and_then(|i| usage.get(i)).map(|u| u.0);
            let output: u64 = ids.iter().filter_map(|i| usage.get(i)).map(|u| u.1).sum();
            b.input_tokens = input;
            b.output_tokens = Some(output);
            ids.clear();
        };

    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let ty = v["type"].as_str().unwrap_or_default();
        match ty {
            "summary" => summary = v["summary"].as_str().map(str::to_string),
            "custom-title" => {
                custom_title = v["customTitle"]
                    .as_str()
                    .or(v["title"].as_str())
                    .map(str::to_string)
            }
            _ => {}
        }
        if ty != "user" && ty != "assistant" {
            continue;
        }
        if v["isSidechain"].as_bool() == Some(true) {
            continue;
        }
        if cwd.is_empty() {
            cwd = st(&v["cwd"]);
        }
        let ts = v["timestamp"].as_str().map(parse_ts).unwrap_or(0);
        let msg = &v["message"];

        if ty == "assistant" {
            if let Some(m) = msg["model"].as_str().filter(|m| !m.starts_with('<')) {
                b.model = Some(m.to_string());
            }
            if let Some(id) = msg["id"].as_str() {
                let u = &msg["usage"];
                let input = u["input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_read_input_tokens"].as_u64().unwrap_or(0)
                    + u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                usage.insert(
                    id.to_string(),
                    (input, u["output_tokens"].as_u64().unwrap_or(0)),
                );
                if !turn_ids.iter().any(|t| t == id) {
                    turn_ids.push(id.to_string());
                }
            }
            for block in msg["content"].as_array().into_iter().flatten() {
                let e = match block["type"].as_str() {
                    Some("text") => AgentEvent::TextFinal(st(&block["text"])),
                    Some("thinking") => AgentEvent::ThinkingFinal(st(&block["thinking"])),
                    Some("tool_use") | Some("server_tool_use") => AgentEvent::ToolStart {
                        id: st(&block["id"]),
                        name: st(&block["name"]),
                        input: block["input"].clone(),
                    },
                    _ => continue,
                };
                b.event(e, ts);
            }
            continue;
        }

        // User line: real prompt, tool results, or harness noise.
        if v["isMeta"].as_bool() == Some(true) || v["isCompactSummary"].as_bool() == Some(true) {
            continue;
        }
        let mut texts = vec![];
        let mut interrupted = false;
        match &msg["content"] {
            Value::String(t) => {
                interrupted |= t.starts_with("[Request interrupted by user");
                texts.extend(claude_user_text(t));
            }
            Value::Array(blocks) => {
                for block in blocks {
                    match block["type"].as_str() {
                        Some("tool_result") => b.event(
                            AgentEvent::ToolEnd {
                                id: st(&block["tool_use_id"]),
                                output: tool_result_text(&block["content"]),
                                is_error: block["is_error"].as_bool().unwrap_or(false),
                            },
                            ts,
                        ),
                        Some("text") => {
                            let t = block["text"].as_str().unwrap_or_default();
                            interrupted |= t.starts_with("[Request interrupted by user");
                            texts.extend(claude_user_text(t));
                        }
                        Some("image") => texts.push("[image]".into()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if interrupted {
            b.stopped = true;
        }
        if !texts.is_empty() {
            flush_usage(&mut b, &usage, &mut turn_ids);
            b.user(texts.join("\n\n"), ts);
        }
    }
    flush_usage(&mut b, &usage, &mut turn_ids);
    b.finish(session_id.to_string(), cwd, custom_title.or(summary))
}

// ─────────────────────────────── Codex ───────────────────────────────

fn codex_user_text(content: &Value) -> Option<String> {
    let mut texts = vec![];
    for c in content.as_array().into_iter().flatten() {
        match c["type"].as_str() {
            Some("input_text") | Some("text") => texts.push(st(&c["text"])),
            Some("input_image") => texts.push("[image]".into()),
            _ => {}
        }
    }
    let t = texts.join("\n").trim().to_string();
    let injected = [
        "<environment_context",
        "<user_instructions",
        "# AGENTS.md instructions",
        "<turn_aborted",
        "<user_shell_command",
    ];
    if t.is_empty() || injected.iter().any(|p| t.starts_with(p)) {
        return None;
    }
    Some(t)
}

/// A shell command from Codex arguments: a string, or argv like `["bash", "-lc", "…"]`.
fn command_string(cmd: &Value) -> String {
    match cmd {
        Value::String(s) => s.clone(),
        Value::Array(a) => {
            let argv: Vec<String> = a.iter().map(st).collect();
            let wrapped = argv.len() >= 3
                && matches!(
                    argv[argv.len() - 2].as_str(),
                    "-lc" | "-c" | "-Command" | "/c" | "-NoProfile"
                );
            if wrapped {
                argv[argv.len() - 1].clone()
            } else {
                argv.join(" ")
            }
        }
        _ => String::new(),
    }
}

/// `*** Begin Patch` text → `file_change` entries with per-file old/new lines.
type PatchFile = (String, &'static str, String, String);

fn push_patch_file(cur: &mut Option<PatchFile>, out: &mut Vec<Value>) {
    if let Some((path, kind, old, new)) = cur.take() {
        out.push(json!({ "path": path, "kind": kind, "old": old, "new": new }));
    }
}

fn patch_changes(patch: &str) -> Value {
    const HEADERS: [(&str, &str); 3] = [
        ("*** Add File: ", "add"),
        ("*** Update File: ", "update"),
        ("*** Delete File: ", "delete"),
    ];
    let mut changes: Vec<Value> = vec![];
    let mut cur: Option<PatchFile> = None;
    let push = push_patch_file;
    for line in patch.lines() {
        let header = HEADERS
            .iter()
            .find_map(|&(p, k)| line.strip_prefix(p).map(|rest| (rest.trim().to_string(), k)));
        if let Some((path, kind)) = header {
            push(&mut cur, &mut changes);
            cur = Some((path, kind, String::new(), String::new()));
            continue;
        }
        let Some((_, _, old, new)) = cur.as_mut() else {
            continue;
        };
        if line.starts_with("***") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            new.push_str(rest);
            new.push('\n');
        } else if let Some(rest) = line.strip_prefix('-') {
            old.push_str(rest);
            old.push('\n');
        }
    }
    push(&mut cur, &mut changes);
    Value::Array(changes)
}

/// Map a Codex tool call to the names the UI already knows how to draw.
fn codex_tool(name: &str, args: &Value, raw_input: &str) -> (String, Value) {
    match name {
        "shell" | "container.exec" | "local_shell" | "shell_command" => (
            "shell".into(),
            json!({ "command": command_string(&args["command"]) }),
        ),
        "exec_command" => (
            "shell".into(),
            json!({ "command": command_string(&args["cmd"]) }),
        ),
        "apply_patch" => {
            let patch = args["input"].as_str().unwrap_or(raw_input);
            (
                "file_change".into(),
                json!({ "changes": patch_changes(patch) }),
            )
        }
        "web_search" => ("web_search".into(), json!({ "query": args["query"] })),
        _ => (
            name.to_string(),
            if args.is_null() {
                json!({ "input": raw_input })
            } else {
                args.clone()
            },
        ),
    }
}

/// Codex tool output: plain text, or JSON `{"output": …, "metadata": {"exit_code": n}}`.
fn codex_output(output: &Value) -> (String, bool) {
    let parsed = match output {
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .filter(|v| v.is_object()),
        Value::Object(_) => Some(output.clone()),
        _ => None,
    };
    match parsed {
        Some(v) => {
            let exit = v["metadata"]["exit_code"].as_i64();
            let failed = exit.is_some_and(|c| c != 0) || v["success"].as_bool() == Some(false);
            let text = match &v["output"] {
                Value::Null => tool_result_text(&v["content"]),
                o => tool_result_text(o),
            };
            (text, failed)
        }
        None => (tool_result_text(output), false),
    }
}

pub fn parse_codex(raw: &str) -> Option<Session> {
    let mut b = Builder::new(Provider::Codex);
    let mut id = String::new();
    let mut cwd = String::new();
    let mut last_output: Option<u64> = None;
    let mut turn_output = 0u64;
    let mut searches = 0u32;

    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let ts = v["timestamp"].as_str().map(parse_ts).unwrap_or(0);
        let (kind, p) = match v["type"].as_str() {
            Some(
                k @ ("session_meta" | "response_item" | "event_msg" | "turn_context" | "compacted"),
            ) => (k, &v["payload"]),
            // Older rollouts: a bare meta line, then bare response items.
            _ if v.get("record_type").is_some() => continue,
            _ if id.is_empty() && v["id"].is_string() && v.get("type").is_none() => {
                ("session_meta", &v)
            }
            _ => ("response_item", &v),
        };
        match kind {
            "session_meta" => {
                id = st(&p["id"]);
                cwd = st(&p["cwd"]);
                if let Some(t) = p["timestamp"].as_str() {
                    b.tick(parse_ts(t));
                }
            }
            "turn_context" => {
                if cwd.is_empty() {
                    cwd = st(&p["cwd"]);
                }
                if let Some(m) = p["model"].as_str() {
                    b.model = Some(m.to_string());
                }
            }
            "event_msg" => match p["type"].as_str() {
                Some("token_count") => {
                    let u = &p["info"]["last_token_usage"];
                    if let Some(i) = u["input_tokens"].as_u64() {
                        b.input_tokens = Some(i);
                    }
                    if let Some(o) = u["output_tokens"].as_u64() {
                        turn_output += o;
                        last_output = Some(turn_output);
                    }
                }
                Some("turn_aborted") => b.stopped = true,
                _ => {}
            },
            "response_item" => match p["type"].as_str().unwrap_or_default() {
                "message" => match p["role"].as_str() {
                    Some("user") => {
                        if let Some(t) = codex_user_text(&p["content"]) {
                            b.output_tokens = last_output.take();
                            turn_output = 0;
                            b.user(t, ts);
                        } else if tool_result_text(&p["content"]).contains("<turn_aborted") {
                            b.stopped = true;
                        }
                    }
                    Some("assistant") => {
                        b.event(AgentEvent::TextFinal(tool_result_text(&p["content"])), ts)
                    }
                    _ => {}
                },
                "reasoning" => {
                    let text = p["summary"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| s["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if !text.trim().is_empty() {
                        b.event(AgentEvent::ThinkingFinal(text), ts);
                    }
                }
                "function_call" | "custom_tool_call" => {
                    let name = st(&p["name"]);
                    let raw_args = p["arguments"]
                        .as_str()
                        .or(p["input"].as_str())
                        .unwrap_or_default();
                    let args: Value = serde_json::from_str(raw_args).unwrap_or(Value::Null);
                    let call = st(&p["call_id"]);
                    if name == "update_plan" {
                        let items = args["plan"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|s| TodoItem {
                                text: st(&s["step"]),
                                done: s["status"] == "completed",
                                active: s["status"] == "in_progress",
                            })
                            .collect();
                        b.event(AgentEvent::Todo(items), ts);
                        // Keep the call id known so its output doesn't create a row.
                        b.event(
                            AgentEvent::ToolStart {
                                id: call,
                                name: "__hidden".into(),
                                input: Value::Null,
                            },
                            ts,
                        );
                        continue;
                    }
                    let (name, input) = codex_tool(&name, &args, raw_args);
                    b.event(
                        AgentEvent::ToolStart {
                            id: call,
                            name,
                            input,
                        },
                        ts,
                    );
                }
                "local_shell_call" => {
                    let call = p["call_id"]
                        .as_str()
                        .or(p["id"].as_str())
                        .unwrap_or_default()
                        .to_string();
                    let cmd = command_string(&p["action"]["command"]);
                    b.event(
                        AgentEvent::ToolStart {
                            id: call,
                            name: "shell".into(),
                            input: json!({ "command": cmd }),
                        },
                        ts,
                    );
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let (output, is_error) = codex_output(&p["output"]);
                    b.event(
                        AgentEvent::ToolEnd {
                            id: st(&p["call_id"]),
                            output,
                            is_error,
                        },
                        ts,
                    );
                }
                "web_search_call" => {
                    searches += 1;
                    let call = format!("web-search-{searches}");
                    let query = p["action"]["query"].clone();
                    b.event(
                        AgentEvent::ToolStart {
                            id: call.clone(),
                            name: "web_search".into(),
                            input: json!({ "query": query }),
                        },
                        ts,
                    );
                    b.event(
                        AgentEvent::ToolEnd {
                            id: call,
                            output: String::new(),
                            is_error: false,
                        },
                        ts,
                    );
                }
                _ => {}
            },
            _ => {}
        }
    }
    if id.is_empty() {
        return None;
    }
    b.output_tokens = last_output;
    b.finish(id, cwd, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(parse_ts("1970-01-01T00:00:00Z"), 0);
        assert_eq!(parse_ts("2025-06-01T12:34:56.789Z"), 1_748_781_296_789);
        assert_eq!(parse_ts("2025-06-01T14:34:56.789+02:00"), 1_748_781_296_789);
    }

    #[test]
    fn claude_transcript() {
        let raw = r#"
{"type":"summary","summary":"Fix the parser","leafUuid":"x"}
{"type":"user","cwd":"D:\\proj","sessionId":"abc","timestamp":"2025-06-01T10:00:00Z","message":{"role":"user","content":"<command-name>/clear</command-name><command-args></command-args>"}}
{"type":"user","isMeta":true,"cwd":"D:\\proj","timestamp":"2025-06-01T10:00:00Z","message":{"role":"user","content":"Caveat: ignore"}}
{"type":"user","cwd":"D:\\proj","timestamp":"2025-06-01T10:00:01Z","message":{"role":"user","content":"read main.rs"}}
{"type":"assistant","timestamp":"2025-06-01T10:00:02Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"thinking","thinking":"hmm"}],"usage":{"input_tokens":10,"output_tokens":5}}}
{"type":"assistant","timestamp":"2025-06-01T10:00:03Z","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"main.rs"}}],"usage":{"input_tokens":10,"output_tokens":7}}}
{"type":"user","timestamp":"2025-06-01T10:00:04Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"fn main(){}"}]}}
{"type":"assistant","isSidechain":true,"timestamp":"2025-06-01T10:00:04Z","message":{"id":"s","content":[{"type":"text","text":"sub"}]}}
{"type":"assistant","timestamp":"2025-06-01T10:00:05Z","message":{"id":"m2","model":"claude-opus-5-5","content":[{"type":"text","text":"It's empty."}],"usage":{"input_tokens":20,"output_tokens":3}}}
"#;
        let s = parse_claude(raw, "abc").unwrap();
        assert_eq!(s.title, "Fix the parser");
        assert_eq!(s.cwd, "D:\\proj");
        assert_eq!(s.thread_claude.as_deref(), Some("abc"));
        // "/clear", "read main.rs", then the agent turn; meta and sidechain lines are dropped.
        assert_eq!(s.messages.len(), 3, "{:#?}", s.messages);
        assert!(matches!(&s.messages[0].parts[0], Part::Text { text, .. } if text == "/clear"));
        let agent = &s.messages[2];
        assert_eq!(agent.role, Role::Agent);
        assert_eq!(agent.parts.len(), 3);
        assert!(
            matches!(&agent.parts[1], Part::Tool { output, status: ToolStatus::Ok, .. } if output == "fn main(){}")
        );
        assert_eq!(agent.meta.output_tokens, Some(10));
        assert_eq!(agent.meta.duration_ms, Some(4000));
        assert_eq!(agent.meta.model.as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn codex_rollout() {
        let raw = r#"
{"timestamp":"2025-09-01T10:00:00Z","type":"session_meta","payload":{"id":"uuid-1","timestamp":"2025-09-01T10:00:00Z","cwd":"/work"}}
{"timestamp":"2025-09-01T10:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}
{"timestamp":"2025-09-01T10:00:00Z","type":"turn_context","payload":{"cwd":"/work","model":"gpt-5.5-codex"}}
{"timestamp":"2025-09-01T10:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a readme"}]}}
{"timestamp":"2025-09-01T10:00:02Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Planning"}]}}
{"timestamp":"2025-09-01T10:00:03Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"ls\"]}","call_id":"c1"}}
{"timestamp":"2025-09-01T10:00:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"{\"output\":\"src\\n\",\"metadata\":{\"exit_code\":0}}"}}
{"timestamp":"2025-09-01T10:00:05Z","type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Add File: README.md\n+# Hi\n*** End Patch","call_id":"c2"}}
{"timestamp":"2025-09-01T10:00:06Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c2","output":"Success"}}
{"timestamp":"2025-09-01T10:00:07Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":40}}}}
{"timestamp":"2025-09-01T10:00:08Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}}
"#;
        let s = parse_codex(raw).unwrap();
        assert_eq!(s.thread_codex.as_deref(), Some("uuid-1"));
        assert_eq!(s.title, "add a readme");
        assert_eq!(s.messages.len(), 2);
        let a = &s.messages[1];
        assert!(
            matches!(&a.parts[1], Part::Tool { name, input, .. } if name == "shell" && input["command"] == "ls")
        );
        assert!(
            matches!(&a.parts[2], Part::Tool { name, input, .. } if name == "file_change" && input["changes"][0]["new"] == "# Hi\n")
        );
        assert_eq!(a.meta.output_tokens, Some(40));
        assert_eq!(a.meta.model.as_deref(), Some("gpt-5.5-codex"));
    }
}
