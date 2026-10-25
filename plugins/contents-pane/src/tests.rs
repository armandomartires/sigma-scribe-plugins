use super::*;
use sigma_plugin_sdk::api::Scene;
use sigma_plugin_sdk::test_scene;

fn scene(n: usize, height: i32) -> Scene {
    let mut s = test_scene(height);
    s.headings = (0..n)
        .map(|i| HeadingRow {
            level: (i % 3) as u8 + 1,
            line: i as u32 * 4,
            text: format!("Heading {i}"),
        })
        .collect();
    s
}

fn texts(f: &Frame) -> Vec<&str> {
    f.ops
        .iter()
        .filter_map(|o| match o {
            Op::Text { s, .. } => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn it_draws_the_title_and_one_named_row_per_heading() {
    let f = handle(Event::Open(scene(3, 600)));
    assert_eq!(
        texts(&f),
        ["Contents", "Heading 0", "Heading 1", "Heading 2"]
    );
    let labels: Vec<&str> = f.hits.iter().map(|h| h.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Heading:Heading 0",
            "Heading:Heading 1",
            "Heading:Heading 2"
        ]
    );
    assert!(f.actions.is_empty());
}

#[test]
fn a_tap_on_a_row_names_that_heading_and_elsewhere_names_none() {
    handle(Event::Open(scene(3, 600)));
    let row = handle(Event::Open(scene(3, 600))).hits[1].rect;
    let f = handle(Event::Tap {
        x: row.x + 5,
        y: row.y + 5,
    });
    assert_eq!(f.actions, [Action::GotoHeading(1)]);
    assert!(!f.ops.is_empty(), "the pane keeps its picture");
    let f = handle(Event::Tap { x: 1130, y: 110 }); // the header
    assert!(f.actions.is_empty() && !f.ops.is_empty());
}

#[test]
fn rows_that_do_not_fit_are_counted() {
    // 44 px header + room for two 44 px rows.
    let f = handle(Event::Open(scene(5, 44 + 2 * 44)));
    assert_eq!(f.hits.len(), 2);
    assert!(texts(&f).contains(&"3 more\u{2026}"), "{:?}", texts(&f));
}

#[test]
fn no_headings_shows_the_hint() {
    let f = handle(Event::Open(scene(0, 600)));
    assert!(f.hits.is_empty());
    assert!(texts(&f).join(" ").contains("No headings"));
}

#[test]
fn level_one_is_bold_and_deeper_levels_are_indented() {
    let f = handle(Event::Open(scene(3, 600)));
    let rows: Vec<(Face, f32)> = f
        .ops
        .iter()
        .filter_map(|o| match o {
            Op::Text { face, x, s, .. } if s.starts_with("Heading") => Some((*face, *x)),
            _ => None,
        })
        .collect();
    assert_eq!(rows[0].0, Face::Bold);
    assert!(rows[0].1 < rows[1].1 && rows[1].1 < rows[2].1);
}
