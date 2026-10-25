use super::*;
use sigma_plugin_api::{Note, Op};

fn scene(n: usize, links: &[(u32, u32)]) -> Scene {
    Scene {
        width: 1440,
        height: 1920,
        scale: 1440.0 / 1264.0,
        ui_px: 17.0 * 1440.0 / 1264.0,
        top: 66,
        bottom: 1920,
        notes: (0..n)
            .map(|i| Note {
                name: if i == 0 {
                    "a.md".into()
                } else {
                    format!("n{i:02}.md")
                },
                current: i == 0,
                detail: "No links".into(),
            })
            .collect(),
        edges: links.to_vec(),
        links: links.len() as u32,
    }
}

fn open(n: usize, links: &[(u32, u32)]) -> Frame {
    handle(Event::Open(scene(n, links)))
}

fn centre(f: &Frame, label: &str) -> (i32, i32) {
    let h = f.hits.iter().find(|h| h.label == label).unwrap_or_else(|| {
        panic!(
            "no hit {label}: {:?}",
            f.hits.iter().map(|h| &h.label).collect::<Vec<_>>()
        )
    });
    (h.rect.x + h.rect.w / 2, h.rect.y + h.rect.h / 2)
}

fn tap(f: &Frame, label: &str) -> Frame {
    let (x, y) = centre(f, label);
    handle(Event::Tap { x, y })
}

const LINKS: [(u32, u32); 3] = [(0, 1), (1, 2), (2, 0)];

#[test]
fn opens_on_the_map_with_controls_and_nodes() {
    let f = open(5, &LINKS);
    assert_eq!(f.redraw, Redraw::Full);
    for l in [
        "Map",
        "List",
        "Zoom+",
        "Zoom-",
        "Node:a.md",
        "Node:n01.md",
        "Node:n04.md",
    ] {
        centre(&f, l);
    }
    assert!(
        !f.hits.iter().any(|h| h.label == "Open"),
        "no Open without a selection"
    );
    assert!(!f.ops.is_empty());
}

#[test]
fn select_then_open_in_preview() {
    let f = open(5, &LINKS);
    let f = tap(&f, "Node:n01.md");
    assert_eq!(f.redraw, Redraw::Clean);
    centre(&f, "Open");
    assert!(f
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s.contains("tap again to open"))));
    let g = tap(&f, "Node:n01.md");
    assert_eq!(g.actions, vec![Action::PreviewNote(1)]);
    // The Open button does the same for the selection.
    let f = tap(&f, "Open");
    assert_eq!(f.actions, vec![Action::PreviewNote(1)]);
}

#[test]
fn tapping_empty_space_clears_the_selection() {
    let f = open(5, &LINKS);
    let f = tap(&f, "Node:n01.md");
    let g = handle(Event::Tap { x: 700, y: 1500 });
    assert_eq!(g.redraw, Redraw::Clean);
    assert!(!g.hits.iter().any(|h| h.label == "Open"));
    // Nothing selected, nothing to clear.
    assert_eq!(handle(Event::Tap { x: 700, y: 1500 }).redraw, Redraw::None);
    let _ = f;
}

#[test]
fn zoom_steps_and_limits() {
    let f = open(5, &LINKS);
    assert_eq!(tap(&f, "Zoom-").redraw, Redraw::None, "already at x1");
    let mut f = f;
    for _ in 0..3 {
        f = tap(&f, "Zoom+");
        assert_eq!(f.redraw, Redraw::Full);
    }
    assert_eq!(tap(&f, "Zoom+").redraw, Redraw::None, "at the top zoom");
    assert_eq!(
        handle(Event::Wheel { x: 0, y: 0, dy: -1 }).redraw,
        Redraw::Full
    );
}

