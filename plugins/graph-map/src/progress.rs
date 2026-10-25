//! The picture shown while the layout is built in steps: a title, a bar and
//! a percentage, so a big library never looks like a hang.

use crate::host::{translate, width};
use crate::view::Px;
use sigma_plugin_api::{Face, Op, Rect, Scene, Tone};

/// Ops for the screen at `done` of `total` iterations.
pub fn ops(scene: &Scene, done: usize, total: usize) -> Vec<Op> {
    let s = Px(scene.scale);
    let ui = scene.ui_px;
    let (w, h) = (scene.width, scene.height);
    let mut out = vec![Op::Fill {
        r: Rect::new(0, 0, w, h),
        c: Tone::Paper,
    }];
    let centred = |text: &str, face: Face, size: f32| (w as f32 - width(face, size, text)) / 2.0;
    let y = scene.top as f32 + (h - scene.top) as f32 * 0.4;
    let title = translate("Building the graph\u{2026}");
    out.push(Op::Text {
        face: Face::Bold,
        size: ui * 1.3,
        x: centred(&title, Face::Bold, ui * 1.3),
        y,
        s: title,
        c: Tone::Ink,
    });
    let bar_w = (w - 2 * s.px(120.0)).clamp(s.px(200.0), s.px(640.0));
    let bar_h = s.px(28.0);
    let bar = Rect::new((w - bar_w) / 2, y as i32 + s.px(36.0), bar_w, bar_h);
    let th = s.px(2.0).max(2);
    out.push(Op::Stroke {
        r: bar,
        t: th,
        c: Tone::Ink,
    });
    let frac = if total == 0 {
        1.0
    } else {
        (done as f64 / total as f64).clamp(0.0, 1.0)
    };
    let inner = Rect::new(
        bar.x + th + s.px(3.0),
        bar.y + th + s.px(3.0),
        bar.w - 2 * (th + s.px(3.0)),
        bar.h - 2 * (th + s.px(3.0)),
    );
    let filled = (f64::from(inner.w) * frac).round() as i32;
    if filled > 0 {
        out.push(Op::Fill {
            r: Rect::new(inner.x, inner.y, filled, inner.h),
            c: Tone::Ink,
        });
    }
    let pct = format!("{} %", (frac * 100.0).round() as i32);
    out.push(Op::Text {
        face: Face::Regular,
        size: ui,
        x: centred(&pct, Face::Regular, ui),
        y: (bar.y + bar.h + s.px(44.0)) as f32,
        s: pct,
        c: Tone::Mute,
    });
    out
}
