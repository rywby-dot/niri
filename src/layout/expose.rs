//! Exposé geometry is computed once, before starting its transition.

use smithay::backend::renderer::element::utils::{
    Relocate, RelocateRenderElement, RescaleRenderElement,
};
use smithay::output::Output;
use smithay::utils::{Logical, Point, Rectangle, Scale, Size};

use super::monitor::{MonitorInnerRenderElement, MonitorRenderElement};
use super::{Layout, LayoutElement};
use crate::animation::Animation;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::scale_override::ScaleOverrideRenderElement;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;

#[derive(Debug)]
pub(super) struct ExposeWindow<I> {
    pub(super) id: I,
    pub(super) origin: Point<f64, Logical>,
    pub(super) target: Rectangle<f64, Logical>,
    pub(super) size: Size<f64, Logical>,
    pub(super) origin_size: Size<f64, Logical>,
}

impl<I> ExposeWindow<I> {
    pub(super) fn geometry(&self, progress: f64) -> Rectangle<f64, Logical> {
        Rectangle::new(
            self.origin + (self.target.loc - self.origin).upscale(progress),
            Size::from((
                self.origin_size.w + (self.target.size.w - self.origin_size.w) * progress,
                self.origin_size.h + (self.target.size.h - self.origin_size.h) * progress,
            )),
        )
    }
}

#[derive(Debug)]
pub(super) struct AllOutputsExpose<I> {
    pub(super) output: Output,
    pub(super) open: bool,
    animation: Animation,
    windows: Vec<ExposeWindow<I>>,
    area: Rectangle<f64, Logical>,
    selected: usize,
    destination: AllOutputsExposeDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllOutputsExposeDestination {
    Layout,
    LocalExpose,
    Overview,
}

impl<I> AllOutputsExpose<I> {
    pub(super) fn selected_window(&self) -> Option<&I> {
        self.windows.get(self.selected).map(|window| &window.id)
    }

    pub(super) fn contains_window(&self, id: &I) -> bool
    where
        I: PartialEq,
    {
        self.windows.iter().any(|window| &window.id == id)
    }

    pub(super) fn window_under(&self, pos: Point<f64, Logical>) -> Option<&I> {
        if !self.open {
            return None;
        }
        let progress = self.animation.clamped_value();
        self.windows
            .iter()
            .rev()
            .find(|window| window.geometry(progress).contains(pos))
            .map(|window| &window.id)
    }
}

impl<W: LayoutElement> Layout<W> {
    #[cfg(test)]
    pub(super) fn expose_geometries_global(&self) -> Vec<(W::Id, Rectangle<f64, Logical>)> {
        if let Some(expose) = &self.all_outputs_expose {
            let offset = expose.output.current_location().to_f64();
            let progress = expose.animation.clamped_value();
            return expose
                .windows
                .iter()
                .map(|window| {
                    let rect = window.geometry(progress);
                    (
                        window.id.clone(),
                        Rectangle::new(offset + rect.loc, rect.size),
                    )
                })
                .collect();
        }

        self.monitors()
            .flat_map(|mon| {
                let offset = mon.output.current_location().to_f64();
                mon.workspaces.iter().flat_map(move |ws| {
                    ws.tiles().filter_map(move |tile| {
                        let rect = mon.expose_window_geometry(tile.window().id())?;
                        Some((
                            tile.window().id().clone(),
                            Rectangle::new(offset + rect.loc, rect.size),
                        ))
                    })
                })
            })
            .collect()
    }

    pub fn all_outputs_expose_output(&self) -> Option<&Output> {
        self.all_outputs_expose
            .as_ref()
            .map(|expose| &expose.output)
    }

    /// Includes remote thumbnails without changing a window's output ownership.
    pub fn windows_rendered_on_output<'a>(
        &'a self,
        output: &'a Output,
    ) -> impl Iterator<Item = &'a W> + 'a {
        let include_all = self.is_all_outputs_expose_on_output(output);
        let remote = self
            .monitors()
            .filter(move |mon| include_all && mon.output != *output)
            .flat_map(|mon| mon.workspaces.iter().flat_map(|ws| ws.windows()));
        self.windows_for_output(output).chain(remote)
    }

    pub fn is_all_outputs_expose_open(&self) -> bool {
        self.all_outputs_expose
            .as_ref()
            .is_some_and(|expose| expose.open)
    }

    pub fn is_all_outputs_expose_on_output(&self, output: &Output) -> bool {
        self.all_outputs_expose
            .as_ref()
            .is_some_and(|expose| expose.output == *output)
    }

