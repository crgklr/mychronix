//! The Map view: the track up close, a corner at a time, on satellite imagery
//! or the street map. The lap's line through each corner against the best you
//! drove that corner (the same comparison Corners makes, see `corner_model`),
//! colored by how the kart slowed, speed, RPM or where time was gained and
//! lost; every lap faint, to see how repeatable the line is; the corner's
//! numbers beside it, and speed through it underneath.

use std::sync::{Arc, Mutex};

use super::*;
use crate::corner_model::{CornerModel, Slowing, STEP_M};

const BRAKE: Color32 = Color32::from_rgb(0xe6, 0x67, 0x67);
const TRAIL: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x4b);
const LIFT: Color32 = Color32::from_rgb(0x59, 0x61, 0x6d);
/// The overlays' glass: dark enough to read over satellite imagery.
const GLASS: Color32 = Color32::from_rgba_premultiplied(18, 21, 25, 225);

/// What colors the lap's line.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum ColorBy {
    /// Braking, trail braking and lifting, as Corners reads them; green otherwise.
    #[default]
    Slowing,
    Speed,
    Rpm,
    /// Where time was gained and lost against the reference.
    Gain,
}

/// Map view state.
pub(super) struct MapState {
    /// The whole track rather than one corner.
    pub whole: bool,
    pub color: ColorBy,
    /// Every counting lap's line, faint.
    pub all_laps: bool,
    pub satellite: bool,
    memory: walkers::MapMemory,
    /// What the view is moving to, and from, with when the move started.
    target: Option<(f64, f64, f64)>,
    shown: Option<(f64, f64, f64)>,
    anim: Option<((f64, f64, f64), f64)>,
}

impl Default for MapState {
    fn default() -> Self {
        Self { whole: false, color: ColorBy::default(), all_laps: false, satellite: true, memory: walkers::MapMemory::default(), target: None, shown: None, anim: None }
    }
}

/// Meters east and north of (lat0, lon0) back to a position (the inverse of the corner model's projection).
fn to_lat_lon(lat0: f64, lon0: f64, (x, y): (f64, f64)) -> aim_track::LatLon {
    aim_track::LatLon { lat: lat0 + y / 110_540.0, lon: lon0 + x / (111_320.0 * lat0.to_radians().cos()) }
}

/// Center and zoom that fit `points` into `size` pixels. A kart track's corner
/// needs to zoom past the imagery's own detail (zoom 19); the map stretches the
/// deepest tiles it has.
fn fit(points: &[aim_track::LatLon], size: Vec2) -> Option<(aim_track::LatLon, f64)> {
    if points.len() < 2 {
        return None;
    }
    let (mut lat0, mut lat1, mut lon0, mut lon1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in points {
        lat0 = lat0.min(p.lat);
        lat1 = lat1.max(p.lat);
        lon0 = lon0.min(p.lon);
        lon1 = lon1.max(p.lon);
    }
    let c = aim_track::LatLon { lat: (lat0 + lat1) / 2.0, lon: (lon0 + lon1) / 2.0 };
    let width_m = ((lon1 - lon0) * 111_320.0 * c.lat.to_radians().cos()).max(15.0);
    let height_m = ((lat1 - lat0) * 110_540.0).max(15.0);
    // Meters per pixel at zoom z (256-pixel tiles): 156543.03 × cos(lat) / 2^z.
    let mpp = (width_m / (size.x as f64 * 0.85)).max(height_m / (size.y as f64 * 0.85));
    let zoom = (156_543.03 * c.lat.to_radians().cos() / mpp).log2().clamp(3.0, 21.5);
    Some((c, zoom))
}

/// Low to high: blue, through teal and yellow, to red.
fn heat(t: f32) -> Color32 {
    super::heat_color(t)
}

/// One line to draw on the map.
struct Path {
    points: Vec<aim_track::LatLon>,
    colors: Vec<Color32>,
    width: f32,
    dashed: bool,
}

/// Draws the lines and the corner numbers; reports a corner number clicked.
struct Overlay {
    paths: Vec<Path>,
    /// Each corner's number, its apex, a point a meter toward the outside of
    /// the corner (to place the number beside the line), and whether it's the one shown.
    corners: Vec<(usize, aim_track::LatLon, aim_track::LatLon, bool)>,
    clicked: Arc<Mutex<Option<usize>>>,
}

impl walkers::Plugin for Overlay {
    fn run(self: Box<Self>, ui: &mut Ui, response: &egui::Response, projector: &walkers::Projector, _memory: &walkers::MapMemory) {
        ui.set_clip_rect(response.rect.intersect(ui.clip_rect()));
        let painter = ui.painter();
        let to_screen = |p: &aim_track::LatLon| projector.project(walkers::lat_lon(p.lat, p.lon)).to_pos2();
        for path in &self.paths {
            let pts: Vec<egui::Pos2> = path.points.iter().map(to_screen).collect();
            if path.dashed {
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(path.width, path.colors[0]), 6.0, 5.0));
                continue;
            }
            if path.colors.len() == 1 {
                painter.add(egui::Shape::line(pts, Stroke::new(path.width, path.colors[0])));
                continue;
            }
            // A dark casing so the colors read over satellite imagery.
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(path.width + 3.0, Color32::from_black_alpha(140))));
            for k in 0..pts.len().saturating_sub(1) {
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(path.width, path.colors[k.min(path.colors.len() - 1)]));
            }
        }
        for (number, apex, outside, on) in &self.corners {
            let (a, o) = (to_screen(apex), to_screen(outside));
            let c = a + (o - a).normalized() * 22.0;
            let r = if *on { 13.0 } else { 11.0 };
            painter.circle_filled(c, r + 2.0, Color32::from_black_alpha(170));
            painter.circle_filled(c, r, if *on { TEXT } else { SURFACE_ACTIVE });
            painter.text(c, egui::Align2::CENTER_CENTER, (number + 1).to_string(), semibold(12.5), if *on { ON_TEXT } else { TEXT });
            let hit = ui.interact(Rect::from_center_size(c, Vec2::splat(30.0)), ui.id().with(("map_corner", number)), Sense::click());
            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if hit.clicked() {
                *self.clicked.lock().unwrap() = Some(*number);
            }
        }
    }
}

