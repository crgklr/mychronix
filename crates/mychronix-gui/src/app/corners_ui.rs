//! The Corners tab: braking in each corner for the chosen lap against the best
//! lap, as RaceStudio's corner reports (braking from the highest speed to the
//! lowest), with the corners numbered on a map.

use super::*;
use crate::braking::{self, Zone};

/// Corners tab state.
#[derive(Default)]
pub(super) struct CornersState {
    pub hovered: Option<usize>,
}

impl App {
    pub(super) fn corners_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        self.lap_strip(ui, session, false);
        ui.add_space(12.0);

        let Some(best) = session.best_lap(MIN_LAP_MS).copied() else {
            card(ui, |ui| {
                ui.label(RichText::new("Corners need a complete lap with GPS.").color(SECONDARY));
            });
            return;
        };
        // The open lap, or the best when the whole session is shown.
        let lap = self.lap.and_then(|i| session.laps.get(i)).copied().unwrap_or(best);
        let (Some((reference, ref_profile)), Some((zones, profile))) = (braking::lap_zones(session, &best), braking::lap_zones(session, &lap)) else {
            card(ui, |ui| {
                ui.label(RichText::new("Corners need GPS for the lap.").color(SECONDARY));
            });
            return;
        };
        let matched = braking::match_zones(&reference, ref_profile.length(), &zones, profile.length());
        let is_best = lap == best;
        let lap_n = |l: &aim_xrk::Lap| session.laps.iter().position(|x| x == l).map(|i| i + 1).unwrap_or(0);

        ui.label(
            RichText::new(if is_best {
                format!("Braking in each corner of the best lap ({}), from the highest speed before the corner to the lowest in it. Pick another lap to compare it.", lap_n(&best))
            } else {
                format!("Lap {} against the best lap ({}): where and how hard the driver brakes in each corner. Brake point compares where braking starts.", lap_n(&lap), lap_n(&best))
            })
            .size(13.0)
            .color(SECONDARY),
        );
        ui.add_space(10.0);

        let gap = 12.0;
        let height = ui.available_height();
        let map_w = (ui.available_width() * 0.27).clamp(240.0, 480.0);
        let table_w = ui.available_width() - map_w - gap;
        let mut hovered = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(table_w, height), column, |ui| {
                ui.set_width(table_w);
                card(ui, |ui| {
                    egui::ScrollArea::both().id_salt("corners_table").auto_shrink([false, true]).show(ui, |ui| {
                        hovered = corners_table(ui, &reference, &ref_profile, &zones, &profile, &matched, is_best, self.corners.hovered);
                    });
                });
            });
            ui.allocate_ui_with_layout(vec2(map_w, height), column, |ui| {
                ui.set_width(map_w);
                corners_map(ui, session, &best, &reference, self.corners.hovered);
            });
        });
        self.corners.hovered = hovered;
    }
}

/// The table: one row per corner of the best lap. Returns the hovered row.
fn corners_table(
    ui: &mut Ui,
    reference: &[Zone],
    ref_profile: &crate::compare::DistanceProfile,
    zones: &[Zone],
    profile: &crate::compare::DistanceProfile,
    matched: &[Option<usize>],
    is_best: bool,
    highlighted: Option<usize>,
) -> Option<usize> {
    let speed = |kmh: f64| format!("{:.1}", crate::units::speed(kmh));
    let dist = |m: f64| format!("{:.0}", crate::units::distance(m));
    let (su, du) = (crate::units::speed_unit(), crate::units::distance_unit());
    // A difference against the best lap: green when better, gold when worse.
    let delta = |ui: &mut Ui, d: f64, text: String, better_when_higher: bool| {
        // Nothing to show when it rounds to zero ("-0", "+0.00", "0 ft later").
        if is_best || !text.chars().any(|c| c.is_ascii_digit() && c != '0') {
            return;
        }
        let better = (d > 0.0) == better_when_higher;
        ui.label(RichText::new(text).size(12.0).color(if better { theme::GOOD } else { theme::FAVORITE }));
    };
    // A difference with no better or worse to it (how hard, how long on the brakes).
    let neutral = |ui: &mut Ui, text: String| {
        if !is_best && text.chars().any(|c| c.is_ascii_digit() && c != '0') {
            ui.label(RichText::new(text).size(12.0).color(DIM));
        }
    };
    let mut hovered = None;
    let headers = [
        ("Corner", "Numbered from the start line, on the best lap"),
        ("Brake point", "Where braking starts: later than the best lap means braking deeper into the corner"),
        ("Entry", "Highest speed before the corner"),
        ("Corner speed", "Lowest speed in the corner"),
        ("Braking", "Distance from the highest speed to the lowest"),
        ("Avg decel", "Average deceleration from the highest speed to the lowest (RaceStudio's measure; includes lifting)"),
        ("Peak", "Hardest deceleration in the corner (GPS)"),
        ("On brakes", "Time actually braking (harder than −0.15 g, RaceStudio's threshold): the rest of the slowing is lifting or scrubbing speed"),
        ("Time", "Time from the highest speed to the lowest"),
    ];
    egui::Grid::new("corners_grid").spacing([14.0, 10.0]).min_col_width(52.0).show(ui, |ui| {
        for (h, tip) in headers {
            ui.label(RichText::new(h).font(medium(13.0)).color(DIM)).on_hover_text(tip);
        }
        ui.end_row();
        let scale = ref_profile.length() / profile.length();
        for (k, r) in reference.iter().enumerate() {
            let z = matched[k].map(|i| &zones[i]);
            let strong = highlighted == Some(k);
            let text = |s: String| RichText::new(s).size(14.0).color(if strong { TEXT } else { SECONDARY });
            let row_top = ui.cursor().top();
            ui.label(RichText::new(format!("{}", k + 1)).font(semibold(15.0)).color(TEXT));
            match z {
                None => {
                    ui.label(RichText::new("No braking found here on this lap").size(13.0).color(DIM));
                    for _ in 0..7 {
                        ui.label("");
                    }
                }
                Some(z) => {
                    // Brake point: meters into the lap on the best lap's scale, against the best.
                    let point = z.start_m * scale;
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{} {du}", dist(point))));
                        let d = point - r.start_m;
                        let words = if d > 0.0 { "later" } else { "earlier" };
                        delta(ui, d, format!("{} {du} {words}", dist(d.abs())), true);
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{} {su}", speed(z.entry_kmh))));
                        let d = crate::units::speed(z.entry_kmh) - crate::units::speed(r.entry_kmh);
                        delta(ui, d, format!("{d:+.1}"), true);
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{} {su}", speed(z.min_kmh))));
                        let d = crate::units::speed(z.min_kmh) - crate::units::speed(r.min_kmh);
                        delta(ui, d, format!("{d:+.1}"), true);
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{} {du}", dist(z.distance_m()))));
                        let d = crate::units::distance(z.distance_m() * scale) - crate::units::distance(r.distance_m());
                        delta(ui, d, format!("{d:+.0}"), false);
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{:.2} g", z.decel_g())));
                        let d = z.decel_g() - r.decel_g();
                        delta(ui, d, format!("{d:+.2}"), true);
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{:.2} g", z.peak_g)));
                        let d = z.peak_g - r.peak_g;
                        neutral(ui, format!("{d:+.2}"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{:.2} s", z.brake_s)));
                        let d = z.brake_s - r.brake_s;
                        neutral(ui, format!("{d:+.2}"));
                    });
                    ui.horizontal(|ui| {
                        ui.label(text(format!("{:.2} s", z.time_s())));
                        let d = z.time_s() - r.time_s();
                        delta(ui, d, format!("{d:+.2}"), false);
                    });
                }
            }
            ui.end_row();
            let row = Rect::from_min_max(pos2(ui.min_rect().left(), row_top), pos2(ui.min_rect().right(), ui.cursor().top()));
            if ui.rect_contains_pointer(row) {
                hovered = Some(k);
            }
        }
    });
    if reference.is_empty() {
        ui.label(RichText::new("No corners found: speed never drops enough to brake for one.").color(SECONDARY));
    }
    hovered
}

