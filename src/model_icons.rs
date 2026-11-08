//! Brand icons for any model you put in Open Agent.
//!
//! Icons come from the open-source LobeHub icon set (MIT,
//! github.com/lobehub/lobe-icons), served as PNGs from jsDelivr. Each icon is
//! downloaded once in the background, cached in `%APPDATA%\Open Agent\icons`,
//! and turned into a GPU texture. Until an icon arrives, or if a model is
//! unknown, a lettered badge is drawn instead.

use crate::model::Provider;
use crate::theme;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const CDN: &str = "https://cdn.jsdelivr.net/npm/@lobehub/icons-static-png@2.6.0/dark";

/// Known model families → LobeHub icon slugs (checked in order, first match wins).
const FAMILIES: &[(&[&str], &str)] = &[
    (
        &["claude", "anthropic", "opus", "sonnet", "haiku", "fable"],
        "claude-color",
    ),
    (
        &[
            "gpt", "openai", "codex", "chatgpt", "o1", "o3", "o4", "davinci",
        ],
        "openai",
    ),
    (&["gemini", "gemma", "bard"], "gemini-color"),
    (&["deepseek"], "deepseek-color"),
    (&["grok", "xai"], "grok"),
    (&["qwen", "qwq", "tongyi"], "qwen-color"),
    (
        &[
            "mistral",
            "mixtral",
            "codestral",
            "devstral",
            "magistral",
            "ministral",
            "pixtral",
        ],
        "mistral-color",
    ),
    (&["kimi", "moonshot"], "kimi-color"),
    (&["llama", "meta"], "meta-color"),
    (&["glm", "zhipu", "chatglm"], "zhipu-color"),
    (&["minimax", "abab"], "minimax-color"),
    (&["phi", "microsoft", "copilot"], "microsoft-color"),
    (&["command", "cohere", "aya"], "cohere-color"),
    (&["nova", "bedrock", "amazon"], "aws-color"),
    (&["sonar", "perplexity"], "perplexity-color"),
    (&["ollama"], "ollama"),
    (&["hunyuan"], "hunyuan-color"),
    (&["doubao", "seed"], "doubao-color"),
    (&["yi-", "01-ai"], "yi-color"),
    (&["baichuan"], "baichuan-color"),
    (&["ernie", "wenxin"], "wenxin-color"),
    (&["nemotron", "nvidia"], "nvidia-color"),
    (&["openrouter"], "openrouter"),
];

/// The icon slugs to try, best first, for a provider + model name.
pub fn candidates(provider: Provider, model: &str) -> Vec<String> {
    let m = model.to_lowercase();
    if m.is_empty() || m == "default" {
        return vec![default_slug(provider).into()];
    }
    for (keys, slug) in FAMILIES {
        if keys.iter().any(|k| m.contains(k)) {
            return vec![(*slug).into()];
        }
    }
    // Unknown model: guess from its first word ("foo-2-pro" → "foo"), then fall back to the agent's icon.
    let word: String = m
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|w| w.len() >= 2)
        .unwrap_or_default()
        .to_string();
    let mut out = vec![];
    if !word.is_empty() {
        out.push(format!("{word}-color"));
        out.push(word);
    }
    out.push(default_slug(provider).into());
    out
}

fn default_slug(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "claude-color",
        Provider::Codex => "openai",
    }
}

fn cache_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Open Agent")
        .join("icons")
}

enum Slot {
    Loading,
    Missing,
    /// Full-resolution source, alpha premultiplied, ready to resample.
    Ready(Arc<image::RgbaImage>),
}

#[derive(Default)]
pub struct ModelIcons {
    slots: HashMap<String, Slot>,
    /// Decoded images waiting on the UI thread (None = not found).
    inbox: Arc<Mutex<Vec<(String, Option<image::RgbaImage>)>>>,
    /// Remembers which candidate list resolved to which slug.
    resolved: HashMap<String, String>,
    failed: HashSet<String>,
    /// One texture per (slug, physical pixel size), resampled to match the screen 1:1.
    textures: HashMap<(String, u32), TextureHandle>,
}

impl ModelIcons {
    /// Collect finished downloads. Call once per frame.
    pub fn pump(&mut self, _ctx: &egui::Context) {
        let done: Vec<_> = std::mem::take(&mut *self.inbox.lock().unwrap());
        for (slug, img) in done {
            let slot = match img {
                Some(img) => Slot::Ready(Arc::new(img)),
                None => {
                    self.failed.insert(slug.clone());
                    Slot::Missing
                }
            };
            self.slots.insert(slug, slot);
        }
    }

    fn request(&mut self, slug: &str, ctx: &egui::Context) {
        if self.slots.contains_key(slug) {
            return;
        }
        self.slots.insert(slug.to_string(), Slot::Loading);
        let (slug, inbox, ctx) = (slug.to_string(), self.inbox.clone(), ctx.clone());
        std::thread::spawn(move || {
            let img = load_or_fetch(&slug);
            inbox.lock().unwrap().push((slug, img));
            ctx.request_repaint();
        });
    }