impl App {
    pub(super) fn map_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        let Some(model) = self.corner_model(session) else {
            card(ui, |ui| {
                ui.label(RichText::new("The map needs GPS for the lap, and a corner to brake for.").color(SECONDARY));
            });
            return;
        };
        let Some(gps) = session.gps.as_ref() else { return };
        let (lat0, lon0) = (gps.latitude[0], gps.longitude[0]);
        // The lap looked at (the best for the whole session), and the corner, shared with Corners.
        let picked = self.lap.and_then(|i| model.trace_of(i));
        let trace = picked.unwrap_or(model.best);
        let corner = match self.corners.selected {
            Some((t, k)) if t == trace && k < model.segments.len() => k,
            _ => model.worst(trace),
        };
        self.corners.selected = Some((trace, corner));
        let whole = self.map_view.whole;
        // Against: the best you drove this corner, or for the whole track, the best lap.
        let reference = if whole { model.best } else { model.best_of[corner] };
        let rpm = session.channels.iter().find(|c| c.name == "RPM" && !is_dead(c));
        let lap = session.laps[model.traces[trace].lap];

        let colors = line_colors(&model, trace, reference, self.map_view.color, rpm, lap.start_ms);
        let latlon = |t: usize| -> Vec<aim_track::LatLon> { model.traces[t].xy.iter().map(|&p| to_lat_lon(lat0, lon0, p)).collect() };
        let mut paths = Vec::new();
        if self.map_view.all_laps {
            for t in (0..model.traces.len()).filter(|&t| model.counts[t] && t != trace) {
                paths.push(Path { points: latlon(t), colors: vec![Color32::from_white_alpha(70)], width: 1.2, dashed: false });
            }
        }
        if reference != trace {
            paths.push(Path { points: latlon(reference), colors: vec![Color32::from_white_alpha(220)], width: 1.6, dashed: true });
        }
        paths.push(Path { points: latlon(trace), colors, width: 4.0, dashed: false });
        let best_xy = &model.traces[model.best].xy;
        let at = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(best_xy.len() - 1);
        let corners: Vec<(usize, aim_track::LatLon, aim_track::LatLon, bool)> = model
            .segments
            .iter()
            .enumerate()
            .map(|(k, s)| {
                // The number sits off the line, on the outside of the corner.
                let i = at(s.apex);
                let (a, b) = (best_xy[i.saturating_sub(3)], best_xy[(i + 3).min(best_xy.len() - 1)]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = (dx * dx + dy * dy).sqrt().max(1e-6);
                let mid = best_xy[i];
                let center = model.traces[model.best].xy.iter().fold((0.0, 0.0), |s, p| (s.0 + p.0, s.1 + p.1));
                let n = best_xy.len() as f64;
                let out = if (mid.0 - center.0 / n) * (-dy / len) + (mid.1 - center.1 / n) * (dx / len) > 0.0 { 1.0 } else { -1.0 };
                let off = (mid.0 - dy / len * out, mid.1 + dx / len * out);
                (k, to_lat_lon(lat0, lon0, mid), to_lat_lon(lat0, lon0, off), !whole && k == corner)
            })
            .collect();
        let clicked = Arc::new(Mutex::new(None));
        let overlay = Overlay { paths, corners, clicked: clicked.clone() };

        // Where to look: the whole track, or the corner from its braking point to the next one's.
        let rect = ui.available_rect_before_wrap();
        let panel_w = 300.0;
        let view_pts: Vec<aim_track::LatLon> = if whole {
            best_xy.iter().map(|&p| to_lat_lon(lat0, lon0, p)).collect()
        } else {
            // From before the braking point to a little past the apex: the corner, not the straight after it.
            let s = model.segments[corner];
            let from = (s.from - 15.0).min(s.apex - 60.0);
            (at(from)..=at((s.apex + 45.0).min(s.to))).map(|i| to_lat_lon(lat0, lon0, best_xy[i])).collect()
        };
        let free = vec2((rect.width() - panel_w - 40.0).max(200.0), (rect.height() - if whole { 120.0 } else { 300.0 }).max(200.0));
        if let Some((c, zoom)) = fit(&view_pts, free) {
            // Keep the subject clear of the panel on the right and the trace at the bottom.
            let mpp = 156_543.03 * c.lat.to_radians().cos() / 2f64.powf(zoom);
            let east = (panel_w as f64 + 20.0) / 2.0 * mpp;
            let south = if whole { 30.0 } else { 140.0 } * mpp;
            let lon = c.lon + east / (111_320.0 * c.lat.to_radians().cos());
            let lat = c.lat - south / 110_540.0;
            self.map_view_move_to((lat, lon, zoom), ui.ctx());
        }

        let satellite = self.map_view.satellite;
        let start = walkers::lat_lon(lat0, lon0);
        let mut memory = std::mem::take(&mut self.map_view.memory);
        let tiles = super::tracks_ui::tiles(&mut self.map_tiles, &mut self.sat_tiles, ui.ctx(), satellite);
        let map = ui.add(walkers::Map::new(Some(tiles), &mut memory, start).with_plugin(overlay));
        self.map_view.memory = memory;
        if let Some(k) = clicked.lock().unwrap().take() {
            self.corners.selected = Some((trace, k));
            self.map_view.whole = false;
        }
        let rect = map.rect;

        // Overlays: corners along the top left, color and imagery top right, the
        // numbers on the right, the lap and legend bottom left, speed above them.
        let ov = |ui: &mut Ui, at: Rect, add: &mut dyn FnMut(&mut Ui)| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                Frame::new().fill(GLASS).stroke(Stroke::new(1.0, theme::OUTLINE)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(6)).show(ui, |ui| add(ui));
            });
        };
        let n = model.segments.len();
        let mut go_corner: Option<Option<usize>> = None;
        ov(ui, Rect::from_min_size(rect.left_top() + vec2(14.0, 14.0), vec2(rect.width() * 0.55, 44.0)), &mut |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if chip(ui, "Whole track", whole).clicked() {
                    go_corner = Some(None);
                }
                for k in 0..n {
                    if chip(ui, &(k + 1).to_string(), !whole && k == corner).clicked() {
                        go_corner = Some(Some(k));
                    }
                }
            });
        });
        let mut color = self.map_view.color;
        let mut sat = satellite;
        ov(ui, Rect::from_min_size(pos2(rect.right() - 560.0, rect.top() + 14.0), vec2(546.0, 44.0)), &mut |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if chip(ui, "Satellite", sat).clicked() {
                    sat = true;
                }
                if chip(ui, "Map", !sat).clicked() {
                    sat = false;
                }
                ui.add_space(8.0);
                for (c, label) in [(ColorBy::Gain, "Gain / lose"), (ColorBy::Rpm, "RPM"), (ColorBy::Speed, "Speed"), (ColorBy::Slowing, "Braking · trail · lift")] {
                    if c == ColorBy::Rpm && rpm.is_none() {
                        continue;
                    }
                    if chip(ui, label, color == c).clicked() {
                        color = c;
                    }
                }
                ui.label(RichText::new("Color the line by").size(12.5).color(DIM));
            });
        });
        self.map_view.color = color;
        self.map_view.satellite = sat;

        // The numbers, on the right.
        let side = Rect::from_min_size(pos2(rect.right() - panel_w - 14.0, rect.top() + 68.0), vec2(panel_w, rect.height() - 82.0));
        ov(ui, side, &mut |ui| {
            ui.set_width(panel_w - 12.0);
            Frame::new().inner_margin(Margin::same(8)).show(ui, |ui| {
                if whole {
                    whole_numbers(ui, &model, trace, &mut go_corner);
                } else {
                    corner_numbers(ui, &model, trace, corner, rpm, &session.laps, self.map_view.all_laps);
                }
            });
        });

        // The lap, what it's against, and every lap's line.
        let laps_with: Vec<usize> = (0..model.traces.len()).collect();
        let pos = laps_with.iter().position(|&t| t == trace).unwrap_or(0);
        let mut step: Option<usize> = None;
        let mut all = self.map_view.all_laps;
        ov(ui, Rect::from_min_size(pos2(rect.left() + 14.0, rect.bottom() - 58.0), vec2(rect.width() - panel_w - 60.0, 44.0)), &mut |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if chip(ui, "‹", false).on_hover_text("Previous lap").clicked() && pos > 0 {
                    step = Some(laps_with[pos - 1]);
                }
                let tr = &model.traces[trace];
                ui.label(RichText::new(format!("Lap {}", tr.number)).font(semibold(14.0)).color(TEXT));
                ui.label(RichText::new(format_lap_time(lap.duration_ms())).size(13.5).color(SECONDARY));
                if chip(ui, "›", false).on_hover_text("Next lap").clicked() && pos + 1 < laps_with.len() {
                    step = Some(laps_with[pos + 1]);
                }
                ui.label(RichText::new("against").size(13.0).color(DIM));
                let against = if reference == trace && whole {
                    "the best of every corner (this is the best lap)".to_string()
                } else if reference == trace {
                    "nothing: it's your best here".to_string()
                } else if whole {
                    format!("lap {} (the best lap)", model.traces[reference].number)
                } else {
                    format!("lap {} (your best corner {})", model.traces[reference].number, corner + 1)
                };
                ui.label(RichText::new(against).size(13.5).color(SECONDARY)).on_hover_text("The dashed line");
                ui.add_space(6.0);
                if chip(ui, "All laps", all).on_hover_text("Every lap that counts, faint: how repeatable the line is").clicked() {
                    all = !all;
                }
                if picked.is_none() {
                    ui.label(RichText::new(if self.lap.is_some() { "(that lap doesn't go all the way round: showing the best)" } else { "" }).size(12.0).color(DIM));
                }
            });
        });
        self.map_view.all_laps = all;
        // What the colors mean.
        ov(ui, Rect::from_min_size(pos2(rect.left() + 14.0, rect.bottom() - 100.0), vec2(rect.width() - panel_w - 60.0, 36.0)), &mut |ui| {
            legend(ui, color, &model, trace, rpm, lap.start_ms);
        });
        // Speed through the corner, this lap against the best of it, with how the kart slowed underneath.
        if !whole {
            let at_rect = Rect::from_min_size(pos2(rect.left() + 14.0, rect.bottom() - 100.0 - 10.0 - 150.0), vec2((rect.width() - panel_w - 60.0).min(620.0), 150.0));
            ov(ui, at_rect, &mut |ui| speed_trace(ui, &model, trace, reference, corner));
        }
        let credit = if self.map_view.satellite { "Imagery © Esri, Maxar, Earthstar Geographics" } else { "© OpenStreetMap contributors" };
        let ink = if self.map_view.satellite { Color32::from_white_alpha(150) } else { Color32::from_black_alpha(150) };
        ui.painter().text(rect.right_bottom() - vec2(panel_w + 26.0, 6.0), egui::Align2::RIGHT_BOTTOM, credit, regular(10.5), ink);

        // The arrow keys step through the corners.
        let (left, right) = ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
        if right {
            go_corner = Some(Some(if whole { 0 } else { (corner + 1) % n }));
        } else if left {
            go_corner = Some(Some(if whole { n - 1 } else { (corner + n - 1) % n }));
        }
        match go_corner {
            Some(None) => self.map_view.whole = true,
            Some(Some(k)) => {
                self.map_view.whole = false;
                self.corners.selected = Some((trace, k));
            }
            None => {}
        }
        if let Some(t) = step {
            self.lap = Some(model.traces[t].lap);
            self.corners.selected = Some((t, corner));
            self.cursor_s = None;
        }
    }

    /// Moves the view to (lat, lon, zoom) over a third of a second, once per new target.
    fn map_view_move_to(&mut self, to: (f64, f64, f64), ctx: &egui::Context) {
        let st = &mut self.map_view;
        let now = ctx.input(|i| i.time);
        let changed = st.target.is_none_or(|t| (t.0 - to.0).abs() > 1e-7 || (t.1 - to.1).abs() > 1e-7 || (t.2 - to.2).abs() > 1e-3);
        if changed {
            let from = st.shown.unwrap_or(to);
            st.anim = Some((from, now));
            st.target = Some(to);
        }
        if let Some((from, t0)) = st.anim {
            let f = ((now - t0) / 0.35).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - f).powi(3);
            let at = (from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e, from.2 + (to.2 - from.2) * e);
            st.memory.center_at(walkers::lat_lon(at.0, at.1));
            let _ = st.memory.set_zoom(at.2);
            st.shown = Some(at);
            if f < 1.0 {
                ctx.request_repaint();
            } else {
                st.anim = None;
            }
        }
    }
}