    pub fn toggle_expose_all_outputs(&mut self) {
        if self.is_all_outputs_expose_open() {
            self.close_expose();
            return;
        }
        if self.interactive_move.is_some() {
            return;
        }
        let Some(output) = self.active_output().cloned() else {
            return;
        };
        self.open_all_outputs_expose(output);
        for mon in self.monitors_mut() {
            mon.cancel_expose();
        }
        self.overview_open = false;
        self.overview_progress = None;
        self.set_monitors_overview_state();
    }

    /// Native coordinates relative to the host output, including offscreen workspaces.
    fn all_outputs_expose_windows(&self, output: &Output) -> Vec<ExposeWindow<W::Id>> {
        let mut windows: Vec<_> = self
            .monitors()
            .flat_map(|mon| {
                let offset = (mon.output.current_location() - output.current_location()).to_f64();
                let zoom = mon.overview_zoom();
                mon.workspaces_with_render_geo_cull(false)
                    .flat_map(move |(ws, geo)| {
                        ws.tiles_with_render_positions().map(move |(tile, pos, _)| {
                            let size = tile.tile_size();
                            let native = Rectangle::new(
                                offset + geo.loc + pos.upscale(zoom),
                                size.upscale(zoom),
                            );
                            let origin = mon
                                .expose_window_geometry(tile.window().id())
                                .map(|rect| Rectangle::new(offset + rect.loc, rect.size))
                                .unwrap_or(native);
                            (
                                tile.opening_order,
                                ExposeWindow {
                                    id: tile.window().id().clone(),
                                    origin: origin.loc,
                                    origin_size: origin.size,
                                    target: native,
                                    size,
                                },
                            )
                        })
                    })
            })
            .collect();
        windows.sort_by_key(|(order, _)| *order);
        windows.into_iter().map(|(_, window)| window).collect()
    }

    fn open_all_outputs_expose(&mut self, output: Output) {
        let Some(mon) = self.monitor_for_output(&output) else {
            return;
        };
        let area = mon.working_area;
        let mut windows = self.all_outputs_expose_windows(&output);
        let sizes: Vec<_> = windows.iter().map(|window| window.size).collect();
        let targets = arrange(&sizes, area, 16.);
        if let Some(previous) = &self.all_outputs_expose {
            let offset = (previous.output.current_location() - output.current_location()).to_f64();
            let progress = previous.animation.clamped_value();
            for window in &mut windows {
                if let Some(old) = previous.windows.iter().find(|old| old.id == window.id) {
                    let rect = old.geometry(progress);
                    window.origin = offset + rect.loc;
                    window.origin_size = rect.size;
                }
            }
        }
        for (window, target) in windows.iter_mut().zip(targets) {
            window.target = target;
        }
        let selected = self
            .focus()
            .and_then(|focused| windows.iter().position(|window| window.id == *focused.id()))
            .unwrap_or(0);
        self.all_outputs_expose = Some(AllOutputsExpose {
            output,
            open: true,
            windows,
            area,
            selected,
            destination: AllOutputsExposeDestination::Layout,
            animation: Animation::new(
                self.clock.clone(),
                0.,
                1.,
                0.,
                self.options.animations.expose_open_close.0,
            ),
        });
    }

    pub(super) fn refresh_all_outputs_expose(&mut self) {
        let Some(expose) = self
            .all_outputs_expose
            .as_ref()
            .filter(|expose| expose.open)
        else {
            return;
        };
        let output = expose.output.clone();
        let Some(mon) = self.monitor_for_output(&output) else {
            self.all_outputs_expose = None;
            return;
        };
        let windows = self.all_outputs_expose_windows(&output);
        if expose.area != mon.working_area
            || windows.len() != expose.windows.len()
            || windows
                .iter()
                .zip(&expose.windows)
                .any(|(a, b)| a.id != b.id || a.size != b.size)
        {
            self.open_all_outputs_expose(output);
        }
    }

    pub(super) fn cycle_all_outputs_expose(&mut self, forward: bool) {
        self.refresh_all_outputs_expose();
        let Some(expose) = &mut self.all_outputs_expose else {
            return;
        };
        let count = expose.windows.len();
        if count == 0 {
            return;
        }
        expose.selected = if forward {
            (expose.selected + 1) % count
        } else {
            (expose.selected + count - 1) % count
        };
    }

