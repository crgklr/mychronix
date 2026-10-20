//! The Corners tab, read the way a data team reads corners: each corner runs
//! from its braking point to the next one's, so the exit counts; each lap's
//! corners are compared with the fastest the driver has driven each one this
//! session (see `corner_model`). A table tells each corner's story left to
//! right (speed given up, how the kart slowed, steering, time); the map and
//! "what stands out" sit beside it; the selected corner is drawn up close below.

use super::*;
use crate::corner_model::{CornerModel, Slowing, Tone, STEP_M};

const BRAKE: Color32 = Color32::from_rgb(0xe6, 0x67, 0x67);
const TRAIL: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x4b);
const LIFT: Color32 = Color32::from_rgb(0x59, 0x61, 0x6d);
/// The lap being looked at, against the best (green).
const THIS_LAP: Color32 = Color32::from_rgb(0x5b, 0x9c, 0xf6);
const ROW_H: f32 = 62.0;

fn slowing_color(s: Slowing) -> Color32 {
    match s {
        Slowing::Brake => BRAKE,
        Slowing::Trail => TRAIL,
        Slowing::Lift => LIFT,
    }
}

/// Corners tab state.
#[derive(Default)]
pub(super) struct CornersState {
    /// The corner hovered in the table (lights it on the map).
    hovered: Option<usize>,
    /// The corner looked at, and the trace it was picked on: a new lap opens
    /// on the corner where that lap lost the most.
    pub selected: Option<(usize, usize)>,
    /// Built once per (session, best lap); None inside when it can't be (no GPS).
    model: Option<(usize, i64, Option<std::sync::Arc<CornerModel>>)>,
    bumps: Option<(usize, i64, Vec<crate::bumps::Bump>)>,
    /// Measured last frame, so both cards end at the same height: what each
    /// card adds around its content (the map card's height besides the map,
    /// the table card's besides its rows).
    map_extra_h: Option<f32>,
    table_extra_h: Option<f32>,
}

impl App {
    /// The session's corners (see `corner_model`), built once per session and
    /// best lap, for Corners, Laps and Map. None without GPS or a best lap.
    pub(super) fn corner_model(&mut self, session: &std::sync::Arc<Session>) -> Option<std::sync::Arc<CornerModel>> {
        let best = session.best_lap(MIN_LAP_MS).copied()?;
        let key = (std::sync::Arc::as_ptr(session) as usize, best.start_ms);
        if self.corners.model.as_ref().is_none_or(|m| (m.0, m.1) != key) {
            let counts: Vec<bool> = slow_laps(session).into_iter().map(|slow| !slow).collect();
            let started = std::time::Instant::now();
            let model = CornerModel::build(session, &best, &counts);
            log::debug!("corners: model built in {:?}", started.elapsed());
            self.corners.model = Some((key.0, key.1, model.map(std::sync::Arc::new)));
            self.corners.bumps = Some((key.0, key.1, crate::bumps::recurring(session, &best)));
            self.corners.selected = None;
        }
        self.corners.model.as_ref().and_then(|m| m.2.clone())
    }

    pub(super) fn corners_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        self.lap_strip(ui, session, false);
        ui.add_space(12.0);
        if session.best_lap(MIN_LAP_MS).is_none() {
            card(ui, |ui| {
                ui.label(RichText::new("Corners need a complete lap with GPS.").color(SECONDARY));
            });
            return;
        }
        let Some(model) = self.corner_model(session) else {
            card(ui, |ui| {
                ui.label(RichText::new("Corners need GPS for the lap, and a corner to brake for.").color(SECONDARY));
            });
            return;
        };
        // The lap picked in the strip, or the best for the whole session.
        let trace = match self.lap {
            Some(i) => match model.trace_of(i) {
                Some(t) => t,
                None => {
                    card(ui, |ui| {
                        ui.label(RichText::new("This lap doesn't go all the way round (an out-lap or in-lap). Pick another lap.").color(SECONDARY));
                    });
                    return;
                }
            },
            None => model.best,
        };
        let corner = match self.corners.selected {
            Some((t, k)) if t == trace && k < model.segments.len() => k,
            _ => model.worst(trace),
        };
        self.corners.selected = Some((trace, corner));
        let bumps = self.corners.bumps.as_ref().map(|b| b.2.clone()).unwrap_or_default();

