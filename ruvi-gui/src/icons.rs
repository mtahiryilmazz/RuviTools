use std::f32::consts::TAU;

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2, epaint::PathShape};

use crate::app::{ActiveTab, StudioTheme};

/// Draws a crisp geometric vector icon for any `ActiveTab` inside `rect`.
pub fn draw_tab_icon(painter: &egui::Painter, rect: Rect, tab: ActiveTab, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s * 0.105).clamp(1.3, 2.0), color);

    match tab {
        ActiveTab::Downloader => {
            // Vertical shaft + downward chevron + bottom U-tray
            let top = c.y - s * 0.34;
            let tip_y = c.y + s * 0.12;
            painter.line_segment([Pos2::new(c.x, top), Pos2::new(c.x, tip_y)], stroke);

            let arm = s * 0.22;
            painter.line_segment(
                [
                    Pos2::new(c.x - arm, tip_y - arm * 0.85),
                    Pos2::new(c.x, tip_y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + arm, tip_y - arm * 0.85),
                    Pos2::new(c.x, tip_y),
                ],
                stroke,
            );

            let tray_left = c.x - s * 0.34;
            let tray_right = c.x + s * 0.34;
            let tray_top = c.y + s * 0.16;
            let tray_bot = c.y + s * 0.34;
            painter.add(PathShape::line(
                vec![
                    Pos2::new(tray_left, tray_top),
                    Pos2::new(tray_left, tray_bot),
                    Pos2::new(tray_right, tray_bot),
                    Pos2::new(tray_right, tray_top),
                ],
                stroke,
            ));
        }
        ActiveTab::MediaStudio => {
            // Clapperboard / media frame + centered play triangle
            let frame = Rect::from_center_size(c, Vec2::new(s * 0.76, s * 0.68));
            painter.rect_stroke(frame, s * 0.1, stroke, egui::StrokeKind::Middle);

            let header_y = frame.top() + frame.height() * 0.28;
            painter.line_segment(
                [
                    Pos2::new(frame.left(), header_y),
                    Pos2::new(frame.right(), header_y),
                ],
                stroke,
            );
            // Two diagonal clapper stripes in the top bar
            for frac in [0.30_f32, 0.62_f32] {
                let x = frame.left() + frame.width() * frac;
                painter.line_segment(
                    [
                        Pos2::new(x - s * 0.05, frame.top()),
                        Pos2::new(x + s * 0.04, header_y),
                    ],
                    stroke,
                );
            }
            // Centered play triangle below header bar
            let play_center = Pos2::new(c.x + s * 0.02, (header_y + frame.bottom()) * 0.5);
            let r = s * 0.14;
            painter.add(PathShape::convex_polygon(
                vec![
                    Pos2::new(play_center.x - r * 0.65, play_center.y - r * 0.85),
                    Pos2::new(play_center.x + r * 0.95, play_center.y),
                    Pos2::new(play_center.x - r * 0.65, play_center.y + r * 0.85),
                ],
                color,
                Stroke::NONE,
            ));
        }
        ActiveTab::PlaylistQueue => {
            // Small play indicator + 3 stacked queue bars
            let left = c.x - s * 0.36;
            let right = c.x + s * 0.36;
            let play_x = left + s * 0.04;
            let y1 = c.y - s * 0.24;
            let y2 = c.y;
            let y3 = c.y + s * 0.24;

            painter.add(PathShape::convex_polygon(
                vec![
                    Pos2::new(play_x, y1 - s * 0.09),
                    Pos2::new(play_x + s * 0.14, y1),
                    Pos2::new(play_x, y1 + s * 0.09),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(left + s * 0.24, y1), Pos2::new(right, y1)],
                stroke,
            );
            painter.line_segment([Pos2::new(left, y2), Pos2::new(right, y2)], stroke);
            painter.line_segment(
                [Pos2::new(left, y3), Pos2::new(right - s * 0.12, y3)],
                stroke,
            );
        }
        ActiveTab::Telemetry => {
            // Baseline + 3 rising telemetry bars
            let base_y = c.y + s * 0.32;
            let left = c.x - s * 0.36;
            let right = c.x + s * 0.36;
            painter.line_segment([Pos2::new(left, base_y), Pos2::new(right, base_y)], stroke);

            let bar_w = s * 0.16;
            let heights = [s * 0.28, s * 0.56, s * 0.42];
            let centers = [c.x - s * 0.23, c.x, c.x + s * 0.23];
            for (bx, bh) in centers.into_iter().zip(heights) {
                let bar_rect = Rect::from_min_max(
                    Pos2::new(bx - bar_w * 0.5, base_y - bh),
                    Pos2::new(bx + bar_w * 0.5, base_y - s * 0.04),
                );
                painter.rect_filled(bar_rect, s * 0.04, color);
            }
        }
        ActiveTab::RuviTube => {
            // Puzzle / extension piece outline with top & right connector tabs
            let x0 = c.x - s * 0.30;
            let x1 = c.x + s * 0.20;
            let y0 = c.y - s * 0.20;
            let y1 = c.y + s * 0.30;
            let tab = s * 0.14;
            let mid_x = (x0 + x1) * 0.5;
            let mid_y = (y0 + y1) * 0.5;

            let pts = vec![
                Pos2::new(x0, y0),
                Pos2::new(mid_x - tab * 0.55, y0),
                Pos2::new(mid_x - tab * 0.70, y0 - tab),
                Pos2::new(mid_x + tab * 0.70, y0 - tab),
                Pos2::new(mid_x + tab * 0.55, y0),
                Pos2::new(x1, y0),
                Pos2::new(x1, mid_y - tab * 0.55),
                Pos2::new(x1 + tab, mid_y - tab * 0.70),
                Pos2::new(x1 + tab, mid_y + tab * 0.70),
                Pos2::new(x1, mid_y + tab * 0.55),
                Pos2::new(x1, y1),
                Pos2::new(x0, y1),
            ];
            painter.add(PathShape::closed_line(pts, stroke));
        }
        ActiveTab::Settings => {
            // Precision 8-tooth gear
            let outer_r = s * 0.37;
            let inner_r = s * 0.25;
            let hole_r = s * 0.12;
            painter.circle_stroke(c, inner_r, stroke);
            painter.circle_stroke(c, hole_r, stroke);
            let tooth_stroke = Stroke::new((s * 0.13).clamp(1.6, 2.5), color);
            for i in 0..8 {
                let angle = (i as f32) * (TAU / 8.0);
                let dir = Vec2::new(angle.cos(), angle.sin());
                painter.line_segment(
                    [c + dir * (inner_r + s * 0.01), c + dir * outer_r],
                    tooth_stroke,
                );
            }
        }
    }
}

