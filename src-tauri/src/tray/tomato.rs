use std::f32::consts::PI;

use tiny_skia::{FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use super::{build_arc_path, rgba_color, TrayColors, CENTER, SIZE};

const RADIUS: f32 = 17.0;
const START: f32 = 0.22 * PI;
const SWEEP: f32 = 0.56 * PI;

/// Draw the tomato silhouette and a phase-colored progress arc around its base.
pub fn render_tray_icon_rgba(
    colors: &TrayColors,
    paused: bool,
    progress: f32,
    round_type: &str,
    countdown: bool,
) -> Vec<u8> {
    let mut pixmap = Pixmap::new(SIZE, SIZE).expect("tray icon pixmap");
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    let transform = Transform::identity();
    let mut fruit = PathBuilder::new();
    fruit.move_to(32.0, 19.0);
    fruit.cubic_to(15.0, 11.0, 5.0, 20.0, 5.0, 34.0);
    fruit.cubic_to(5.0, 49.0, 17.0, 58.0, 32.0, 58.0);
    fruit.cubic_to(47.0, 58.0, 59.0, 49.0, 59.0, 34.0);
    fruit.cubic_to(59.0, 20.0, 49.0, 11.0, 32.0, 19.0);
    fruit.close();
    paint.set_color_rgba8(231, 63, 57, 255);
    pixmap.fill_path(
        &fruit.finish().unwrap(),
        &paint,
        FillRule::Winding,
        transform,
        None,
    );

    let stroke = Stroke {
        width: 4.5,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };

    paint.set_color_rgba8(201, 49, 46, 255);
    let track = build_arc_path(CENTER, CENTER, RADIUS, START, START + SWEEP);
    pixmap.stroke_path(&track, &paint, &stroke, transform, None);

    paint.set_color(rgba_color(match round_type {
        "short-break" => colors.short_round,
        "long-break" => colors.long_round,
        _ => colors.focus_round,
    }));
    let fraction = if countdown { 1.0 - progress } else { progress };
    let sweep = fraction.clamp(0.0, 1.0) * SWEEP;
    if sweep > 0.001 {
        let arc = build_arc_path(CENTER, CENTER, RADIUS, START, START + sweep);
        pixmap.stroke_path(&arc, &paint, &stroke, transform, None);
    } else {
        // Keep the current phase visible at the empty end of either dial mode.
        let mut marker = PathBuilder::new();
        marker.push_circle(
            CENTER + RADIUS * START.cos(),
            CENTER + RADIUS * START.sin(),
            2.25,
        );
        pixmap.fill_path(
            &marker.finish().unwrap(),
            &paint,
            FillRule::Winding,
            transform,
            None,
        );
    }

    let mut highlight = PathBuilder::new();
    highlight.move_to(16.0, 29.0);
    highlight.cubic_to(17.0, 25.0, 20.0, 22.0, 25.0, 22.0);
    paint.set_color_rgba8(255, 145, 123, 255);
    let highlight_stroke = Stroke {
        width: 3.5,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &highlight.finish().unwrap(),
        &paint,
        &highlight_stroke,
        transform,
        None,
    );

    let mut leaves = PathBuilder::new();
    leaves.move_to(28.0, 15.0);
    leaves.quad_to(28.0, 8.0, 32.0, 4.0);
    leaves.quad_to(36.0, 3.0, 36.0, 7.0);
    leaves.quad_to(33.0, 11.0, 33.0, 15.0);
    leaves.quad_to(39.0, 11.0, 45.0, 15.0);
    leaves.quad_to(39.0, 16.0, 38.0, 21.0);
    leaves.line_to(32.0, 18.0);
    leaves.line_to(27.0, 21.0);
    leaves.quad_to(25.0, 17.0, 19.0, 15.0);
    leaves.quad_to(23.0, 12.0, 28.0, 15.0);
    leaves.close();
    paint.set_color_rgba8(107, 184, 77, 255);
    pixmap.fill_path(
        &leaves.finish().unwrap(),
        &paint,
        FillRule::Winding,
        transform,
        None,
    );

    if paused {
        paint.set_color_rgba8(255, 248, 238, 255);
        for x in [23.0, 35.0] {
            let bar = PathBuilder::from_rect(Rect::from_xywh(x, 28.0, 6.0, 17.0).unwrap());
            pixmap.fill_path(&bar, &paint, FillRule::Winding, transform, None);
        }
    }

    pixmap.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_tomato_has_a_red_fruit_green_calyx_and_transparent_corners() {
        let icon = render_tray_icon_rgba(&TrayColors::default(), false, 0.0, "work", false);
        let pixel = |x, y| {
            let offset = ((y * SIZE + x) * 4) as usize;
            &icon[offset..offset + 4]
        };
        assert_eq!(pixel(32, 39), &[231, 63, 57, 255]);
        assert_eq!(pixel(31, 12), &[107, 184, 77, 255]);
        for (x, y) in [(0, 0), (0, 63), (63, 0), (63, 63)] {
            assert_eq!(pixel(x, y), &[0, 0, 0, 0]);
        }
    }

    #[test]
    fn icon_shows_the_phase_even_when_the_progress_arc_is_empty() {
        let colors = TrayColors::default();
        let render = |phase| render_tray_icon_rgba(&colors, false, 0.0, phase, false);
        assert_ne!(render("work"), render("short-break"));
        assert_ne!(render("short-break"), render("long-break"));
    }

    #[test]
    fn countdown_progress_reverses_the_elapsed_arc() {
        let colors = TrayColors::default();
        assert_eq!(
            render_tray_icon_rgba(&colors, false, 0.75, "work", true),
            render_tray_icon_rgba(&colors, false, 0.25, "work", false)
        );
    }
}
