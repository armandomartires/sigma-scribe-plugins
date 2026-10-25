use super::*;
use sigma_plugin_sdk::test_scene;

#[test]
fn the_field_fills_the_body_below_the_header() {
    let scene = test_scene(800);
    let f = handle(Event::Open(scene.clone()));
    let field = f.field.expect("a field");
    assert_eq!(
        (field.rect.x, field.rect.y, field.rect.w, field.rect.h),
        (scene.left + 12, scene.top + 44, 320 - 24, 800 - 44)
    );
    assert_eq!(field.placeholder, PLACEHOLDER);
    assert!(f.actions.is_empty() && f.hits.is_empty());
}

#[test]
fn it_draws_only_a_title_and_a_rule() {
    let f = handle(Event::Open(test_scene(800)));
    assert_eq!(f.ops.len(), 2);
    assert!(matches!(&f.ops[0], Op::Text { s, .. } if s == "Scratch"));
}

#[test]
fn other_events_do_nothing() {
    assert_eq!(handle(Event::Back), Frame::default());
}