/// Draws a crisp Sun (Light) or Moon crescent (Dark) vector icon.
pub fn draw_theme_icon(painter: &egui::Painter, rect: Rect, theme: StudioTheme, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s * 0.105).clamp(1.3, 2.0), color);

    match theme {
        StudioTheme::Dark => {
            // Crescent moon built as a closed outline (background-independent)
            let mut pts = Vec::with_capacity(20);
            let r_outer = s * 0.34;
            for i in 0..=10 {
                let t = -0.35 * std::f32::consts::PI
                    + (i as f32 / 10.0) * (1.28 * std::f32::consts::PI);
                pts.push(Pos2::new(c.x + r_outer * t.cos(), c.y + r_outer * t.sin()));
            }
            let r_inner = s * 0.26;
            let inner_c = Pos2::new(c.x + s * 0.15, c.y - s * 0.11);
            for i in (0..=8).rev() {
                let t =
                    0.05 * std::f32::consts::PI + (i as f32 / 8.0) * (0.90 * std::f32::consts::PI);
                pts.push(Pos2::new(
                    inner_c.x + r_inner * t.cos(),
                    inner_c.y + r_inner * t.sin(),
                ));
            }
            painter.add(PathShape::closed_line(pts, stroke));
        }
        StudioTheme::Light => {
            let core_r = s * 0.19;
            painter.circle_stroke(c, core_r, stroke);
            let ray_in = s * 0.28;
            let ray_out = s * 0.39;
            for i in 0..8 {
                let angle = (i as f32) * (TAU / 8.0);
                let dir = Vec2::new(angle.cos(), angle.sin());
                painter.line_segment([c + dir * ray_in, c + dir * ray_out], stroke);
            }
        }
    }
}

/// Draws a crisp warning triangle with exclamation mark.
pub fn draw_warning_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s * 0.105).clamp(1.3, 2.0), color);

    let top = Pos2::new(c.x, c.y - s * 0.34);
    let left = Pos2::new(c.x - s * 0.36, c.y + s * 0.30);
    let right = Pos2::new(c.x + s * 0.36, c.y + s * 0.30);
    painter.add(PathShape::closed_line(vec![top, right, left], stroke));
    painter.line_segment(
        [
            Pos2::new(c.x, c.y - s * 0.11),
            Pos2::new(c.x, c.y + s * 0.09),
        ],
        stroke,
    );
    painter.circle_filled(Pos2::new(c.x, c.y + s * 0.19), (s * 0.065).max(1.0), color);
}

/// Draws a crisp check mark inside `rect`.
pub fn draw_check_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s * 0.12).clamp(1.4, 2.2), color);
    painter.add(PathShape::line(
        vec![
            Pos2::new(c.x - s * 0.30, c.y + s * 0.02),
            Pos2::new(c.x - s * 0.07, c.y + s * 0.26),
            Pos2::new(c.x + s * 0.33, c.y - s * 0.24),
        ],
        stroke,
    ));
}

/// Draws a crisp cross mark inside `rect`.
pub fn draw_cross_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = Stroke::new((s * 0.12).clamp(1.4, 2.2), color);
    let d = s * 0.26;
    painter.line_segment(
        [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)],
        stroke,
    );
}

/// Draws a crisp status dot.
pub fn draw_status_dot(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    painter.circle_filled(center, radius, color);
}