        let mut pick = None;
        let mut measured: (Option<f32>, Option<f32>) = (None, None);
        egui::ScrollArea::vertical().id_salt("corners_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let gap = 14.0;
            let side_w = (ui.available_width() * 0.34).clamp(320.0, 560.0);
            let table_w = ui.available_width() - side_w - gap;
            // Both cards end at the same height: the taller of the map at least
            // square (a small track has to read too) and the table at its
            // natural row height. The other grows to match: the table's rows,
            // or the map. What each card adds around its content is measured
            // on the frame before, so they match exactly.
            let (map_extra, table_extra) = (self.corners.map_extra_h.unwrap_or(80.0), self.corners.table_extra_h.unwrap_or(178.0));
            let n = model.segments.len() as f32;
            let card_h = ((side_w - 32.0) + map_extra).max(table_extra + ROW_H * n);
            let map_h = card_h - map_extra;
            let row_h = (card_h - table_extra) / n;
            let table_h = card_h;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let column = Layout::top_down(Align::Min);
                ui.allocate_ui_with_layout(vec2(table_w, table_h), column, |ui| {
                    ui.set_width(table_w);
                    let (k, card_h) = self.corners_table(ui, &model, trace, corner, row_h);
                    pick = pick.or(k);
                    measured.0 = Some(card_h - row_h * n);
                });
                // The map has the column to itself, its card as tall as the table's.
                ui.allocate_ui_with_layout(vec2(side_w, table_h), column, |ui| {
                    ui.set_width(side_w);
                    let (k, card_h) = corners_map(ui, &model, trace, corner, self.corners.hovered, &bumps, map_h);
                    pick = pick.or(k);
                    measured.1 = Some(card_h - map_h);
                });
            });
            ui.add_space(gap);
            up_close(ui, &model, trace, corner);
        });
        if let Some(k) = pick {
            self.corners.selected = Some((trace, k));
        }
        // A changed measurement: draw again at once with the right row height.
        let changed = |old: Option<f32>, new: Option<f32>| new.is_some_and(|n| old.is_none_or(|o| (o - n).abs() > 0.5));
        if changed(self.corners.table_extra_h, measured.0) || changed(self.corners.map_extra_h, measured.1) {
            ui.ctx().request_repaint();
        }
        self.corners.table_extra_h = measured.0.or(self.corners.table_extra_h);
        self.corners.map_extra_h = measured.1.or(self.corners.map_extra_h);
    }

    /// The table: one row per corner. Returns a corner clicked, and the card's height.
    fn corners_table(&mut self, ui: &mut Ui, model: &CornerModel, trace: usize, selected: usize, row_h: f32) -> (Option<usize>, f32) {
        let mut clicked = None;
        let mut hovered = None;
        let card = Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 12 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let lap = &model.traces[trace];
            let ideal = model.ideal_s();
            let lap_s = model.stats[trace].iter().map(|s| s.time_s).sum::<f64>();
            // Title, and the best of every corner with what it means.
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Lap {}, corner by corner", lap.number)).font(medium(14.5)).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    ui.painter().circle_stroke(r.center(), 7.5, Stroke::new(1.0, DIM));
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", semibold(10.5), DIM);
                    let counts = if model.counts[trace] { String::new() } else { format!(" Lap {} was too slow to count toward bests (an out-lap, in-lap or slow lap).", lap.number) };
                    resp.on_hover_text(format!(
                        "Each corner runs from its braking point to the next one's, so a good exit counts. Lap {}'s times are compared with the fastest you've driven each corner this session, from the laps that count; together those make the best of every corner, {}.{counts}",
                        lap.number,
                        format_lap_time((ideal * 1000.0).round() as i64)
                    ));
                    ui.label(RichText::new("left on this lap").size(13.0).color(DIM));
                    ui.label(RichText::new(format!("{:.2} s", (lap_s - ideal).max(0.0))).font(semibold(13.0)).color(theme::FAVORITE));
                    ui.label(RichText::new("·").size(13.0).color(DIM));
                    ui.label(RichText::new(format_lap_time((ideal * 1000.0).round() as i64)).font(semibold(13.0)).color(TEXT));
                    ui.label(RichText::new("Best of every corner").size(13.0).color(DIM));
                });
            });
            ui.add_space(8.0);

            // Columns: corner, speed, slowing down, steering (when there is any), time.
            let steering = model.traces[trace].steering.is_some();
            let width = ui.available_width();
            let fixed = 76.0 + if steering { 84.0 } else { 0.0 } + 104.0;
            let flex = (width - fixed).max(200.0);
            let (speed_w, slow_w) = (flex / 2.45, flex * 1.45 / 2.45);
            let xs = {
                let mut x = vec![0.0, 76.0, 76.0 + speed_w, 76.0 + speed_w + slow_w];
                if steering {
                    x.push(x[3] + 84.0);
                }
                x
            };
            let headers: Vec<&str> = if steering {
                vec!["CORNER", "SPEED  (APEX ← ENTRY)", "SLOWING DOWN", "STEERING", "TIME"]
            } else {
                vec!["CORNER", "SPEED  (APEX ← ENTRY)", "SLOWING DOWN", "TIME"]
            };
            let (head, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
            for (h, x) in headers.iter().zip(&xs) {
                ui.painter().text(pos2(head.left() + x + if *x == 0.0 { 8.0 } else { 0.0 }, head.center().y), egui::Align2::LEFT_CENTER, *h, medium(11.0), DIM);
            }
            ui.painter().hline(head.x_range(), head.bottom(), Stroke::new(1.0, theme::OUTLINE));

            // Shared scales, so corners compare at a glance.
            let counting: Vec<usize> = (0..model.traces.len()).filter(|&i| model.counts[i] || i == trace).collect();
            let all = || counting.iter().flat_map(|&i| model.stats[i].iter());
            let v_lo = all().map(|s| crate::units::speed(s.min_kmh)).fold(f64::MAX, f64::min);
            let v_hi = all().map(|s| crate::units::speed(s.entry_kmh)).fold(f64::MIN, f64::max);
            let (v_lo, v_hi) = ((v_lo / 5.0).floor() * 5.0, (v_hi / 5.0).ceil() * 5.0);
            let max_slow = all().map(|s| s.slowing_m()).fold(1.0, f64::max);
            let (su, du) = (crate::units::speed_unit(), crate::units::distance_unit());

            for (k, a) in model.stats[trace].iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), Sense::click());
                let p = ui.painter();
                if k == selected {
                    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL), SURFACE_ACTIVE);
                } else if resp.hovered() {
                    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL), SURFACE_HOVER);
                }
                p.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, theme::OUTLINE));
                if resp.hovered() {
                    hovered = Some(k);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    clicked = Some(k);
                }
                let pad = (row_h - ROW_H) / 2.0;
                let (top, mid, low) = (rect.top() + pad + 18.0, rect.top() + pad + 38.0, rect.top() + pad + 46.0);
                let x = |c: usize| rect.left() + xs[c];
                let b = &model.stats[model.best_of[k]][k];
                // Corner
                p.text(pos2(x(0) + 10.0, rect.center().y), egui::Align2::LEFT_CENTER, (k + 1).to_string(), semibold(17.0), TEXT);
                // Speed: the apex in bold, the entry beside it, and the drop as a bar on the shared scale.
                let (min_v, entry_v) = (crate::units::speed(a.min_kmh), crate::units::speed(a.entry_kmh));
                let r = p.text(pos2(x(1), top), egui::Align2::LEFT_CENTER, format!("{min_v:.1}"), semibold(15.0), TEXT);
                p.text(pos2(r.right() + 5.0, top), egui::Align2::LEFT_CENTER, format!("← {entry_v:.1} {su}"), regular(13.0), DIM);
                let bar_w = speed_w - 24.0;
                let vx = |v: f64| x(1) + 4.0 + ((v - v_lo) / (v_hi - v_lo).max(1.0)).clamp(0.0, 1.0) as f32 * (bar_w - 8.0);
                p.hline(x(1) + 4.0..=x(1) + bar_w - 4.0, mid, Stroke::new(2.0, GRID));
                p.rect_filled(Rect::from_min_max(pos2(vx(min_v), mid - 3.0), pos2(vx(entry_v).max(vx(min_v) + 2.0), mid + 3.0)), CornerRadius::same(3), SECONDARY.gamma_multiply(0.55));
                p.circle_filled(pos2(vx(min_v), mid), 5.0, TEXT);
                // Slowing down: one bar, its length the slowing distance, colored by what the kart was doing.
                let slow_bar = slow_w - 24.0;
                let scale = slow_bar as f64 / max_slow;
                let mut bx = x(2);
                let mut i = 0;
                while i < a.slowing.len() {
                    let mut j = i;
                    while j < a.slowing.len() && a.slowing[j] == a.slowing[i] {
                        j += 1;
                    }
                    let w = ((j - i) as f64 * STEP_M * scale) as f32;
                    p.rect_filled(Rect::from_min_size(pos2(bx, top - 5.0), vec2(w, 10.0)), CornerRadius::ZERO, slowing_color(a.slowing[i]));
                    bx += w;
                    i = j;
                }
                let from = format!("{:.0} {du} from {:.0} {du}", crate::units::distance(a.slowing_m()), crate::units::distance(a.brake_m));
                let detail = if a.brake_s >= 0.05 { format!("{from} · {:.1} s on the brakes, peak {:.2} g", a.brake_s, a.peak_g) } else { format!("{from} · no braking") };
                p.text(pos2(x(2), mid + 2.0), egui::Align2::LEFT_CENTER, detail, regular(12.0), DIM);
                // Steering: the largest angle, and a dot per correction.
                if steering && let Some((angle, corr)) = a.steering {
                    p.text(pos2(x(3), top), egui::Align2::LEFT_CENTER, format!("{angle:.0}°"), regular(14.0), TEXT);
                    if corr == 0 {
                        p.text(pos2(x(3), mid + 2.0), egui::Align2::LEFT_CENTER, "smooth", regular(12.0), DIM);
                    }
                    for c in 0..corr.min(10) {
                        p.circle_filled(pos2(x(3) + 4.0 + c as f32 * 9.0, mid + 2.0), 3.0, SECONDARY);
                    }
                }
                // Time: the whole corner, against the best of it.
                let tx = x(xs.len() - 1);
                p.text(pos2(tx, rect.top() + pad + 14.0), egui::Align2::LEFT_CENTER, format!("{:.2} s", a.time_s), semibold(15.0), TEXT);
                let is_best = model.best_of[k] == trace;
                let (pill, fill, ink) = if is_best {
                    ("best".to_string(), theme::GOOD.gamma_multiply(0.18), theme::GOOD)
                } else if model.loss(trace, k) < 0.005 {
                    // Slower only in the thousandths: no loss worth coloring.
                    ("+0.00".to_string(), SURFACE_ACTIVE, SECONDARY)
                } else {
                    (format!("+{:.2}", model.loss(trace, k)), BRAKE.gamma_multiply(0.18), Color32::from_rgb(0xf3, 0x9a, 0x9a))
                };
                let galley = p.layout_no_wrap(pill, semibold(11.5), ink);
                let pr = Rect::from_min_size(pos2(tx, rect.top() + pad + 26.0), galley.size() + vec2(12.0, 2.0));
                p.rect_filled(pr, CornerRadius::same(8), fill);
                p.galley(pr.min + vec2(6.0, 1.0), galley, ink);
                p.text(pos2(tx, low + 6.0), egui::Align2::LEFT_CENTER, format!("best {:.2}", b.time_s), regular(11.5), DIM);
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let mut key = |color: Color32, text: &str| {
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 6.0), Sense::hover());
                    ui.painter().rect_filled(r, CornerRadius::same(3), color);
                    ui.label(RichText::new(text).size(12.0).color(DIM));
                    ui.add_space(8.0);
                };
                key(BRAKE, "braking");
                if model.has_trail {
                    key(TRAIL, "braking while turning (trail)");
                }
                key(LIFT, "slowing off the brakes (lift or slide)");
                ui.label(RichText::new("Time is the whole corner: its braking point to the next one's.").size(12.0).color(DIM));
            });
        });
        self.corners.hovered = hovered;
        (clicked, card.response.rect.height())
    }
}

