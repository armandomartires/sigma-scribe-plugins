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
        settings: Vec::new(),
        left: 0,
        todos: Vec::new(),
        headings: Vec::new(),
    }
}

/// Opens the screen and lets a first-time layout finish, as the host does by
/// sending `Step` when idle; returns the finished screen's frame.
fn open(n: usize, links: &[(u32, u32)]) -> Frame {
    let mut f = handle(Event::Open(scene(n, links)));
    while f.pending {
        f = handle(Event::Step);
    }
    f
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

// --- building in steps, and settings ---

fn big_scene(n: usize) -> Scene {
    let links: Vec<(u32, u32)> = (0..n as u32).map(|i| (i, (i * 7 + 3) % n as u32)).collect();
    scene(n, &links)
}

fn clear_cache() {
    crate::host::CACHE.with(|c| c.borrow_mut().clear());
}

/// The bar's fill: the widest ink rectangle in the progress picture.
fn bar_fill(f: &Frame) -> i32 {
    f.ops
        .iter()
        .filter_map(|o| match o {
            Op::Fill {
                r,
                c: sigma_plugin_api::Tone::Ink,
            } => Some(r.w),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn a_new_library_builds_in_steps_with_a_growing_bar() {
    clear_cache();
    let first = handle(Event::Open(big_scene(60)));
    assert!(first.pending, "the host must send Step");
    assert_eq!(first.redraw, Redraw::Full);
    assert!(first.hits.is_empty() && first.actions.is_empty());
    assert!(first
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s.contains("Building"))));
    assert!(first
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s == "0 %")));
    let mut fills = vec![bar_fill(&first)];
    let mut steps = 0;
    let done = loop {
        let f = handle(Event::Step);
        steps += 1;
        assert!(steps <= 40, "it must finish");
        if !f.pending {
            break f;
        }
        assert_eq!(f.redraw, Redraw::Partial, "bar updates are partial");
        fills.push(bar_fill(&f));
    };
    assert!(
        (8..=14).contains(&steps),
        "about {STEPS} steps, got {steps}"
    );
    assert!(
        fills.windows(2).all(|w| w[0] <= w[1]),
        "the bar never shrinks: {fills:?}"
    );
    assert!(fills.last().unwrap() > fills.first().unwrap());
    assert_eq!(
        done.redraw,
        Redraw::Full,
        "the finished screen repaints fully"
    );
    assert!(
        done.hits.iter().any(|h| h.label == "Zoom+"),
        "the real screen"
    );
    // Once built, a stray Step changes nothing and asks for nothing.
    let idle = handle(Event::Step);
    assert!(!idle.pending && idle.redraw == Redraw::None && idle.actions.is_empty());
}

#[test]
fn building_in_steps_gives_the_same_picture_as_at_once() {
    clear_cache();
    let scene = big_scene(60);
    handle(Event::Open(scene.clone()));
    let stepped = loop {
        let f = handle(Event::Step);
        if !f.pending {
            break f;
        }
    };
    // The cache hit path of a second open draws from the saved picture.
    let again = handle(Event::Open(scene));
    assert!(!again.pending, "now cached: no build");
    assert_eq!(stepped.ops, again.ops, "same layout, same picture");
}

#[test]
fn an_unchanged_library_opens_at_once_the_second_time() {
    clear_cache();
    let f = handle(Event::Open(big_scene(40)));
    assert!(f.pending);
    while handle(Event::Step).pending {}
    let f = handle(Event::Open(big_scene(40)));
    assert!(
        !f.pending && f.redraw == Redraw::Full,
        "straight to the screen"
    );
    assert!(f.hits.iter().any(|h| h.label == "Map"));
}

