//! The Scratch pane: a title, a rule and a text field. The app owns the
//! text (keyboard, handwriting, saving); this plugin only places it.

use sigma_plugin_sdk::api::{Event, Field, Frame, Op, Redraw};
use sigma_plugin_sdk::{export, rect, Pane};

/// Shown while the field is empty.
const PLACEHOLDER: &str = "A side note. It stays off the page.";

fn handle(ev: Event) -> Frame {
    let Event::Open(scene) = ev else {
        return Frame::default();
    };
    let pane = Pane::of(&scene);
    let mut ops: Vec<Op> = Vec::new();
    pane.header(&mut ops, "Scratch");
    Frame {
        ops,
        field: Some(Field {
            rect: rect(pane.body.x, pane.body.y, pane.body.w, pane.body.h),
            placeholder: PLACEHOLDER.to_owned(),
        }),
        redraw: Redraw::Partial,
        ..Frame::default()
    }
}

export!(handle);

#[cfg(test)]
mod tests;