/// The best lap's line split into corners: the selected one bright with this
/// lap's slowing drawn over it, the hovered one lit, numbers at the apexes,
/// bumps as diamonds. Returns a corner clicked, and the card's height.
fn corners_map(ui: &mut Ui, model: &CornerModel, trace: usize, selected: usize, hovered: Option<usize>, bumps: &[crate::bumps::Bump], height: f32) -> (Option<usize>, f32) {
    let mut clicked = None;
    let card = Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 12 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add(Icon::MapPin.image(DIM, 16.0));
            ui.label(RichText::new("Track").font(medium(14.5)).color(TEXT));
        });
        ui.label(RichText::new("Corners as segments, braking point to braking point").size(12.5).color(DIM));
        if !bumps.is_empty() {
            ui.label(RichText::new("◆  bumps or curbs hit on most laps").size(12.5).color(DIM));
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let best = &model.traces[model.best];
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for &(x, y) in &best.xy {
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        let area = rect.shrink(18.0);
        let s = (area.width() as f64 / (x1 - x0).max(1.0)).min(area.height() as f64 / (y1 - y0).max(1.0));
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let pt = |(x, y): (f64, f64)| pos2(area.center().x + ((x - cx) * s) as f32, area.center().y - ((y - cy) * s) as f32);
        let last = best.xy.len() - 1;
        let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
        let p = ui.painter_at(rect);
        for (k, seg) in model.segments.iter().enumerate() {
            let pts: Vec<egui::Pos2> = (idx(seg.from)..=idx(seg.to)).map(|i| pt(best.xy[i])).collect();
            let (color, w) = if k == selected { (TEXT, 6.0) } else if hovered == Some(k) { (SECONDARY, 5.0) } else { (SURFACE_ACTIVE, 5.0) };
            p.add(egui::Shape::line(pts, Stroke::new(w, color)));
        }
        // This lap's slowing for the selected corner, along the line.
        let a = &model.stats[trace][selected];
        for (i, st) in a.slowing.iter().enumerate() {
            let m = a.brake_m + i as f64 * STEP_M;
            p.line_segment([pt(best.xy[idx(m)]), pt(best.xy[idx(m + STEP_M)])], Stroke::new(6.0, slowing_color(*st)));
        }
        // Start line, bumps, corner numbers.
        let start = pt(best.xy[0]);
        p.line_segment([start - vec2(0.0, 9.0), start + vec2(0.0, 9.0)], Stroke::new(3.0, TEXT));
        for b in bumps {
            let q = pt(best.xy[idx(b.at * model.length_m)]);
            let d = 6.0;
            p.add(egui::Shape::convex_polygon(vec![pos2(q.x, q.y - d), pos2(q.x + d, q.y), pos2(q.x, q.y + d), pos2(q.x - d, q.y)], TEXT, Stroke::new(1.5, SURFACE)));
            ui.interact(Rect::from_center_size(q, vec2(16.0, 16.0)), ui.id().with(("bump", b.laps, (b.at * 1000.0) as i64)), Sense::hover())
                .on_hover_text(format!("A bump or curb hit on {} of {} laps (jolts up to {:.1} g). Curbs can be fast if the kart stays settled; a bump on the line is worth avoiding.", b.laps, b.of, b.peak_g));
        }
        for (k, seg) in model.segments.iter().enumerate() {
            let q = pt(best.xy[idx(seg.apex)]);
            let on = k == selected;
            p.circle_filled(q, 11.0, if on { TEXT } else { SURFACE });
            p.circle_stroke(q, 11.0, Stroke::new(1.5, if on { TEXT } else { SECONDARY }));
            p.text(q, egui::Align2::CENTER_CENTER, (k + 1).to_string(), semibold(12.0), if on { ON_TEXT } else { TEXT });
        }
        // A click picks the corner whose line is nearest.
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
            let nearest = (0..=last).min_by(|&i, &j| pt(best.xy[i]).distance_sq(pos).total_cmp(&pt(best.xy[j]).distance_sq(pos)));
            if let Some(i) = nearest.filter(|&i| pt(best.xy[i]).distance(pos) < 24.0) {
                let m = i as f64 * STEP_M;
                clicked = model.segments.iter().position(|s| m >= s.from && m < s.to).or(Some(model.segments.len() - 1));
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    });
    (clicked, card.response.rect.height())
}

