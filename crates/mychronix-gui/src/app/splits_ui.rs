//! The Splits tab: RaceStudio's Split Times Report, with split details and a
//! map of the selected split.

use super::*;
use crate::split_report::{self, Row, Values};
use aim_track::LatLon;

/// Splits tab state for the open session.
#[derive(Default)]
pub(super) struct SplitsState {
    pub count: usize,
    pub values: Values,
    pub cumulative: bool,
    pub color: bool,
    pub selected: Option<usize>,
    pub excluded: std::collections::BTreeSet<i32>,
    pub hovered_row: Option<usize>,
    computed: Option<Computed>,
}

/// Split times worked out for one session and split count.
struct Computed {
    path: PathBuf,
    count: usize,
    /// The best lap's line, which the splits sit on.
    reference: Vec<LatLon>,
    /// Index into `reference` where each split after the first begins.
    boundaries: Vec<usize>,
    /// Length of each split, m.
    lengths: Vec<f64>,
    rows: Vec<Row>,
    /// Per row: logger times at the start, each split line and the finish.
    crossings: Vec<Option<Vec<f64>>>,
}

fn compute(path: &std::path::Path, session: &Session, count: usize) -> Option<Computed> {
    let gps = session.gps.as_ref()?;
    let best = session.best_lap(MIN_LAP_MS)?;
    let lo = gps.times_ms.partition_point(|&t| t < best.start_ms);
    let hi = gps.times_ms.partition_point(|&t| t <= best.end_ms);
    let reference: Vec<LatLon> = (lo..hi).map(|i| LatLon { lat: gps.latitude[i], lon: gps.longitude[i] }).collect();
    let sectors = splits::Sectors::even(&reference, reference.first().copied(), count)?;
    let points: Vec<LatLon> = gps.latitude.iter().zip(&gps.longitude).map(|(&lat, &lon)| LatLon { lat, lon }).collect();
    let split_points = sectors.split_points();
    let mut cum = vec![0.0];
    for w in sectors.path.windows(2) {
        cum.push(cum.last().unwrap() + w[0].distance_m(&w[1]));
    }
    let mut edges = vec![0usize];
    edges.extend(sectors.boundaries.iter().copied());
    edges.push(sectors.path.len() - 1);
    let lengths = edges.windows(2).map(|w| cum[w[1]] - cum[w[0]]).collect();
    let mut rows = Vec::new();
    let mut crossings = Vec::new();
    for lap in &session.laps {
        let times = splits::sector_times(&gps.times_ms, &points, lap.start_ms, lap.end_ms, &split_points);
        crossings.push(times.as_ref().map(|t| {
            let mut c = vec![lap.start_ms as f64];
            for d in t {
                c.push(c.last().unwrap() + *d as f64);
            }
            c
        }));
        rows.push(Row { lap_number: lap.number, lap_ms: lap.duration_ms(), splits: times });
    }
    Some(Computed { path: path.to_path_buf(), count, reference: sectors.path.clone(), boundaries: sectors.boundaries.clone(), lengths, rows, crossings })
}

/// Green (fast) through amber to red (slow).
fn rank_color(t: f32) -> Color32 {
    const RED: Color32 = Color32::from_rgb(0xe6, 0x67, 0x67);
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { mix(theme::GOOD, theme::FAVORITE, t * 2.0) } else { mix(theme::FAVORITE, RED, (t - 0.5) * 2.0) }
}

fn seconds(ms: i64) -> String {
    if ms.abs() >= 60_000 { format_lap_time(ms) } else { format!("{:.3}", ms as f64 / 1000.0) }
}

