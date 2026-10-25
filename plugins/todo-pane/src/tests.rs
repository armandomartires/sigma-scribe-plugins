use super::*;
use sigma_plugin_sdk::api::{Note, TodoRow};
use sigma_plugin_sdk::test_scene;

fn scene(todos: &[(&str, bool)], height: i32) -> Scene {
    let mut s = test_scene(height);
    s.notes = vec![Note {
        name: "a.md".into(),
        current: true,
        detail: String::new(),
    }];
    s.todos = todos
        .iter()
        .enumerate()
        .map(|(i, (t, done))| TodoRow {
            note: 0,
            line: i as u32,
            done: *done,
            text: (*t).into(),
        })
        .collect();
    s
}

const LIST: [(&str, bool); 3] = [("one", false), ("two", true), ("three", false)];

fn texts(f: &Frame) -> Vec<String> {
    f.ops
        .iter()
        .filter_map(|o| match o {
            Op::Text { s, .. } => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn reset() {
    FILTER.with(|f| f.set(Filter::All));
}

fn centre(f: &Frame, label: &str) -> (i32, i32) {
    let r = f.hits.iter().find(|h| h.label == label).unwrap().rect;
    (r.x + r.w / 2, r.y + r.h / 2)
}

#[test]
fn every_to_do_has_a_box_and_a_text_target_and_the_filter_a_chip() {
    reset();
    let f = handle(Event::Open(scene(&LIST, 900)));
    for t in ["one", "two", "three"] {
        for p in ["TodoBox:", "Todo:"] {
            assert!(
                f.hits.iter().any(|h| h.label == format!("{p}{t}")),
                "{p}{t}"
            );
        }
    }
    assert!(f.hits.iter().any(|h| h.label == "Filter"));
    let t = texts(&f);
    assert!(t.contains(&"To-dos".to_string()) && t.contains(&"All".to_string()));
    assert!(t.iter().filter(|s| s.as_str() == "a.md").count() == 3);
}

#[test]
fn the_box_toggles_and_the_text_opens_by_index_in_the_full_list() {
    reset();
    let f = handle(Event::Open(scene(&LIST, 900)));
    let (x, y) = centre(&f, "TodoBox:three");
    assert_eq!(handle(Event::Tap { x, y }).actions, [Action::ToggleTodo(2)]);
    let (x, y) = centre(&f, "Todo:two");
    assert_eq!(handle(Event::Tap { x, y }).actions, [Action::GotoTodo(1)]);
}

#[test]
fn the_filter_cycles_and_keeps_full_list_indexes() {
    reset();
    let f = handle(Event::Open(scene(&LIST, 900)));
    let (x, y) = centre(&f, "Filter");
    // All -> Open: "two" is gone, "three" is still item 2 of the list.
    let f = handle(Event::Tap { x, y });
    assert_eq!(f.redraw, Redraw::Partial);
    assert!(f.hits.iter().all(|h| h.label != "Todo:two"));
    assert!(texts(&f).contains(&"Open".to_string()));
    let (x, y) = centre(&f, "TodoBox:three");
    assert_eq!(handle(Event::Tap { x, y }).actions, [Action::ToggleTodo(2)]);
    // Open -> Done: only "two".
    let (x, y) = centre(&f, "Filter");
    let f = handle(Event::Tap { x, y });
    assert!(f.hits.iter().all(|h| h.label != "Todo:one"));
    assert!(f.hits.iter().any(|h| h.label == "Todo:two"));
    // Done -> All.
    let (x, y) = centre(&f, "Filter");
    let f = handle(Event::Tap { x, y });
    assert!(texts(&f).contains(&"All".to_string()));
    assert_eq!(f.hits.len(), 1 + 2 * 3);
}

#[test]
fn the_filter_survives_the_scene_being_sent_again() {
    reset();
    let f = handle(Event::Open(scene(&LIST, 900)));
    let (x, y) = centre(&f, "Filter");
    handle(Event::Tap { x, y });
    let f = handle(Event::Open(scene(&[("one", false), ("two", true)], 900)));
    assert!(f.hits.iter().all(|h| h.label != "Todo:two"), "still Open");
    assert!(texts(&f).contains(&"Open".to_string()));
}

#[test]
fn done_items_are_muted_and_struck_through() {
    reset();
    let f = handle(Event::Open(scene(&[("two", true)], 900)));
    let muted = f
        .ops
        .iter()
        .any(|o| matches!(o, Op::Text { s, c: Tone::Mute, .. } if s == "two"));
    assert!(muted);
    // A one-pixel mute rule across the text.
    assert!(f
        .ops
        .iter()
        .any(|o| matches!(o, Op::Fill { r, c: Tone::Mute } if r.h == 1 && r.w > 10)));
}

#[test]
fn rows_that_do_not_fit_are_counted_and_an_empty_filter_shows_the_hint() {
    reset();
    // Header 44 + room for one 58 px row.
    let f = handle(Event::Open(scene(&LIST, 44 + 58)));
    assert!(
        texts(&f).contains(&"2 more\u{2026}".to_string()),
        "{:?}",
        texts(&f)
    );
    reset();
    let f = handle(Event::Open(scene(&[], 900)));
    assert!(texts(&f).join(" ").contains("No to-dos"));
}
