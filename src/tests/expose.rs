use niri_config::gestures::HotCorners;
use niri_config::output::Output as OutputConfig;
use niri_config::{Action, Color, Config};
use smithay::backend::renderer::element::{Element as _, Id};
use smithay::backend::renderer::Color32F;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity,
};
use smithay::utils::{Point, Rectangle, Size};
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::{ClientId, LayerConfigureProps};
use super::Fixture;
use crate::layout::focus_ring::FocusRingRenderElement;
use crate::layout::tile::TileRenderElement;
use crate::layout::{ExposeDirection, LayoutElement as _, SizingMode};
use crate::niri::{HotCornerAction, KeyboardFocus};
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::{RenderCtx, RenderTarget};

fn create_window(f: &mut Fixture, id: ClientId) -> WlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    surface
}

#[test]
fn expose_hot_corners_resolve_actions_and_per_output_overrides() {
    let mut config = Config::default();
    config.gestures.hot_corners_expose = Some(HotCorners {
        top_right: true,
        ..Default::default()
    });
    config.gestures.hot_corners_expose_all_outputs = Some(HotCorners::default());
    config.outputs.0.push(OutputConfig {
        name: "headless-2".into(),
        hot_corners_expose: Some(HotCorners {
            bottom_left: true,
            ..Default::default()
        }),
        hot_corners_expose_all_outputs: Some(HotCorners {
            off: true,
            ..Default::default()
        }),
        ..Default::default()
    });

    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let output1 = f.niri_output(1);
    let output2 = f.niri_output(2);
    let geo1 = f.niri().global_space.output_geometry(&output1).unwrap();
    let geo2 = f.niri().global_space.output_geometry(&output2).unwrap();

    assert_eq!(
        f.niri().contents_under(geo1.loc.to_f64()).hot_corner,
        Some(HotCornerAction::ExposeAllOutputs)
    );
    assert_eq!(
        f.niri()
            .contents_under((geo1.loc + Point::from((geo1.size.w - 1, 0))).to_f64())
            .hot_corner,
        Some(HotCornerAction::Expose)
    );
    assert_eq!(
        f.niri().contents_under(geo2.loc.to_f64()).hot_corner,
        Some(HotCornerAction::Overview)
    );
    assert_eq!(
        f.niri()
            .contents_under((geo2.loc + Point::from((0, geo2.size.h - 1))).to_f64())
            .hot_corner,
        Some(HotCornerAction::Expose)
    );
}

#[test]
fn close_action_targets_the_expose_selection() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let id = f.add_client();
    let first = create_window(&mut f, id);
    let second = create_window(&mut f, id);
    f.niri().layout.open_expose();
    f.niri().layout.focus_expose(ExposeDirection::Left);
    f.niri_state().do_action(Action::CloseWindow, false);
    f.double_roundtrip(id);
    assert!(f.client(id).window(&first).close_requested);
    assert!(!f.client(id).window(&second).close_requested);
    assert!(f.niri().layout.is_expose_open());
}

#[test]
fn closing_expose_restores_keyboard_focus_before_the_animation_finishes() {
    for all_outputs in [false, true] {
        let mut f = Fixture::new();
        f.add_output(1, (1280, 720));
        let id = f.add_client();
        create_window(&mut f, id);
        if all_outputs {
            f.niri().layout.toggle_expose_all_outputs();
            f.niri_state()
                .do_action(Action::ToggleExposeAllOutputs, false);
        } else {
            f.niri().layout.open_expose();
            f.niri_state().do_action(Action::ToggleExpose, false);
        }
        f.double_roundtrip(id);

        assert!(matches!(
            &f.niri().keyboard_focus,
            KeyboardFocus::Layout {
                surface: Some(_)
            }
        ));
        let output = f.niri().layout.active_output().unwrap().clone();
        assert!(f.niri().layout.has_expose_on_output(&output));
        f.niri_complete_animations();
    }
}

