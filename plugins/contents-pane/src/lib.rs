//! The Contents pane: the headings of the open note, one row each. Tapping
//! a row asks the app to jump there.

use sigma_plugin_sdk::api::{Action, Event, Face, Frame, HeadingRow, Op, Rect, Redraw, Tone};
use sigma_plugin_sdk::{export, fit_text, hit, Pane};
use std::cell::RefCell;

const EMPTY: &str = "No headings in this note. Start a line with #.";

/// The pane as last laid out: where each heading's row is.
struct Contents {
    ops: Vec<Op>,
    hits: Vec<sigma_plugin_sdk::api::Hit>,
    /// Row rectangle and index into the scene's headings.
    rows: Vec<(Rect, u32)>,
}

thread_local! {
    static STATE: RefCell<Option<Contents>> = const { RefCell::new(None) };
}

fn layout(pane: &Pane, headings: &[HeadingRow]) -> Contents {
    let mut ops = Vec::new();
    let mut hits = Vec::new();
    let mut rows = Vec::new();
    pane.header(&mut ops, "Contents");
    // 44 px rows: the touch-target minimum.
    let row_h = pane.d(44.0);
    let fit = (pane.body.h / row_h).max(0) as usize;
    for (k, h) in headings.iter().take(fit).enumerate() {
        let r = Rect::new(
            pane.body.x,
            pane.body.y + k as i32 * row_h,
            pane.body.w,
            row_h,
        );
        rows.push((r, k as u32));
        hits.push(hit(k as u32, r, format!("Heading:{}", h.text)));
    }
    if headings.is_empty() {
        pane.hint(&mut ops, EMPTY);
    }
    for ((r, _), h) in rows.iter().zip(headings) {
        let indent = pane.d(14.0) * (i32::from(h.level) - 1).min(3);
        let face = if h.level == 1 {
            Face::Bold
        } else {
            Face::Regular
        };
        ops.push(Op::Text {
            face,
            size: pane.ui,
            x: (r.x + indent) as f32,
            y: Pane::baseline(*r, face, pane.ui),
            s: fit_text(&h.text, face, pane.ui, (r.w - indent) as f32),
            c: Tone::Ink,
        });
    }
    if headings.len() > fit {
        pane.more(&mut ops, headings.len() - fit);
    }
    Contents { ops, hits, rows }
}

fn frame(c: &Contents, actions: Vec<Action>) -> Frame {
    Frame {
        ops: c.ops.clone(),
        hits: c.hits.clone(),
        actions,
        redraw: Redraw::Partial,
        ..Frame::default()
    }
}

fn handle(ev: Event) -> Frame {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        match ev {
            Event::Open(scene) => {
                let c = layout(&Pane::of(&scene), &scene.headings);
                let f = frame(&c, Vec::new());
                *s = Some(c);
                f
            }
            Event::Tap { x, y } => match s.as_ref() {
                Some(c) => {
                    let row = c.rows.iter().find(|(r, _)| r.contains(x, y));
                    frame(
                        c,
                        row.map(|(_, i)| Action::GotoHeading(*i))
                            .into_iter()
                            .collect(),
                    )
                }
                None => Frame::default(),
            },
            _ => s
                .as_ref()
                .map_or_else(Frame::default, |c| frame(c, Vec::new())),
        }
    })
}

export!(handle);

#[cfg(test)]
mod tests;