/// The color of each point of the lap's line.
fn line_colors(model: &CornerModel, trace: usize, reference: usize, by: ColorBy, rpm: Option<&Channel>, start_ms: i64) -> Vec<Color32> {
    let tr = &model.traces[trace];
    let n = tr.xy.len();
    match by {
        ColorBy::Slowing => {
            let mut c = vec![theme::GOOD; n];
            for a in &model.stats[trace] {
                let first = (a.brake_m / STEP_M).round().max(0.0) as usize;
                for (i, s) in a.slowing.iter().enumerate() {
                    if let Some(x) = c.get_mut(first + i) {
                        *x = match s {
                            Slowing::Brake => BRAKE,
                            Slowing::Trail => TRAIL,
                            Slowing::Lift => LIFT,
                        };
                    }
                }
            }
            c
        }
        ColorBy::Speed => {
            let (lo, hi) = super::robust_range(tr.kmh.iter().copied());
            tr.kmh.iter().map(|v| heat(((v - lo) / (hi - lo).max(1e-6)) as f32)).collect()
        }
        ColorBy::Rpm => {
            let Some(ch) = rpm else { return vec![theme::GOOD; n] };
            let v: Vec<f64> = tr.t.iter().map(|t| super::sample_at(ch, start_ms + (t * 1000.0) as i64).unwrap_or(0.0)).collect();
            let (lo, hi) = super::robust_range(v.iter().copied());
            v.iter().map(|x| heat(((x - lo) / (hi - lo).max(1e-6)) as f32)).collect()
        }
        ColorBy::Gain => {
            let r = &model.traces[reference];
            // How fast the gap to the reference grows (losing) or shrinks (gaining), per point.
            let rate: Vec<f64> = (0..n).map(|j| {
                let (a, b) = (j.saturating_sub(2), (j + 2).min(n - 1));
                (tr.t[b] - tr.t[a]) - (r.t[b] - r.t[a])
            }).collect();
            // Scaled to the 95th percentile, so one spike doesn't wash the rest out.
            let mut mags: Vec<f64> = rate.iter().map(|x| x.abs()).collect();
            mags.sort_by(f64::total_cmp);
            let scale = mags.get(mags.len() * 95 / 100).copied().unwrap_or(0.0).max(1e-6);
            rate.iter().map(|x| super::gain_color((x / scale) as f32)).collect()
        }
    }
}