#[test]
fn top_layer_launcher_receives_focus_and_pointer_input_in_expose() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let id = f.add_client();
    create_window(&mut f, id);
    f.niri().layout.open_expose();
    f.double_roundtrip(id);
    assert!(f.niri().keyboard_focus.is_expose());

    let layer = f.client(id).create_layer(None, Layer::Top, "launcher");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Top),
        size: Some((200, 200)),
        kb_interactivity: Some(KeyboardInteractivity::Exclusive),
        ..Default::default()
    });
    layer.commit();
    f.roundtrip(id);
    let layer = f.client(id).layer(&surface);
    layer.attach_new_buffer();
    layer.set_size(200, 200);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);
    assert!(f.niri().layout.is_expose_open());
    assert!(matches!(
        f.niri().keyboard_focus,
        KeyboardFocus::LayerShell { .. }
    ));
    let under = f.niri().contents_under(Point::from((50., 50.)));
    assert!(under.layer.is_some());
    assert!(under.window.is_none());
}

#[test]
fn egl_all_outputs_expose_preserves_geometry_across_output_scales() {
    let mut config = Config::default();
    config.layout.border.off = true;
    config.layout.focus_ring.off = true;
    config.animations.expose_open_close.0.off = true;
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let host = f.niri_output(1);
    let remote = f.niri_output(2);
    remote.change_current_state(
        None,
        None,
        Some(smithay::output::Scale::Fractional(2.)),
        Some((1280, 0).into()),
    );
    f.niri().layout.update_output_size(&remote);
    let id = f.add_client();
    create_window(&mut f, id);
    f.niri_focus_output(2);
    create_window(&mut f, id);
    f.niri_focus_output(1);
    f.niri_complete_animations();
    f.niri().layout.toggle_expose_all_outputs();
    f.niri_complete_animations();
    f.niri().update_render_elements(Some(&host));
    let mut tiles: Vec<_> = f
        .niri()
        .layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .collect();
    tiles.sort_by_key(|tile| tile.window().id().get());
    assert_eq!(
        tiles.iter().map(|tile| tile.scale()).collect::<Vec<_>>(),
        [1., 2.]
    );
    let sizes: Vec<_> = tiles.iter().map(|tile| tile.tile_size()).collect();
    let ids: Vec<_> = tiles
        .iter()
        .map(|tile| Id::from_wayland_resource(tile.window().toplevel().wl_surface()))
        .collect();
    assert_eq!(sizes, [Size::from((100., 100.)), Size::from((100., 100.))]);
    // Two square windows fill one centered row, with 16 logical pixels between them.
    let rects: [Rectangle<f64, smithay::utils::Logical>; 2] = [
        Rectangle::new((16., 52.).into(), Size::from((616., 616.))),
        Rectangle::new((648., 52.).into(), Size::from((616., 616.))),
    ];
    let expected: Vec<_> = ids.into_iter().zip(rects).collect();
    let state = f.niri_state();
    let mut seen = Vec::new();
    state
        .backend
        .with_primary_renderer(|renderer| {
            state.niri.layout.render_workspaces_for_output(
                RenderCtx {
                    renderer,
                    target: RenderTarget::Output,
                    xray: None,
                },
                &host,
                false,
                &mut |elem| {
                    if let Some((id, rect)) = expected.iter().find(|(id, _)| *id == *elem.id()) {
                        let actual = elem.geometry(1.0.into());
                        let planned = rect.to_i32_round::<i32>();
                        seen.push((id.clone(), actual, planned));
                    }
                },
            );
        })
        .unwrap();
    assert_eq!(seen.len(), 2);
    for (_, actual, planned) in seen {
        assert!(
            (actual.loc.x - planned.loc.x).abs() <= 1,
            "{actual:?} != {planned:?}"
        );
        assert!(
            (actual.loc.y - planned.loc.y).abs() <= 1,
            "{actual:?} != {planned:?}"
        );
        assert!(
            (actual.size.w - planned.size.w).abs() <= 1,
            "{actual:?} != {planned:?}"
        );
        assert!(
            (actual.size.h - planned.size.h).abs() <= 1,
            "{actual:?} != {planned:?}"
        );
    }
    assert_eq!(f.niri().layout.windows_for_output(&host).count(), 1);
    assert_eq!(f.niri().layout.windows_for_output(&remote).count(), 1);

    // Both Exposé variants are local to their host output. The remote output keeps rendering its
    // regular workspace throughout the transition.
    f.niri().layout.toggle_expose();
    assert!(!f.niri().layout.is_all_outputs_expose_on_output(&remote));
    assert_eq!(f.niri().layout.windows_rendered_on_output(&remote).count(), 1);
    f.niri_complete_animations();
    assert!(f.niri().layout.all_outputs_expose_output().is_none());
    assert!(f.niri().layout.is_expose_open());
}

