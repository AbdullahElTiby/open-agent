//! A small, themed Markdown renderer for agent replies (pulldown-cmark → egui).

use crate::icons::{self, Icon};
use crate::theme::{self, *};
use eframe::egui::{
    self, text::LayoutJob, Align, Color32, CornerRadius, FontId, Frame, Id, Layout, Margin,
    RichText, Sense, Stroke, TextFormat, Ui,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

struct Style {
    strong: u32,
    em: u32,
    strike: u32,
    link: u32,
    heading: Option<HeadingLevel>,
}

pub fn render(ui: &mut Ui, text: &str, id: Id) {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut st = Style {
        strong: 0,
        em: 0,
        strike: 0,
        link: 0,
        heading: None,
    };
    let mut job = LayoutJob::default();
    let mut lists: Vec<Option<u64>> = vec![];
    let mut quote = 0usize;
    let mut code: Option<(String, String)> = None;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut cell = String::new();
    let mut blocks = 0usize;
    let mut first_block = true;

    let indent = |lists: &Vec<Option<u64>>, quote: usize| {
        lists.len().saturating_sub(1) as f32 * 18.0 + quote as f32 * 14.0
    };

    let fmt = |st: &Style| -> TextFormat {
        let (font, color) = match st.heading {
            Some(HeadingLevel::H1) => (theme::semibold(20.0), TEXT),
            Some(HeadingLevel::H2) => (theme::semibold(17.0), TEXT),
            Some(_) => (theme::semibold(15.0), TEXT),
            None if st.strong > 0 => (theme::semibold(14.0), TEXT),
            None => (theme::body(14.0), Color32::from_rgb(0xdc, 0xdc, 0xdc)),
        };
        let mut f = TextFormat {
            font_id: font,
            color,
            line_height: Some(22.0),
            ..Default::default()
        };
        if st.em > 0 {
            f.italics = true;
        }
        if st.strike > 0 {
            f.strikethrough = Stroke::new(1.0, TEXT_2);
        }
        if st.link > 0 {
            f.color = Color32::from_rgb(0x8a, 0xb4, 0xf8);
            f.underline = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x6a, 0xa0));
        }
        f
    };

    let flush = |ui: &mut Ui,
                 job: &mut LayoutJob,
                 lists: &Vec<Option<u64>>,
                 quote: usize,
                 first: &mut bool| {
        if job.text.trim().is_empty() {
            *job = LayoutJob::default();
            return;
        }
        let mut j = std::mem::take(job);
        if !*first {
            ui.add_space(4.0);
        }
        *first = false;
        let ind = indent(lists, quote);
        ui.horizontal(|ui| {
            ui.add_space(ind);
            if quote > 0 {
                let (r, _) = ui.allocate_exact_size(egui::vec2(3.0, 20.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, BORDER_STRONG);
                ui.add_space(8.0);
            }
            j.wrap.max_width = ui.available_width();
            ui.add(egui::Label::new(j).wrap());
        });
    };

    for ev in Parser::new_ext(text, opts) {
        if let Some((_, buf)) = code.as_mut() {
            match ev {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, buf) = code.take().unwrap();
                    if !first_block {
                        ui.add_space(6.0);
                    }
                    first_block = false;
                    code_block(
                        ui,
                        &lang,
                        buf.trim_end_matches('\n'),
                        id.with(("code", blocks)),
                    );
                    blocks += 1;
                }
                _ => {}
            }
            continue;
        }
        if let Some(rows) = table.as_mut() {
            match ev {
                Event::Text(t) | Event::Code(t) => cell.push_str(&t),
                Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => rows.push(vec![]),
                Event::End(TagEnd::TableCell) => {
                    if let Some(r) = rows.last_mut() {
                        r.push(std::mem::take(&mut cell));
                    }
                }
                Event::End(TagEnd::Table) => {
                    let rows = table.take().unwrap();
                    ui.add_space(6.0);
                    render_table(ui, &rows, id.with(("table", blocks)));
                    blocks += 1;
                    first_block = false;
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    if !first_block {
                        ui.add_space(8.0);
                    }
                    st.heading = Some(level);
                }
                Tag::Paragraph => {}
                Tag::BlockQuote(_) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    quote += 1;
                }
                Tag::CodeBlock(kind) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => {
                            l.split_whitespace().next().unwrap_or("").to_string()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    lists.push(start);
                }
                Tag::Item => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    let marker = match lists.last_mut() {
                        Some(Some(n)) => {
                            let m = format!("{n}.  ");
                            *n += 1;
                            m
                        }
                        _ => "•  ".into(),
                    };
                    let mut f = fmt(&st);
                    f.color = TEXT_3;
                    job.append(&marker, 0.0, f);
                }
                Tag::Emphasis => st.em += 1,
                Tag::Strong => st.strong += 1,
                Tag::Strikethrough => st.strike += 1,
                Tag::Link { .. } => st.link += 1,
                Tag::Table(_) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    table = Some(vec![]);
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    st.heading = None;
                }
                TagEnd::Paragraph | TagEnd::Item => {
                    flush(ui, &mut job, &lists, quote, &mut first_block)
                }
                TagEnd::BlockQuote(_) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    quote = quote.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    flush(ui, &mut job, &lists, quote, &mut first_block);
                    lists.pop();
                }
                TagEnd::Emphasis => st.em = st.em.saturating_sub(1),
                TagEnd::Strong => st.strong = st.strong.saturating_sub(1),
                TagEnd::Strikethrough => st.strike = st.strike.saturating_sub(1),
                TagEnd::Link => st.link = st.link.saturating_sub(1),
                _ => {}
            },
            Event::Text(t) => job.append(&t, 0.0, fmt(&st)),
            Event::Code(t) => {
                let f = TextFormat {
                    font_id: theme::mono(12.5),
                    color: Color32::from_rgb(0xe6, 0xe6, 0xe6),
                    background: Color32::from_rgb(0x2b, 0x2b, 0x2b),
                    expand_bg: 2.0,
                    line_height: Some(22.0),
                    ..Default::default()
                };
                job.append(&format!("\u{2009}{t}\u{2009}"), 0.0, f);
            }
            Event::SoftBreak => job.append(" ", 0.0, fmt(&st)),
            Event::HardBreak => job.append("\n", 0.0, fmt(&st)),
            Event::TaskListMarker(done) => {
                let mut f = fmt(&st);
                f.color = if done { GREEN } else { TEXT_3 };
                job.append(if done { "☑ " } else { "☐ " }, 0.0, f);
            }
            Event::Rule => {
                flush(ui, &mut job, &lists, quote, &mut first_block);
                ui.add_space(6.0);
                ui.separator();
            }
            Event::Html(t) | Event::InlineHtml(t) => job.append(&t, 0.0, fmt(&st)),
            _ => {}
        }
    }
    flush(ui, &mut job, &lists, quote, &mut first_block);
}

