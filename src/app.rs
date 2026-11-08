//! The Open Agent window: thread sidebar · conversation · composer · changes panel.

use crate::agent::{self, AgentEvent, RunRequest, Runs};
use crate::icons::{self, Icon};
use crate::import;
use crate::markdown;
use crate::model::*;
use crate::model_icons::{self, ModelIcons};
use crate::providers::{self, ProviderStatus};
use crate::theme::{self, *};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Id, Key, Layout,
    Margin, Modifiers, Panel, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui,
    ViewportCommand,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const COLUMN: f32 = 760.0;

enum Action {
    Send(String),
    NewThread,
    Select(u64),
    Delete(u64),
    PickFolder,
    Stop,
    Import(import::Source),
}

#[derive(Default)]
struct View {
    expanded: HashSet<String>,
    selected_file: Option<String>,
}

pub struct App {
    saved: Saved,
    next_id: u64,
    /// session id → run id
    runs: HashMap<u64, u64>,
    /// run id → (session id, agent message id)
    owners: HashMap<u64, (u64, u64)>,
    tx: Sender<(u64, AgentEvent)>,
    rx: Receiver<(u64, AgentEvent)>,
    procs: Runs,
    providers: Arc<Mutex<(ProviderStatus, ProviderStatus)>>,
    composer: String,
    view: View,
    model_icons: ModelIcons,
    logo: Option<egui::TextureHandle>,
    actions: Vec<Action>,
    dirty: bool,
    last_save: f64,
    ctx: egui::Context,
    /// Pending background import of Claude Code / Codex transcripts.
    importing: Option<Receiver<import::Report>>,
    /// Last import result and when it arrived (shown briefly in the sidebar).
    import_status: Option<(String, f64)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::install_style(&cc.egui_ctx);

        let mut saved = model::load().unwrap_or_default();
        let mut next_id = saved
            .sessions
            .iter()
            .flat_map(|s| std::iter::once(s.id).chain(s.messages.iter().map(|m| m.id)))
            .max()
            .unwrap_or(0)
            + 1;
        if saved.sessions.is_empty() {
            saved.sessions.push(Session::new(next_id, None));
            saved.active = next_id;
            saved.sidebar = true;
            saved.changes = true;
            next_id += 1;
        }
        if !saved.sessions.iter().any(|s| s.id == saved.active) {
            saved.active = saved.sessions[0].id;
        }