#[test]
fn egl_expose_decorations_follow_border_and_focus_ring_settings() {
    let border_active = Color::new_unpremul(0., 1., 0., 1.);
    let border_inactive = Color::new_unpremul(0., 0., 1., 1.);
    let ring_active = Color::new_unpremul(1., 0., 0., 1.);
    for (border_on, ring_on) in [(false, false), (false, true), (true, false), (true, true)] {
        for mode in [
            SizingMode::Normal,
            SizingMode::Fullscreen,
            SizingMode::Maximized,
        ] {
            let mut config = Config::default();
            config.layout.border.off = !border_on;
            config.layout.border.active_color = border_active;
            config.layout.border.inactive_color = border_inactive;
            config.layout.focus_ring.off = !ring_on;
            config.layout.focus_ring.active_color = ring_active;
            let mut f = Fixture::with_config(config);
            f.niri_state().backend.headless().add_renderer().unwrap();
            f.add_output(1, (1280, 720));
            let id = f.add_client();
            create_window(&mut f, id);
            let second = create_window(&mut f, id);
            let selected = f.niri().layout.focus().unwrap().window.clone();
            match mode {
                SizingMode::Normal => (),
                SizingMode::Fullscreen => f.niri().layout.set_fullscreen(&selected, true),
                SizingMode::Maximized => f.niri().layout.set_maximized(&selected, true),
            }
            f.double_roundtrip(id);
            let window = f.client(id).window(&second);
            let (_, configure) = window.configures_received.last().unwrap();
            window.set_size(configure.size.0 as u16, configure.size.1 as u16);
            window.ack_last_and_commit();
            f.double_roundtrip(id);
            f.niri_complete_animations();
            assert_eq!(f.niri().layout.focus().unwrap().sizing_mode(), mode);
            f.niri().layout.open_expose();
            f.niri_complete_animations();
            f.niri().update_render_elements(None);

            let state = f.niri_state();
            state
                .backend
                .with_primary_renderer(|renderer| {
                    let tiles: Vec<_> = state
                        .niri
                        .layout
                        .workspaces()
                        .flat_map(|(_, _, ws)| ws.tiles())
                        .collect();
                    assert_eq!(tiles.len(), 2);
                    for tile in tiles {
                        let is_selected = tile.window().window == selected;
                        let expected_border: Color32F = if is_selected {
                            border_active
                        } else {
                            border_inactive
                        }
                        .into();
                        let mut rings = 0;
                        let mut borders = 0;
                        let mut ring_left = f64::INFINITY;
                        let mut saw_contents = false;
                        tile.render_expose(
                            RenderCtx {
                                renderer,
                                target: RenderTarget::Output,
                                xray: None,
                            },
                            Point::default(),
                            XrayPos::default(),
                            is_selected,
                            &mut |elem| {
                                if let TileRenderElement::FocusRing(
                                    FocusRingRenderElement::SolidColor(elem),
                                ) = elem
                                {
                                    // The background of the ring must be behind, not over, the contents.
                                    assert!(saw_contents);
                                    assert_eq!(elem.alpha(), 1.);
                                    if elem.color() == Color32F::from(ring_active) {
                                        assert!(is_selected && ring_on);
                                        ring_left = ring_left.min(elem.geo().loc.x);
                                        rings += 1;
                                    } else {
                                        assert!(border_on);
                                        assert_eq!(elem.color(), expected_border);
                                        borders += 1;
                                    }
                                } else {
                                    saw_contents = true;
                                }
                            },
                        );
                        assert_eq!(
                            rings > 0,
                            is_selected && ring_on,
                            "incorrect focus ring for {mode:?}"
                        );
                        assert_eq!(borders > 0, border_on, "incorrect border for {mode:?}");
                        if is_selected && border_on && ring_on && !mode.is_normal() {
                            assert_eq!(ring_left, -8., "focus ring must surround the border");
                        }
                    }
                })
                .unwrap();
        }
    }
}