fn render_table(ui: &mut Ui, rows: &[Vec<String>], id: Id) {
    Frame::new()
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            egui::Grid::new(id)
                .spacing([18.0, 6.0])
                .striped(false)
                .show(ui, |ui| {
                    for (i, row) in rows.iter().enumerate() {
                        for c in row {
                            let t = RichText::new(c).size(13.0);
                            ui.label(if i == 0 {
                                t.font(theme::semibold(13.0)).color(TEXT)
                            } else {
                                t.color(TEXT_2)
                            });
                        }
                        ui.end_row();
                    }
                });
        });
}

pub fn code_block(ui: &mut Ui, lang: &str, code: &str, id: Id) {
    Frame::new()
        .fill(CODE_BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // header
            Frame::new()
                .inner_margin(Margin {
                    left: 12,
                    right: 6,
                    top: 5,
                    bottom: 5,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if lang.is_empty() { "text" } else { lang })
                                .font(theme::mono(11.0))
                                .color(TEXT_3),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let copied_id = id.with("copied");
                            let copied = ui
                                .ctx()
                                .data(|d| d.get_temp::<f64>(copied_id))
                                .is_some_and(|t| ui.input(|i| i.time) - t < 1.5);
                            if copy_button(ui, copied).clicked() {
                                ui.ctx().copy_text(code.to_string());
                                let now = ui.input(|i| i.time);
                                ui.ctx().data_mut(|d| d.insert_temp(copied_id, now));
                                ui.ctx()
                                    .request_repaint_after(std::time::Duration::from_millis(1600));
                            }
                        });
                    });
                });
            let r = ui.min_rect();
            ui.painter()
                .hline(r.x_range(), r.bottom(), Stroke::new(1.0, BORDER));
            egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        let mut job = highlight(code, lang);
                        job.wrap.max_width = f32::INFINITY;
                        ui.add(egui::Label::new(job).extend());
                    });
            });
        });
}