#[test]
fn toggle_to_the_list_and_back() {
    let f = open(5, &LINKS);
    let f = tap(&f, "List");
    assert_eq!(f.redraw, Redraw::Full);
    assert!(
        !f.hits.iter().any(|h| h.label == "Zoom+"),
        "no zoom in the list"
    );
    centre(&f, "Graph:n03.md");
    assert!(f
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s == "3 links")));
    assert_eq!(tap(&f, "List").redraw, Redraw::None, "already the list");
    let g = tap(&f, "Graph:n03.md");
    assert_eq!(g.actions, vec![Action::OpenNote(3)]);
    let f = tap(&f, "Map");
    assert_eq!(f.redraw, Redraw::Full);
    centre(&f, "Zoom+");
}

#[test]
fn the_list_pages_and_the_wheel_pages_it() {
    let f = open(60, &[]);
    let f = tap(&f, "List");
    assert!(f
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s.contains("page 1 of"))));
    assert_eq!(
        handle(Event::PageTurn { forward: false }).redraw,
        Redraw::None,
        "first page"
    );
    let g = handle(Event::PageTurn { forward: true });
    assert_eq!(g.redraw, Redraw::Partial);
    assert!(g
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s.contains("page 2 of"))));
    // Wheel down is back a page (we are on page 2), then the first page
    // has nowhere further back; wheel up pages forward again.
    assert_eq!(
        handle(Event::Wheel { x: 0, y: 0, dy: 1 }).redraw,
        Redraw::Partial
    );
    assert_eq!(
        handle(Event::Wheel { x: 0, y: 0, dy: 1 }).redraw,
        Redraw::None
    );
    assert_eq!(
        handle(Event::Wheel { x: 0, y: 0, dy: -1 }).redraw,
        Redraw::Partial
    );
}

#[test]
fn drag_pans_the_map_and_never_selects() {
    let f = open(8, &LINKS);
    let a = handle(Event::Drag {
        phase: Phase::Start,
        x: 700,
        y: 1000,
    });
    assert_eq!(a.redraw, Redraw::None);
    let m = handle(Event::Drag {
        phase: Phase::Move,
        x: 650,
        y: 1000,
    });
    assert_eq!(m.redraw, Redraw::Partial);
    let e = handle(Event::Drag {
        phase: Phase::End,
        x: 600,
        y: 1000,
    });
    assert_eq!(e.redraw, Redraw::Partial);
    assert!(e.actions.is_empty());
    // The picture moved: some node box is not where it was.
    let before = f
        .hits
        .iter()
        .find(|h| h.label == "Node:n01.md")
        .unwrap()
        .rect;
    let after = e
        .hits
        .iter()
        .find(|h| h.label == "Node:n01.md")
        .unwrap()
        .rect;
    assert!(after.x < before.x, "{before:?} → {after:?}");
    // A drag that did not start on the map does nothing.
    handle(Event::Drag {
        phase: Phase::Start,
        x: 700,
        y: 20,
    });
    assert_eq!(
        handle(Event::Drag {
            phase: Phase::End,
            x: 600,
            y: 20
        })
        .redraw,
        Redraw::None
    );
}

#[test]
fn page_buttons_pan_the_map_by_half_a_view() {
    open(8, &LINKS);
    assert_eq!(
        handle(Event::PageTurn { forward: true }).redraw,
        Redraw::Partial
    );
    assert_eq!(
        handle(Event::PageTurn { forward: false }).redraw,
        Redraw::Partial
    );
}

#[test]
fn back_closes_and_a_frame_never_arrives_before_open() {
    let f = open(3, &LINKS);
    assert_eq!(handle(Event::Back).actions, vec![Action::Close]);
    let _ = f;
    APP.with(|a| *a.borrow_mut() = None);
    assert_eq!(handle(Event::Tap { x: 1, y: 1 }), Frame::default());
}

#[test]
fn the_abi_version_matches_the_api() {
    // The entry points that move bytes through 32-bit pointers run under
    // the wasm host (the app's plugin tests), not on a 64-bit machine.
    assert_eq!(sigma_abi(), sigma_plugin_api::ABI);
}

#[test]
fn garbage_input_is_an_empty_frame_not_a_crash() {
    let f = match Event::decode(&[9, 9]) {
        Ok(ev) => handle(ev),
        Err(_) => Frame::default(),
    };
    assert_eq!(f, Frame::default());
}
