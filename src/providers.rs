//! Detect which agent backends are installed and signed in.

use crate::resolve;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

#[derive(Clone, Default, Debug)]
pub struct ProviderStatus {
    pub checked: bool,
    pub installed: bool,
    pub version: Option<String>,
    pub auth: Option<String>,
    pub models: Vec<String>,
    /// Display names reported by the CLI, keyed by model id (e.g. "claude-opus-4-8" → "Opus 4.8").
    pub model_names: HashMap<String, String>,
}

/// Used when the Claude Code CLI can't be asked for its model list.
pub const CLAUDE_FALLBACK_MODELS: &[&str] = &["default", "opus", "sonnet", "fable", "haiku"];

/// Ask Claude Code which models it offers, the way the Claude Agent SDK does:
/// start it in stream-json mode, send an `initialize` control request, and
/// read `models` from the reply. No prompt is sent, so no model is called.
fn claude_models(path: &Path) -> Option<(Vec<String>, HashMap<String, String>)> {
    let mut child = resolve::command(path)
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
        ])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let req =
        r#"{"type":"control_request","request_id":"oa_models","request":{"subtype":"initialize"}}"#;
    if writeln!(stdin, "{req}")
        .and_then(|_| stdin.flush())
        .is_err()
    {
        let _ = child.kill();
        return None;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if v["type"] == "control_response" && v["response"]["request_id"] == "oa_models" {
                let _ = tx.send(v["response"]["response"]["models"].clone());
                return;
            }
        }
    });
    let models = rx.recv_timeout(Duration::from_secs(30)).ok();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    let mut ids = Vec::new();
    let mut names = HashMap::new();
    for m in models?.as_array()? {
        let Some(id) = m["value"].as_str() else {
            continue;
        };
        if let Some(name) = m["displayName"].as_str() {
            // "Default (recommended)" → "Default"
            let name = if id == "default" { "Default" } else { name };
            names.insert(id.to_string(), name.to_string());
        }
        ids.push(id.to_string());
    }
    (!ids.is_empty()).then_some((ids, names))
}

fn last_line(path: &Path, args: &[&str]) -> Option<String> {
    let out = resolve::command(path)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        s = String::from_utf8_lossy(&out.stderr).trim().to_string();
    }
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .map(|l| l.trim().to_string())
}

/// Codex keeps the list of models your ChatGPT plan can use in a local cache.
fn codex_models() -> Vec<String> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| PathBuf::from(h).join(".codex")));
    let Some(dir) = codex_home else { return vec![] };
    let Ok(raw) = std::fs::read_to_string(dir.join("models_cache.json")) else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return vec![];
    };
    let list = v.get("models").or(v.get("data")).unwrap_or(&v);
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            m.get("slug")
                .or(m.get("id"))
                .or(m.get("model"))
                .and_then(|x| x.as_str())
        })
        .filter(|n| !n.contains("review") && !n.contains("reserve"))
        .map(str::to_string)
        .collect()
}

pub fn check(name: &str) -> ProviderStatus {
    match resolve::find(name) {
        None => ProviderStatus {
            checked: true,
            ..Default::default()
        },
        Some(path) => {
            let (models, model_names) = match name {
                "codex" => (codex_models(), HashMap::new()),
                "claude" => claude_models(&path).unwrap_or_else(|| {
                    (
                        CLAUDE_FALLBACK_MODELS
                            .iter()
                            .map(|s| s.to_string())
                            .collect(),
                        HashMap::new(),
                    )
                }),
                _ => (vec![], HashMap::new()),
            };
            ProviderStatus {
                checked: true,
                installed: true,
                version: last_line(&path, &["--version"]).map(|v| {
                    v.chars()
                        .filter(|c| c.is_ascii_digit() || *c == '.')
                        .collect()
                }),
                auth: if name == "codex" {
                    last_line(&path, &["login", "status"])
                } else {
                    None
                },
                models,
                model_names,
            }
        }
    }
}
