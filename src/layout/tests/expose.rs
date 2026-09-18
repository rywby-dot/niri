use niri_config::animations::{Curve, EasingParams, Kind};

use super::*;

fn setup() -> Layout<TestWindow> {
    let mut options = Options::default();
    options.animations.expose_open_close.0.kind = Kind::Easing(EasingParams {
        duration_ms: 1000,
        curve: Curve::Linear,
    });
    check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
            Op::MoveWindowToWorkspace {
                window_id: Some(1),
                workspace_idx: 1,
            },
            Op::CompleteAnimations,
        ],
    )
}

fn targets(layout: &Layout<TestWindow>) -> Vec<(usize, Rectangle<f64, Logical>)> {
    let mut tiles: Vec<_> = layout
        .active_monitor_ref()
        .unwrap()
        .workspaces
        .iter()
        .flat_map(|ws| ws.tiles())
        .collect();
    tiles.sort_by_key(|tile| tile.opening_order);
    let sizes: Vec<_> = tiles.iter().map(|tile| tile.tile_size()).collect();
    let rects = crate::layout::expose::arrange(
        &sizes,
        Rectangle::from_size(output_size(layout.active_output().unwrap())),
        16.,
    );
    tiles
        .into_iter()
        .zip(rects)
        .map(|(tile, rect)| (*tile.window().id(), rect))
        .collect()
}

#[test]
fn selecting_a_window_switches_workspace_and_focus() {
    let mut layout = setup();
    let targets = targets(&layout);
    assert_eq!(
        targets.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [1, 2]
    );
    layout.open_expose();
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    let output = layout.active_output().unwrap();
    for (id, rect) in targets {
        let pos = rect.loc + Point::from((rect.size.w / 2., rect.size.h / 2.));
        let (win, hit) = layout.window_under(output, pos).unwrap();
        assert_eq!(*win.id(), id);
        assert!(matches!(hit, HitType::Activate { .. }));
    }
    layout.select_expose_window(&1);
    assert!(!layout.is_expose_open());
    assert_eq!(layout.focus().unwrap().id(), &1);
    assert!(layout.active_workspace().unwrap().has_window(&1));
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.verify_invariants();
}

#[test]
fn visible_windows_move_directly_to_their_planned_positions() {
    let mut layout = setup();
    Op::MoveWindowToWorkspace {
        window_id: Some(1),
        workspace_idx: 0,
    }
    .apply(&mut layout);
    Op::FocusWorkspace(0).apply(&mut layout);
    Op::CompleteAnimations.apply(&mut layout);
    let (tile, origin, _) = layout
        .active_workspace()
        .unwrap()
        .tiles_with_render_positions()
        .find(|(tile, _, _)| tile.window().id() == &2)
        .unwrap();
    let original = Rectangle::new(origin, tile.tile_size());
    let target = targets(&layout)
        .into_iter()
        .find(|(id, _)| *id == 2)
        .unwrap()
        .1;
    let output = layout.active_output().unwrap().clone();
    layout.open_expose();
    let center = |rect: Rectangle<f64, Logical>| {
        rect.loc + Point::from((rect.size.w / 2., rect.size.h / 2.))
    };
    assert_eq!(
        layout
            .window_under(&output, center(original))
            .unwrap()
            .0
            .id(),
        &2
    );
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    let midpoint = Rectangle::new(
        original.loc + (target.loc - original.loc).upscale(0.5),
        Size::from((
            (original.size.w + target.size.w) / 2.,
            (original.size.h + target.size.h) / 2.,
        )),
    );
    assert_eq!(
        layout
            .window_under(&output, center(midpoint))
            .unwrap()
            .0
            .id(),
        &2
    );
    // Reopening during the transition must retain the current position and size.
    layout.close_expose();
    layout.open_expose();
    assert_eq!(
        layout
            .window_under(&output, center(midpoint))
            .unwrap()
            .0
            .id(),
        &2
    );
    layout.verify_invariants();
}

#[test]
fn keyboard_selection_updates_focus_and_cancel_preserves_it() {
    let mut layout = setup();
    let focus = *layout.focus().unwrap().id();
    layout.open_expose();
    layout.cycle_expose(true);
    let selected = *layout.focus().unwrap().id();
    assert_ne!(selected, focus);
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    layout.close_expose();
    assert_eq!(*layout.focus().unwrap().id(), selected);
    // Catch the closing transition and reverse it.
    layout.open_expose();
    assert!(layout.is_expose_open());
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.confirm_expose();
    assert_eq!(*layout.focus().unwrap().id(), selected);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.verify_invariants();
}

#[test]
fn configured_focus_actions_are_respected_before_rendering() {
    let mut layout = setup();
    layout.open_expose();
    layout.activate_window(&1);
    layout.confirm_expose();
    assert_eq!(*layout.focus().unwrap().id(), 1);
    assert!(!layout.is_expose_open());
    layout.verify_invariants();
}

#[test]
fn arrow_selection_changes_real_focus_without_leaving_expose() {
    let mut layout = setup();
    layout.open_expose();
    layout.activate_window(&1);
    layout.focus_expose(ExposeDirection::Right);
    assert_eq!(*layout.focus().unwrap().id(), 2);
    assert!(layout.is_expose_open());
    layout.focus_expose(ExposeDirection::Right);
    assert_eq!(*layout.focus().unwrap().id(), 2);
    layout.focus_expose(ExposeDirection::Left);
    assert_eq!(*layout.focus().unwrap().id(), 1);
    layout.verify_invariants();
}

#[test]
fn opening_overview_leaves_expose() {
    let mut layout = setup();
    layout.open_expose();
    layout.toggle_overview();
    assert!(!layout.is_expose_open());
    assert!(layout.is_overview_open());
    layout.open_expose();
    assert!(layout.is_expose_open());
    assert!(!layout.is_overview_open());
    layout.overview_gesture_begin();
    assert!(!layout.is_expose_open());
    assert!(layout.is_overview_open());
    layout.verify_invariants();
}

#[test]
fn windows_and_outputs_can_change_while_expose_is_open() {
    let mut layout = setup();
    layout.open_expose();
    Op::AddOutput(2).apply(&mut layout);
    assert!(layout.monitors().all(|mon| mon.is_expose_open()));
    Op::AddWindow {
        params: TestWindowParams::new(3),
    }
    .apply(&mut layout);
    Op::CloseWindow(2).apply(&mut layout);
    layout.update_render_elements(None);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    for (id, rect) in targets(&layout) {
        let pos = rect.loc + Point::from((rect.size.w / 2., rect.size.h / 2.));
        let (win, _) = layout
            .window_under(layout.active_output().unwrap(), pos)
            .unwrap();
        assert_eq!(*win.id(), id);
    }
    layout.verify_invariants();
}