impl App {
    pub(super) fn splits_view(&mut self, ui: &mut Ui, session: &Session) {
        let Some(path) = self.path.clone() else { return };
        let st = &mut self.splits;
        if st.count < 2 {
            st.count = 4;
        }
        let fresh = st.computed.as_ref().is_none_or(|c| c.path != path || c.count != st.count);
        if fresh {
            if st.computed.as_ref().is_none_or(|c| c.path != path) {
                // A new session: out, in and slow laps start unticked; nothing selected.
                let slow = slow_laps(session);
                st.excluded = session.laps.iter().zip(&slow).filter(|(_, s)| **s).map(|(l, _)| l.number).collect();
                st.selected = None;
            }
            st.computed = compute(&path, session, st.count);
        }

        // Toolbar.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(RichText::new("Splits").size(12.5).color(DIM));
            for n in 2..=8 {
                if chip(ui, &n.to_string(), st.count == n).clicked() {
                    st.count = n;
                    st.selected = None;
                }
            }
            ui.add_space(12.0);
            ui.label(RichText::new("Show").size(12.5).color(DIM));
            for (v, label, tip) in [
                (Values::Times, "Times", "Each split's time"),
                (Values::VsBestSplit, "vs best split", "Against the best time for that split this session"),
                (Values::VsBestLap, "vs best lap", "Against the same split of the best lap"),
            ] {
                if chip(ui, label, st.values == v).on_hover_text(tip).clicked() {
                    st.values = v;
                }
            }
            ui.add_space(12.0);
            if chip(ui, "Each split", !st.cumulative).on_hover_text("Fractional: each split on its own").clicked() {
                st.cumulative = false;
            }
            if chip(ui, "Running total", st.cumulative).on_hover_text("Cumulative: added up from the start of the lap").clicked() {
                st.cumulative = true;
            }
            ui.add_space(12.0);
            if chip(ui, "Color", st.color).on_hover_text("Shade each split from fastest (green) to slowest (red)").clicked() {
                st.color = !st.color;
            }
        });
        ui.add_space(12.0);

        let Some(c) = &st.computed else {
            card(ui, |ui| {
                ui.label(RichText::new("Splits need GPS and a complete lap to place them on.").color(SECONDARY));
            });
            return;
        };
        let n = c.count;
        let included: Vec<bool> = c.rows.iter().map(|r| !st.excluded.contains(&r.lap_number)).collect();
        let inc_rows: Vec<&Row> = c.rows.iter().zip(&included).filter(|(_, on)| **on).map(|(r, _)| r).collect();
        let best = split_report::best_splits(&inc_rows, n);
        let best_lap_row = inc_rows.iter().filter(|r| r.splits.is_some()).min_by_key(|r| r.lap_ms).copied();
        let best_lap_splits = best_lap_row.and_then(|r| r.splits.clone());
        let theoretical = split_report::best_theoretical(&best);
        let rolling = split_report::best_rolling(&c.rows, &included, n);
        let rolling_cells: std::collections::HashSet<(usize, usize)> = rolling
            .map(|(_, ri, k)| {
                let mut cells = std::collections::HashSet::new();
                let (mut r, mut s) = (ri, k);
                for _ in 0..n {
                    cells.insert((r, s));
                    s += 1;
                    if s == n {
                        s = 0;
                        r += 1;
                    }
                }
                cells
            })
            .unwrap_or_default();

        let gap = 14.0;
        let height = ui.available_height();
        let detail_w = (ui.available_width() * 0.38).clamp(320.0, 560.0);
        let table_w = ui.available_width() - detail_w - gap;
        let mut toggle = None;
        let mut select = None;
        let mut hovered_row = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(table_w, height), column, |ui| {
                ui.set_width(table_w);
                card(ui, |ui| {
                    // The header stays put while the laps scroll; both scroll sideways together.
                    egui::ScrollArea::horizontal().id_salt("splits_table_x").auto_shrink([false, true]).show(ui, |ui| {
                        let lap_w = 112.0;
                        let cell_w = 86.0;
                        let row_h = 26.0;
                        // Header: split number and length; click to see it in detail.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.allocate_exact_size(vec2(lap_w, 34.0), Sense::hover());
                            let (r, _) = ui.allocate_exact_size(vec2(cell_w, 34.0), Sense::hover());
                            ui.painter().text(r.left_center(), egui::Align2::LEFT_CENTER, "Lap time", medium(13.0), SECONDARY);
                            for k in 0..n {
                                let (r, resp) = ui.allocate_exact_size(vec2(cell_w, 34.0), Sense::click());
                                let on = st.selected == Some(k);
                                if on || resp.hovered() {
                                    ui.painter().rect_filled(r, CornerRadius::same(theme::RADIUS_SMALL), if on { SURFACE_ACTIVE } else { SURFACE_HOVER });
                                }
                                ui.painter().text(r.left_top() + vec2(6.0, 9.0), egui::Align2::LEFT_CENTER, format!("Split {}", k + 1), medium(13.0), if on { TEXT } else { SECONDARY });
                                ui.painter().text(r.left_top() + vec2(6.0, 25.0), egui::Align2::LEFT_CENTER, format!("{:.0} m", c.lengths[k]), regular(11.5), DIM);
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.on_hover_text("Show this split in detail").clicked() {
                                    select = Some(if on { None } else { Some(k) });
                                }
                            }
                        });
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().id_salt("splits_table").max_height(height - 70.0 - 38.0).auto_shrink([false, true]).show(ui, |ui| {
                        // Columns' ranges, for coloring.
                        let ranges: Vec<Option<(i64, i64)>> = (0..n)
                            .map(|k| {
                                let v: Vec<i64> = inc_rows.iter().filter_map(|r| r.splits.as_ref().map(|s| s[k])).collect();
                                Some((*v.iter().min()?, *v.iter().max()?))
                            })
                            .collect();
                        for (ri, r) in c.rows.iter().enumerate() {
                            let on = included[ri];
                            let resp = ui
                                .horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let (lr, _) = ui.allocate_exact_size(vec2(lap_w, row_h), Sense::hover());
                                    let cb = Rect::from_center_size(pos2(lr.left() + 10.0, lr.center().y), Vec2::splat(16.0));
                                    let cbr = ui.interact(cb, ui.id().with(("split_cb", ri)), Sense::click());
                                    ui.painter().rect_stroke(cb, CornerRadius::same(4), Stroke::new(1.3, if on { TEXT } else { DIM }), egui::StrokeKind::Inside);
                                    if on {
                                        Icon::Check.image(TEXT, 12.0).paint_at(ui, cb.shrink(2.0));
                                    }
                                    if cbr.on_hover_text(if on { "Leave out" } else { "Include" }).clicked() {
                                        toggle = Some(r.lap_number);
                                    }
                                    ui.painter().text(pos2(lr.left() + 28.0, lr.center().y), egui::Align2::LEFT_CENTER, format!("Lap {}", r.lap_number), medium(13.5), if on { TEXT } else { DIM });
                                    let is_best = best_lap_row.is_some_and(|b| b.lap_number == r.lap_number);
                                    let (tr, _) = ui.allocate_exact_size(vec2(cell_w, row_h), Sense::hover());
                                    ui.painter().text(tr.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, format_lap_time(r.lap_ms), regular(13.5), if !on { DIM } else if is_best { theme::GOOD } else { TEXT });
                                    for k in 0..n {
                                        let (cr, _) = ui.allocate_exact_size(vec2(cell_w, row_h), Sense::hover());
                                        let Some(s) = &r.splits else {
                                            ui.painter().text(cr.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, "—", regular(13.5), DIM);
                                            continue;
                                        };
                                        let v = split_report::cell(s, k, st.values, st.cumulative, &best, best_lap_splits.as_deref());
                                        if st.color && on {
                                            if let Some((lo, hi)) = ranges[k] {
                                                let t = if hi > lo { (s[k] - lo) as f32 / (hi - lo) as f32 } else { 0.0 };
                                                ui.painter().rect_filled(cr, CornerRadius::same(4), rank_color(t).gamma_multiply(0.28));
                                            }
                                        }
                                        if rolling_cells.contains(&(ri, k)) && on {
                                            ui.painter().rect_stroke(cr, CornerRadius::same(4), Stroke::new(1.5, theme::FAVORITE), egui::StrokeKind::Inside);
                                        }
                                        let is_best_split = on && best[k] == Some(s[k]);
                                        let text = match (st.values, v) {
                                            (_, None) => "—".to_string(),
                                            (Values::Times, Some(v)) => seconds(v),
                                            (_, Some(v)) => format_delta(v),
                                        };
                                        let color = if !on { DIM } else if is_best_split && !st.cumulative { theme::GOOD } else { TEXT };
                                        let font = if st.hovered_row == Some(ri) { semibold(13.5) } else { regular(13.5) };
                                        ui.painter().text(cr.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, text, font, color);
                                    }
                                })
                                .response;
                            if resp.hovered() {
                                hovered_row = Some(ri);
                            }
                        }
                        // Statistics of the split times over the ticked laps.
                        ui.add_space(8.0);
                        hairline(ui);
                        ui.add_space(6.0);
                        let col = |k: usize| inc_rows.iter().filter_map(|r| r.splits.as_ref().map(|s| s[k])).collect::<Vec<i64>>();
                        let laps_col: Vec<i64> = inc_rows.iter().filter(|r| r.splits.is_some()).map(|r| r.lap_ms).collect();
                        let stat_rows: [(&str, Box<dyn Fn(&[i64]) -> Option<String>>); 5] = [
                            ("Best", Box::new(|v: &[i64]| v.iter().min().map(|&x| seconds(x)))),
                            ("Average", Box::new(|v: &[i64]| split_report::stats(v).map(|s| seconds(s.0.round() as i64)))),
                            ("Median", Box::new(|v: &[i64]| split_report::stats(v).map(|s| seconds(s.1.round() as i64)))),
                            ("Slowest", Box::new(|v: &[i64]| split_report::stats(v).map(|s| seconds(s.2)))),
                            ("Std dev", Box::new(|v: &[i64]| split_report::stats(v).map(|s| format!("{:.3}", s.3 / 1000.0)))),
                        ];
                        for (name, f) in &stat_rows {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let (lr, _) = ui.allocate_exact_size(vec2(lap_w, 22.0), Sense::hover());
                                ui.painter().text(lr.left_center() + vec2(28.0, 0.0), egui::Align2::LEFT_CENTER, *name, medium(12.5), SECONDARY);
                                for k in 0..=n {
                                    let v = if k == 0 { f(&laps_col) } else { f(&col(k - 1)) };
                                    let (cr, _) = ui.allocate_exact_size(vec2(cell_w, 22.0), Sense::hover());
                                    let text = match (k, v) {
                                        (0, Some(t)) if !t.contains(':') && *name != "Std dev" => t,
                                        (_, Some(t)) => t,
                                        _ => "—".into(),
                                    };
                                    ui.painter().text(cr.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, text, regular(12.5), SECONDARY);
                                }
                            });
                        }
                        });
                    });
                    ui.add_space(8.0);
                    // The two laps you could have done.
                    ui.horizontal_wrapped(|ui| {
                        if let Some(t) = theoretical {
                            ui.label(RichText::new("Best theoretical").size(12.5).color(DIM));
                            ui.label(RichText::new(format_lap_time(t)).font(semibold(14.0)).color(theme::GOOD))
                                .on_hover_text("Your best time in every split added together");
                            ui.add_space(14.0);
                        }
                        if let Some((t, ri, k)) = rolling {
                            ui.label(RichText::new("Best rolling").size(12.5).color(DIM));
                            ui.label(RichText::new(format_lap_time(t)).font(semibold(14.0)).color(theme::FAVORITE)).on_hover_text(format!(
                                "The fastest lap's worth of splits actually driven in a row, starting at lap {} split {} (outlined)",
                                c.rows[ri].lap_number,
                                k + 1
                            ));
                            ui.add_space(14.0);
                        }
                        ui.label(RichText::new(format!("Statistics over {} ticked laps.", inc_rows.len())).size(12.0).color(DIM));
                    });
                });
            });
            ui.allocate_ui_with_layout(vec2(detail_w, height), column, |ui| {
                ui.set_width(detail_w);
                egui::ScrollArea::vertical().id_salt("split_detail").auto_shrink([false, false]).show(ui, |ui| {
                    split_detail(ui, session, c, &included, st.selected, st.hovered_row, height);
                });
            });
        });
        st.hovered_row = hovered_row;
        if let Some(sel) = select {
            st.selected = sel;
        }
        if let Some(lap) = toggle {
            if !st.excluded.remove(&lap) {
                st.excluded.insert(lap);
            }
        }
    }
}