/// The legend for what colors the line.
fn legend(ui: &mut Ui, by: ColorBy, model: &CornerModel, trace: usize, rpm: Option<&Channel>, start_ms: i64) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let key = |ui: &mut Ui, color: Color32, text: &str| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 6.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(3), color);
            ui.label(RichText::new(text).size(12.5).color(SECONDARY));
            ui.add_space(6.0);
        };
        let ramp = |ui: &mut Ui, f: &dyn Fn(f32) -> Color32| {
            let (bar, _) = ui.allocate_exact_size(vec2(120.0, 6.0), Sense::hover());
            for j in 0..40 {
                let r = Rect::from_min_size(pos2(bar.left() + bar.width() * j as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                ui.painter().rect_filled(r, 0.0, f(j as f32 / 39.0));
            }
        };
        match by {
            ColorBy::Slowing => {
                key(ui, theme::GOOD, "Not slowing");
                key(ui, BRAKE, "Braking");
                if model.has_trail {
                    key(ui, TRAIL, "Braking while turning (trail)");
                }
                key(ui, LIFT, "Lift or slide");
            }
            ColorBy::Gain => {
                ui.label(RichText::new("Gaining").size(12.5).color(SECONDARY));
                ramp(ui, &|t| super::gain_color(-1.0 + 2.0 * t));
                ui.label(RichText::new("Losing time").size(12.5).color(SECONDARY));
            }
            ColorBy::Speed => {
                let (lo, hi) = super::robust_range(model.traces[trace].kmh.iter().copied());
                let u = crate::units::speed_unit();
                ui.label(RichText::new(format!("{:.0} {u}", crate::units::speed(lo))).size(12.5).color(SECONDARY));
                ramp(ui, &heat);
                ui.label(RichText::new(format!("{:.0} {u}", crate::units::speed(hi))).size(12.5).color(SECONDARY));
            }
            ColorBy::Rpm => {
                if let Some(ch) = rpm {
                    let v = model.traces[trace].t.iter().filter_map(|t| super::sample_at(ch, start_ms + (t * 1000.0) as i64));
                    let (lo, hi) = super::robust_range(v);
                    ui.label(RichText::new(format!("{lo:.0} rpm")).size(12.5).color(SECONDARY));
                    ramp(ui, &heat);
                    ui.label(RichText::new(format!("{hi:.0} rpm")).size(12.5).color(SECONDARY));
                }
            }
        }
    });
}