        let providers = Arc::new(Mutex::new((
            ProviderStatus::default(),
            ProviderStatus::default(),
        )));
        {
            let p = providers.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let (c, x) = (providers::check("claude"), providers::check("codex"));
                *p.lock().unwrap() = (c, x);
                ctx.request_repaint();
            });
        }

        let (tx, rx) = channel();
        App {
            saved,
            next_id,
            runs: HashMap::new(),
            owners: HashMap::new(),
            tx,
            rx,
            procs: Runs::default(),
            providers,
            composer: String::new(),
            view: View::default(),
            model_icons: ModelIcons::default(),
            logo: image::load_from_memory(include_bytes!("../assets/logo-96.png"))
                .ok()
                .map(|img| {
                    let img = img.into_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    cc.egui_ctx.load_texture(
                        "app-logo",
                        egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                        egui::TextureOptions::LINEAR,
                    )
                }),
            actions: vec![],
            dirty: false,
            last_save: 0.0,
            ctx: cc.egui_ctx.clone(),
            importing: None,
            import_status: None,
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn active_idx(&self) -> usize {
        self.saved
            .sessions
            .iter()
            .position(|s| s.id == self.saved.active)
            .unwrap_or(0)
    }

    fn active(&self) -> &Session {
        &self.saved.sessions[self.active_idx()]
    }

    fn active_mut(&mut self) -> &mut Session {
        let i = self.active_idx();
        self.dirty = true;
        &mut self.saved.sessions[i]
    }

    fn running(&self) -> bool {
        self.runs.contains_key(&self.saved.active)
    }

    // ───────────────────────────── actions ─────────────────────────────

    fn send(&mut self, prompt: String) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() || self.running() {
            return;
        }
        let (user_id, agent_id, run_id) = (self.id(), self.id(), self.id());
        let s = self.active_mut();
        let sid = s.id;
        let provider = s.provider;
        let relay = relay_context(s, provider);

        if s.messages.is_empty() {
            s.title = prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(60)
                .collect();
        }
        s.messages.push(Message {
            id: user_id,
            role: Role::User,
            provider,
            created: now_ms(),
            status: MsgStatus::Done,
            parts: vec![Part::Text {
                text: prompt.clone(),
                open: false,
            }],
            meta: Meta::default(),
        });
        let mut parts = vec![];
        if relay.is_some() {
            parts.push(Part::Notice {
                text: format!("Context handed over to {}", provider.label()),
                tone: Tone::Relay,
            });
        }
        s.messages.push(Message {
            id: agent_id,
            role: Role::Agent,
            provider,
            created: now_ms(),
            status: MsgStatus::Streaming,
            parts,
            meta: Meta::default(),
        });
        s.updated = now_ms();

        let req = RunRequest {
            run_id,
            provider,
            prompt: format!("{}{}", relay.unwrap_or_default(), prompt),
            cwd: Some(s.cwd.clone()),
            model: Some(s.model(provider).to_string()),
            effort: Some(s.effort(provider).to_string()),
            mode: s.mode,
            resume_id: s.thread(provider).cloned(),
        };
        self.runs.insert(sid, run_id);
        self.owners.insert(run_id, (sid, agent_id));
        let ctx = self.ctx.clone();
        if let Err(e) = agent::start(req, self.tx.clone(), self.procs.clone(), move || {
            ctx.request_repaint()
        }) {
            let _ = self.tx.send((run_id, AgentEvent::Error(e)));
            let _ = self.tx.send((run_id, AgentEvent::Exit(Some(1))));
        }
        self.save_now();
    }

    fn stop(&mut self) {
        let sid = self.saved.active;
        let Some(&run) = self.runs.get(&sid) else {
            return;
        };
        for m in &mut self.active_mut().messages {
            if m.status == MsgStatus::Streaming {
                m.status = MsgStatus::Stopped;
            }
        }
        self.procs.stop(run);
    }

    fn new_thread(&mut self) {
        if self.active().messages.is_empty() {
            return;
        }
        let id = self.id();
        let s = Session::new(id, Some(self.active()));
        self.saved.sessions.insert(0, s);
        self.saved.active = id;
        self.composer.clear();
        self.dirty = true;
    }

    fn delete(&mut self, id: u64) {
        if let Some(run) = self.runs.remove(&id) {
            self.procs.stop(run);
        }
        self.saved.sessions.retain(|s| s.id != id);
        if self.saved.sessions.is_empty() {
            let nid = self.id();
            self.saved.sessions.push(Session::new(nid, None));
        }
        if self.saved.active == id {
            self.saved.active = self.saved.sessions[0].id;
        }
        self.save_now();
    }

    fn pick_folder(&mut self) {
        let cur = self.active().cwd.clone();
        let mut dlg = rfd::FileDialog::new().set_title("Choose a workspace folder");
        if !cur.is_empty() {
            dlg = dlg.set_directory(&cur);
        }
        if let Some(dir) = dlg.pick_folder() {
            let s = self.active_mut();
            s.cwd = dir.to_string_lossy().into_owned();
            s.clear_threads(); // agent threads are tied to a folder
        }
    }

    fn drain_events(&mut self) {
        while let Ok((run, ev)) = self.rx.try_recv() {
            let Some(&(sid, mid)) = self.owners.get(&run) else {
                continue;
            };
            let Some(s) = self.saved.sessions.iter_mut().find(|s| s.id == sid) else {
                continue;
            };
            let provider = s
                .messages
                .iter()
                .find(|m| m.id == mid)
                .map(|m| m.provider)
                .unwrap_or(s.provider);
            match &ev {
                AgentEvent::Session { session_id, .. } if !session_id.is_empty() => {
                    s.set_thread(provider, session_id.clone())
                }
                AgentEvent::Done {
                    session_id: Some(id),
                    ..
                } if !id.is_empty() => s.set_thread(provider, id.clone()),
                _ => {}
            }
            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                apply_event(m, &ev);
            }
            s.updated = now_ms();
            self.dirty = true;
            if matches!(ev, AgentEvent::Exit(_)) {
                self.runs.remove(&sid);
                self.owners.remove(&run);
                self.save_now();
            }
        }
    }

    fn start_import(&mut self, source: import::Source) {
        if self.importing.is_some() {
            return;
        }
        let known: HashSet<String> = self
            .saved
            .sessions
            .iter()
            .flat_map(|s| [s.thread_claude.clone(), s.thread_codex.clone()])
            .flatten()
            .collect();
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(import::run(source, &known));
            ctx.request_repaint();
        });
        self.importing = Some(rx);
        self.import_status = None;
    }

    fn finish_import(&mut self) {
        let Some(rx) = &self.importing else { return };
        let report = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => import::Report::default(),
        };
        self.importing = None;
        let added = report.sessions.len();
        for mut s in report.sessions {
            s.id = self.id();
            for m in &mut s.messages {
                m.id = self.id();
            }
            self.saved.sessions.push(s);
        }
        // Drop the untouched placeholder thread once real history exists.
        if added > 0 && self.saved.sessions.len() > added {
            let active = self.saved.active;
            let placeholder = self
                .saved
                .sessions
                .iter()
                .position(|s| s.id == active && s.messages.is_empty());
            if let Some(i) = placeholder {
                self.saved.sessions.remove(i);
                self.saved.active = self
                    .saved
                    .sessions
                    .iter()
                    .max_by_key(|s| s.updated)
                    .map(|s| s.id)
                    .unwrap_or(0);
            }
        }
        let mut msg = match added {
            0 => "No new sessions found".to_string(),
            1 => "Imported 1 thread".to_string(),
            n => format!("Imported {n} threads"),
        };
        if report.claude > 0 && report.codex > 0 {
            msg.push_str(&format!(
                " ({} Claude, {} Codex)",
                report.claude, report.codex
            ));
        }
        if report.skipped > 0 {
            msg.push_str(&format!(" · {} already here", report.skipped));
        }
        if report.failed > 0 {
            msg.push_str(&format!(" · {} unreadable", report.failed));
        }
        self.import_status = Some((msg, self.ctx.input(|i| i.time)));
        self.save_now();
    }

    fn save_now(&mut self) {
        model::save(&self.saved);
        self.dirty = false;
        self.last_save = self.ctx.input(|i| i.time);
    }

    // ───────────────────────────── window chrome ─────────────────────────────

    fn window_drag(&self, ui: &Ui, rect: Rect, id: &str) {
        let resp = ui.interact(rect, Id::unique(id), Sense::click_and_drag());
        if resp.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if resp.double_clicked() {
            let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
        }
    }

    fn window_controls(&self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        for (icon, close) in [
            (Icon::Close, true),
            (Icon::Maximize, false),
            (Icon::Minimize, false),
        ] {
            let (rect, resp) = ui.allocate_exact_size(vec2(46.0, 44.0), Sense::click());
            let bg = if resp.hovered() {
                if close {
                    Color32::from_rgb(0xc4, 0x2b, 0x1c)
                } else {
                    HOVER
                }
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 0.0, bg);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(11.0, 11.0)),
                icon,
                if resp.hovered() { TEXT } else { TEXT_2 },
            );
            if resp.clicked() {
                ctx.send_viewport_cmd(match icon {
                    Icon::Close => ViewportCommand::Close,
                    Icon::Maximize => ViewportCommand::Maximized(!max),
                    _ => ViewportCommand::Minimized(true),
                });
            }
        }
    }

    /// Frameless windows need their own resize edges.
    fn resize_edges(&self, ui: &Ui) {
        if ui.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let r = ui.max_rect();
        let Some(p) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let m = 5.0;
        let (l, rt, t, b) = (
            p.x - r.left() < m,
            r.right() - p.x < m,
            p.y - r.top() < m,
            r.bottom() - p.y < m,
        );
        use egui::ResizeDirection as D;
        let dir = match (l, rt, t, b) {
            (true, _, true, _) => Some((D::NorthWest, CursorIcon::ResizeNwSe)),
            (_, true, true, _) => Some((D::NorthEast, CursorIcon::ResizeNeSw)),
            (true, _, _, true) => Some((D::SouthWest, CursorIcon::ResizeNeSw)),
            (_, true, _, true) => Some((D::SouthEast, CursorIcon::ResizeNwSe)),
            (true, ..) => Some((D::West, CursorIcon::ResizeHorizontal)),
            (_, true, ..) => Some((D::East, CursorIcon::ResizeHorizontal)),
            (_, _, true, _) => Some((D::North, CursorIcon::ResizeVertical)),
            (.., true) => Some((D::South, CursorIcon::ResizeVertical)),
            _ => None,
        };
        if let Some((d, cursor)) = dir {
            ui.ctx().set_cursor_icon(cursor);
            if ui.input(|i| i.pointer.primary_pressed()) {
                ui.ctx().send_viewport_cmd(ViewportCommand::BeginResize(d));
            }
        }
    }

    // ───────────────────────────── sidebar ─────────────────────────────

    fn sidebar(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        // Title strip doubles as a drag handle.
        let strip = Rect::from_min_size(full.min, vec2(full.width(), 44.0));
        self.window_drag(ui, strip, "drag-sidebar");
        let mut tx = strip.left() + 16.0;
        if let Some(logo) = &self.logo {
            let r = Rect::from_center_size(pos2(tx + 10.0, strip.center().y), vec2(20.0, 20.0));
            egui::Image::from_texture(logo)
                .fit_to_exact_size(r.size())
                .paint_at(ui, r);
            tx += 28.0;
        }
        ui.painter().text(
            pos2(tx, strip.center().y),
            Align2::LEFT_CENTER,
            "Open Agent",
            theme::semibold(13.0),
            TEXT,
        );
        let tog = Rect::from_center_size(
            pos2(strip.right() - 24.0, strip.center().y),
            vec2(28.0, 28.0),
        );
        if icon_button(ui, tog, Icon::Sidebar, "Hide sidebar (Ctrl+B)").clicked() {
            self.saved.sidebar = false;
            self.dirty = true;
        }
        ui.add_space(46.0);

        Frame::new()
            .inner_margin(Margin::symmetric(8, 0))
            .show(ui, |ui| {
                let resp = list_row(ui, 34.0, false, |ui, rect, hovered| {
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(
                            pos2(rect.left() + 18.0, rect.center().y),
                            vec2(14.0, 14.0),
                        ),
                        Icon::Pencil,
                        TEXT,
                    );
                    ui.painter().text(
                        pos2(rect.left() + 36.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        "New thread",
                        theme::body(13.5),
                        TEXT,
                    );
                    if hovered {
                        ui.painter().text(
                            pos2(rect.right() - 10.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            "Ctrl+N",
                            theme::body(11.5),
                            TEXT_4,
                        );
                    }
                });
                if resp.clicked() {
                    self.actions.push(Action::NewThread);
                }

                let busy = self.importing.is_some();
                let time = ui.input(|i| i.time);
                let resp = list_row(ui, 34.0, false, |ui, rect, _| {
                    let lead = pos2(rect.left() + 18.0, rect.center().y);
                    if busy {
                        spinner(ui, lead, 5.0, time, TEXT_2);
                    } else {
                        icons::paint(
                            ui.painter(),
                            Rect::from_center_size(lead, vec2(14.0, 14.0)),
                            Icon::Import,
                            TEXT,
                        );
                    }
                    let label = if busy {
                        "Importing sessions…"
                    } else {
                        "Import sessions"
                    };
                    ui.painter().text(
                        pos2(rect.left() + 36.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        theme::body(13.5),
                        TEXT,
                    );
                });
                let resp = resp.on_hover_text(
                    "Bring in your existing Claude Code, Claude Desktop and Codex sessions",
                );
                if !busy {
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.set_min_width(230.0);
                        ui.label(
                            RichText::new("Import sessions from")
                                .font(theme::body(11.5))
                                .color(TEXT_3),
                        );
                        for (source, text) in [
                            (import::Source::All, "Everything"),
                            (import::Source::Claude, "Claude Code & Claude Desktop"),
                            (import::Source::Codex, "Codex"),
                        ] {
                            if ui
                                .selectable_label(
                                    false,
                                    RichText::new(text).font(theme::body(13.0)),
                                )
                                .clicked()
                            {
                                self.actions.push(Action::Import(source));
                                ui.close();
                            }
                        }
                    });
                } else {
                    ui.ctx().request_repaint_after(Duration::from_millis(80));
                }
                if let Some((msg, at)) = &self.import_status {
                    if time - at < 10.0 {
                        ui.label(
                            RichText::new(format!("   {msg}"))
                                .font(theme::body(11.5))
                                .color(TEXT_3),
                        );
                        ui.ctx()
                            .request_repaint_after(Duration::from_secs_f64(10.0 - (time - at)));
                    }
                }

                ui.add_space(14.0);
                ui.label(
                    RichText::new("   Threads")
                        .font(theme::body(12.0))
                        .color(TEXT_3),
                );
                ui.add_space(2.0);

                let avail = ui.available_height() - 86.0;
                egui::ScrollArea::vertical()
                    .max_height(avail.max(60.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut order: Vec<usize> = (0..self.saved.sessions.len()).collect();
                        order.sort_by_key(|&i| std::cmp::Reverse(self.saved.sessions[i].updated));
                        let now = now_ms();
                        let time = ui.input(|i| i.time);
                        for i in order {
                            let s = &self.saved.sessions[i];
                            let (sid, active, live) = (
                                s.id,
                                s.id == self.saved.active,
                                self.runs.contains_key(&s.id),
                            );
                            let title = s.title.clone();
                            let ago = ago(now.saturating_sub(s.updated));
                            let (sprov, smodel) = (s.provider, s.model(s.provider).to_string());
                            let model_icons = &mut self.model_icons;
                            let mut del = false;
                            let resp = list_row(ui, 32.0, active, |ui, rect, hovered| {
                                let lead = pos2(rect.left() + 14.0, rect.center().y);
                                if live {
                                    spinner(ui, lead, 5.0, time, TEXT_2);
                                } else {
                                    model_icons.paint(
                                        ui,
                                        Rect::from_center_size(lead, vec2(14.0, 14.0)),
                                        sprov,
                                        &smodel,
                                    );
                                }
                                let right = if hovered { 34.0 } else { 40.0 };
                                let tr = Rect::from_min_max(
                                    pos2(rect.left() + 28.0, rect.top()),
                                    pos2(rect.right() - right, rect.bottom()),
                                );
                                let galley = ui.painter().layout_no_wrap(
                                    title.clone(),
                                    theme::body(13.5),
                                    if active { TEXT } else { TEXT_2 },
                                );
                                let g = if galley.size().x > tr.width() {
                                    ui.painter().layout(
                                        title.clone(),
                                        theme::body(13.5),
                                        if active { TEXT } else { TEXT_2 },
                                        f32::INFINITY,
                                    )
                                } else {
                                    galley
                                };
                                ui.painter().with_clip_rect(tr).galley(
                                    pos2(tr.left(), rect.center().y - g.size().y / 2.0),
                                    g,
                                    TEXT,
                                );
                                if hovered {
                                    let dr = Rect::from_center_size(
                                        pos2(rect.right() - 16.0, rect.center().y),
                                        vec2(22.0, 22.0),
                                    );
                                    let dresp =
                                        ui.interact(dr, Id::unique(("del", sid)), Sense::click());
                                    if dresp.hovered() {
                                        ui.painter().rect_filled(dr, 5.0, ACTIVE);
                                    }
                                    icons::paint(
                                        ui.painter(),
                                        dr.shrink(5.0),
                                        Icon::Trash,
                                        if dresp.hovered() { RED } else { TEXT_3 },
                                    );
                                    del = dresp.clicked();
                                } else {
                                    ui.painter().text(
                                        pos2(rect.right() - 10.0, rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        &ago,
                                        theme::body(11.5),
                                        TEXT_4,
                                    );
                                }
                            });
                            if del {
                                self.actions.push(Action::Delete(sid));
                            } else if resp.clicked() {
                                self.actions.push(Action::Select(sid));
                            }
                        }
                    });

                // Agent status, pinned to the bottom.
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.add_space(10.0);
                    let (c, x) = self.providers.lock().unwrap().clone();
                    for (p, st) in [(Provider::Codex, x), (Provider::Claude, c)] {
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                        let ok = st.installed;
                        let ir = Rect::from_center_size(
                            pos2(rect.left() + 14.0, rect.center().y),
                            vec2(14.0, 14.0),
                        );
                        if st.checked && !ok {
                            ui.painter().circle_filled(ir.center(), 3.0, RED);
                        } else {
                            self.model_icons.paint(ui, ir, p, "default");
                        }
                        let name = match p {
                            Provider::Claude => "Claude Code",
                            Provider::Codex => "Codex",
                        };
                        ui.painter().text(
                            pos2(rect.left() + 26.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            theme::body(12.5),
                            TEXT_2,
                        );
                        let detail = if !st.checked {
                            "checking…".to_string()
                        } else if !ok {
                            "not installed".into()
                        } else if p == Provider::Codex
                            && st.auth.as_deref().is_some_and(|a| a.contains("ChatGPT"))
                        {
                            "ChatGPT".into()
                        } else {
                            st.version
                                .clone()
                                .map(|v| format!("v{v}"))
                                .unwrap_or_default()
                        };
                        ui.painter().text(
                            pos2(rect.right() - 10.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            detail,
                            theme::body(11.5),
                            TEXT_4,
                        );
                    }
                    ui.add_space(4.0);
                    let r = ui.available_rect_before_wrap();
                    ui.painter()
                        .hline(r.x_range(), r.bottom(), Stroke::new(1.0, BORDER));
                });
            });
    }

    // ───────────────────────────── header ─────────────────────────────

    fn header(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        self.window_drag(ui, rect, "drag-header");
        let running = self.running();
        let s = self.active();
        let title = s.title.clone();
        let cwd = s.cwd.clone();
        let folder = folder_name(&cwd);
        let time = ui.input(|i| i.time);

        ui.horizontal_centered(|ui| {
            ui.add_space(if self.saved.sidebar { 14.0 } else { 8.0 });
            if !self.saved.sidebar {
                let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                if icon_button(ui, r, Icon::Sidebar, "Show sidebar (Ctrl+B)").clicked() {
                    self.saved.sidebar = true;
                    self.dirty = true;
                }
                ui.add_space(4.0);
            }
            if running {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                spinner(ui, r.center(), 5.0, time, TEXT_2);
            }
            ui.add(
                egui::Label::new(
                    RichText::new(&title)
                        .font(theme::semibold(13.5))
                        .color(TEXT),
                )
                .truncate()
                .selectable(false),
            );
            let resp = chip(
                ui,
                Icon::Folder,
                if folder.is_empty() {
                    "Choose folder".into()
                } else {
                    folder.clone()
                },
                false,
            );
            if resp
                .on_hover_text(if cwd.is_empty() {
                    "Choose a workspace folder"
                } else {
                    &cwd
                })
                .clicked()
            {
                self.actions.push(Action::PickFolder);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.window_controls(ui);
                ui.add_space(6.0);
                let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                if icon_button(ui, r, Icon::Panel, "Toggle changes (Ctrl+.)").clicked() {
                    self.saved.changes = !self.saved.changes;
                    self.dirty = true;
                }
            });
        });
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, BORDER),
        );
    }

    // ───────────────────────────── conversation ─────────────────────────────

    fn conversation(&mut self, ui: &mut Ui) {
        let idx = self.active_idx();
        if self.saved.sessions[idx].messages.is_empty() {
            self.empty_state(ui);
            return;
        }
        let time = ui.input(|i| i.time);
        let session = &self.saved.sessions[idx];
        let view = &mut self.view;
        let model_icons = &mut self.model_icons;
        egui::ScrollArea::vertical()
            .id_salt(("conv", session.id))
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                centered_column(ui, |ui| {
                    ui.add_space(24.0);
                    let mut prev_provider: Option<Provider> = None;
                    for m in &session.messages {
                        match m.role {
                            Role::User => user_bubble(ui, m),
                            Role::Agent => {
                                let show_name = prev_provider != Some(m.provider);
                                prev_provider = Some(m.provider);
                                let model = m
                                    .meta
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| session.model(m.provider).to_string());
                                agent_message(ui, m, view, model_icons, &model, time, show_name);
                            }
                        }
                        ui.add_space(18.0);
                    }
                    ui.add_space(12.0);
                });
            });
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let s = self.active();
        let folder = folder_name(&s.cwd);
        let provider = s.provider;
        let h = ui.available_height();
        centered_column(ui, |ui| {
            ui.add_space((h * 0.28).max(30.0));
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Let's build")
                        .font(theme::semibold(30.0))
                        .color(TEXT),
                );
                ui.add_space(6.0);
                let label = if folder.is_empty() {
                    "Choose a workspace folder".to_string()
                } else {
                    folder
                };
                let r = ui.add(
                    egui::Button::new(
                        RichText::new(format!("{label}  ▾"))
                            .font(theme::body(15.0))
                            .color(TEXT_3),
                    )
                    .frame(false),
                );
                if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    self.actions.push(Action::PickFolder);
                }
            });
            ui.add_space(36.0);
            let ideas = [
                ("Explain this codebase", "Give me a tour of this codebase: architecture, key modules, and how data flows."),
                ("Find and fix a bug", "Find the most likely bug in this project, explain it, and fix it."),
                ("Write tests", "Find the least-tested critical code here and write focused tests for it."),
                ("Refactor something", "Suggest the highest-impact refactor in this project, then do it."),
            ];
            let w = (ui.available_width() - 10.0) / 2.0;
            for row in ideas.chunks(2) {
                ui.horizontal(|ui| {
                    for (title, prompt) in row {
                        let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
                        let hov = resp.hovered();
                        ui.painter().rect(
                            rect,
                            12.0,
                            if hov { HOVER } else { PANEL },
                            Stroke::new(1.0, BORDER),
                            StrokeKind::Inside,
                        );
                        ui.painter().text(
                            pos2(rect.left() + 16.0, rect.top() + 22.0),
                            Align2::LEFT_CENTER,
                            *title,
                            theme::body(13.5),
                            TEXT,
                        );
                        let sub = ui.painter().layout(
                            prompt.to_string(),
                            theme::body(12.0),
                            TEXT_3,
                            f32::INFINITY,
                        );
                        ui.painter().with_clip_rect(rect.shrink(14.0)).galley(
                            pos2(rect.left() + 16.0, rect.top() + 34.0),
                            sub,
                            TEXT_3,
                        );
                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            self.actions.push(Action::Send(prompt.to_string()));
                        }
                    }
                });
                ui.add_space(4.0);
            }
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} is ready · Ctrl+J switches agents",
                        provider.label()
                    ))
                    .font(theme::body(12.0))
                    .color(TEXT_4),
                );
            });
        });
    }

    // ───────────────────────────── composer ─────────────────────────────

    fn composer(&mut self, ui: &mut Ui) {
        let running = self.running();
        let (claude_status, codex_status) = self.providers.lock().unwrap().clone();
        let edit_id = Id::unique("composer-input");

        centered_column(ui, |ui| {
            ui.add_space(4.0);
            // Enter sends, Shift+Enter is a newline.
            let focused = ui.memory(|m| m.has_focus(edit_id));
            if focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) && !running {
                let text = std::mem::take(&mut self.composer);
                self.actions.push(Action::Send(text));
            }

            let stroke = if focused {
                BORDER_STRONG.gamma_multiply(1.4)
            } else {
                BORDER_STRONG
            };
            Frame::new()
                .fill(RAISED)
                .stroke(Stroke::new(1.0, stroke))
                .corner_radius(CornerRadius::same(20))
                .inner_margin(Margin {
                    left: 16,
                    right: 10,
                    top: 12,
                    bottom: 10,
                })
                .shadow(egui::Shadow {
                    offset: [0, 6],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                })
                .show(ui, |ui| {
                    let provider = self.active().provider;
                    let hint = if running {
                        format!("{} is working…  Esc to stop", provider.label())
                    } else {
                        format!(
                            "Ask {} anything. @ to mention files is not needed — it can explore.",
                            provider.label()
                        )
                    };
                    let hint = if running {
                        hint
                    } else {
                        format!("Ask {} to build, fix or explain anything", provider.label())
                    };
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .id_salt("composer-scroll")
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.composer)
                                    .id(edit_id)
                                    .hint_text(RichText::new(hint).color(TEXT_4))
                                    .font(theme::body(14.5))
                                    .text_color(TEXT)
                                    .frame(Frame::new())
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .lock_focus(true),
                            );
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        self.model_menu(ui, &claude_status, &codex_status, running);
                        self.effort_menu(ui);
                        self.mode_menu(ui);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let (rect, resp) =
                                ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                            let can_send = !self.composer.trim().is_empty();
                            let (bg, fg, icon) = if running {
                                (TEXT, Color32::BLACK, Icon::Stop)
                            } else if can_send {
                                (TEXT, Color32::BLACK, Icon::ArrowUp)
                            } else {
                                (ACTIVE, TEXT_4, Icon::ArrowUp)
                            };
                            let bg = if resp.hovered() && (running || can_send) {
                                Color32::from_gray(0xd0)
                            } else {
                                bg
                            };
                            ui.painter().circle_filled(rect.center(), 16.0, bg);
                            icons::paint(
                                ui.painter(),
                                Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
                                icon,
                                fg,
                            );
                            if resp
                                .on_hover_text(if running {
                                    "Stop (Esc)"
                                } else {
                                    "Send (Enter)"
                                })
                                .clicked()
                            {
                                if running {
                                    self.actions.push(Action::Stop);
                                } else if can_send {
                                    let text = std::mem::take(&mut self.composer);
                                    self.actions.push(Action::Send(text));
                                }
                            }
                        });
                    });
                });

            // Footer: workspace + mode hint.
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let folder = folder_name(&self.active().cwd);
                let mode = self.active().mode;
                let r = chip(
                    ui,
                    Icon::Folder,
                    if folder.is_empty() {
                        "No folder".into()
                    } else {
                        folder
                    },
                    false,
                );
                if r.clicked() {
                    self.actions.push(Action::PickFolder);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(6.0);
                    let col = if mode == Mode::Auto { AMBER } else { TEXT_4 };
                    ui.add(
                        egui::Label::new(
                            RichText::new(mode.hint())
                                .font(theme::body(11.5))
                                .color(col),
                        )
                        .truncate(),
                    );
                });
            });
            ui.add_space(8.0);
        });
    }

    fn model_menu(
        &mut self,
        ui: &mut Ui,
        claude: &ProviderStatus,
        codex_st: &ProviderStatus,
        running: bool,
    ) {
        let s = self.active();
        let p = s.provider;
        let model = s.model(p).to_string();
        let name_of = |prov: Provider, m: &str| {
            let st = match prov {
                Provider::Claude => claude,
                Provider::Codex => codex_st,
            };
            st.model_names
                .get(m)
                .cloned()
                .unwrap_or_else(|| pretty_model(m))
        };
        let label = RichText::new(format!("{}  ·  {}  ▾", p.label(), name_of(p, &model)))
            .font(theme::body(12.5))
            .color(TEXT_2);
        let ctx = ui.ctx().clone();
        let current_icon = self.model_icons.image(&ctx, p, &model, 14.0);
        let r = ui.add_enabled_ui(!running, |ui| {
            let contents = |ui: &mut Ui| {
                ui.set_min_width(240.0);
                let claude_models: Vec<String> = if claude.models.is_empty() {
                    providers::CLAUDE_FALLBACK_MODELS
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    claude.models.clone()
                };
                let mut codex: Vec<String> = vec!["default".into()];
                codex.extend(codex_st.models.iter().cloned());
                for (prov, models) in [(Provider::Claude, claude_models), (Provider::Codex, codex)]
                {
                    ui.label(
                        RichText::new(match prov {
                            Provider::Claude => "Claude Code",
                            Provider::Codex => "Codex",
                        })
                        .font(theme::body(11.5))
                        .color(TEXT_3),
                    );
                    for m in models {
                        let sel = p == prov && model == m;
                        let text = RichText::new(name_of(prov, &m)).font(theme::body(13.0));
                        let resp = match self.model_icons.image(&ctx, prov, &m, 15.0) {
                            Some(img) => ui.selectable_label(sel, (img, text)),
                            None => ui.selectable_label(sel, text),
                        };
                        if resp.clicked() {
                            let s = self.active_mut();
                            s.provider = prov;
                            *s.model_mut(prov) = m.clone();
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                }
            };
            match current_icon {
                Some(img) => ui.menu_button((img, label), contents),
                None => ui.menu_button(label, contents),
            }
        });
        r.response
            .on_hover_text("Agent and model (Ctrl+J switches agent)");
    }

    fn effort_menu(&mut self, ui: &mut Ui) {
        let s = self.active();
        let p = s.provider;
        let cur = s.effort(p).to_string();
        ui.menu_button(
            RichText::new(format!("{}  ▾", pretty_effort(&cur)))
                .font(theme::body(12.5))
                .color(TEXT_2),
            |ui| {
                ui.set_min_width(150.0);
                ui.label(
                    RichText::new("Reasoning")
                        .font(theme::body(11.5))
                        .color(TEXT_3),
                );
                for e in p.efforts() {
                    if ui.selectable_label(cur == *e, pretty_effort(e)).clicked() {
                        *self.active_mut().effort_mut(p) = e.to_string();
                        ui.close();
                    }
                }
            },
        )
        .response
        .on_hover_text("Reasoning effort");
    }

    fn mode_menu(&mut self, ui: &mut Ui) {
        let cur = self.active().mode;
        let col = if cur == Mode::Auto { AMBER } else { TEXT_2 };
        ui.menu_button(
            RichText::new(format!("{}  ▾", cur.label()))
                .font(theme::body(12.5))
                .color(col),
            |ui| {
                ui.set_min_width(280.0);
                ui.label(
                    RichText::new("Approvals")
                        .font(theme::body(11.5))
                        .color(TEXT_3),
                );
                for m in Mode::ALL {
                    let resp = ui.selectable_label(
                        cur == m,
                        RichText::new(m.label()).font(theme::body(13.0)),
                    );
                    ui.label(
                        RichText::new(m.hint())
                            .font(theme::body(11.5))
                            .color(TEXT_4),
                    );
                    ui.add_space(2.0);
                    if resp.clicked() {
                        self.active_mut().mode = m;
                        ui.close();
                    }
                }
            },
        )
        .response
        .on_hover_text("What the agent may do without asking");
    }

    // ───────────────────────────── changes panel ─────────────────────────────

    fn changes_panel(&mut self, ui: &mut Ui) {
        let s = &self.saved.sessions[self.active_idx()];
        let changes = collect_changes(s);
        let view = &mut self.view;
        let rect = ui.max_rect();
        ui.painter()
            .vline(rect.left(), rect.y_range(), Stroke::new(1.0, BORDER));

        Frame::new()
            .inner_margin(Margin {
                left: 14,
                right: 12,
                top: 14,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Changes")
                            .font(theme::semibold(13.5))
                            .color(TEXT),
                    );
                    let (a, d) = changes
                        .iter()
                        .fold((0, 0), |(a, d), c| (a + c.added, d + c.removed));
                    if !changes.is_empty() {
                        ui.label(
                            RichText::new(format!("{} files", changes.len()))
                                .font(theme::body(12.0))
                                .color(TEXT_3),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("−{d}"))
                                    .font(theme::mono(11.5))
                                    .color(RED),
                            );
                            ui.label(
                                RichText::new(format!("+{a}"))
                                    .font(theme::mono(11.5))
                                    .color(GREEN),
                            );
                        });
                    }
                });
                ui.add_space(8.0);
                if changes.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No changes yet")
                                .font(theme::body(13.0))
                                .color(TEXT_3),
                        );
                        ui.label(
                            RichText::new("Files the agent edits show up here")
                                .font(theme::body(12.0))
                                .color(TEXT_4),
                        );
                    });
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for c in &changes {
                            let sel = view.selected_file.as_deref() == Some(c.path.as_str());
                            let resp = list_row(ui, 30.0, sel, |ui, rect, _| {
                                let name = c
                                    .path
                                    .rsplit(['/', '\\'])
                                    .next()
                                    .unwrap_or(&c.path)
                                    .to_string();
                                let (fi, col) = match c.kind {
                                    'A' => (Icon::Plus, GREEN),
                                    'D' => (Icon::Close, RED),
                                    _ => (Icon::File, TEXT_3),
                                };
                                icons::paint(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        pos2(rect.left() + 14.0, rect.center().y),
                                        vec2(13.0, 13.0),
                                    ),
                                    fi,
                                    col,
                                );
                                let tr = Rect::from_min_max(
                                    pos2(rect.left() + 28.0, rect.top()),
                                    pos2(rect.right() - 74.0, rect.bottom()),
                                );
                                ui.painter().with_clip_rect(tr).text(
                                    pos2(tr.left(), rect.center().y),
                                    Align2::LEFT_CENTER,
                                    name,
                                    theme::body(13.0),
                                    TEXT,
                                );
                                let counts = format!("+{}", c.added);
                                ui.painter().text(
                                    pos2(rect.right() - 36.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    counts,
                                    theme::mono(11.5),
                                    GREEN,
                                );
                                ui.painter().text(
                                    pos2(rect.right() - 6.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    format!("−{}", c.removed),
                                    theme::mono(11.5),
                                    RED,
                                );
                            });
                            let resp = resp.on_hover_text(&c.path);
                            if resp.clicked() {
                                view.selected_file = if sel { None } else { Some(c.path.clone()) };
                            }
                            if sel {
                                ui.add_space(4.0);
                                if c.hunks.is_empty() {
                                    ui.label(
                                        RichText::new("No diff available for this change")
                                            .font(theme::body(12.0))
                                            .color(TEXT_4),
                                    );
                                }
                                for (old, new) in &c.hunks {
                                    diff_view(ui, old, new);
                                    ui.add_space(6.0);
                                }
                                ui.add_space(6.0);
                            }
                        }
                    });
            });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (n, b, dot, j, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::N),
                i.consume_key(Modifiers::COMMAND, Key::B),
                i.consume_key(Modifiers::COMMAND, Key::Period),
                i.consume_key(Modifiers::COMMAND, Key::J),
                i.key_pressed(Key::Escape),
            )
        });
        if n {
            self.actions.push(Action::NewThread);
        }
        if b {
            self.saved.sidebar = !self.saved.sidebar;
            self.dirty = true;
        }
        if dot {
            self.saved.changes = !self.saved.changes;
            self.dirty = true;
        }
        if j && !self.running() {
            let s = self.active_mut();
            s.provider = s.provider.other();
        }
        if esc && self.running() {
            self.actions.push(Action::Stop);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_events();
        self.finish_import();
        self.model_icons.pump(&ctx);
        self.shortcuts(&ctx);
        let (mut sidebar_open, mut changes_open) = (self.saved.sidebar, self.saved.changes);

        Panel::left("sidebar")
            .exact_size(264.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(SIDEBAR))
            .show_collapsible(ui, &mut sidebar_open, |ui| self.sidebar(ui));

        // Header spans the full width right of the sidebar, so window controls sit top-right.
        Panel::top("header")
            .exact_size(44.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(BG))
            .show(ui, |ui| self.header(ui));

        Panel::right("changes")
            .exact_size(320.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(PANEL))
            .show_collapsible(ui, &mut changes_open, |ui| self.changes_panel(ui));

        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG))
            .show(ui, |ui| {
                Panel::bottom("composer")
                    .show_separator_line(false)
                    .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(0, 4)))
                    .show(ui, |ui| self.composer(ui));
                self.conversation(ui);
            });

        self.resize_edges(ui);

        for a in std::mem::take(&mut self.actions) {
            match a {
                Action::Send(t) => self.send(t),
                Action::NewThread => self.new_thread(),
                Action::Select(id) => {
                    self.saved.active = id;
                    self.dirty = true;
                }
                Action::Delete(id) => self.delete(id),
                Action::PickFolder => self.pick_folder(),
                Action::Stop => self.stop(),
                Action::Import(source) => self.start_import(source),
            }
        }

        // Only animate while an agent is working; otherwise egui sleeps until input.
        if !self.runs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        let now = ctx.input(|i| i.time);
        if self.dirty && now - self.last_save > 2.0 {
            self.save_now();
        } else if self.dirty {
            ctx.request_repaint_after(Duration::from_millis(2100));
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }
}