/// The selected corner up close, headed by what stands out about it: speed
/// through it by distance (this lap, the best of it in green, the other laps
/// that count faint), the time gap building up along the way, and the
/// numbers side by side.
fn up_close(ui: &mut Ui, model: &CornerModel, trace: usize, corner: usize) {
    card(ui, |ui| {
        let seg = model.segments[corner];
        let bi = model.best_of[corner];
        let (a, b) = (&model.stats[trace][corner], &model.stats[bi][corner]);
        let this = &model.traces[trace];
        let facts_w = 270.0;
        let chart_w = (ui.available_width() - facts_w - 24.0).max(320.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            ui.vertical(|ui| {
                ui.set_width(chart_w);
                // The headline: what stands out about this corner on this lap.
                let mut notes = model.notes(trace, corner);
                if notes.is_empty() {
                    notes.push((Tone::Plain, format!("within {:.2} s of your best", model.loss(trace, corner).max(0.01))));
                }
                let tone = |t: Tone| match t {
                    Tone::Good => theme::GOOD,
                    Tone::Attention => theme::FAVORITE,
                    Tone::Plain => SECONDARY,
                };
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(RichText::new(format!("Corner {}", corner + 1)).font(semibold(16.0)).color(TEXT));
                    ui.label(RichText::new("·").size(15.0).color(DIM));
                    let headline = if notes[0].0 == Tone::Good { "your best".to_string() } else { notes[0].1.clone() };
                    ui.label(RichText::new(headline).font(medium(15.0)).color(tone(notes[0].0)));
                });
                let colors = if bi == trace {
                    format!("Lap {} is your best here; the other laps that count, faint", this.number)
                } else {
                    format!("Lap {} (blue) against your best corner {}, from lap {} (green); the other laps that count, faint", this.number, corner + 1, model.traces[bi].number)
                };
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if let Some((t, text)) = notes.get(1) {
                        ui.label(RichText::new(text).size(13.0).color(tone(*t)));
                        ui.label(RichText::new("·").size(13.0).color(DIM));
                    }
                    ui.label(RichText::new(colors).size(12.5).color(DIM));
                });
                ui.add_space(6.0);
                speed_chart(ui, model, trace, corner, chart_w);
                gap_chart(ui, model, trace, corner, chart_w);
            });
            ui.vertical(|ui| {
                ui.set_width(facts_w);
                ui.label(RichText::new(if bi == trace { format!("Lap {}", this.number) } else { format!("Lap {} vs lap {}", this.number, model.traces[bi].number) }).size(12.5).color(DIM));
                ui.add_space(6.0);
                let (su, du) = (crate::units::speed_unit(), crate::units::distance_unit());
                let speed = |v: f64| crate::units::speed(v);
                // (label, this lap, the best, higher is better, decimals, unit)
                let rows: [(&str, f64, f64, bool, usize, &str); 6] = [
                    ("Brakes at", crate::units::distance(a.brake_m), crate::units::distance(b.brake_m), true, 0, du),
                    ("Entry", speed(a.entry_kmh), speed(b.entry_kmh), true, 1, su),
                    ("Apex", speed(a.min_kmh), speed(b.min_kmh), true, 1, su),
                    ("Down the straight", speed(a.exit_kmh), speed(b.exit_kmh), true, 1, su),
                    ("On the brakes", a.brake_s, b.brake_s, false, 2, "s"),
                    ("Corner time", a.time_s, b.time_s, false, 2, "s"),
                ];
                egui::Grid::new(("corner_facts", corner)).num_columns(3).spacing(vec2(14.0, 7.0)).show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new("THIS LAP").font(medium(10.5)).color(DIM));
                    ui.label(RichText::new(if bi == trace { "" } else { "VS BEST" }).font(medium(10.5)).color(DIM));
                    ui.end_row();
                    for (label, x, y, higher, dec, unit) in rows {
                        ui.label(RichText::new(label).size(13.0).color(SECONDARY));
                        ui.label(RichText::new(format!("{x:.dec$} {unit}")).size(13.0).color(TEXT));
                        let d = x - y;
                        if bi != trace && d.abs() >= 0.5 * 10f64.powi(-(dec as i32)) {
                            let good = if higher { d > 0.0 } else { d < 0.0 };
                            ui.label(RichText::new(format!("{d:+.dec$}")).size(13.0).color(if good { theme::GOOD } else { Color32::from_rgb(0xf3, 0x9a, 0x9a) }));
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(format!("Brakes at: {du} from the start of the lap.")).size(12.0).color(DIM)).wrap());
                let _ = seg;
            });
        });
    });
}

