use niri_config::animations::{Curve, EasingParams, Kind};

use super::*;

fn center(rect: Rectangle<f64, Logical>) -> Point<f64, Logical> {
    rect.loc + Point::from((rect.size.w / 2., rect.size.h / 2.))
}

fn overview_geometries(layout: &Layout<TestWindow>) -> Vec<(usize, Rectangle<f64, Logical>)> {
    layout
        .monitors()
        .flat_map(|mon| {
            let offset = mon.output.current_location().to_f64();
            let zoom = mon.overview_zoom();
            mon.workspaces_with_render_geo_cull(false)
                .flat_map(move |(ws, geo)| {
                    ws.tiles_with_render_positions().map(move |(tile, pos, _)| {
                        (
                            *tile.window().id(),
                            Rectangle::new(
                                offset + geo.loc + pos.upscale(zoom),
                                tile.tile_size().upscale(zoom),
                            ),
                        )
                    })
                })
        })
        .collect()
}

fn assert_geometries_eq(
    actual: &[(usize, Rectangle<f64, Logical>)],
    expected: &[(usize, Rectangle<f64, Logical>)],
) {
    assert_eq!(actual.len(), expected.len());
    for (id, actual) in actual {
        let expected = expected.iter().find(|(other, _)| other == id).unwrap().1;
        assert!((actual.loc.x - expected.loc.x).abs() < 0.001);
        assert!((actual.loc.y - expected.loc.y).abs() < 0.001);
        assert!((actual.size.w - expected.size.w).abs() < 0.001);
        assert!((actual.size.h - expected.size.h).abs() < 0.001);
    }
}

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

fn setup_two_outputs() -> Layout<TestWindow> {
    let mut layout = setup();
    for op in [
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::MoveWindowToWorkspace {
            window_id: Some(3),
            workspace_idx: 1,
        },
        Op::FocusOutput(1),
        Op::CompleteAnimations,
    ] {
        op.apply(&mut layout);
    }
    layout
}

fn all_targets(
    layout: &Layout<TestWindow>,
    output: &Output,
) -> Vec<(usize, Rectangle<f64, Logical>)> {
    let mut tiles: Vec<_> = layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .collect();
    tiles.sort_by_key(|tile| tile.opening_order);
    let sizes: Vec<_> = tiles.iter().map(|tile| tile.tile_size()).collect();
    let rects = crate::layout::expose::arrange(
        &sizes,
        layout.monitor_for_output(output).unwrap().working_area,
        16.,
    );
    tiles
        .into_iter()
        .zip(rects)
        .map(|(tile, rect)| (*tile.window().id(), rect))
        .collect()
}

#[test]
fn all_outputs_expose_shows_remote_windows_without_moving_them() {
    let mut layout = setup_two_outputs();
    let host = layout.active_output().unwrap().clone();
    let targets = all_targets(&layout, &host);
    assert_eq!(
        targets.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    layout.toggle_expose_all_outputs();
    assert!(layout.is_all_outputs_expose_open());
    assert!(layout.monitors().all(|mon| !mon.is_expose_open()));
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    for (id, rect) in targets {
        let (window, hit) = layout.window_under(&host, center(rect)).unwrap();
        assert_eq!(*window.id(), id);
        assert!(matches!(hit, HitType::Activate { .. }));
    }
    assert_eq!(
        layout
            .windows_for_output(&host)
            .map(|win| *win.id())
            .collect::<Vec<_>>()
            .len(),
        2
    );
    assert_eq!(layout.windows_rendered_on_output(&host).count(), 3);
    let remote = layout
        .windows()
        .find(|(_, win)| *win.id() == 3)
        .unwrap()
        .0
        .unwrap()
        .output
        .clone();
    assert_ne!(remote, host);
    layout.select_expose_window(&3);
    assert!(!layout.is_expose_open());
    assert_eq!(*layout.focus().unwrap().id(), 3);
    assert_eq!(layout.active_output(), Some(&remote));
    Op::CompleteAnimations.apply(&mut layout);
    assert!(layout.all_outputs_expose_output().is_none());
    layout.verify_invariants();
}

#[test]
fn all_outputs_keyboard_navigation_keeps_the_original_host() {
    let mut layout = setup_two_outputs();
    let host = layout.active_output().unwrap().clone();
    layout.toggle_expose_all_outputs();
    for expected in [3, 1, 2] {
        layout.cycle_expose(true);
        assert_eq!(*layout.focus().unwrap().id(), expected);
        assert_eq!(layout.all_outputs_expose_output(), Some(&host));
        assert!(layout.is_expose_open());
    }
    layout.activate_window(&3);
    layout.confirm_expose();
    assert_eq!(*layout.focus().unwrap().id(), 3);
    assert!(!layout.is_expose_open());
    layout.verify_invariants();
}

#[test]
fn all_outputs_grid_updates_after_windows_and_outputs_change() {
    let mut layout = setup_two_outputs();
    let host = layout.active_output().unwrap().clone();
    layout.toggle_expose_all_outputs();
    Op::FocusOutput(2).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(4),
    }
    .apply(&mut layout);
    Op::CloseWindow(3).apply(&mut layout);
    Op::AddOutput(3).apply(&mut layout);
    layout.update_render_elements(Some(&host));
    assert_eq!(layout.all_outputs_expose_output(), Some(&host));
    assert!(layout.monitors().all(|mon| !mon.is_expose_open()));
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    for (id, rect) in all_targets(&layout, &host) {
        assert_eq!(
            *layout.window_under(&host, center(rect)).unwrap().0.id(),
            id
        );
    }
    Op::RemoveOutput(2).apply(&mut layout);
    layout.update_render_elements(None);
    assert!(layout.is_all_outputs_expose_open());
    Op::RemoveOutput(1).apply(&mut layout);
    assert!(!layout.is_all_outputs_expose_open());
    layout.verify_invariants();
}