/// The whole track's numbers: what's left on this lap, corner by corner.
fn whole_numbers(ui: &mut Ui, model: &CornerModel, trace: usize, go: &mut Option<Option<usize>>) {
    let tr = &model.traces[trace];
    let lap_s: f64 = model.stats[trace].iter().map(|s| s.time_s).sum();
    let left = (lap_s - model.ideal_s()).max(0.0);
    ui.label(RichText::new(format!("Lap {}", tr.number)).font(semibold(18.0)).color(TEXT));
    ui.label(RichText::new("against the best of every corner").size(12.5).color(DIM));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{left:.2} s")).font(semibold(22.0)).color(theme::FAVORITE));
        ui.label(RichText::new("left on this lap").size(13.0).color(SECONDARY));
    });
    ui.add_space(8.0);
    for k in 0..model.segments.len() {
        let best = model.best_of[k] == trace;
        let loss = model.loss(trace, k);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(6), SURFACE_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(rect.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, format!("Corner {}", k + 1), regular(13.5), TEXT);
        let (text, color) = if best { ("best".to_string(), theme::GOOD) } else { (format!("+{loss:.2}"), if loss >= 0.005 { BRAKE } else { SECONDARY }) };
        ui.painter().text(rect.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, text, medium(13.5), color);
        if resp.clicked() {
            *go = Some(Some(k));
        }
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Pick a corner to zoom in on it.").size(12.0).color(DIM));
}

