//! Self-update for local builds.
//!
//! A release build launched from anywhere other than the install folder
//! (`%LOCALAPPDATA%\Open Agent\bin`) copies itself there, remembers where it was
//! built, and relaunches from the installed copy. That keeps `target/release`
//! unlocked so `cargo build --release` can always overwrite it.
//!
//! The installed copy then watches that build output. When a build with a new
//! build id lands there, the app swaps the new binary in (a running exe can be
//! renamed on Windows, just not overwritten), relaunches and exits.
//!
//! Debug builds never install or update. Set `OPEN_AGENT_NO_UPDATE=1` to opt out.

use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, SystemTime};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_ID: &str = env!("OA_BUILD_ID");

/// Marker embedded in the binary so another build can be identified by
/// scanning its bytes, without running it.
#[used]
static BUILD_MARK: &str = concat!(
    "OA-BUILD[",
    env!("CARGO_PKG_VERSION"),
    "|",
    env!("OA_BUILD_ID"),
    "]"
);

/// Passed to the relaunched app so it can say what happened.
pub const ARG_INSTALLED: &str = "--oa-installed";
pub const ARG_UPDATED: &str = "--oa-updated";

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    pub build: String,
    source: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct InstallInfo {
    /// The build output this installed copy updates from.
    source: PathBuf,
}

fn enabled() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("OPEN_AGENT_NO_UPDATE").is_none()
}

fn install_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("Open Agent").join("bin"))
}

fn info_path(dir: &Path) -> PathBuf {
    dir.join("install.json")
}

fn current_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()?.canonicalize().ok()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Sibling path with a tag before the extension: `open-agent.exe` → `open-agent.old.exe`.
fn tagged(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    let name = match exe.extension() {
        Some(ext) => format!("{stem}.{tag}.{}", ext.to_string_lossy()),
        None => format!("{stem}.{tag}"),
    };
    exe.with_file_name(name)
}

/// True while some process is running this executable (Windows denies write access to it).
fn in_use(path: &Path) -> bool {
    cfg!(windows) && path.exists() && std::fs::OpenOptions::new().write(true).open(path).is_err()
}

/// Read `(version, build id)` out of a binary's embedded marker.
fn read_mark(path: &Path) -> Option<(String, String)> {
    let bytes = std::fs::read(path).ok()?;
    let needle = b"OA-BUILD[";
    let mut from = 0;
    while let Some(off) = find(&bytes[from..], needle) {
        let start = from + off + needle.len();
        let rest = &bytes[start..bytes.len().min(start + 64)];
        if let Some(end) = rest.iter().position(|&b| b == b']') {
            if let Ok(s) = std::str::from_utf8(&rest[..end]) {
                if let Some((v, b)) = s.split_once('|') {
                    let ok_v = !v.is_empty()
                        && v.chars()
                            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c));
                    let ok_b = !b.is_empty() && b.chars().all(|c| c.is_ascii_digit());
                    if ok_v && ok_b {
                        return Some((v.to_string(), b.to_string()));
                    }
                }
            }
        }
        from = start;
    }
    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn launch(exe: &Path, extra: &str) -> std::io::Result<()> {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != ARG_INSTALLED && a != ARG_UPDATED)
        .collect();
    Command::new(exe).args(args).arg(extra).spawn().map(|_| ())
}

/// Run before the window opens. Returns `true` when this process should exit
/// because the installed copy has taken over.
pub fn bootstrap() -> bool {
    // Keep the marker in the binary: `#[used]` alone doesn't stop the linker dropping it.
    std::hint::black_box(BUILD_MARK.as_ptr());
    if !enabled() {
        return false;
    }
    let (Some(exe), Some(dir)) = (current_exe(), install_dir()) else {
        return false;
    };
    if exe.parent().is_some_and(|p| same_dir(p, &dir)) {
        // Running installed: tidy up the binary a previous update replaced.
        let old = tagged(&exe, "old");
        std::thread::spawn(move || {
            for _ in 0..40 {
                if !old.exists() || std::fs::remove_file(&old).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        return false;
    }

    // Running from a build folder: install this build, then hand over to it.
    let Some(name) = exe.file_name() else {
        return false;
    };
    let installed = dir.join(name);
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let info = serde_json::to_string_pretty(&InstallInfo {
        source: exe.clone(),
    })
    .unwrap_or_default();
    if std::fs::write(info_path(&dir), info).is_err() {
        return false;
    }
    if in_use(&installed) {
        // The installed app is already open; its watcher will pick this build up.
        return true;
    }
    let tmp = tagged(&installed, "new");
    if std::fs::copy(&exe, &tmp).is_err() || std::fs::rename(&tmp, &installed).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    launch(&installed, ARG_INSTALLED).is_ok()
}

/// Watch the recorded build output for new builds (installed copies only).
pub fn watch(ctx: egui::Context) -> Option<Receiver<Update>> {
    if !enabled() {
        return None;
    }
    let exe = current_exe()?;
    let dir = install_dir()?;
    if !exe.parent().is_some_and(|p| same_dir(p, &dir)) {
        return None;
    }
    let info: InstallInfo =
        serde_json::from_str(&std::fs::read_to_string(info_path(&dir)).ok()?).ok()?;
    let source = info.source;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let stamp = |p: &Path| {
            std::fs::metadata(p)
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        };
        let mut seen = None;
        let mut checked = None;
        let mut newest: u64 = BUILD_ID.parse().unwrap_or(0);
        loop {
            let now = stamp(&source);
            // Only look once the file has stopped changing (the linker is done).
            if now.is_some() && now == seen && now != checked {
                checked = now;
                if let Some((version, build)) = read_mark(&source) {
                    let id = build.parse::<u64>().unwrap_or(0);
                    // Only move forward: an older build in the folder is not an update.
                    if id > newest {
                        newest = id;
                        let update = Update {
                            version,
                            build,
                            source: source.clone(),
                        };
                        if tx.send(update).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            }
            seen = now;
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    Some(rx)
}

/// Swap the new build in and start it. On success the caller must exit.
pub fn apply(u: &Update) -> Result<(), String> {
    let exe = current_exe().ok_or("can't locate the running app")?;
    let new = tagged(&exe, "new");
    let old = tagged(&exe, "old");
    std::fs::copy(&u.source, &new).map_err(|e| format!("copy failed: {e}"))?;
    if read_mark(&new).map(|(_, b)| b) != Some(u.build.clone()) {
        let _ = std::fs::remove_file(&new);
        return Err("the build changed while copying; will retry".into());
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("can't move the running app aside: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("can't put the new build in place: {e}"));
    }
    if let Err(e) = launch(&exe, ARG_UPDATED) {
        let _ = std::fs::rename(&exe, &new);
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("can't start the new build: {e}"));
    }
    Ok(())
}

/// What to tell the user on startup, if this launch came from an install or update.
pub fn startup_note() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == ARG_UPDATED) {
        Some(format!("Updated to v{VERSION} · {}", build_label(BUILD_ID)))
    } else if args.iter().any(|a| a == ARG_INSTALLED) {
        Some("Installed. New release builds will update the app automatically.".into())
    } else {
        None
    }
}

/// "built 3m ago" style label for a build id (unix seconds).
pub fn build_label(build: &str) -> String {
    let Ok(t) = build.parse::<u64>() else {
        return format!("build {build}");
    };
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(t);
    let s = now.saturating_sub(t);
    match s {
        0..=59 => "built just now".into(),
        60..=3599 => format!("built {}m ago", s / 60),
        3600..=86_399 => format!("built {}h ago", s / 3600),
        _ => format!("built {}d ago", s / 86_400),
    }
}
