//! Calm, monochrome dark theme in the spirit of the Codex desktop app.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a); // main surface
pub const SIDEBAR: Color32 = Color32::from_rgb(0x13, 0x13, 0x13);
pub const PANEL: Color32 = Color32::from_rgb(0x16, 0x16, 0x16);
pub const RAISED: Color32 = Color32::from_rgb(0x24, 0x24, 0x24); // composer, bubbles
pub const HOVER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const ACTIVE: Color32 = Color32::from_rgb(0x2f, 0x2f, 0x2f);
pub const CODE_BG: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
pub const BORDER: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2c);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);

pub const TEXT: Color32 = Color32::from_rgb(0xec, 0xec, 0xec);
pub const TEXT_2: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);
pub const TEXT_3: Color32 = Color32::from_rgb(0x74, 0x74, 0x74);
pub const TEXT_4: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);

pub const GREEN: Color32 = Color32::from_rgb(0x4c, 0xc2, 0x7a);
pub const RED: Color32 = Color32::from_rgb(0xf0, 0x6a, 0x6a);
pub const AMBER: Color32 = Color32::from_rgb(0xe8, 0xb3, 0x4f);
pub const DIFF_ADD_BG: Color32 = Color32::from_rgb(0x17, 0x2a, 0x1e);
pub const DIFF_DEL_BG: Color32 = Color32::from_rgb(0x30, 0x19, 0x1a);

pub const MONO: &str = "mono";
pub const SEMIBOLD: &str = "semibold";

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO.into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Use the OS's own UI fonts (Segoe UI + Cascadia on Windows) — no bundled font files.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var_os("WINDIR")
        .map(|w| std::path::PathBuf::from(w).join("Fonts"))
        .unwrap_or_default();
    let mut load = |key: &str, files: &[&str]| -> bool {
        for f in files {
            if let Ok(bytes) = std::fs::read(dir.join(f)) {
                fonts
                    .font_data
                    .insert(key.into(), Arc::new(FontData::from_owned(bytes)));
                return true;
            }
        }
        false
    };
    let has_ui = load("ui", &["segoeui.ttf"]);
    let has_sb = load("ui-semibold", &["seguisb.ttf", "segoeuib.ttf"]);
    let has_mono = load(
        "ui-mono",
        &["CascadiaMono.ttf", "CascadiaCode.ttf", "consola.ttf"],
    );
    let has_sym = load("ui-symbol", &["seguisym.ttf"]);

    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    if has_ui {
        prop.insert(0, "ui".into());
    }
    if has_sym {
        prop.push("ui-symbol".into());
    }
    let base_prop = prop.clone();

    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    if has_mono {
        mono.insert(0, "ui-mono".into());
    }
    let base_mono = mono.clone();

    let mut sb = base_prop.clone();
    if has_sb {
        sb.insert(0, "ui-semibold".into());
    }
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), sb);
    fonts
        .families
        .insert(FontFamily::Name(MONO.into()), base_mono);
    ctx.set_fonts(fonts);
}

pub fn install_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, body(11.5)),
            (TextStyle::Body, body(14.0)),
            (TextStyle::Button, body(13.0)),
            (TextStyle::Heading, semibold(20.0)),
            (TextStyle::Monospace, mono(12.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating = true;
        style.interaction.selectable_labels = true;

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = None;
        v.panel_fill = BG;
        v.window_fill = RAISED;
        v.window_stroke = Stroke::new(1.0, BORDER_STRONG);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_shadow = egui::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(140),
        };
        v.popup_shadow = v.window_shadow;
        v.extreme_bg_color = CODE_BG;
        v.faint_bg_color = PANEL;
        v.code_bg_color = Color32::from_rgb(0x2a, 0x2a, 0x2a);
        v.hyperlink_color = Color32::from_rgb(0x8a, 0xb4, 0xf8);
        v.selection.bg_fill = Color32::from_rgb(0x2f, 0x4a, 0x7a);
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.text_cursor.stroke = Stroke::new(1.6, TEXT);

        let r = CornerRadius::same(8);
        v.widgets.noninteractive.bg_fill = BG;
        v.widgets.noninteractive.weak_bg_fill = BG;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.noninteractive.corner_radius = r;

        v.widgets.inactive.bg_fill = RAISED;
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_2);
        v.widgets.inactive.corner_radius = r;

        v.widgets.hovered.bg_fill = HOVER;
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.corner_radius = r;
        v.widgets.hovered.expansion = 0.0;

        v.widgets.active.bg_fill = ACTIVE;
        v.widgets.active.weak_bg_fill = ACTIVE;
        v.widgets.active.bg_stroke = Stroke::NONE;
        v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.active.corner_radius = r;
        v.widgets.active.expansion = 0.0;

        v.widgets.open.bg_fill = ACTIVE;
        v.widgets.open.weak_bg_fill = ACTIVE;
        v.widgets.open.bg_stroke = Stroke::NONE;
        v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.open.corner_radius = r;
    });
}