/// Speed by distance through the corner (from just before its braking point
/// to the next one's).
fn speed_chart(ui: &mut Ui, model: &CornerModel, trace: usize, corner: usize, width: f32) {
    let seg = model.segments[corner];
    let bi = model.best_of[corner];
    let a = &model.stats[trace][corner];
    let from = (seg.from - 10.0).max(0.0);
    let last = model.traces[trace].kmh.len() - 1;
    let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
    let ks: Vec<usize> = (idx(from)..=idx(seg.to)).collect();
    // The laps that count, plus this one: slow laps would only squash the scale.
    let shown: Vec<usize> = (0..model.traces.len()).filter(|&i| model.counts[i] || i == trace).collect();
    let speeds = || shown.iter().flat_map(|&i| ks.iter().map(move |&k| crate::units::speed(model.traces[i].kmh[k])));
    let (v_lo, v_hi) = ((speeds().fold(f64::MAX, f64::min) / 5.0).floor() * 5.0, (speeds().fold(f64::MIN, f64::max) / 5.0).ceil() * 5.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 250.0), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(40.0, 6.0), rect.max - vec2(8.0, 24.0));
    let x = |m: f64| plot.left() + ((m - from) / (seg.to - from).max(1.0)) as f32 * plot.width();
    let y = |v: f64| plot.bottom() - ((v - v_lo) / (v_hi - v_lo).max(1.0)) as f32 * plot.height();
    let p = ui.painter_at(rect);
    let mut v = v_lo;
    while v <= v_hi + 0.1 {
        p.hline(plot.x_range(), y(v), Stroke::new(1.0, GRID));
        p.text(pos2(plot.left() - 8.0, y(v)), egui::Align2::RIGHT_CENTER, format!("{v:.0}"), regular(11.0), DIM);
        v += 5.0;
    }
    p.text(pos2(plot.left() + 6.0, plot.top() + 8.0), egui::Align2::LEFT_CENTER, crate::units::speed_unit(), regular(11.0), DIM);
    // This lap's slowing, shaded.
    p.rect_filled(Rect::from_x_y_ranges(x(a.brake_m)..=x(a.min_m).max(x(a.brake_m)), plot.y_range()), CornerRadius::ZERO, BRAKE.gamma_multiply(0.07));
    let line = |i: usize, color: Color32, w: f32| {
        let pts: Vec<egui::Pos2> = ks.iter().map(|&k| pos2(x(k as f64 * STEP_M), y(crate::units::speed(model.traces[i].kmh[k])))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(w, color)));
    };
    for &i in &shown {
        if i != trace && i != bi {
            line(i, SECONDARY.gamma_multiply(0.28), 1.2);
        }
    }
    if bi != trace {
        line(bi, theme::GOOD, 2.4);
    }
    line(trace, if bi == trace { theme::GOOD } else { THIS_LAP }, 2.8);
    // Where this lap brakes and reaches the apex.
    for (m, label) in [(a.brake_m, "braking"), (a.min_m, "apex")] {
        p.extend(egui::Shape::dashed_line(&[pos2(x(m), plot.top()), pos2(x(m), plot.bottom())], Stroke::new(1.0, DIM), 3.0, 4.0));
        p.text(pos2(x(m) + 4.0, plot.bottom() + 12.0), egui::Align2::LEFT_CENTER, label, regular(11.0), DIM);
    }
    p.text(pos2(plot.right(), plot.bottom() + 12.0), egui::Align2::RIGHT_CENTER, "next corner's braking point", regular(11.0), DIM);
}

