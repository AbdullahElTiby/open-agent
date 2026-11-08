//! Tiny line icons drawn with the painter — crisp at any DPI, no icon font needed.

use eframe::egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum Icon {
    Plus,
    Folder,
    ArrowUp,
    Stop,
    Close,
    Minimize,
    Maximize,
    Sidebar,
    Panel,
    ChevronRight,
    ChevronDown,
    Terminal,
    File,
    Pencil,
    Search,
    Globe,
    Check,
    Warn,
    Brain,
    Bot,
    Trash,
    Copy,
    Dot,
    Import,
}

pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 16.0; // design grid: 16×16
    let st = Stroke::new(1.35 * s.max(0.9), color);
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * s, c.y + (y - 8.0) * s);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(
            pts.iter().map(|&(x, y)| at(x, y)).collect(),
            st,
        ));
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        p.rect_stroke(
            Rect::from_min_max(at(x0, y0), at(x1, y1)),
            r * s,
            st,
            eframe::egui::StrokeKind::Middle,
        );
    };
    match icon {
        Icon::Plus => {
            line(&[(8.0, 3.0), (8.0, 13.0)]);
            line(&[(3.0, 8.0), (13.0, 8.0)]);
        }
        Icon::Folder => {
            line(&[
                (2.0, 4.5),
                (6.0, 4.5),
                (7.5, 6.0),
                (14.0, 6.0),
                (14.0, 12.5),
                (2.0, 12.5),
                (2.0, 4.5),
            ]);
        }
        Icon::ArrowUp => {
            line(&[(8.0, 13.0), (8.0, 3.5)]);
            line(&[(4.0, 7.5), (8.0, 3.5), (12.0, 7.5)]);
        }
        Icon::Stop => {
            p.rect_filled(
                Rect::from_center_size(c, vec2(8.0 * s, 8.0 * s)),
                1.5 * s,
                color,
            );
        }
        Icon::Close => {
            line(&[(4.0, 4.0), (12.0, 12.0)]);
            line(&[(12.0, 4.0), (4.0, 12.0)]);
        }
        Icon::Minimize => line(&[(3.5, 8.0), (12.5, 8.0)]),
        Icon::Maximize => rrect(4.0, 4.0, 12.0, 12.0, 1.0),
        Icon::Sidebar => {
            rrect(2.0, 3.0, 14.0, 13.0, 2.0);
            line(&[(6.0, 3.0), (6.0, 13.0)]);
        }
        Icon::Panel => {
            rrect(2.0, 3.0, 14.0, 13.0, 2.0);
            line(&[(10.0, 3.0), (10.0, 13.0)]);
        }
        Icon::ChevronRight => line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)]),
        Icon::ChevronDown => line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)]),
        Icon::Terminal => {
            line(&[(3.0, 5.0), (6.5, 8.0), (3.0, 11.0)]);
            line(&[(8.0, 11.5), (13.0, 11.5)]);
        }
        Icon::File => {
            line(&[
                (4.0, 2.5),
                (9.5, 2.5),
                (12.5, 5.5),
                (12.5, 13.5),
                (4.0, 13.5),
                (4.0, 2.5),
            ]);
            line(&[(9.5, 2.5), (9.5, 5.5), (12.5, 5.5)]);
        }
        Icon::Pencil => {
            line(&[
                (3.0, 13.0),
                (3.5, 10.5),
                (10.5, 3.5),
                (12.5, 5.5),
                (5.5, 12.5),
                (3.0, 13.0),
            ]);
        }
        Icon::Search => {
            p.circle_stroke(at(7.0, 7.0), 4.0 * s, st);
            line(&[(10.0, 10.0), (13.5, 13.5)]);
        }
        Icon::Globe => {
            p.circle_stroke(c, 5.5 * s, st);
            line(&[(2.5, 8.0), (13.5, 8.0)]);
            p.add(Shape::ellipse_stroke(c, vec2(2.4 * s, 5.5 * s), st));
        }
        Icon::Check => line(&[(3.5, 8.5), (6.5, 11.5), (12.5, 4.5)]),
        Icon::Warn => {
            line(&[(8.0, 2.5), (14.0, 13.0), (2.0, 13.0), (8.0, 2.5)]);
            line(&[(8.0, 6.5), (8.0, 9.5)]);
            p.circle_filled(at(8.0, 11.3), 0.8 * s, color);
        }
        Icon::Brain => {
            p.circle_stroke(c, 5.0 * s, st);
            line(&[(8.0, 3.0), (8.0, 13.0)]);
            line(&[(5.0, 7.0), (8.0, 8.5)]);
            line(&[(11.0, 9.0), (8.0, 10.5)]);
        }
        Icon::Bot => {
            rrect(3.0, 5.0, 13.0, 13.0, 2.5);
            line(&[(8.0, 5.0), (8.0, 2.5)]);
            p.circle_filled(at(6.0, 9.0), 0.9 * s, color);
            p.circle_filled(at(10.0, 9.0), 0.9 * s, color);
        }
        Icon::Trash => {
            line(&[(3.0, 4.5), (13.0, 4.5)]);
            line(&[(6.5, 4.5), (6.5, 2.8), (9.5, 2.8), (9.5, 4.5)]);
            line(&[(4.5, 4.5), (5.2, 13.5), (10.8, 13.5), (11.5, 4.5)]);
        }
        Icon::Copy => {
            rrect(5.5, 5.5, 13.5, 13.5, 1.5);
            line(&[
                (3.0, 10.5),
                (2.5, 10.5),
                (2.5, 2.5),
                (10.5, 2.5),
                (10.5, 3.0),
            ]);
        }
        Icon::Dot => {
            p.circle_filled(c, 3.0 * s, color);
        }
        Icon::Import => {
            line(&[(8.0, 2.5), (8.0, 10.0)]);
            line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)]);
            line(&[(2.5, 10.5), (2.5, 13.5), (13.5, 13.5), (13.5, 10.5)]);
        }
    }
}

/// Points along an arc — used for the run spinner.
pub fn arc(center: Pos2, r: f32, start: f32, sweep: f32) -> Vec<Pos2> {
    let n = 20;
    (0..=n)
        .map(|i| {
            let a = start + sweep * i as f32 / n as f32;
            pos2(center.x + r * a.cos(), center.y + r * a.sin())
        })
        .collect()
}