/// The best lap's line with each braking zone drawn heavier and its corner number at the apex.
fn corners_map(ui: &mut Ui, session: &Session, best: &aim_xrk::Lap, zones: &[Zone], highlighted: Option<usize>) {
    let Some(gps) = &session.gps else { return };
    let lo = gps.times_ms.partition_point(|&t| t < best.start_ms);
    let hi = gps.times_ms.partition_point(|&t| t <= best.end_ms);
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height() - 24.0);
            ui.horizontal(|ui| {
                ui.add(Icon::MapPin.image(DIM, 16.0));
                ui.label(RichText::new("Corners").font(medium(14.5)).color(TEXT));
                ui.label(RichText::new("on the best lap  ·  braking in red").size(12.5).color(DIM));
            });
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), (ui.available_height() - 8.0).max(120.0)), Sense::hover());
            if hi <= lo + 1 {
                return;
            }
            let (lat0, lon0) = (gps.latitude[lo], gps.longitude[lo]);
            let kx = 111_320.0 * lat0.to_radians().cos();
            let xy: Vec<(f64, f64)> = (lo..hi).map(|i| ((gps.longitude[i] - lon0) * kx, (gps.latitude[i] - lat0) * 110_540.0)).collect();
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for &(x, y) in &xy {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
            let area = rect.shrink(20.0);
            let s = (area.width() as f64 / (x1 - x0).max(1.0)).min(area.height() as f64 / (y1 - y0).max(1.0));
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            let pts: Vec<egui::Pos2> = xy.iter().map(|&(x, y)| pos2(area.center().x + ((x - cx) * s) as f32, area.center().y - ((y - cy) * s) as f32)).collect();
            let painter = ui.painter_at(rect);
            const BRAKE: Color32 = Color32::from_rgb(0xe6, 0x67, 0x67);
            let zone_of = |t: i64| zones.iter().position(|z| t >= z.start_ms && t <= z.apex_ms);
            for k in 0..pts.len() - 1 {
                let t = gps.times_ms[lo + k];
                let (color, width) = match zone_of(t) {
                    Some(z) if highlighted.is_none() || highlighted == Some(z) => (BRAKE, 5.0),
                    Some(_) => (BRAKE.gamma_multiply(0.35), 4.0),
                    None => (GRID.gamma_multiply(2.5), 3.0),
                };
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(width, color));
            }
            // Corner numbers at the apexes.
            for (n, z) in zones.iter().enumerate() {
                let k = gps.times_ms[lo..hi].partition_point(|&t| t < z.apex_ms).min(pts.len() - 1);
                let strong = highlighted.is_none_or(|h| h == n);
                let p = pts[k];
                painter.circle_filled(p, 11.0, if strong { TEXT } else { SURFACE_ACTIVE });
                painter.text(p, egui::Align2::CENTER_CENTER, format!("{}", n + 1), semibold(12.0), if strong { ON_TEXT } else { SECONDARY });
            }
        });
}