use crate::model;

// ───────────────────────────── message rendering ─────────────────────────────

fn user_bubble(ui: &mut Ui, m: &Message) {
    let text: String = m
        .parts
        .iter()
        .filter_map(|p| {
            if let Part::Text { text, .. } = p {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    let w = ui.available_width();
    let max_w = (w * 0.78).min(560.0);
    let galley = ui
        .painter()
        .layout(text, theme::body(14.0), TEXT, max_w - 28.0);
    let size = galley.size() + vec2(28.0, 20.0);
    let (row, _) = ui.allocate_exact_size(vec2(w, size.y), Sense::hover());
    let bubble = Rect::from_min_size(pos2(row.right() - size.x, row.top()), size);
    ui.painter().rect_filled(bubble, 18.0, RAISED);
    ui.painter()
        .galley(bubble.min + vec2(14.0, 10.0), galley, TEXT);
}

fn agent_message(
    ui: &mut Ui,
    m: &Message,
    view: &mut View,
    icons: &mut ModelIcons,
    model: &str,
    time: f64,
    show_name: bool,
) {
    if show_name {
        ui.horizontal(|ui| {
            model_icons::icon(ui, icons, m.provider, model, 16.0);
            let mut t = m.provider.label().to_string();
            if let Some(model) = &m.meta.model {
                t = format!("{t}  ·  {model}");
            }
            ui.label(RichText::new(t).font(theme::body(12.0)).color(TEXT_3));
        });
        ui.add_space(2.0);
    }
    let streaming = m.status == MsgStatus::Streaming;
    let visible: Vec<(usize, &Part)> = m
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| !matches!(p, Part::Tool { name, .. } if name == "__hidden"))
        .collect();

    for (i, p) in &visible {
        let key = format!("{}:{i}", m.id);
        match p {
            Part::Text { text, .. } if !text.is_empty() => {
                markdown::render(ui, text, Id::unique(("md", m.id, *i)));
                ui.add_space(4.0);
            }
            Part::Thinking {
                text,
                open,
                started,
                ended,
            } => thinking_row(ui, text, *open, *started, *ended, &key, view, time),
            Part::Tool {
                name,
                input,
                output,
                status,
                ..
            } => tool_row(ui, name, input, output, *status, &key, view, time),
            Part::Todo { items } => todo_card(ui, items),
            Part::Notice { text, tone } => notice(ui, text, *tone),
            _ => {}
        }
    }

    let last_is_live = matches!(
        visible.last(),
        Some((_, Part::Text { open: true, .. }))
            | Some((_, Part::Thinking { open: true, .. }))
            | Some((
                _,
                Part::Tool {
                    status: ToolStatus::Running,
                    ..
                }
            ))
    );
    if streaming && !last_is_live {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
            spinner(ui, r.center(), 5.0, time, TEXT_3);
            shimmer_label(ui, "Working", time);
        });
    }
    match m.status {
        MsgStatus::Stopped => {
            ui.label(
                RichText::new("Stopped")
                    .font(theme::body(12.0))
                    .color(TEXT_3),
            );
        }
        MsgStatus::Done | MsgStatus::Error if m.meta.duration_ms.is_some() => {
            let mut bits = vec![format!(
                "{:.1}s",
                m.meta.duration_ms.unwrap_or(0) as f64 / 1000.0
            )];
            if let Some(t) = m.meta.input_tokens {
                bits.push(format!("{} in", tokens(t)));
            }
            if let Some(t) = m.meta.output_tokens {
                bits.push(format!("{} out", tokens(t)));
            }
            ui.label(
                RichText::new(bits.join("  ·  "))
                    .font(theme::body(11.5))
                    .color(TEXT_4),
            );
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn thinking_row(
    ui: &mut Ui,
    text: &str,
    open: bool,
    started: u64,
    ended: u64,
    key: &str,
    view: &mut View,
    time: f64,
) {
    if text.trim().is_empty() && !open {
        return;
    }
    let expanded = view.expanded.contains(key);
    let resp = ui
        .horizontal(|ui| {
            if open {
                shimmer_label(ui, "Thinking", time);
            } else {
                let secs = if started > 0 && ended > started {
                    format!(
                        "Thought for {}s",
                        ((ended - started) as f64 / 1000.0).round().max(1.0)
                    )
                } else {
                    "Thought".into()
                };
                ui.label(RichText::new(secs).font(theme::body(13.0)).color(TEXT_3));
            }
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            icons::paint(
                ui.painter(),
                r,
                if expanded {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                TEXT_4,
            );
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        toggle(&mut view.expanded, key);
    }
    if expanded && !text.trim().is_empty() {
        ui.horizontal(|ui| {
            let h = ui.available_height().min(400.0);
            let _ = h;
            ui.add_space(2.0);
            let start = ui.cursor().min;
            ui.add_space(12.0);
            let r = ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(text.trim())
                            .font(theme::body(13.0))
                            .color(TEXT_3),
                    )
                    .wrap(),
                );
            });
            let bottom = r.response.rect.bottom();
            ui.painter()
                .vline(start.x, start.y..=bottom, Stroke::new(2.0, BORDER_STRONG));
        });
    }
    ui.add_space(2.0);
}

struct ToolDesc {
    icon: Icon,
    verb: String,
    target: String,
    added: usize,
    removed: usize,
}

fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if parts.len() > 3 {
        format!("…/{}", parts[parts.len() - 3..].join("/"))
    } else {
        p.to_string()
    }
}