/// One corner's numbers: this lap against the best you drove it, as in Corners' up close.
fn corner_numbers(ui: &mut Ui, model: &CornerModel, trace: usize, corner: usize, rpm: Option<&Channel>, laps: &[aim_xrk::Lap], all_laps: bool) {
    let bi = model.best_of[corner];
    let own = bi == trace;
    let (a, b) = (&model.stats[trace][corner], &model.stats[bi][corner]);
    let d = a.time_s - b.time_s;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Corner {}", corner + 1)).font(semibold(18.0)).color(TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = if own { format!("lap {}: your best here", model.traces[trace].number) } else { format!("lap {} vs {}", model.traces[trace].number, model.traces[bi].number) };
            ui.label(RichText::new(text).size(12.0).color(DIM));
        });
    });
    ui.horizontal(|ui| {
        if own {
            ui.label(RichText::new("best").font(semibold(22.0)).color(theme::GOOD));
        } else {
            ui.label(RichText::new(format!("+{d:.2} s")).font(semibold(22.0)).color(if d >= 0.005 { BRAKE } else { SECONDARY }));
            ui.label(RichText::new("lost here").size(13.0).color(SECONDARY));
        }
    });
    ui.add_space(6.0);
    let (su, du) = (crate::units::speed_unit(), crate::units::distance_unit());
    let speed = |v: f64| crate::units::speed(v);
    let apex_rpm = |t: usize, s: &crate::corner_model::Stats| {
        let ch = rpm?;
        let tr = &model.traces[t];
        let i = ((s.min_m / STEP_M).round() as usize).min(tr.t.len() - 1);
        super::sample_at(ch, laps[tr.lap].start_ms + (tr.t[i] * 1000.0) as i64)
    };
    let peak_g = |t: usize| {
        let s = model.segments[corner];
        let tr = &model.traces[t];
        let (i0, i1) = ((s.from / STEP_M) as usize, ((s.to / STEP_M) as usize).min(tr.cornering.len() - 1));
        tr.cornering[i0..=i1].iter().fold(0.0_f64, |m, g| m.max(g.abs()))
    };
    // (label, this lap, the best, higher is better, decimals, unit, big)
    let mut rows: Vec<(&str, f64, f64, bool, usize, String, bool)> = vec![
        ("Entry", speed(a.entry_kmh), speed(b.entry_kmh), true, 1, su.to_string(), true),
        ("Apex", speed(a.min_kmh), speed(b.min_kmh), true, 1, su.to_string(), true),
        ("Down the straight", speed(a.exit_kmh), speed(b.exit_kmh), true, 1, su.to_string(), true),
        ("Brakes at", crate::units::distance(a.brake_m), crate::units::distance(b.brake_m), true, 0, du.to_string(), false),
        ("On the brakes", a.brake_s, b.brake_s, false, 2, "s".into(), false),
    ];
    if let (Some(x), Some(y)) = (apex_rpm(trace, a), apex_rpm(bi, b)) {
        rows.push(("Apex RPM", x, y, true, 0, "".into(), false));
    }
    if let (Some((x, _)), Some((y, _))) = (a.steering, b.steering) {
        rows.push(("Steering", x, y, false, 0, "°".into(), false));
    }
    rows.push(("Cornering", peak_g(trace), peak_g(bi), true, 2, "g".into(), false));
    egui::Grid::new(("map_numbers", corner)).num_columns(3).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        ui.label("");
        ui.label(RichText::new(format!("LAP {}", model.traces[trace].number)).font(medium(10.5)).color(DIM));
        ui.label(RichText::new(if own { String::new() } else { format!("LAP {}", model.traces[bi].number) }).font(medium(10.5)).color(DIM));
        ui.end_row();
        for (label, x, y, higher, dec, unit, big) in rows {
            ui.label(RichText::new(label).size(13.0).color(SECONDARY));
            let size = if big { 17.0 } else { 13.5 };
            ui.label(RichText::new(format!("{x:.dec$} {unit}")).font(if big { semibold(size) } else { regular(size) }).color(TEXT));
            if own {
                ui.label("");
            } else {
                let worse = if higher { x < y } else { x > y };
                let close = (x - y).abs() < 0.5 * 10f64.powi(-(dec as i32));
                ui.label(RichText::new(format!("{y:.dec$}")).size(13.5).color(if close || !worse { DIM } else { Color32::from_rgb(0xf3, 0x9a, 0x9a) }));
            }
            ui.end_row();
        }
    });
    ui.add_space(8.0);
    ui.add(egui::Label::new(RichText::new(format!("Brakes at: {du} from the start of the lap.")).size(12.0).color(DIM)).wrap());
    // How repeatable the line is at the apex: the spread of where each counting lap went.
    if all_laps {
        let s = model.segments[corner];
        let best_xy = &model.traces[model.best].xy;
        let i = ((s.apex / STEP_M).round() as usize).min(best_xy.len() - 2);
        let (p, q) = (best_xy[i.saturating_sub(1)], best_xy[i + 1]);
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let offs: Vec<f64> = (0..model.traces.len()).filter(|&t| model.counts[t]).map(|t| {
            let a = model.traces[t].xy[i];
            ((a.0 - best_xy[i].0) * -dy + (a.1 - best_xy[i].1) * dx) / len
        }).collect();
        if offs.len() > 1 {
            let mean = offs.iter().sum::<f64>() / offs.len() as f64;
            let sd = (offs.iter().map(|o| (o - mean).powi(2)).sum::<f64>() / offs.len() as f64).sqrt();
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(format!("Line at the apex across {} laps: ±{:.1} {du}", offs.len(), crate::units::distance(sd))).size(12.5).color(TEXT)).wrap());
        }
    } else {
        ui.add(egui::Label::new(RichText::new("Turn on All laps to see how repeatable the line is here.").size(12.0).color(DIM)).wrap());
    }
}

