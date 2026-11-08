//! Detect which agent backends are installed and signed in.

use crate::resolve;
use serde::Serialize;
use std::process::Stdio;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    installed: bool,
    version: Option<String>,
    path: Option<String>,
    auth: Option<String>,
    models: Option<Vec<String>>,
}

/// Codex keeps the list of models your ChatGPT plan can use in a local cache.
fn codex_models() -> Option<Vec<String>> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(home).join(".codex"));
    let raw = std::fs::read_to_string(codex_home.join("models_cache.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let list = v.get("models").or(v.get("data")).unwrap_or(&v).as_array()?;
    let names: Vec<String> = list
        .iter()
        .filter_map(|m| {
            m.get("slug").or(m.get("id")).or(m.get("model")).and_then(|x| x.as_str())
        })
        .filter(|n| !n.contains("review") && !n.contains("reserve"))
        .map(str::to_string)
        .collect();
    (!names.is_empty()).then_some(names)
}

#[derive(Serialize)]
pub struct Providers {
    claude: ProviderStatus,
    codex: ProviderStatus,
}

fn run_capture(path: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = resolve::command(path)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        s = String::from_utf8_lossy(&out.stderr).trim().to_string();
    }
    let last = s.lines().filter(|l| !l.trim().is_empty()).last()?.trim().to_string();
    Some(last)
}

fn status(name: &str, auth_args: Option<&[&str]>) -> ProviderStatus {
    match resolve::find(name) {
        None => ProviderStatus { installed: false, version: None, path: None, auth: None, models: None },
        Some(path) => ProviderStatus {
            installed: true,
            models: if name == "codex" { codex_models() } else { None },
            version: run_capture(&path, &["--version"]),
            auth: auth_args.and_then(|a| run_capture(&path, a)),
            path: Some(path.to_string_lossy().into_owned()),
        },
    }
}

#[tauri::command]
pub async fn check_providers() -> Providers {
    tauri::async_runtime::spawn_blocking(|| Providers {
        claude: status("claude", None),
        codex: status("codex", Some(&["login", "status"])),
    })
    .await
    .unwrap_or_else(|_| Providers {
        claude: status("claude", None),
        codex: status("codex", None),
    })
}
