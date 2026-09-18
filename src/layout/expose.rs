//! Exposé geometry is computed once, before starting its transition.

use smithay::utils::{Logical, Point, Rectangle, Size};

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