fn lines(s: &str) -> usize {
    if s.is_empty() {
        0
    } else {
        s.lines().count().max(1)
    }
}

fn describe(name: &str, input: &Value, running: bool) -> ToolDesc {
    let st = |k: &str| {
        input
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let ing = |a: &str, b: &str| {
        if running {
            a.to_string()
        } else {
            b.to_string()
        }
    };
    let mut d = ToolDesc {
        icon: Icon::Bot,
        verb: name.to_string(),
        target: String::new(),
        added: 0,
        removed: 0,
    };
    match name {
        "Read" => {
            d.icon = Icon::File;
            d.verb = ing("Reading", "Read");
            d.target = short_path(&st("file_path"));
        }
        "Write" => {
            d.icon = Icon::Pencil;
            d.verb = ing("Creating", "Created");
            d.target = short_path(&st("file_path"));
            d.added = lines(&st("content"));
        }
        "Edit" => {
            d.icon = Icon::Pencil;
            d.verb = ing("Editing", "Edited");
            d.target = short_path(&st("file_path"));
            d.added = lines(&st("new_string"));
            d.removed = lines(&st("old_string"));
        }
        "MultiEdit" => {
            d.icon = Icon::Pencil;
            d.verb = ing("Editing", "Edited");
            d.target = short_path(&st("file_path"));
            for e in input
                .get("edits")
                .and_then(|e| e.as_array())
                .into_iter()
                .flatten()
            {
                d.added += lines(e["new_string"].as_str().unwrap_or_default());
                d.removed += lines(e["old_string"].as_str().unwrap_or_default());
            }
        }
        "NotebookEdit" => {
            d.icon = Icon::Pencil;
            d.verb = ing("Editing", "Edited");
            d.target = short_path(&st("notebook_path"));
        }
        "Bash" | "PowerShell" | "shell" => {
            d.icon = Icon::Terminal;
            d.verb = ing("Running", "Ran");
            d.target = st("command").lines().next().unwrap_or_default().to_string();
        }
        "Glob" => {
            d.icon = Icon::Search;
            d.verb = ing("Finding files", "Found files");
            d.target = st("pattern");
        }
        "Grep" => {
            d.icon = Icon::Search;
            d.verb = ing("Searching", "Searched");
            d.target = st("pattern");
        }
        "LS" => {
            d.icon = Icon::Folder;
            d.verb = ing("Listing", "Listed");
            d.target = short_path(&st("path"));
        }
        "WebFetch" => {
            d.icon = Icon::Globe;
            d.verb = ing("Fetching", "Fetched");
            d.target = st("url");
        }
        "WebSearch" | "web_search" => {
            d.icon = Icon::Globe;
            d.verb = ing("Searching the web", "Searched the web");
            d.target = st("query");
        }
        "Task" | "Agent" => {
            d.icon = Icon::Bot;
            d.verb = ing("Running subagent", "Subagent");
            d.target = st("description");
        }
        "ExitPlanMode" => {
            d.icon = Icon::Check;
            d.verb = "Plan ready".into();
        }
        "file_change" => {
            d.icon = Icon::Pencil;
            let ch = input
                .get("changes")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            d.verb = ing(
                "Editing",
                if ch.len() > 1 {
                    "Edited files"
                } else {
                    "Edited"
                },
            );
            d.target = ch
                .iter()
                .filter_map(|c| c["path"].as_str())
                .map(short_path)
                .collect::<Vec<_>>()
                .join(", ");
        }
        n if n.starts_with("mcp") => {
            d.icon = Icon::Globe;
            d.verb = n
                .trim_start_matches("mcp:")
                .trim_start_matches("mcp__")
                .replace("__", " / ");
        }
        _ => {}
    }
    d
}

#[allow(clippy::too_many_arguments)]
fn tool_row(
    ui: &mut Ui,
    name: &str,
    input: &Value,
    output: &str,
    status: ToolStatus,
    key: &str,
    view: &mut View,
    time: f64,
) {
    let running = status == ToolStatus::Running;
    let d = describe(name, input, running);
    let expanded = view.expanded.contains(key);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    let col = if hov { TEXT } else { TEXT_2 };
    let mut x = rect.left();
    let ir = Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0));
    if running {
        spinner(ui, ir.center(), 5.0, time, TEXT_3);
    } else {
        icons::paint(
            p,
            ir,
            d.icon,
            if status == ToolStatus::Error {
                RED
            } else {
                TEXT_3
            },
        );
    }
    x += 22.0;
    let verb = p.layout_no_wrap(d.verb.clone(), theme::body(13.0), col);
    let vw = verb.size().x;
    p.galley(pos2(x, rect.center().y - verb.size().y / 2.0), verb, col);
    x += vw + 8.0;
    let right_reserved = if d.added + d.removed > 0 { 90.0 } else { 24.0 };
    let tr = Rect::from_min_max(
        pos2(x, rect.top()),
        pos2(rect.right() - right_reserved, rect.bottom()),
    );
    if !d.target.is_empty() && tr.width() > 20.0 {
        let g = p.layout_no_wrap(d.target.clone(), theme::mono(12.0), TEXT_3);
        let gw = g.size().x;
        p.with_clip_rect(tr)
            .galley(pos2(x, rect.center().y - g.size().y / 2.0), g, TEXT_3);
        if gw > tr.width() {
            // fade out overflow
            let fade = Rect::from_min_max(pos2(tr.right() - 24.0, tr.top()), tr.max);
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(fade.left_top(), Color32::TRANSPARENT);
            mesh.colored_vertex(fade.right_top(), BG);
            mesh.colored_vertex(fade.right_bottom(), BG);
            mesh.colored_vertex(fade.left_bottom(), Color32::TRANSPARENT);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(mesh);
        }
    }
    if d.added + d.removed > 0 {
        p.text(
            pos2(rect.right() - 54.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("+{}", d.added),
            theme::mono(11.5),
            GREEN,
        );
        p.text(
            pos2(rect.right() - 22.0, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("−{}", d.removed),
            theme::mono(11.5),
            RED,
        );
    }
    if hov || expanded {
        icons::paint(
            p,
            Rect::from_center_size(pos2(rect.right() - 7.0, rect.center().y), vec2(11.0, 11.0)),
            if expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            TEXT_3,
        );
    }
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        toggle(&mut view.expanded, key);
    }

    if expanded {
        ui.add_space(2.0);
        let st = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or_default();
        match name {
            "Edit" => diff_view(ui, st("old_string"), st("new_string")),
            "MultiEdit" => {
                for e in input
                    .get("edits")
                    .and_then(|e| e.as_array())
                    .into_iter()
                    .flatten()
                {
                    diff_view(
                        ui,
                        e["old_string"].as_str().unwrap_or_default(),
                        e["new_string"].as_str().unwrap_or_default(),
                    );
                    ui.add_space(4.0);
                }
            }
            "Write" => diff_view(ui, "", st("content")),
            "Bash" | "PowerShell" | "shell" => {
                let mut body = format!("$ {}", st("command"));
                if !output.is_empty() {
                    body.push_str("\n\n");
                    body.push_str(&truncate(output, 12000));
                }
                markdown::code_block(ui, "shell", &body, Id::unique(("tool", key)));
            }
            _ => {
                let mut body = String::new();
                if !input.is_null()
                    && name != "Read"
                    && input.as_object().is_some_and(|o| !o.is_empty())
                {
                    body.push_str(&serde_json::to_string_pretty(input).unwrap_or_default());
                }
                if !output.is_empty() {
                    if !body.is_empty() {
                        body.push_str("\n\n");
                    }
                    body.push_str(&truncate(output, 12000));
                }
                if !body.is_empty() {
                    markdown::code_block(ui, "text", &body, Id::unique(("tool", key)));
                }
            }
        }
        ui.add_space(4.0);
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut cut = n;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… ({} more bytes)", &s[..cut], s.len() - cut)
}

fn diff_view(ui: &mut Ui, old: &str, new: &str) {
    Frame::new()
        .fill(CODE_BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(0, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let row = |ui: &mut Ui, sign: &str, line: &str, bg: Color32, fg: Color32| {
                let text = format!("{sign} {line}");
                let g =
                    ui.painter()
                        .layout(text, theme::mono(12.0), fg, ui.available_width() - 20.0);
                let (r, _) = ui.allocate_exact_size(
                    vec2(ui.available_width(), g.size().y + 2.0),
                    Sense::hover(),
                );
                if bg != Color32::TRANSPARENT {
                    ui.painter().rect_filled(r, 0.0, bg);
                }
                ui.painter()
                    .galley(pos2(r.left() + 10.0, r.top() + 1.0), g, fg);
            };
            ui.spacing_mut().item_spacing.y = 0.0;
            let max = 400;
            for l in old.lines().take(max) {
                row(ui, "-", l, DIFF_DEL_BG, Color32::from_rgb(0xf2, 0xa3, 0xa3));
            }
            for l in new.lines().take(max) {
                row(ui, "+", l, DIFF_ADD_BG, Color32::from_rgb(0xa3, 0xe0, 0xb5));
            }
            if old.lines().count() > max || new.lines().count() > max {
                row(ui, " ", "…", Color32::TRANSPARENT, TEXT_4);
            }
        });
}

fn todo_card(ui: &mut Ui, items: &[TodoItem]) {
    let done = items.iter().filter(|i| i.done).count();
    Frame::new()
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Plan")
                        .font(theme::semibold(13.0))
                        .color(TEXT),
                );
                ui.label(
                    RichText::new(format!("{done} of {}", items.len()))
                        .font(theme::body(12.0))
                        .color(TEXT_3),
                );
            });
            ui.add_space(2.0);
            for it in items {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                    let b = Rect::from_center_size(r.center(), vec2(13.0, 13.0));
                    if it.done {
                        ui.painter().rect_filled(b, 4.0, TEXT_2);
                        icons::paint(ui.painter(), b.shrink(1.5), Icon::Check, Color32::BLACK);
                    } else {
                        ui.painter().rect_stroke(
                            b,
                            4.0,
                            Stroke::new(1.2, if it.active { TEXT } else { TEXT_4 }),
                            StrokeKind::Inside,
                        );
                    }
                    let col = if it.done {
                        TEXT_4
                    } else if it.active {
                        TEXT
                    } else {
                        TEXT_2
                    };
                    let mut rt = RichText::new(&it.text).font(theme::body(13.0)).color(col);
                    if it.done {
                        rt = rt.strikethrough();
                    }
                    ui.add(egui::Label::new(rt).wrap());
                });
            }
        });
    ui.add_space(4.0);
}