/// The map of the splits, and with a split selected: speed through it, its
/// time and driven distance by lap, and time against distance. `height` is
/// the column's: the map alone fills it.
#[allow(clippy::too_many_arguments)]
fn split_detail(ui: &mut Ui, session: &Session, c: &Computed, included: &[bool], selected: Option<usize>, hovered_row: Option<usize>, height: f32) {
    let Some(gps) = session.gps.as_ref() else { return };
    let top = ui.cursor().top();
    // Laps in the selected split, ranked by their time there (for color).
    let in_split: Vec<(usize, f64, f64, i64)> = match selected {
        Some(k) => c
            .crossings
            .iter()
            .enumerate()
            .filter(|(ri, _)| included[*ri])
            .filter_map(|(ri, x)| x.as_ref().map(|x| (ri, x[k], x[k + 1], (x[k + 1] - x[k]).round() as i64)))
            .collect(),
        None => Vec::new(),
    };
    let (lo_t, hi_t) = in_split.iter().fold((i64::MAX, i64::MIN), |(a, b), x| (a.min(x.3), b.max(x.3)));
    let rank = |t: i64| if hi_t > lo_t { (t - lo_t) as f32 / (hi_t - lo_t) as f32 } else { 0.0 };

    // Map.
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(Icon::MapPin.image(DIM, 16.0));
            let title = match selected {
                Some(k) => format!("Split {} on the track", k + 1),
                None => "Splits on the track".to_string(),
            };
            ui.label(RichText::new(title).font(medium(14.5)).color(TEXT));
        });
        let note = if selected.is_some() { "Each ticked lap through the split, fastest green to slowest red" } else { "Click a split's heading to see it in detail" };
        ui.label(RichText::new(note).size(12.5).color(SECONDARY));
        ui.add_space(14.0);
        // On its own the map takes the column's height (less the card's margin);
        // with a split's details under it, a fixed height.
        let map_h = if selected.is_none() { (top + height - 20.0 - ui.cursor().top()).max(300.0) } else { 380.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), map_h), Sense::hover());
        let r0 = c.reference[0];
        let kx = 111_320.0 * r0.lat.to_radians().cos();
        let xy = |p: &LatLon| ((p.lon - r0.lon) * kx, (p.lat - r0.lat) * 110_540.0);
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in &c.reference {
            let (x, y) = xy(p);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        let area = rect.shrink(16.0);
        let s = (area.width() as f64 / (x1 - x0).max(1.0)).min(area.height() as f64 / (y1 - y0).max(1.0));
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let to_screen = |p: &LatLon| {
            let (x, y) = xy(p);
            pos2(area.center().x + ((x - cx) * s) as f32, area.center().y - ((y - cy) * s) as f32)
        };
        let painter = ui.painter_at(rect);
        let pts: Vec<egui::Pos2> = c.reference.iter().map(to_screen).collect();
        let mut edges = vec![0usize];
        edges.extend(c.boundaries.iter().copied());
        edges.push(pts.len() - 1);
        for (k, w) in edges.windows(2).enumerate() {
            let color = if selected.is_some() { theme::OUTLINE } else { SERIES[k % SERIES.len()] };
            painter.add(egui::Shape::line(pts[w[0]..=w[1]].to_vec(), Stroke::new(if selected.is_some() { 3.0 } else { 4.0 }, color)));
        }
        // Each lap's line through the selected split.
        for &(ri, a, b, t) in in_split.iter().rev() {
            let lo = gps.times_ms.partition_point(|&x| (x as f64) < a);
            let hi = gps.times_ms.partition_point(|&x| (x as f64) <= b);
            let line: Vec<egui::Pos2> = (lo..hi).map(|i| to_screen(&LatLon { lat: gps.latitude[i], lon: gps.longitude[i] })).collect();
            let w = if hovered_row == Some(ri) { 4.0 } else { 2.0 };
            painter.add(egui::Shape::line(line, Stroke::new(w, rank_color(rank(t)))));
        }
        // Split lines and labels.
        for (k, &b) in c.boundaries.iter().enumerate() {
            painter.circle_filled(pts[b], 5.0, TEXT);
            painter.text(pts[b] + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, format!("{}", k + 2), medium(12.0), TEXT);
        }
        painter.circle_filled(pts[0], 5.0, theme::GOOD);
        painter.text(pts[0] + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, "1", medium(12.0), TEXT);
    });
    let Some(k) = selected else { return };
    if in_split.is_empty() {
        return;
    }
    ui.add_space(10.0);
    // Speed through the split against distance into it, per lap.
    let speed = session.channels.iter().find(|ch| ch.name == "GPS Speed");
    let mut per_lap: Vec<(usize, i64, Vec<[f64; 2]>, f64)> = Vec::new();
    for &(ri, a, b, t) in &in_split {
        let lap = aim_xrk::Lap { number: 0, start_ms: a.round() as i64, end_ms: b.round() as i64 };
        let Some(profile) = crate::compare::DistanceProfile::from_gps(session, &lap) else { continue };
        // Speed samples in the split, placed by distance into it.
        let line: Vec<[f64; 2]> = speed
            .map(|ch| {
                let lo = ch.times_ms.partition_point(|&x| x < lap.start_ms);
                let hi = ch.times_ms.partition_point(|&x| x <= lap.end_ms);
                (lo..hi).map(|i| [profile.dist_at(ch.times_ms[i] as f64), ch.values[i]]).collect()
            })
            .unwrap_or_default();
        per_lap.push((ri, t, line, profile.length()));
    }
    let small = |ui: &mut Ui, title: &str, units: &str, add: &mut dyn FnMut(&mut egui_plot::PlotUi)| {
        graph_card(ui, 170.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(medium(13.5)).color(TEXT));
                ui.label(RichText::new(units).size(12.5).color(DIM));
            });
            Plot::new(("split_detail", title.to_string()))
                .height(ui.available_height())
                .show_background(false)
                .grid_color(GRID)
                .y_axis_min_width(42.0)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .show_x(false)
                .show_y(false)
                .show(ui, |plot| add(plot));
        });
        ui.add_space(10.0);
    };
    small(ui, &format!("Speed through split {}", k + 1), &format!("{} by meters in", crate::units::speed_unit()), &mut |plot| {
        for (ri, t, line, _) in &per_lap {
            let w = if hovered_row == Some(*ri) { 3.0 } else { 1.5 };
            plot.line(Line::new(format!("lap {ri}"), PlotPoints::from(line.clone())).color(rank_color(rank(*t))).width(w));
        }
    });
    let by_lap = |f: &dyn Fn(&(usize, i64, Vec<[f64; 2]>, f64)) -> f64| -> Vec<[f64; 2]> { per_lap.iter().map(|x| [c.rows[x.0].lap_number as f64, f(x)]).collect() };
    small(ui, "Split time by lap", "s", &mut |plot| {
        let pts = by_lap(&|x| x.1 as f64 / 1000.0);
        plot.line(Line::new("time", PlotPoints::from(pts.clone())).color(SERIES[0]).width(1.5));
        plot.points(egui_plot::Points::new("laps", PlotPoints::from(pts)).color(SERIES[0]).radius(3.5));
    });
    small(ui, "Distance driven by lap", "m", &mut |plot| {
        let pts = by_lap(&|x| x.3);
        plot.line(Line::new("distance", PlotPoints::from(pts.clone())).color(SERIES[1]).width(1.5));
        plot.points(egui_plot::Points::new("laps", PlotPoints::from(pts)).color(SERIES[1]).radius(3.5));
    });
    small(ui, "Time against distance driven", "s by m", &mut |plot| {
        for (ri, t, _, d) in &per_lap {
            plot.points(egui_plot::Points::new(format!("lap {ri}"), PlotPoints::from(vec![[*d, *t as f64 / 1000.0]])).color(rank_color(rank(*t))).radius(5.0));
        }
    });
}
