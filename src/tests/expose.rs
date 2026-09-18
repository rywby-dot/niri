use niri_config::{Action, Color, Config};
use smithay::backend::renderer::element::Element as _;
use smithay::backend::renderer::Color32F;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity,
};
use smithay::utils::Point;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::{ClientId, LayerConfigureProps};
use super::Fixture;
use crate::layout::focus_ring::FocusRingRenderElement;
use crate::layout::tile::TileRenderElement;
use crate::layout::{ExposeDirection, LayoutElement as _, SizingMode};
use crate::niri::KeyboardFocus;
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