    pub(super) fn focus_all_outputs_expose(&mut self, direction: ExposeDirection) {
        self.refresh_all_outputs_expose();
        let Some(expose) = &mut self.all_outputs_expose else {
            return;
        };
        let targets: Vec<_> = expose.windows.iter().map(|window| window.target).collect();
        if let Some(idx) = neighbor(&targets, expose.selected, direction) {
            expose.selected = idx;
        }
    }

    pub(super) fn close_all_outputs_expose(&mut self) {
        let Some(expose) = self.all_outputs_expose.as_ref() else {
            return;
        };
        let native = self.all_outputs_expose_windows(&expose.output);
        let targets = native
            .into_iter()
            .map(|window| (window.id, window.target))
            .collect();
        self.close_all_outputs_expose_to(targets, AllOutputsExposeDestination::Layout);
    }

    fn close_all_outputs_expose_to(
        &mut self,
        targets: Vec<(W::Id, Rectangle<f64, Logical>)>,
        destination: AllOutputsExposeDestination,
    ) {
        let Some(expose) = self.all_outputs_expose.as_mut() else {
            return;
        };
        let progress = expose.animation.clamped_value();
        for window in &mut expose.windows {
            let rect = window.geometry(progress);
            window.origin = rect.loc;
            window.origin_size = rect.size;
            if let Some((_, target)) = targets.iter().find(|(id, _)| *id == window.id) {
                window.target = *target;
            }
        }
        expose.open = false;
        expose.destination = destination;
        expose.animation = Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            self.options.animations.expose_open_close.0,
        );
    }

    fn local_expose_targets(&self, host: &Output) -> Vec<(W::Id, Rectangle<f64, Logical>)> {
        self.monitors()
            .flat_map(|mon| {
                let offset = (mon.output.current_location() - host.current_location()).to_f64();
                mon.workspaces.iter().flat_map(move |ws| {
                    ws.tiles().filter_map(move |tile| {
                        let target = mon.expose_window_target(tile.window().id())?;
                        Some((
                            tile.window().id().clone(),
                            Rectangle::new(offset + target.loc, target.size),
                        ))
                    })
                })
            })
            .collect()
    }

    pub(super) fn transition_all_outputs_to_local_expose(&mut self) {
        let Some(host) = self
            .all_outputs_expose
            .as_ref()
            .map(|expose| expose.output.clone())
        else {
            return;
        };
        if let Some(mon) = self.monitor_for_output_mut(&host) {
            mon.open_expose();
        }
        let local = self.local_expose_targets(&host);
        let expose = self.all_outputs_expose.as_ref().unwrap();
        let progress = expose.animation.clamped_value();
        let targets = expose
            .windows
            .iter()
            .map(|window| {
                let current = window.geometry(progress);
                let target = local
                    .iter()
                    .find(|(id, _)| *id == window.id)
                    .map_or(current, |(_, target)| *target);
                (window.id.clone(), target)
            })
            .collect();
        self.close_all_outputs_expose_to(targets, AllOutputsExposeDestination::LocalExpose);
    }

    pub(super) fn transition_all_outputs_to_overview(&mut self) {
        let Some(expose) = self.all_outputs_expose.as_ref() else {
            return;
        };
        let host = expose.output.clone();
        let progress = expose.animation.clamped_value();
        let overview = self.all_outputs_expose_windows(&host);
        let targets = expose
            .windows
            .iter()
            .map(|window| {
                let current = window.geometry(progress);
                let is_host_window = self
                    .windows()
                    .find(|(_, candidate)| candidate.id() == &window.id)
                    .and_then(|(mon, _)| mon)
                    .is_some_and(|mon| mon.output == host);
                let target = if is_host_window {
                    overview
                        .iter()
                        .find(|candidate| candidate.id == window.id)
                        .map_or(current, |candidate| candidate.target)
                } else {
                    current
                };
                (window.id.clone(), target)
            })
            .collect();
        self.close_all_outputs_expose_to(targets, AllOutputsExposeDestination::Overview);
    }

    pub(super) fn advance_all_outputs_expose(&mut self) {
        let Some(expose) = &self.all_outputs_expose else {
            return;
        };
        if !expose.open {
            if expose.animation.is_done() {
                self.all_outputs_expose = None;
                return;
            }
            let destination = expose.destination;
            let native = match destination {
                AllOutputsExposeDestination::Layout => self
                    .all_outputs_expose_windows(&expose.output)
                    .into_iter()
                    .map(|window| (window.id, window.target))
                    .collect(),
                AllOutputsExposeDestination::Overview => return,
                AllOutputsExposeDestination::LocalExpose => {
                    self.local_expose_targets(&expose.output)
                }
            };
            for window in &mut self.all_outputs_expose.as_mut().unwrap().windows {
                if let Some((_, target)) = native.iter().find(|(id, _)| *id == window.id) {
                    window.target = *target;
                }
            }
        }
    }

    pub(super) fn all_outputs_expose_animating(&self, output: Option<&Output>) -> bool {
        self.all_outputs_expose.as_ref().is_some_and(|expose| {
            output.is_none_or(|output| expose.output == *output)
                && (!expose.animation.is_done()
                    || self.monitors().any(|mon| mon.are_animations_ongoing()))
        })
    }

    pub fn render_workspaces_for_output<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        focus_ring: bool,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        if let Some(expose) = self
            .all_outputs_expose
            .as_ref()
            .filter(|expose| expose.output == *output)
        {
            let scale = output.current_scale().fractional_scale();
            let progress = expose.animation.clamped_value();
            for (idx, window) in expose.windows.iter().enumerate().rev() {
                let Some(tile) = self
                    .workspaces()
                    .flat_map(|(_, _, ws)| ws.tiles())
                    .find(|tile| *tile.window().id() == window.id)
                else {
                    continue;
                };
                let rect = window.geometry(progress);
                let zoom_x = rect.size.w / tile.tile_size().w.max(1.);
                let zoom_y = rect.size.h / tile.tile_size().h.max(1.);
                tile.render_expose(
                    ctx.r(),
                    Point::default(),
                    XrayPos::new(rect.loc, zoom_x),
                    focus_ring && expose.open && idx == expose.selected,
                    &mut |elem| {
                        // Tile elements were prepared at the source output's physical scale.
                        let elem = ScaleOverrideRenderElement::from_element(elem, tile.scale());
                        let elem = MonitorInnerRenderElement::ExposeAtSourceScale(elem);
                        let elem = RescaleRenderElement::from_element(
                            elem,
                            Point::default(),
                            Scale::from((
                                zoom_x * scale / tile.scale(),
                                zoom_y * scale / tile.scale(),
                            )),
                        );
                        push(RelocateRenderElement::from_element(
                            elem,
                            rect.loc.to_physical_precise_round(scale),
                            Relocate::Relative,
                        ));
                    },
                );
            }
        } else if let Some(mon) = self.monitor_for_output(output) {
            mon.render_workspaces(ctx, focus_ring, push);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Navigate the planned rows, not the intermediate positions during animation.
pub(super) fn neighbor(
    rects: &[Rectangle<f64, Logical>],
    selected: usize,
    direction: ExposeDirection,
) -> Option<usize> {
    let current = rects.get(selected)?;
    match direction {
        ExposeDirection::Left => selected
            .checked_sub(1)
            .filter(|&idx| rects[idx].loc.y == current.loc.y),
        ExposeDirection::Right => {
            let idx = selected + 1;
            rects
                .get(idx)
                .filter(|rect| rect.loc.y == current.loc.y)
                .map(|_| idx)
        }
        ExposeDirection::Up | ExposeDirection::Down => {
            let row_y = if direction == ExposeDirection::Up {
                rects[..selected]
                    .iter()
                    .rev()
                    .find(|rect| rect.loc.y < current.loc.y)?
                    .loc
                    .y
            } else {
                rects[selected + 1..]
                    .iter()
                    .find(|rect| rect.loc.y > current.loc.y)?
                    .loc
                    .y
            };
            let center_x = current.loc.x + current.size.w / 2.;
            rects
                .iter()
                .enumerate()
                .filter(|(_, rect)| rect.loc.y == row_y)
                .min_by(|(_, a), (_, b)| {
                    let distance = |rect: &Rectangle<f64, Logical>| {
                        (rect.loc.x + rect.size.w / 2. - center_x).abs()
                    };
                    distance(a).total_cmp(&distance(b))
                })
                .map(|(idx, _)| idx)
        }
    }
}

/// Pack windows in opening order, preserving their aspect ratios. Find the largest
/// common scale that fits, wrapping to the next row when necessary and centering
/// each row horizontally.
pub(super) fn arrange(
    sizes: &[Size<f64, Logical>],
    area: Rectangle<f64, Logical>,
    gap: f64,
) -> Vec<Rectangle<f64, Logical>> {
    if sizes.is_empty() {
        return Vec::new();
    }

    let gap = gap
        .min(area.size.w / (sizes.len() + 3) as f64)
        .min(area.size.w / 4.)
        .min(area.size.h / 4.)
        .max(0.);
    let available: Size<f64, Logical> = Size::from((
        (area.size.w - 2. * gap).max(1.),
        (area.size.h - 2. * gap).max(1.),
    ));
    let pack = |scale: f64| {
        let mut rects = Vec::with_capacity(sizes.len());
        let (mut x, mut y, mut row_height) = (0., 0., 0_f64);
        for size in sizes {
            let size = Size::from((size.w.max(1.) * scale, size.h.max(1.) * scale));
            if x > 0. && x + size.w > available.w {
                x = 0.;
                y += row_height + gap;
                row_height = 0.;
            }
            rects.push(Rectangle::new(Point::from((x, y)), size));
            x += size.w + gap;
            row_height = row_height.max(size.h);
        }
        (rects, y + row_height)
    };

    // Bound the search by the largest scale at which every individual window fits.
    let mut high = sizes.iter().fold(f64::INFINITY, |scale, size| {
        scale
            .min(available.w / size.w.max(1.))
            .min(available.h / size.h.max(1.))
    });
    let mut low = 0.;
    for _ in 0..48 {
        let mid = (low + high) / 2.;
        if pack(mid).1 <= available.h {
            low = mid;
        } else {
            high = mid;
        }
    }
    let (mut rects, height) = pack(low);
    for row in rects.chunk_by_mut(|a, b| a.loc.y == b.loc.y) {
        let last = row.last().unwrap();
        let width = last.loc.x + last.size.w;
        let offset = area.loc
            + Point::from((
                gap + (available.w - width) / 2.,
                gap + (available.h - height) / 2.,
            ));
        for rect in row {
            rect.loc += offset;
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_stays_in_rows_and_uses_nearest_center_in_adjacent_row() {
        use ExposeDirection::*;
        let rects = [
            Rectangle::new((0., 0.).into(), (80., 60.).into()),
            Rectangle::new((100., 0.).into(), (80., 40.).into()),
            Rectangle::new((200., 0.).into(), (80., 60.).into()),
            Rectangle::new((50., 100.).into(), (80., 60.).into()),
            Rectangle::new((150., 100.).into(), (80., 60.).into()),
            Rectangle::new((100., 200.).into(), (80., 60.).into()),
        ];
        assert_eq!(neighbor(&rects, 1, Left), Some(0));
        assert_eq!(neighbor(&rects, 1, Right), Some(2));
        assert_eq!(neighbor(&rects, 0, Left), None);
        assert_eq!(neighbor(&rects, 2, Right), None);
        assert_eq!(neighbor(&rects, 3, Left), None);
        assert_eq!(neighbor(&rects, 4, Right), None);
        assert_eq!(neighbor(&rects, 0, Down), Some(3));
        assert_eq!(neighbor(&rects, 2, Down), Some(4));
        assert_eq!(neighbor(&rects, 4, Up), Some(1));
        assert_eq!(neighbor(&rects, 4, Down), Some(5));
        assert_eq!(neighbor(&rects, 0, Up), None);
        assert_eq!(neighbor(&rects, 5, Down), None);
        assert_eq!(neighbor(&[], 0, Down), None);
    }

    #[test]
    fn windows_fit_without_overlap_and_keep_their_shape() {
        let area = Rectangle::from_size(Size::from((1920., 1080.)));
        for count in [1, 2, 5, 20, 100] {
            let sizes: Vec<_> = (0..count)
                .map(|i| Size::from((300. + (i % 5) as f64 * 200., 200. + (i % 3) as f64 * 300.)))
                .collect();
            let rects = arrange(&sizes, area, 16.);
            for row in rects.chunk_by(|a, b| a.loc.y == b.loc.y) {
                let first = row.first().unwrap();
                let last = row.last().unwrap();
                let left_margin = first.loc.x - area.loc.x;
                let right_margin = area.loc.x + area.size.w - last.loc.x - last.size.w;
                assert!((left_margin - right_margin).abs() < 1e-9);
            }
            for (i, rect) in rects.iter().enumerate() {
                assert!(area.contains_rect(*rect));
                assert!((rect.size.w / rect.size.h - sizes[i].w / sizes[i].h).abs() < 1e-9);
                for other in &rects[..i] {
                    assert!(!rect.overlaps(*other));
                }
                if i > 0 {
                    assert!(rect.loc.y >= rects[i - 1].loc.y);
                    if rect.loc.y == rects[i - 1].loc.y {
                        assert!(rect.loc.x > rects[i - 1].loc.x);
                    }
                }
            }
        }
    }
}
