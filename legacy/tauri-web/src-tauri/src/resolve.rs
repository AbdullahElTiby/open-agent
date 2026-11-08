//! Locate the `claude` / `codex` executables and build a `Command` for them
//! that works on Windows (.exe / .cmd shims), macOS and Linux.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find an executable on PATH (plus a few well-known install locations).
pub fn find(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };

    let mut dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect())
        .unwrap_or_default();

    // GUI apps don't always inherit the shell PATH — add common install dirs.
    if let Some(home) = home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
        dirs.push(home.join(".npm-global").join("bin"));
        if cfg!(windows) {
            if let Some(local) = env::var_os("LOCALAPPDATA") {
                let local = PathBuf::from(local);
                dirs.push(local.join("Programs").join("OpenAI").join("Codex").join("bin"));
            }
            if let Some(appdata) = env::var_os("APPDATA") {
                dirs.push(PathBuf::from(appdata).join("npm"));
            }
        } else {
            dirs.push(PathBuf::from("/opt/homebrew/bin"));
            dirs.push(PathBuf::from("/usr/local/bin"));
        }
    }

    for dir in dirs {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Build a command for a resolved executable, routing `.cmd`/`.bat` shims
/// through `cmd /C` and hiding the console window on Windows.
pub fn command(path: &Path) -> Command {
    let is_shim = path
        .extension()
        .map(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "cmd" || e == "bat"
        })
        .unwrap_or(false);

    #[allow(unused_mut)]
    let mut cmd = if is_shim {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(path);
        c
    } else {
        Command::new(path)
    };

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // If Open Agent itself is launched from inside a Claude Code session,
    // don't let the child think it's nested.
    cmd.env_remove("CLAUDECODE");
    cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
    cmd
}