fn notice(ui: &mut Ui, text: &str, tone: Tone) {
    match tone {
        Tone::Error => {
            Frame::new()
                .fill(DIFF_DEL_BG)
                .stroke(Stroke::new(1.0, Color32::from_rgb(0x5a, 0x2a, 0x2a)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Warn, RED);
                        ui.add(
                            egui::Label::new(
                                RichText::new(text)
                                    .font(theme::body(13.0))
                                    .color(Color32::from_rgb(0xf4, 0xb4, 0xb4)),
                            )
                            .wrap(),
                        );
                    });
                });
        }
        Tone::Relay => {
            ui.label(
                RichText::new(format!("↻  {text}"))
                    .font(theme::body(12.0))
                    .color(TEXT_3),
            );
        }
        Tone::Info => {
            ui.add(
                egui::Label::new(RichText::new(text).font(theme::body(12.5)).color(TEXT_3)).wrap(),
            );
        }
    }
    ui.add_space(4.0);
}

// ───────────────────────────── changes model ─────────────────────────────

struct Change {
    path: String,
    kind: char,
    added: usize,
    removed: usize,
    hunks: Vec<(String, String)>,
}

fn collect_changes(s: &Session) -> Vec<Change> {
    let mut out: Vec<Change> = vec![];
    let mut touch = |path: &str, kind: char, old: &str, new: &str| {
        let i = match out.iter().position(|c| c.path == path) {
            Some(i) => i,
            None => {
                out.push(Change {
                    path: path.to_string(),
                    kind,
                    added: 0,
                    removed: 0,
                    hunks: vec![],
                });
                out.len() - 1
            }
        };
        let c = &mut out[i];
        if kind == 'D' {
            c.kind = 'D';
        }
        c.added += lines(new);
        c.removed += lines(old);
        if !old.is_empty() || !new.is_empty() {
            c.hunks.push((old.to_string(), new.to_string()));
        }
    };
    for m in s.messages.iter().filter(|m| m.role == Role::Agent) {
        for p in &m.parts {
            let Part::Tool {
                name,
                input,
                status,
                ..
            } = p
            else {
                continue;
            };
            if *status == ToolStatus::Error {
                continue;
            }
            let st = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or_default();
            match name.as_str() {
                "Edit" => touch(st("file_path"), 'M', st("old_string"), st("new_string")),
                "Write" => touch(st("file_path"), 'A', "", st("content")),
                "MultiEdit" => {
                    for e in input
                        .get("edits")
                        .and_then(|e| e.as_array())
                        .into_iter()
                        .flatten()
                    {
                        touch(
                            st("file_path"),
                            'M',
                            e["old_string"].as_str().unwrap_or_default(),
                            e["new_string"].as_str().unwrap_or_default(),
                        );
                    }
                }
                "NotebookEdit" => touch(st("notebook_path"), 'M', "", st("new_source")),
                "file_change" => {
                    for c in input
                        .get("changes")
                        .and_then(|c| c.as_array())
                        .into_iter()
                        .flatten()
                    {
                        let kind = match c["kind"].as_str() {
                            Some("add") => 'A',
                            Some("delete") => 'D',
                            _ => 'M',
                        };
                        // Imported Codex patches carry old/new lines; live events only carry paths.
                        let (old, new) = (
                            c["old"].as_str().unwrap_or_default(),
                            c["new"].as_str().unwrap_or_default(),
                        );
                        touch(c["path"].as_str().unwrap_or_default(), kind, old, new);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

// ───────────────────────────── small widgets ─────────────────────────────

fn centered_column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let avail = ui.available_width();
    let w = (avail - 48.0).clamp(200.0, COLUMN);
    ui.horizontal(|ui| {
        ui.add_space(((avail - w) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui);
        });
    });
}

fn list_row(
    ui: &mut Ui,
    h: f32,
    selected: bool,
    paint: impl FnOnce(&mut Ui, Rect, bool),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let hovered = resp.hovered();
    if selected || hovered {
        ui.painter()
            .rect_filled(rect, 8.0, if selected { ACTIVE } else { HOVER });
    }
    paint(ui, rect, hovered);
    resp
}

fn icon_button(ui: &mut Ui, rect: Rect, icon: Icon, tip: &str) -> Response {
    let resp = ui.interact(rect, Id::unique(("icon-btn", tip)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, HOVER);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
        icon,
        if resp.hovered() { TEXT } else { TEXT_3 },
    );
    resp.on_hover_text(tip)
}

fn chip(ui: &mut Ui, icon: Icon, text: String, strong: bool) -> Response {
    let g =
        ui.painter()
            .layout_no_wrap(text, theme::body(12.5), if strong { TEXT } else { TEXT_3 });
    let size = vec2(g.size().x + 32.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hov = resp.hovered();
    if hov {
        ui.painter().rect_filled(rect, 7.0, HOVER);
    }
    let col = if hov { TEXT } else { TEXT_3 };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(13.0, 13.0)),
        icon,
        col,
    );
    ui.painter().galley(
        pos2(rect.left() + 26.0, rect.center().y - g.size().y / 2.0),
        g,
        col,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

fn spinner(ui: &Ui, center: egui::Pos2, r: f32, time: f64, color: Color32) {
    let start = (time * 6.0) as f32;
    ui.painter().add(egui::Shape::line(
        icons::arc(center, r, start, 4.2),
        Stroke::new(1.6, color),
    ));
}

/// Gray text with a soft highlight sweeping through it — the "working" cue.
fn shimmer_label(ui: &mut Ui, text: &str, time: f64) {
    let mut job = egui::text::LayoutJob::default();
    let n = text.chars().count() as f32;
    let pos = ((time * 1.2) % 1.6) as f32 / 1.2 * (n + 6.0) - 3.0;
    for (i, ch) in text.chars().enumerate() {
        let d = (i as f32 - pos).abs();
        let t = (1.0 - d / 3.0).clamp(0.0, 1.0);
        let c = lerp(TEXT_4, TEXT, t);
        job.append(
            &ch.to_string(),
            0.0,
            egui::TextFormat {
                font_id: theme::body(13.0),
                color: c,
                ..Default::default()
            },
        );
    }
    ui.add(egui::Label::new(job).selectable(false));
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn toggle(set: &mut HashSet<String>, key: &str) {
    if !set.remove(key) {
        set.insert(key.to_string());
    }
}

fn folder_name(cwd: &str) -> String {
    cwd.split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or_default()
        .to_string()
}

fn pretty_model(m: &str) -> String {
    match m {
        "default" => "Default".into(),
        "opus" => "Opus".into(),
        "sonnet" => "Sonnet".into(),
        "haiku" => "Haiku".into(),
        "fable" => "Fable".into(),
        other => other.to_string(),
    }
}

fn pretty_effort(e: &str) -> String {
    match e {
        "default" => "Auto effort".into(),
        "xhigh" => "Extra high".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

fn tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

fn ago(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}