#[test]
fn events_while_building_keep_it_going_and_back_leaves() {
    clear_cache();
    handle(Event::Open(big_scene(60)));
    for ev in [
        Event::Tap { x: 700, y: 900 },
        Event::PageTurn { forward: true },
        Event::Wheel { x: 0, y: 0, dy: 1 },
        Event::Drag {
            phase: Phase::Start,
            x: 1,
            y: 700,
        },
    ] {
        let f = handle(ev.clone());
        assert!(f.pending, "{ev:?} must not stop the build");
        assert_eq!(f.redraw, Redraw::None);
        assert!(f.actions.is_empty());
    }
    let f = handle(Event::Back);
    assert_eq!(f.actions, vec![Action::Close]);
    // Leaving mid-build and opening again starts a fresh build.
    let f = handle(Event::Open(big_scene(60)));
    assert!(f.pending);
    assert!(f
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, .. } if s == "0 %")));
}

#[test]
fn a_small_change_builds_faster_from_the_last_picture() {
    clear_cache();
    handle(Event::Open(big_scene(80)));
    while handle(Event::Step).pending {}
    // One more note: a warm start has far fewer iterations than a cold one.
    let mut s = big_scene(81);
    s.notes[80].name = "zz-new.md".into();
    let f = handle(Event::Open(s));
    assert!(f.pending);
    let mut steps = 1;
    while handle(Event::Step).pending {
        steps += 1;
    }
    assert!(steps <= STEPS + 1);
}

fn with_settings(mut s: Scene, kv: &[(&str, &str)]) -> Scene {
    s.settings = kv
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    s
}

#[test]
fn start_on_list_opens_the_list() {
    clear_cache();
    let s = with_settings(big_scene(20), &[("view", "List")]);
    handle(Event::Open(s.clone()));
    let f = loop {
        let f = handle(Event::Step);
        if !f.pending {
            break f;
        }
    };
    assert!(
        f.hits.iter().any(|h| h.label.starts_with("Graph:")),
        "list rows"
    );
    assert!(!f.hits.iter().any(|h| h.label == "Zoom+"));
    // The default opens the map.
    clear_cache();
    handle(Event::Open(big_scene(20)));
    let f = loop {
        let f = handle(Event::Step);
        if !f.pending {
            break f;
        }
    };
    assert!(f.hits.iter().any(|h| h.label == "Zoom+"));
}

#[test]
fn labels_all_labels_every_node() {
    let count = |kv: &[(&str, &str)]| {
        clear_cache();
        handle(Event::Open(with_settings(big_scene(120), kv)));
        let f = loop {
            let f = handle(Event::Step);
            if !f.pending {
                break f;
            }
        };
        f.ops.iter().filter(|o| matches!(o, Op::Text { s, .. } if s.ends_with(".md") || s.starts_with('n') || s == "a")).count()
    };
    let auto = count(&[]);
    let all = count(&[("labels", "All")]);
    assert!(all > auto, "all {all} vs auto {auto}");
}

#[test]
fn quick_layout_takes_fewer_steps_worth_of_iterations_and_has_its_own_cache() {
    let total = |kv: &[(&str, &str)]| {
        clear_cache();
        let f = handle(Event::Open(with_settings(big_scene(100), kv)));
        assert!(f.pending);
        // The bar reads 0 % and ends at 100 %; count the steps it takes.
        let mut n = 1;
        while handle(Event::Step).pending {
            n += 1;
        }
        n
    };
    let best = total(&[]);
    let quick = total(&[("layout", "Quick")]);
    assert!(quick <= best, "quick {quick} vs best {best}");
    // Best and Quick never share a saved picture.
    clear_cache();
    handle(Event::Open(with_settings(
        big_scene(100),
        &[("layout", "Quick")],
    )));
    while handle(Event::Step).pending {}
    let f = handle(Event::Open(big_scene(100)));
    assert!(f.pending, "a Quick picture is not used for Best");
}

// --- logging ---