/// Time behind the best of the corner, building up along the way.
fn gap_chart(ui: &mut Ui, model: &CornerModel, trace: usize, corner: usize, width: f32) {
    let seg = model.segments[corner];
    let bi = model.best_of[corner];
    let from = (seg.from - 10.0).max(0.0);
    let (this, best) = (&model.traces[trace], &model.traces[bi]);
    let last = this.t.len() - 1;
    let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
    let k0 = idx(seg.from);
    let ks: Vec<usize> = (idx(from)..=idx(seg.to)).collect();
    let gaps: Vec<f64> = ks.iter().map(|&k| (this.t[k] - this.t[k0]) - (best.t[k] - best.t[k0])).collect();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 96.0), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(40.0, 18.0), rect.max - vec2(8.0, 6.0));
    let scale = gaps.iter().fold(0.05f64, |m, g| m.max(g.abs()));
    let x = |m: f64| plot.left() + ((m - from) / (seg.to - from).max(1.0)) as f32 * plot.width();
    let y = |g: f64| plot.center().y - (g / scale) as f32 * plot.height() / 2.0;
    let p = ui.painter_at(rect);
    let label = if bi == trace { "Time gap: none, this is the best" } else { "Time behind the best of this corner, building up along the way" };
    p.text(pos2(plot.left(), rect.top() + 7.0), egui::Align2::LEFT_CENTER, label, regular(11.5), DIM);
    p.hline(plot.x_range(), y(0.0), Stroke::new(1.0, GRID));
    p.text(pos2(plot.left() - 8.0, y(0.0)), egui::Align2::RIGHT_CENTER, "0 s", regular(11.0), DIM);
    let pts: Vec<egui::Pos2> = ks.iter().zip(&gaps).map(|(&k, &g)| pos2(x(k as f64 * STEP_M), y(g))).collect();
    // Behind the best (above the line) in red, ahead of it in green.
    let base = y(0.0);
    for (w, g) in pts.windows(2).zip(gaps.windows(2)) {
        let color = if g[0] + g[1] > 0.0 { BRAKE } else { theme::GOOD };
        let quad = vec![w[0], w[1], pos2(w[1].x, base), pos2(w[0].x, base)];
        p.add(egui::Shape::convex_polygon(quad, color.gamma_multiply(0.22), Stroke::NONE));
        p.line_segment([w[0], w[1]], Stroke::new(2.0, if g[1] > 0.0 { Color32::from_rgb(0xf3, 0x9a, 0x9a) } else { theme::GOOD }));
    }
    if let (Some(end), Some(g)) = (pts.last(), gaps.last()) {
        // Beside the end of the line, kept inside the chart.
        let y = (end.y - 12.0).clamp(rect.top() + 10.0, rect.bottom() - 10.0);
        p.text(pos2(end.x, y), egui::Align2::RIGHT_CENTER, format!("{g:+.2} s"), medium(12.0), TEXT);
    }
}