    /// The resolved slug for this model, if its image is loaded. Starts a download if needed.
    fn slug(&mut self, ctx: &egui::Context, provider: Provider, model: &str) -> Option<String> {
        let key = format!("{provider:?}/{model}");
        if let Some(slug) = self.resolved.get(&key) {
            if let Some(Slot::Ready(_)) = self.slots.get(slug) {
                return Some(slug.clone());
            }
        }
        for slug in candidates(provider, model) {
            if self.failed.contains(&slug) {
                continue;
            }
            match self.slots.get(&slug) {
                Some(Slot::Ready(_)) => {
                    self.resolved.insert(key, slug.clone());
                    return Some(slug);
                }
                Some(Slot::Loading) => return None,
                Some(Slot::Missing) => continue,
                None => {
                    self.request(&slug, ctx);
                    return None;
                }
            }
        }
        None
    }

    /// A texture of exactly `px` × `px` physical pixels for this model.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        provider: Provider,
        model: &str,
        px: u32,
    ) -> Option<TextureHandle> {
        let slug = self.slug(ctx, provider, model)?;
        let px = px.clamp(1, 512);
        let key = (slug.clone(), px);
        if let Some(t) = self.textures.get(&key) {
            return Some(t.clone());
        }
        let Some(Slot::Ready(src)) = self.slots.get(&slug) else {
            return None;
        };
        let small =
            image::imageops::resize(src.as_ref(), px, px, image::imageops::FilterType::Lanczos3);
        let img = ColorImage::from_rgba_premultiplied([px as usize, px as usize], small.as_raw());
        let tex = ctx.load_texture(format!("icon-{slug}-{px}"), img, TextureOptions::LINEAR);
        self.textures.insert(key, tex.clone());
        Some(tex)
    }

    /// Paint the model's icon into `rect` (a lettered badge until it loads).
    pub fn paint(&mut self, ui: &Ui, rect: Rect, provider: Provider, model: &str) {
        let ppp = ui.ctx().pixels_per_point();
        let px = (rect.width() * ppp).round().max(1.0);
        // Snap to the physical pixel grid so each texel lands on exactly one screen pixel.
        let min = pos2(
            ((rect.center().x * ppp) - px / 2.0).round() / ppp,
            ((rect.center().y * ppp) - px / 2.0).round() / ppp,
        );
        let snapped = Rect::from_min_size(min, vec2(px / ppp, px / ppp));
        match self.texture(ui.ctx(), provider, model, px as u32) {
            Some(tex) => {
                ui.painter().image(
                    tex.id(),
                    snapped,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                let label = if model.is_empty() || model == "default" {
                    provider.label()
                } else {
                    model
                };
                let letter = label
                    .chars()
                    .find(|c| c.is_alphanumeric())
                    .unwrap_or('?')
                    .to_ascii_uppercase();
                ui.painter().circle_filled(
                    rect.center(),
                    rect.width() / 2.0,
                    Color32::from_gray(0x3a),
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    letter,
                    theme::semibold(rect.height() * 0.6),
                    theme::TEXT,
                );
            }
        }
    }

    /// An `egui::Image` sized for inline use in buttons and menus.
    pub fn image(
        &mut self,
        ctx: &egui::Context,
        provider: Provider,
        model: &str,
        size: f32,
    ) -> Option<egui::Image<'static>> {
        let ppp = ctx.pixels_per_point();
        let px = (size * ppp).round().max(1.0);
        self.texture(ctx, provider, model, px as u32)
            .map(|t| egui::Image::from_texture(&t).fit_to_exact_size(vec2(px / ppp, px / ppp)))
    }
}

/// Allocate a square in the layout and paint the icon there.
pub fn icon(ui: &mut Ui, icons: &mut ModelIcons, provider: Provider, model: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    icons.paint(ui, rect, provider, model);
}

#[allow(dead_code)]
pub fn centered(r: Rect, size: f32) -> Rect {
    Rect::from_center_size(pos2(r.center().x, r.center().y), vec2(size, size))
}

/// Decode a PNG and premultiply its alpha, so resampling doesn't pull dark
/// colour out of fully transparent pixels (which leaves fringes on edges).
fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut img = image::load_from_memory(bytes).ok()?.into_rgba8();
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + 127) / 255) as u8;
        }
    }
    Some(img)
}

fn load_or_fetch(slug: &str) -> Option<image::RgbaImage> {
    let dir = cache_dir();
    let path = dir.join(format!("{slug}.png"));
    let miss = dir.join(format!("{slug}.missing"));
    if let Ok(bytes) = std::fs::read(&path) {
        return decode(&bytes);
    }
    if miss.exists() {
        return None;
    }
    let resp = ureq::get(&format!("{CDN}/{slug}.png")).call();
    let _ = std::fs::create_dir_all(&dir);
    match resp {
        Ok(mut r) => {
            let bytes = r.body_mut().read_to_vec().ok()?;
            let img = decode(&bytes)?;
            let _ = std::fs::write(&path, &bytes);
            Some(img)
        }
        Err(ureq::Error::StatusCode(404)) => {
            let _ = std::fs::write(&miss, b"");
            None
        }
        Err(_) => None, // offline: try again next launch
    }
}