/// Speed through the corner by distance: this lap against the best of it, and how each slowed.
fn speed_trace(ui: &mut Ui, model: &CornerModel, trace: usize, reference: usize, corner: usize) {
    let s = model.segments[corner];
    let last = model.traces[trace].kmh.len() - 1;
    let idx = |m: f64| ((m / STEP_M).round().max(0.0) as usize).min(last);
    let ks: Vec<usize> = (idx(s.from - 10.0)..=idx(s.to)).collect();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 136.0), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(44.0, 6.0), rect.max - vec2(6.0, 40.0));
    let speeds = || [trace, reference].into_iter().flat_map(|t| ks.iter().map(move |&k| crate::units::speed(model.traces[t].kmh[k])));
    let (lo, hi) = (speeds().fold(f64::MAX, f64::min) - 1.0, speeds().fold(f64::MIN, f64::max) + 1.0);
    let x = |j: usize| plot.left() + j as f32 / (ks.len() - 1).max(1) as f32 * plot.width();
    let y = |v: f64| plot.bottom() - ((v - lo) / (hi - lo)) as f32 * plot.height();
    let p = ui.painter_at(rect);
    p.text(pos2(rect.left(), plot.top() + 4.0), egui::Align2::LEFT_CENTER, format!("{hi:.0}"), regular(10.5), DIM);
    p.text(pos2(rect.left(), plot.bottom()), egui::Align2::LEFT_CENTER, format!("{lo:.0}"), regular(10.5), DIM);
    p.text(pos2(rect.left(), plot.center().y), egui::Align2::LEFT_CENTER, crate::units::speed_unit(), regular(10.5), DIM);
    let apex_j = ks.iter().position(|&k| k >= idx(model.stats[trace][corner].min_m)).unwrap_or(0);
    p.extend(egui::Shape::dashed_line(&[pos2(x(apex_j), plot.top()), pos2(x(apex_j), plot.bottom())], Stroke::new(1.0, DIM), 3.0, 3.0));
    p.text(pos2(x(apex_j) + 4.0, plot.top() + 4.0), egui::Align2::LEFT_CENTER, "apex", regular(10.5), DIM);
    let line = |t: usize| -> Vec<egui::Pos2> { ks.iter().enumerate().map(|(j, &k)| pos2(x(j), y(crate::units::speed(model.traces[t].kmh[k])))).collect() };
    if reference != trace {
        p.extend(egui::Shape::dashed_line(&line(reference), Stroke::new(1.5, Color32::from_white_alpha(200)), 5.0, 4.0));
    }
    p.add(egui::Shape::line(line(trace), Stroke::new(2.2, theme::ACCENT)));
    // How each slowed, as strips under the chart.
    for (row, t) in [trace, reference].into_iter().enumerate() {
        let top = plot.bottom() + 8.0 + row as f32 * 14.0;
        p.text(pos2(rect.left(), top + 4.5), egui::Align2::LEFT_CENTER, format!("lap {}", model.traces[t].number), regular(10.5), DIM);
        let a = &model.stats[t][corner];
        let first = idx(a.brake_m);
        for (j, &k) in ks.iter().enumerate().take(ks.len() - 1) {
            let c = if k >= first && k - first < a.slowing.len() {
                match a.slowing[k - first] {
                    Slowing::Brake => BRAKE,
                    Slowing::Trail => TRAIL,
                    Slowing::Lift => LIFT,
                }
            } else {
                theme::GOOD
            };
            p.rect_filled(Rect::from_min_max(pos2(x(j), top), pos2(x(j + 1) + 0.5, top + 9.0)), 0.0, c);
        }
        if reference == trace {
            break;
        }
    }
}
