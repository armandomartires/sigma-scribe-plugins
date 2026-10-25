//! The To-dos pane: every to-do in the library with a check box, the note it
//! lives in and a filter (All, Open, Done). The check box toggles it; the
//! text opens the note at its line. The filter is this plugin's own state.

use sigma_core::outline::Filter;
use sigma_plugin_sdk::api::{Action, Event, Face, Frame, Hit, Op, Rect, Redraw, Scene, Tone};
use sigma_plugin_sdk::{export, fit_text, hit, inset, translate, width, Pane};
use std::cell::{Cell, RefCell};

const EMPTY: &str = "No to-dos. Start a line with - [ ].";

/// The pane as last laid out.
struct Todos {
    ops: Vec<Op>,
    hits: Vec<Hit>,
    filter_rect: Rect,
    tap_slop: i32,
    /// Check box, text and index into the scene's to-dos, per shown row.
    rows: Vec<(Rect, Rect, u32)>,
}

thread_local! {
    static STATE: RefCell<Option<Todos>> = const { RefCell::new(None) };
    static SCENE: RefCell<Option<Scene>> = const { RefCell::new(None) };
    /// Kept while the pane is open and between opens, as the app's was.
    static FILTER: Cell<Filter> = const { Cell::new(Filter::All) };
}

fn paper(r: Rect) -> Op {
    Op::Fill { r, c: Tone::Paper }
}

fn layout(scene: &Scene, filter: Filter) -> Todos {
    let pane = Pane::of(scene);
    let mut ops = Vec::new();
    let mut hits = Vec::new();
    pane.header(&mut ops, "To-dos");
    let fw = pane.d(72.0);
    let f = Rect::new(
        pane.area.x + pane.area.w - pane.d(12.0) - fw,
        pane.area.y,
        fw,
        pane.header.h,
    );
    hits.push(hit(0, f, "Filter"));
    let row_h = pane.d(58.0);
    let fit = (pane.body.h / row_h).max(0) as usize;
    let shown: Vec<usize> = (0..scene.todos.len())
        .filter(|&i| filter.keeps(scene.todos[i].done))
        .collect();
    let mut rows = Vec::new();
    for (k, &i) in shown.iter().take(fit).enumerate() {
        let y = pane.body.y + k as i32 * row_h;
        let bx = pane.d(44.0);
        rows.push((
            Rect::new(pane.body.x, y, bx, row_h),
            Rect::new(pane.body.x + bx, y, pane.body.w - bx, row_h),
            i as u32,
        ));
    }
    // The filter chip.
    ops.push(Op::Stroke {
        r: f,
        t: 1,
        c: Tone::Ink,
    });
    let label = translate(filter.label());
    let w = width(Face::Bold, pane.small, &label);
    ops.push(Op::Text {
        face: Face::Bold,
        size: pane.small,
        x: f.x as f32 + (f.w as f32 - w) / 2.0,
        y: Pane::baseline(f, Face::Bold, pane.small),
        s: label,
        c: Tone::Ink,
    });
    if rows.is_empty() {
        pane.hint(&mut ops, EMPTY);
    }
    for (bx, tx, idx) in &rows {
        let it = &scene.todos[*idx as usize];
        let name = scene
            .notes
            .get(it.note as usize)
            .map_or("", |n| n.name.as_str());
        hits.push(hit(2 * idx + 1, *bx, format!("TodoBox:{}", it.text)));
        hits.push(hit(2 * idx + 2, *tx, format!("Todo:{}", it.text)));
        let side = pane.d(18.0);
        let b = Rect::new(bx.x + pane.d(4.0), tx.y + pane.d(10.0), side, side);
        if it.done {
            ops.push(Op::Fill { r: b, c: Tone::Ink });
            // Check mark in paper.
            for k in 0..side / 2 {
                ops.push(paper(Rect::new(
                    b.x + side / 5 + k / 2,
                    b.y + side / 2 + k / 2,
                    2,
                    2,
                )));
                ops.push(paper(Rect::new(
                    b.x + side / 2 - 2 + k,
                    b.y + side * 3 / 4 - k,
                    2,
                    2,
                )));
            }
        } else {
            ops.push(Op::Stroke {
                r: b,
                t: pane.d(1.75).max(2),
                c: Tone::Ink,
            });
        }
        let (tone, line) = if it.done {
            (Tone::Mute, true)
        } else {
            (Tone::Ink, false)
        };
        let y1 = tx.y as f32 + pane.d(26.0) as f32;
        let s = fit_text(&it.text, Face::Regular, pane.ui, tx.w as f32);
        let end = tx.x as f32 + width(Face::Regular, pane.ui, &s);
        ops.push(Op::Text {
            face: Face::Regular,
            size: pane.ui,
            x: tx.x as f32,
            y: y1,
            s,
            c: tone,
        });
        if line {
            ops.push(Op::Fill {
                r: Rect::new(
                    tx.x,
                    (y1 - pane.ui * 0.3) as i32,
                    (end - tx.x as f32) as i32,
                    1.max(pane.d(1.0)),
                ),
                c: Tone::Mute,
            });
        }
        ops.push(Op::Text {
            face: Face::Regular,
            size: pane.small,
            x: tx.x as f32,
            y: y1 + pane.small * 1.3,
            s: fit_text(name, Face::Regular, pane.small, tx.w as f32),
            c: Tone::Mute,
        });
    }
    let hidden = shown.len().saturating_sub(fit);
    if hidden > 0 {
        pane.more(&mut ops, hidden);
    }
    Todos {
        ops,
        hits,
        filter_rect: f,
        tap_slop: pane.d(4.0),
        rows,
    }
}

fn frame(t: &Todos, redraw: Redraw, actions: Vec<Action>) -> Frame {
    Frame {
        ops: t.ops.clone(),
        hits: t.hits.clone(),
        actions,
        redraw,
        ..Frame::default()
    }
}

fn handle(ev: Event) -> Frame {
    match ev {
        Event::Open(scene) => {
            let t = layout(&scene, FILTER.with(Cell::get));
            let f = frame(&t, Redraw::Partial, Vec::new());
            STATE.with(|s| *s.borrow_mut() = Some(t));
            SCENE.with(|s| *s.borrow_mut() = Some(scene));
            f
        }
        Event::Tap { x, y } => {
            let filter_hit = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .is_some_and(|t| inset(t.filter_rect, -t.tap_slop).contains(x, y))
            });
            if filter_hit {
                FILTER.with(|f| f.set(f.get().next()));
                let t = SCENE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|sc| layout(sc, FILTER.with(Cell::get)))
                });
                return match t {
                    Some(t) => {
                        let f = frame(&t, Redraw::Partial, Vec::new());
                        STATE.with(|s| *s.borrow_mut() = Some(t));
                        f
                    }
                    None => Frame::default(),
                };
            }
            STATE.with(|s| {
                let s = s.borrow();
                let Some(t) = s.as_ref() else {
                    return Frame::default();
                };
                let action = t.rows.iter().find_map(|(b, tx, i)| {
                    if b.contains(x, y) {
                        Some(Action::ToggleTodo(*i))
                    } else if tx.contains(x, y) {
                        Some(Action::GotoTodo(*i))
                    } else {
                        None
                    }
                });
                frame(t, Redraw::None, action.into_iter().collect())
            })
        }
        _ => STATE.with(|s| {
            s.borrow()
                .as_ref()
                .map_or_else(Frame::default, |t| frame(t, Redraw::None, Vec::new()))
        }),
    }
}

export!(handle);

#[cfg(test)]
mod tests;