/// The lines the plugin logged since the last call, and the floor the pretend
/// host reports (the lowest level it would keep).
fn logged() -> Vec<(i32, String)> {
    crate::host::LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

fn set_floor(n: i32) {
    crate::host::FLOOR.with(|f| f.set(n));
}

fn build_all(scene: Scene) {
    let mut f = handle(Event::Open(scene));
    while f.pending {
        f = handle(Event::Step);
    }
}

fn has(lines: &[(i32, String)], level: i32, text: &str) -> bool {
    lines.iter().any(|(l, m)| *l == level && m.contains(text))
}

#[test]
fn a_new_library_logs_its_milestones_at_their_levels() {
    clear_cache();
    set_floor(10);
    logged();
    build_all(big_scene(60));
    let lines = logged();
    assert!(has(&lines, 10, "open: 60 notes"), "{lines:?}");
    assert!(has(&lines, 10, "no saved picture"), "{lines:?}");
    assert!(has(&lines, 20, "building 60 notes in"), "{lines:?}");
    assert!(has(&lines, 20, "fresh"), "{lines:?}");
    assert!(has(&lines, 10, "step 1,"), "{lines:?}");
    assert!(has(&lines, 20, "built in"), "{lines:?}");
    assert!(
        lines.iter().all(|(l, _)| *l < 30),
        "nothing is a problem here"
    );
    // The settings are in the open line.
    clear_cache();
    logged();
    let s = with_settings(
        big_scene(20),
        &[("view", "List"), ("layout", "Quick"), ("labels", "All")],
    );
    build_all(s);
    let lines = logged();
    assert!(
        has(&lines, 10, "start on List, labels All, layout Quick"),
        "{lines:?}"
    );
    assert!(has(&lines, 20, ", quick"), "{lines:?}");
}

#[test]
fn a_library_that_was_built_before_says_it_found_the_picture() {
    clear_cache();
    set_floor(10);
    build_all(big_scene(40));
    logged();
    build_all(big_scene(40));
    let lines = logged();
    assert!(
        has(&lines, 10, "saved picture found (40 notes)"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|(_, m)| m.contains("building")),
        "{lines:?}"
    );
}

#[test]
fn a_changed_library_warm_starts_and_says_so() {
    clear_cache();
    set_floor(10);
    build_all(big_scene(80));
    logged();
    let mut s = big_scene(81);
    s.notes[80].name = "zz-new.md".into();
    build_all(s);
    let lines = logged();
    // A changed library has its own key, so nothing is saved for it yet; the
    // last picture is the starting point, and far fewer iterations run.
    assert!(has(&lines, 10, "no saved picture"), "{lines:?}");
    assert!(has(&lines, 20, "30 iterations (warm start)"), "{lines:?}");
}

#[test]
fn a_damaged_saved_picture_is_a_warning() {
    clear_cache();
    set_floor(10);
    let scene = big_scene(30);
    let names: Vec<String> = scene.notes.iter().map(|n| n.name.clone()).collect();
    let links: Vec<(usize, usize)> = scene
        .edges
        .iter()
        .map(|&(a, b)| (a as usize, b as usize))
        .collect();
    crate::host::store(layout::layout_key(&names, &links), b"junk");
    crate::host::store(0, b"more junk");
    logged();
    build_all(scene);
    let lines = logged();
    assert!(has(&lines, 30, "saved picture is damaged"), "{lines:?}");
    assert!(has(&lines, 30, "last picture is damaged"), "{lines:?}");
    // And it still built a layout.
    assert!(has(&lines, 20, "built in"), "{lines:?}");
}

#[test]
fn the_host_floor_decides_what_is_built_into_a_message() {
    // The host says nothing would be kept: no line is even formatted.
    clear_cache();
    set_floor(100);
    logged();
    build_all(big_scene(40));
    assert!(logged().is_empty());
    // Only WARNING and worse: a normal build has none.
    clear_cache();
    set_floor(30);
    build_all(big_scene(40));
    assert!(logged().is_empty());
    // INFO: the milestones, not the detail.
    clear_cache();
    set_floor(20);
    build_all(big_scene(40));
    let lines = logged();
    assert!(has(&lines, 20, "building") && has(&lines, 20, "built in"));
    assert!(lines.iter().all(|(l, _)| *l >= 20), "{lines:?}");
    set_floor(10);
}