fn copy_button(ui: &mut Ui, copied: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(if copied { 62.0 } else { 52.0 }, 20.0),
        Sense::click(),
    );
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 5.0, HOVER);
    }
    let col = if hovered { TEXT } else { TEXT_3 };
    let ir = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 11.0, rect.center().y),
        egui::vec2(12.0, 12.0),
    );
    icons::paint(
        ui.painter(),
        ir,
        if copied { Icon::Check } else { Icon::Copy },
        col,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 21.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        if copied { "Copied" } else { "Copy" },
        theme::body(11.5),
        col,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ── tiny syntax highlighter: comments, strings, numbers, keywords ──

const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "pub",
    "use",
    "mod",
    "struct",
    "enum",
    "impl",
    "trait",
    "match",
    "if",
    "else",
    "for",
    "while",
    "loop",
    "return",
    "break",
    "continue",
    "in",
    "as",
    "const",
    "static",
    "async",
    "await",
    "move",
    "self",
    "Self",
    "where",
    "type",
    "dyn",
    "ref",
    "true",
    "false",
    "null",
    "None",
    "Some",
    "Ok",
    "Err",
    "function",
    "var",
    "class",
    "export",
    "import",
    "from",
    "default",
    "new",
    "this",
    "interface",
    "extends",
    "implements",
    "def",
    "lambda",
    "with",
    "yield",
    "try",
    "catch",
    "except",
    "finally",
    "throw",
    "raise",
    "package",
    "func",
    "go",
    "defer",
    "select",
    "case",
    "switch",
    "do",
    "public",
    "private",
    "protected",
    "void",
    "int",
    "bool",
    "string",
    "echo",
    "then",
    "fi",
    "elif",
    "and",
    "or",
    "not",
    "is",
    "pass",
    "nil",
    "local",
    "end",
    "unsafe",
    "crate",
    "super",
];

fn highlight(code: &str, lang: &str) -> LayoutJob {
    let base = TextFormat {
        font_id: theme::mono(12.5),
        color: Color32::from_rgb(0xd8, 0xd8, 0xd8),
        line_height: Some(19.0),
        ..Default::default()
    };
    let with = |c: Color32| TextFormat {
        color: c,
        ..base.clone()
    };
    let kw = with(Color32::from_rgb(0xc5, 0x95, 0xf0));
    let string = with(Color32::from_rgb(0x9c, 0xd6, 0x8f));
    let num = with(Color32::from_rgb(0xf0, 0xb0, 0x70));
    let comment = TextFormat {
        italics: true,
        ..with(Color32::from_rgb(0x6a, 0x6a, 0x6a))
    };
    let func = with(Color32::from_rgb(0x7c, 0xc4, 0xf2));
    let hash_comments = matches!(
        lang,
        "py" | "python"
            | "sh"
            | "bash"
            | "shell"
            | "zsh"
            | "toml"
            | "yaml"
            | "yml"
            | "rb"
            | "ruby"
            | "ps1"
            | "powershell"
            | "r"
    );
    let diff = lang == "diff";

    let mut job = LayoutJob::default();
    for (li, line) in code.split('\n').enumerate() {
        if li > 0 {
            job.append("\n", 0.0, base.clone());
        }
        if diff {
            let f = if line.starts_with('+') {
                with(GREEN)
            } else if line.starts_with('-') {
                with(RED)
            } else if line.starts_with("@@") {
                with(TEXT_3)
            } else {
                base.clone()
            };
            job.append(line, 0.0, f);
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let rest: String = chars[i..].iter().collect();
            if rest.starts_with("//")
                || (hash_comments && c == '#')
                || rest.starts_with("--") && matches!(lang, "sql" | "lua")
            {
                job.append(&rest, 0.0, comment.clone());
                break;
            }
            if c == '"' || c == '\'' || c == '`' {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != c {
                    if chars[j] == '\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let end = (j + 1).min(chars.len());
                job.append(
                    &chars[i..end].iter().collect::<String>(),
                    0.0,
                    string.clone(),
                );
                i = end;
                continue;
            }
            if c.is_ascii_digit() {
                let mut j = i;
                while j < chars.len()
                    && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_')
                {
                    j += 1;
                }
                job.append(&chars[i..j].iter().collect::<String>(), 0.0, num.clone());
                i = j;
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let mut j = i;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                let f = if KEYWORDS.contains(&word.as_str()) {
                    kw.clone()
                } else if chars.get(j) == Some(&'(') {
                    func.clone()
                } else {
                    base.clone()
                };
                job.append(&word, 0.0, f);
                i = j;
                continue;
            }
            let mut j = i;
            while j < chars.len()
                && !(chars[j].is_alphanumeric()
                    || matches!(chars[j], '"' | '\'' | '`' | '_' | '/' | '#' | '-'))
            {
                j += 1;
            }
            let j = j.max(i + 1);
            job.append(&chars[i..j].iter().collect::<String>(), 0.0, base.clone());
            i = j;
        }
    }
    job
}

#[allow(dead_code)]
pub fn font(size: f32) -> FontId {
    theme::body(size)
}