#[test]
fn all_outputs_mode_switching_and_reopening_are_mutually_exclusive() {
    let mut layout = setup_two_outputs();
    layout.open_expose();
    layout.toggle_expose_all_outputs();
    assert!(layout.is_all_outputs_expose_open());
    assert!(layout.monitors().all(|mon| !mon.is_expose_open()));
    layout.toggle_expose_all_outputs();
    assert!(!layout.is_expose_open());
    layout.toggle_expose_all_outputs();
    assert!(layout.is_all_outputs_expose_open());
    layout.toggle_overview();
    assert!(layout.is_overview_open());
    assert!(layout.all_outputs_expose_output().is_some());
    Op::CompleteAnimations.apply(&mut layout);
    assert!(layout.all_outputs_expose_output().is_none());
    layout.toggle_expose_all_outputs();
    assert!(!layout.is_overview_open());
    layout.close_expose();
    layout.open_expose();
    assert!(layout.is_expose_open());
    assert!(layout.all_outputs_expose_output().is_some());
    Op::CompleteAnimations.apply(&mut layout);
    assert!(layout.all_outputs_expose_output().is_none());
    layout.verify_invariants();
}

#[test]
fn expose_and_overview_transitions_preserve_window_geometry() {
    let mut layout = setup();
    layout.open_expose();
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    let expose = layout.expose_geometries_global();
    let overview = {
        layout.overview_open = true;
        layout.overview_progress = Some(OverviewProgress::Open);
        layout.set_monitors_overview_state();
        let geometries = overview_geometries(&layout);
        layout.overview_open = false;
        layout.overview_progress = None;
        layout.set_monitors_overview_state();
        geometries
    };

    layout.toggle_overview();
    assert!(layout.is_overview_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &expose);
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    let midpoint: Vec<_> = expose
        .iter()
        .map(|(id, from)| {
            let to = overview.iter().find(|(other, _)| other == id).unwrap().1;
            (
                *id,
                Rectangle::new(
                    from.loc + (to.loc - from.loc).upscale(0.5),
                    Size::from((
                        (from.size.w + to.size.w) / 2.,
                        (from.size.h + to.size.h) / 2.,
                    )),
                ),
            )
        })
        .collect();
    assert_geometries_eq(&layout.expose_geometries_global(), &midpoint);
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    assert!(layout.expose_geometries_global().is_empty());

    let overview = overview_geometries(&layout);
    layout.toggle_expose();
    assert!(!layout.is_overview_open());
    assert!(layout.is_expose_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &overview);
    layout.verify_invariants();
}

#[test]
fn local_and_all_outputs_expose_transition_without_normal_layout() {
    let mut layout = setup_two_outputs();
    layout.open_expose();
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    let local = layout.expose_geometries_global();

    layout.toggle_expose_all_outputs();
    assert!(layout.is_all_outputs_expose_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &local);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    let all_outputs = layout.expose_geometries_global();

    layout.toggle_expose();
    assert!(!layout.is_all_outputs_expose_open());
    assert!(layout.monitors().all(|mon| mon.is_expose_open()));
    assert_geometries_eq(&layout.expose_geometries_global(), &all_outputs);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    assert!(layout.all_outputs_expose_output().is_none());
    assert!(layout.monitors().all(|mon| mon.is_expose_open()));
    assert_geometries_eq(&layout.expose_geometries_global(), &local);
    layout.verify_invariants();
}

#[test]
fn overview_and_all_outputs_expose_transition_without_normal_layout() {
    let mut layout = setup_two_outputs();
    layout.toggle_overview();
    Op::CompleteAnimations.apply(&mut layout);
    let overview = overview_geometries(&layout);

    layout.toggle_expose_all_outputs();
    assert!(!layout.is_overview_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &overview);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    let all_outputs = layout.expose_geometries_global();

    layout.toggle_overview();
    assert!(layout.is_overview_open());
    assert!(!layout.is_all_outputs_expose_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &all_outputs);
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    assert!(layout.all_outputs_expose_output().is_none());
    assert_geometries_eq(&overview_geometries(&layout), &overview);
    layout.verify_invariants();
}

#[test]
fn interrupted_mode_transitions_restart_from_current_geometry() {
    let mut layout = setup_two_outputs();

    layout.toggle_expose_all_outputs();
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.toggle_expose();
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    let all_to_local = layout.expose_geometries_global();
    layout.toggle_expose_all_outputs();
    assert!(layout.is_all_outputs_expose_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &all_to_local);

    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.toggle_overview();
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    let all_to_overview = layout.expose_geometries_global();
    layout.toggle_expose_all_outputs();
    assert!(!layout.is_overview_open());
    assert!(layout.is_all_outputs_expose_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &all_to_overview);

    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.toggle_expose();
    Op::AdvanceAnimations { msec_delta: 1000 }.apply(&mut layout);
    layout.toggle_overview();
    Op::AdvanceAnimations { msec_delta: 500 }.apply(&mut layout);
    let local_to_overview = layout.expose_geometries_global();
    layout.toggle_expose();
    assert!(!layout.is_overview_open());
    assert!(layout.is_expose_open());
    assert_geometries_eq(&layout.expose_geometries_global(), &local_to_overview);
    layout.verify_invariants();
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
