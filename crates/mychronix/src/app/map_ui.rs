//! The Map view: the track up close, a corner at a time, drawn as a track or
//! on satellite imagery. The lap's line through each corner against the best you
//! drove that corner (the same comparison Corners makes, see `corner_model`),
//! colored by how the kart slowed, speed, RPM or where time was gained and
//! lost; every lap faint, to see how repeatable the line is; the corner's
//! numbers beside it, and speed through it underneath.

use std::sync::{Arc, Mutex};

use super::corners_ui::{APEX_NOTE, lap_position};
use super::*;
use crate::corner_model::{CornerModel, STEP_M, Slowing};

const BRAKE: Color32 = theme::LOSE;
const TRAIL: Color32 = theme::TRAIL;
const LIFT: Color32 = theme::LIFT;
/// The drawn track (no imagery): its asphalt, edges and the ground around it.
const TRACK: Color32 = Color32::from_rgb(0x22, 0x27, 0x2d);
const TRACK_EDGE: Color32 = Color32::from_rgb(0x3a, 0x41, 0x4b);
const GROUND: Color32 = Color32::from_rgb(0x0c, 0x0e, 0x11);
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

/// Narrower than this, the Map's numbers move from the side into an overlay.
const MAP_COMPACT_W: f32 = 1000.0;

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
    /// The key's size when last drawn, to fit it beside the lap picker.
    legend: Vec2,
    /// In a narrow window: the numbers open over the map.
    numbers_open: bool,
}

impl Default for MapState {
    fn default() -> Self {
        Self {
            whole: false,
            color: ColorBy::default(),
            all_laps: false,
            satellite: false,
            memory: walkers::MapMemory::default(),
            target: None,
            shown: None,
            anim: None,
            legend: Vec2::ZERO,
            numbers_open: false,
        }
    }
}

/// Meters east and north of (lat0, lon0) back to a position (the inverse of the corner model's projection).
fn to_lat_lon(lat0: f64, lon0: f64, (x, y): (f64, f64)) -> aim_track::LatLon {
    let (lat, lon) = aim_xrk::geo::from_local(lat0, lon0, (x, y));
    aim_track::LatLon { lat, lon }
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
    let c = aim_track::LatLon {
        lat: (lat0 + lat1) / 2.0,
        lon: (lon0 + lon1) / 2.0,
    };
    let width_m = ((lon1 - lon0) * aim_xrk::geo::m_per_deg_lon(c.lat)).max(15.0);
    let height_m = ((lat1 - lat0) * aim_xrk::geo::M_PER_DEG_LAT).max(15.0);
    // Meters per pixel at zoom z (256-pixel tiles): 156543.03 × cos(lat) / 2^z.
    let mpp = (width_m / (size.x as f64 * 0.85)).max(height_m / (size.y as f64 * 0.85));
    let zoom = (156_543.03 * c.lat.to_radians().cos() / mpp)
        .log2()
        .clamp(3.0, 21.5);
    Some((c, zoom))
}

/// One line to draw on the map.
struct Path {
    points: Vec<aim_track::LatLon>,
    colors: Vec<Color32>,
    width: f32,
    dashed: bool,
}

/// Draws the track (without imagery), the lines and the corner numbers;
/// reports a corner number clicked.
struct Overlay {
    /// The track drawn under the lines when there's no imagery: its middle,
    /// how wide it is (m), and where the start/finish line is.
    track: Option<(Vec<aim_track::LatLon>, f64, bool)>,
    paths: Vec<Path>,
    /// Each corner's number, its apex, a point a meter toward the outside of
    /// the corner (to place the number beside the line), and whether it's the one shown.
    corners: Vec<(usize, aim_track::LatLon, aim_track::LatLon, bool)>,
    clicked: Arc<Mutex<Option<usize>>>,
}

impl walkers::Plugin for Overlay {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        response: &egui::Response,
        projector: &walkers::Projector,
        _memory: &walkers::MapMemory,
    ) {
        ui.set_clip_rect(response.rect.intersect(ui.clip_rect()));
        let painter = ui.painter();
        let to_screen =
            |p: &aim_track::LatLon| projector.project(walkers::lat_lon(p.lat, p.lon)).to_pos2();
        if let Some((middle, width_m, closed)) = &self.track {
            let pts: Vec<egui::Pos2> = middle.iter().map(to_screen).collect();
            let px = projector
                .scale_pixel_per_meter(walkers::lat_lon(middle[0].lat, middle[0].lon))
                * *width_m as f32;
            // A loop is drawn closed, so the track joins itself at start/finish.
            let line = |pts: Vec<egui::Pos2>, stroke| {
                if *closed {
                    egui::Shape::closed_line(pts, stroke)
                } else {
                    egui::Shape::line(pts, stroke)
                }
            };
            painter.add(line(pts.clone(), Stroke::new(px + 3.0, TRACK_EDGE)));
            painter.add(line(pts.clone(), Stroke::new(px, TRACK)));
            // Start/finish: a dashed line across the track.
            if pts.len() > 2 {
                let d = (pts[1] - pts[0]).normalized();
                let n = vec2(-d.y, d.x) * px / 2.0;
                painter.extend(egui::Shape::dashed_line(
                    &[pts[0] - n, pts[0] + n],
                    Stroke::new(3.0, TEXT),
                    4.0,
                    4.0,
                ));
            }
        }
        for path in &self.paths {
            let pts: Vec<egui::Pos2> = path.points.iter().map(to_screen).collect();
            if path.dashed {
                painter.extend(egui::Shape::dashed_line(
                    &pts,
                    Stroke::new(path.width, path.colors[0]),
                    6.0,
                    5.0,
                ));
                continue;
            }
            if path.colors.len() == 1 {
                painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(path.width, path.colors[0]),
                ));
                continue;
            }
            // A dark casing so the colors read over satellite imagery.
            painter.add(egui::Shape::line(
                pts.clone(),
                Stroke::new(path.width + 3.0, Color32::from_black_alpha(140)),
            ));
            for k in 0..pts.len().saturating_sub(1) {
                painter.line_segment(
                    [pts[k], pts[k + 1]],
                    Stroke::new(path.width, path.colors[k.min(path.colors.len() - 1)]),
                );
            }
        }
        for (number, apex, outside, on) in &self.corners {
            let (a, o) = (to_screen(apex), to_screen(outside));
            let c = a + (o - a).normalized() * 22.0;
            let r = if *on { 13.0 } else { 11.0 };
            painter.circle_filled(c, r + 2.0, Color32::from_black_alpha(170));
            painter.circle_filled(c, r, if *on { TEXT } else { SURFACE_ACTIVE });
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                (number + 1).to_string(),
                semibold(12.5),
                if *on { ON_TEXT } else { TEXT },
            );
            // Click and drag: a hand moves a little while pressing, and the
            // map underneath would take that as a drag and pan away.
            let hit = ui.interact(
                Rect::from_center_size(c, Vec2::splat(30.0)),
                ui.id().with(("map_corner", number)),
                Sense::click_and_drag(),
            );
            if hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if hit.clicked() {
                *self.clicked.lock().unwrap() = Some(*number);
            }
        }
    }
}

/// What the Map shows: the lap, the corner, and what they're measured against.
struct MapPick<'a> {
    session: &'a Session,
    model: &'a CornerModel,
    /// The lap looked at: the one picked, or the best standing in for it.
    trace: usize,
    /// Whether the lap looked at is the one picked, and whether one was.
    picked: bool,
    lap_asked: bool,
    corner: usize,
    whole: bool,
    /// Against: the best you drove this corner, or for the whole track, the best lap.
    reference: usize,
    rpm: Option<&'a Channel>,
    lap: aim_xrk::Lap,
    /// Where the corner model's meters east and north start.
    origin: (f64, f64),
}

impl<'a> MapPick<'a> {
    fn new(
        session: &'a Session,
        model: &'a CornerModel,
        lap: Option<usize>,
        selected: Option<(usize, usize)>,
        whole: bool,
    ) -> Option<Self> {
        let gps = session.gps.as_ref()?;
        let picked = lap.and_then(|i| model.trace_of(i));
        let trace = picked.unwrap_or(model.best);
        let corner = match selected {
            Some((t, k)) if t == trace && k < model.segments.len() => k,
            _ => model.worst(trace),
        };
        Some(Self {
            session,
            model,
            trace,
            picked: picked.is_some(),
            lap_asked: lap.is_some(),
            corner,
            whole,
            reference: if whole {
                model.best
            } else {
                model.best_of[corner]
            },
            rpm: crate::channels::find(session, crate::channels::Role::Rpm).filter(|c| !is_dead(c)),
            lap: session.laps[model.traces[trace].lap],
            origin: (gps.latitude[0], gps.longitude[0]),
        })
    }

    fn lat_lon(&self, p: (f64, f64)) -> aim_track::LatLon {
        to_lat_lon(self.origin.0, self.origin.1, p)
    }

    /// The best lap's point `m` meters in.
    fn at(&self, m: f64) -> usize {
        let best_xy = &self.model.traces[self.model.best].xy;
        ((m / STEP_M).round().max(0.0) as usize).min(best_xy.len() - 1)
    }
}

/// What was clicked or pressed on the Map this frame, applied together once it's drawn.
#[derive(Default)]
struct MapInput {
    /// A corner (None: the whole track).
    go_corner: Option<Option<usize>>,
    /// Another lap's trace.
    step: Option<usize>,
    color: Option<ColorBy>,
    all_laps: Option<bool>,
    satellite: Option<bool>,
    /// The numbers' overlay opened or closed.
    numbers: Option<bool>,
}

/// The side panel's width beside the map.
const MAP_PANEL_W: f32 = 300.0;

impl App {
    pub(super) fn map_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        let Some(model) = self.corner_model(session, ui.ctx()) else {
            card(ui, |ui| {
                ui.label(
                    RichText::new("The map needs GPS for the lap, and a corner to brake for.")
                        .color(SECONDARY),
                );
            });
            return;
        };
        let Some(pick) = MapPick::new(
            session,
            &model,
            self.analysis.lap,
            self.corners.selected,
            self.map_view.whole,
        ) else {
            return;
        };
        // The corner is shared with Corners.
        self.corners.selected = Some((pick.trace, pick.corner));

        let rect = ui.available_rect_before_wrap();
        // In a narrow window the numbers leave the side for an overlay, opened with "i".
        let compact = rect.width() < MAP_COMPACT_W;
        let panel_w = if compact { 0.0 } else { MAP_PANEL_W };
        self.map_aim(&pick, rect, panel_w, ui.ctx());
        let mut input = MapInput::default();
        let rect = self.map_draw(ui, &pick, &mut input);
        self.map_controls(ui, &pick, rect, compact, panel_w, &mut input);
        map_keys(ui, &pick, &mut input);
        self.map_apply(&pick, input);
    }

    /// The lines, corner numbers and (without imagery) the track, for the map to draw.
    fn map_overlay(&mut self, pick: &MapPick, clicked: Arc<Mutex<Option<usize>>>) -> Overlay {
        let model = pick.model;
        let (trace, reference) = (pick.trace, pick.reference);
        let colors = line_colors(
            model,
            trace,
            reference,
            self.map_view.color,
            pick.rpm,
            pick.lap.start_ms,
        );
        let latlon = |t: usize| -> Vec<aim_track::LatLon> {
            model.traces[t]
                .xy
                .iter()
                .map(|&p| pick.lat_lon(p))
                .collect()
        };
        let mut paths = Vec::new();
        if self.map_view.all_laps {
            for t in (0..model.traces.len()).filter(|&t| model.counts[t] && t != trace) {
                paths.push(Path {
                    points: latlon(t),
                    colors: vec![Color32::from_white_alpha(70)],
                    width: 1.2,
                    dashed: false,
                });
            }
        }
        if reference != trace {
            paths.push(Path {
                points: latlon(reference),
                colors: vec![Color32::from_white_alpha(220)],
                width: 1.6,
                dashed: true,
            });
        }
        paths.push(Path {
            points: latlon(trace),
            colors,
            width: 4.0,
            dashed: false,
        });
        let best_xy = &model.traces[model.best].xy;
        let corners = model
            .segments
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let (mid, off) = corner_label(best_xy, pick.at(s.apex));
                (
                    k,
                    pick.lat_lon(mid),
                    pick.lat_lon(off),
                    !pick.whole && k == pick.corner,
                )
            })
            .collect();
        // Without imagery, the track itself, worked out from where the laps went.
        let width = self
            .path
            .clone()
            .and_then(|p| {
                let lib = self.lib();
                let t = &lib.tracks[lib.sessions.iter().find(|s| s.path == p)?.track?];
                Some(track_width(
                    &crate::settings::get().track_widths,
                    &t.name,
                    t.track_kind,
                ))
            })
            .unwrap_or(8.0);
        let track = (!self.map_view.satellite).then(|| {
            let (middle, closed) = track_shape(model, width);
            (
                middle.iter().map(|&p| pick.lat_lon(p)).collect(),
                width,
                closed,
            )
        });
        Overlay {
            track,
            paths,
            corners,
            clicked,
        }
    }

    /// Where to look: the whole track, or the corner from its braking point to
    /// the next one's, clear of the panel on the right and the trace at the bottom.
    fn map_aim(&mut self, pick: &MapPick, rect: Rect, panel_w: f32, ctx: &egui::Context) {
        let best_xy = &pick.model.traces[pick.model.best].xy;
        let view_pts: Vec<aim_track::LatLon> = if pick.whole {
            best_xy.iter().map(|&p| pick.lat_lon(p)).collect()
        } else {
            // From before the braking point to a little past the apex: the corner, not the straight after it.
            let s = pick.model.segments[pick.corner];
            let from = (s.from - 15.0).min(s.apex - 60.0);
            (pick.at(from)..=pick.at((s.apex + 45.0).min(s.to)))
                .map(|i| pick.lat_lon(best_xy[i]))
                .collect()
        };
        let free = vec2(
            (rect.width() - panel_w - 40.0).max(200.0),
            (rect.height() - if pick.whole { 120.0 } else { 300.0 }).max(200.0),
        );
        if let Some((c, zoom)) = fit(&view_pts, free) {
            let mpp = 156_543.03 * c.lat.to_radians().cos() / 2f64.powf(zoom);
            let east = (panel_w as f64 + 20.0) / 2.0 * mpp;
            let south = if pick.whole { 30.0 } else { 140.0 } * mpp;
            let (lat, lon) = aim_xrk::geo::from_local(c.lat, c.lon, (east, -south));
            self.map_view_move_to((lat, lon, zoom), ctx);
        }
    }

    /// The map itself, with imagery or the track drawn in; a corner number
    /// clicked on it goes to that corner. Gives where it was drawn.
    fn map_draw(&mut self, ui: &mut Ui, pick: &MapPick, input: &mut MapInput) -> Rect {
        let clicked = Arc::new(Mutex::new(None));
        let overlay = self.map_overlay(pick, clicked.clone());
        let start = walkers::lat_lon(pick.origin.0, pick.origin.1);
        let mut memory = std::mem::take(&mut self.map_view.memory);
        ui.painter().rect_filled(
            ui.available_rect_before_wrap(),
            CornerRadius::same(theme::RADIUS),
            GROUND,
        );
        // The wheel zooms, as on any map; dragging moves it.
        let map = if self.map_view.satellite {
            let tiles =
                super::tracks_ui::tiles(&mut self.map_tiles, &mut self.sat_tiles, ui.ctx(), true);
            ui.add(
                walkers::Map::new(super::tracks_ui::shown(tiles), &mut memory, start)
                    .zoom_with_ctrl(false)
                    .with_plugin(overlay),
            )
        } else {
            ui.add(
                walkers::Map::new(None, &mut memory, start)
                    .zoom_with_ctrl(false)
                    .with_plugin(overlay),
            )
        };
        self.map_view.memory = memory;
        if let Some(k) = clicked.lock().unwrap().take() {
            input.go_corner = Some(Some(k));
        }
        map.rect
    }

    /// Everything over the map: corners along the top left, the line's color
    /// top right, the numbers on the right (or behind "i"), the lap bottom left
    /// with the key beside it, speed above, and the imagery switch bottom right.
    fn map_controls(
        &mut self,
        ui: &mut Ui,
        pick: &MapPick,
        rect: Rect,
        compact: bool,
        panel_w: f32,
        input: &mut MapInput,
    ) {
        let corners_row = corner_chips(ui, rect, pick, input);
        // The color choice goes under the corners when they'd run into each other.
        let color_top = if corners_row.right() + 12.0 > rect.right() - 474.0 {
            rect.top() + 66.0
        } else {
            rect.top() + 14.0
        };
        color_chips(ui, rect, color_top, pick, self.map_view.color, input);
        let color = input.color.unwrap_or(self.map_view.color);

        let all_laps = self.map_view.all_laps;
        if compact {
            let at = Rect::from_min_size(
                pos2(rect.right() - 14.0 - 40.0, color_top + 54.0),
                Vec2::splat(40.0),
            );
            if numbers_button(ui, at, pick.whole) {
                input.numbers = Some(true);
            }
        } else {
            let side = Rect::from_min_size(
                pos2(rect.right() - panel_w - 14.0, rect.top() + 68.0),
                vec2(panel_w, rect.height() - 82.0),
            );
            glass(ui, side, &mut |ui| {
                ui.set_width(panel_w - 12.0);
                Frame::new()
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| numbers(ui, pick, all_laps, &mut input.go_corner));
            });
        }

        let picker = lap_picker(ui, rect, panel_w, pick, all_laps, input);
        // What the colors mean: on the bottom row, centered in the room between
        // the lap picker and the satellite switch, or above the lap picker when
        // that room is too narrow. Its size is measured as it's drawn, for the next frame.
        let switch = Rect::from_min_size(
            rect.right_bottom() - vec2(14.0 + 150.0, 14.0 + 44.0),
            vec2(150.0, 44.0),
        );
        let (room_l, room_r) = (picker.right() + 12.0, switch.left() - 12.0);
        let size = self.map_view.legend;
        let beside = size.x > 0.0 && size.x <= room_r - room_l;
        let at = if beside {
            Rect::from_min_size(
                pos2(
                    (room_l + room_r - size.x) / 2.0,
                    picker.center().y - size.y / 2.0,
                ),
                size + vec2(1.0, 1.0),
            )
        } else {
            Rect::from_min_size(
                pos2(rect.left() + 14.0, rect.bottom() - 100.0),
                vec2(rect.width() - panel_w - 60.0, 36.0),
            )
        };
        self.map_view.legend = glass(ui, at, &mut |ui| {
            legend(
                ui,
                color,
                pick.model,
                pick.trace,
                pick.rpm,
                pick.lap.start_ms,
            )
        })
        .size();
        // Speed through the corner, this lap against the best of it, with how the kart slowed underneath.
        if !pick.whole {
            let above = if beside { 58.0 } else { 100.0 };
            let at_rect = Rect::from_min_size(
                pos2(rect.left() + 14.0, rect.bottom() - above - 10.0 - 150.0),
                vec2((rect.width() - panel_w - 60.0).min(620.0), 150.0),
            );
            glass(ui, at_rect, &mut |ui| {
                speed_trace(ui, pick.model, pick.trace, pick.reference, pick.corner)
            });
        }
        satellite_switch(ui, switch, self.map_view.satellite, input);
        // The numbers over the whole map, until X or Escape (or a corner picked from them).
        if compact && input.numbers.unwrap_or(self.map_view.numbers_open) {
            let (close, go) = numbers_overlay(ui, rect, pick, all_laps);
            if go.is_some() {
                input.go_corner = go;
            }
            if close || go.is_some() {
                input.numbers = Some(false);
            }
        }
        if self.map_view.satellite {
            ui.painter().text(
                switch.right_top() + vec2(-4.0, -6.0),
                egui::Align2::RIGHT_BOTTOM,
                "Imagery © Esri, Maxar, Earthstar Geographics",
                regular(10.5),
                Color32::from_white_alpha(150),
            );
        }
    }

    /// What was clicked or pressed this frame, now that the Map is drawn.
    fn map_apply(&mut self, pick: &MapPick, input: MapInput) {
        let st = &mut self.map_view;
        if let Some(c) = input.color {
            st.color = c;
        }
        if let Some(a) = input.all_laps {
            st.all_laps = a;
        }
        if let Some(s) = input.satellite {
            st.satellite = s;
        }
        if let Some(open) = input.numbers {
            st.numbers_open = open;
        }
        match input.go_corner {
            Some(None) => st.whole = true,
            Some(Some(k)) => {
                st.whole = false;
                self.corners.selected = Some((pick.trace, k));
            }
            None => {}
        }
        if let Some(t) = input.step {
            self.analysis.lap = Some(pick.model.traces[t].lap);
            self.corners.selected = Some((t, pick.corner));
            self.analysis.cursor_s = None;
        }
    }

    /// Moves the view to (lat, lon, zoom) over a third of a second, once per new target.
    fn map_view_move_to(&mut self, to: (f64, f64, f64), ctx: &egui::Context) {
        let st = &mut self.map_view;
        let now = ctx.input(|i| i.time);
        let changed = st.target.is_none_or(|t| {
            (t.0 - to.0).abs() > 1e-7 || (t.1 - to.1).abs() > 1e-7 || (t.2 - to.2).abs() > 1e-3
        });
        if changed {
            let from = st.shown.unwrap_or(to);
            st.anim = Some((from, now));
            st.target = Some(to);
        }
        if let Some((from, t0)) = st.anim {
            // A screenshot is taken where the view ends up, not partway there.
            let f = if crate::hooks::get().screenshot.is_some() {
                1.0
            } else {
                ((now - t0) / 0.35).clamp(0.0, 1.0)
            };
            let e = 1.0 - (1.0 - f).powi(3);
            let at = (
                from.0 + (to.0 - from.0) * e,
                from.1 + (to.1 - from.1) * e,
                from.2 + (to.2 - from.2) * e,
            );
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

/// A control over the map, on frosted glass, within `at`; gives the rect it took.
fn glass(ui: &mut Ui, at: Rect, add: &mut dyn FnMut(&mut Ui)) -> Rect {
    ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
        Frame::new()
            .fill(GLASS)
            .stroke(Stroke::new(1.0, theme::OUTLINE))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(6))
            .show(ui, |ui| add(ui))
            .response
            .rect
    })
    .inner
}

/// "Whole track" and each corner's number, top left.
fn corner_chips(ui: &mut Ui, rect: Rect, pick: &MapPick, input: &mut MapInput) -> Rect {
    let at = Rect::from_min_size(
        rect.left_top() + vec2(14.0, 14.0),
        vec2(rect.width() * 0.55, 44.0),
    );
    glass(ui, at, &mut |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if chip(ui, "Whole track", pick.whole).clicked() {
                input.go_corner = Some(None);
            }
            for k in 0..pick.model.segments.len() {
                if chip(ui, &(k + 1).to_string(), !pick.whole && k == pick.corner).clicked() {
                    input.go_corner = Some(Some(k));
                }
            }
        });
    })
}

/// What colors the lap's line, top right (from `top`).
fn color_chips(
    ui: &mut Ui,
    rect: Rect,
    top: f32,
    pick: &MapPick,
    current: ColorBy,
    input: &mut MapInput,
) {
    let at = Rect::from_min_size(pos2(rect.right() - 474.0, top), vec2(460.0, 44.0));
    glass(ui, at, &mut |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (c, label) in [
                (ColorBy::Gain, "Gain / lose"),
                (ColorBy::Rpm, "RPM"),
                (ColorBy::Speed, "Speed"),
                (ColorBy::Slowing, "Braking · trail · lift"),
            ] {
                if c == ColorBy::Rpm && pick.rpm.is_none() {
                    continue;
                }
                if chip(ui, label, input.color.unwrap_or(current) == c).clicked() {
                    input.color = Some(c);
                }
            }
            ui.label(RichText::new("Color the line by").size(12.5).color(DIM));
        });
    });
}

/// The numbers: the whole lap's, or the corner's against the best of it.
fn numbers(ui: &mut Ui, pick: &MapPick, all_laps: bool, go: &mut Option<Option<usize>>) {
    if pick.whole {
        whole_numbers(ui, pick.model, pick.trace, go);
    } else {
        corner_numbers(
            ui,
            pick.model,
            pick.trace,
            pick.corner,
            pick.rpm,
            &pick.session.laps,
            all_laps,
        );
    }
}

/// The round "i" that opens the numbers in a narrow window; true when clicked.
fn numbers_button(ui: &mut Ui, at: Rect, whole: bool) -> bool {
    let resp = ui
        .interact(at, ui.id().with("map_numbers_open"), Sense::click())
        .on_hover_text(if whole {
            "The lap's numbers"
        } else {
            "This corner's numbers"
        });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let p = ui.painter();
    p.circle(at.center(), 20.0, GLASS, Stroke::new(1.0, theme::OUTLINE));
    p.text(
        at.center(),
        egui::Align2::CENTER_CENTER,
        "i",
        semibold(17.0),
        if resp.hovered() { TEXT } else { SECONDARY },
    );
    resp.clicked()
}

/// The lap, what it's against, and every lap's line, bottom left; gives the rect it took.
fn lap_picker(
    ui: &mut Ui,
    rect: Rect,
    panel_w: f32,
    pick: &MapPick,
    all_laps: bool,
    input: &mut MapInput,
) -> Rect {
    let model = pick.model;
    let (trace, reference) = (pick.trace, pick.reference);
    let n = model.traces.len();
    let at = Rect::from_min_size(
        pos2(rect.left() + 14.0, rect.bottom() - 58.0),
        vec2(rect.width() - panel_w - 60.0, 44.0),
    );
    glass(ui, at, &mut |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if chip(ui, "‹", false).on_hover_text("Previous lap").clicked() && trace > 0 {
                input.step = Some(trace - 1);
            }
            ui.label(
                RichText::new(format!("Lap {}", model.traces[trace].number))
                    .font(semibold(14.0))
                    .color(TEXT),
            );
            ui.label(
                RichText::new(format_lap_time(pick.lap.duration_ms()))
                    .size(13.5)
                    .color(SECONDARY),
            );
            if chip(ui, "›", false).on_hover_text("Next lap").clicked() && trace + 1 < n {
                input.step = Some(trace + 1);
            }
            ui.label(RichText::new("against").size(13.0).color(DIM));
            let against = if reference == trace && pick.whole {
                "the best of every corner (this is the best lap)".to_string()
            } else if reference == trace {
                "nothing: it's your best here".to_string()
            } else if pick.whole {
                format!("lap {} (the best lap)", model.traces[reference].number)
            } else {
                format!(
                    "lap {} (your best corner {})",
                    model.traces[reference].number,
                    pick.corner + 1
                )
            };
            ui.label(RichText::new(against).size(13.5).color(SECONDARY))
                .on_hover_text("The dashed line");
            ui.add_space(6.0);
            if chip(ui, "All laps", all_laps)
                .on_hover_text("Every lap that counts, faint: how repeatable the line is")
                .clicked()
            {
                input.all_laps = Some(!all_laps);
            }
            if !pick.picked {
                ui.label(
                    RichText::new(if pick.lap_asked {
                        "(that lap doesn't go all the way round: showing the best)"
                    } else {
                        ""
                    })
                    .size(12.0)
                    .color(DIM),
                );
            }
        });
    })
}

/// Satellite imagery under the track, on or off, in `at`.
fn satellite_switch(ui: &mut Ui, at: Rect, on: bool, input: &mut MapInput) {
    glass(ui, at, &mut |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(4.0);
            ui.label(RichText::new("Satellite").size(13.5).color(if on {
                TEXT
            } else {
                SECONDARY
            }));
            if toggle(ui, on)
                .on_hover_text("Satellite imagery under the track")
                .clicked()
            {
                input.satellite = Some(!on);
            }
        });
    });
}

/// The numbers over the whole map, with an X. Gives whether to close it (X
/// or Escape) and a corner picked from it.
fn numbers_overlay(
    ui: &mut Ui,
    rect: Rect,
    pick: &MapPick,
    all_laps: bool,
) -> (bool, Option<Option<usize>>) {
    let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let mut go = None;
    egui::Area::new(ui.id().with("map_numbers"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0, theme::OUTLINE))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    let inner = rect.size() - vec2(32.0, 32.0);
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    let x = Rect::from_min_size(
                        pos2(rect.right() - 16.0 - 24.0, rect.top() + 16.0),
                        Vec2::splat(24.0),
                    );
                    let resp = ui
                        .put(x, Icon::X.image(SECONDARY, 18.0).sense(Sense::click()))
                        .on_hover_text("Close");
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    close |= resp.clicked();
                    // A readable column, centered.
                    let col_w = (inner.x - 64.0).min(520.0);
                    let col = Rect::from_min_size(
                        pos2(rect.center().x - col_w / 2.0, rect.top() + 16.0),
                        vec2(col_w, inner.y),
                    );
                    ui.scope_builder(egui::UiBuilder::new().max_rect(col), |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("map_numbers_scroll")
                            .show(ui, |ui| {
                                ui.set_width(col_w);
                                numbers(ui, pick, all_laps, &mut go);
                            });
                    });
                });
        });
    (close, go)
}

/// The arrow keys step through the corners (unless they're moving a text cursor).
fn map_keys(ui: &Ui, pick: &MapPick, input: &mut MapInput) {
    let n = pick.model.segments.len();
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (left, right) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft) && !typing,
            i.key_pressed(egui::Key::ArrowRight) && !typing,
        )
    });
    if right {
        input.go_corner = Some(Some(if pick.whole { 0 } else { (pick.corner + 1) % n }));
    } else if left {
        input.go_corner = Some(Some(if pick.whole {
            n - 1
        } else {
            (pick.corner + n - 1) % n
        }));
    }
}

/// The color of each point of the lap's line.
fn line_colors(
    model: &CornerModel,
    trace: usize,
    reference: usize,
    by: ColorBy,
    rpm: Option<&Channel>,
    start_ms: i64,
) -> Vec<Color32> {
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
            tr.kmh
                .iter()
                .map(|v| super::track_speed_color(((v - lo) / (hi - lo).max(1e-6)) as f32))
                .collect()
        }
        ColorBy::Rpm => {
            let Some(ch) = rpm else {
                return vec![theme::GOOD; n];
            };
            // Where RPM wasn't recorded, gray rather than the color of a low reading.
            let v: Vec<Option<f64>> =
                tr.t.iter()
                    .map(|t| super::sample_at(ch, start_ms + (t * 1000.0) as i64))
                    .collect();
            let (lo, hi) = super::robust_range(v.iter().flatten().copied());
            v.iter()
                .map(|x| {
                    x.map_or(LIFT, |x| {
                        super::rpm_color(((x - lo) / (hi - lo).max(1e-6)) as f32)
                    })
                })
                .collect()
        }
        ColorBy::Gain => {
            let r = &model.traces[reference];
            // How fast the gap to the reference grows (losing) or shrinks (gaining), per point.
            let rate: Vec<f64> = (0..n)
                .map(|j| {
                    let (a, b) = (j.saturating_sub(2), (j + 2).min(n - 1));
                    (tr.t[b] - tr.t[a]) - (r.t[b] - r.t[a])
                })
                .collect();
            // Scaled to the 95th percentile, so one spike doesn't wash the rest out.
            let mut mags: Vec<f64> = rate.iter().map(|x| x.abs()).collect();
            mags.sort_by(f64::total_cmp);
            let scale = mags
                .get(mags.len() * 95 / 100)
                .copied()
                .unwrap_or(0.0)
                .max(1e-6);
            rate.iter()
                .map(|x| super::gain_color((x / scale) as f32))
                .collect()
        }
    }
}

/// The legend for what colors the line.
fn legend(
    ui: &mut Ui,
    by: ColorBy,
    model: &CornerModel,
    trace: usize,
    rpm: Option<&Channel>,
    start_ms: i64,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let key = |ui: &mut Ui, color: Color32, text: &str| {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(5), color);
            ui.label(RichText::new(text).size(12.5).color(SECONDARY));
            ui.add_space(6.0);
        };
        // A row of pills from f(0) to f(1), one color each, so a color on the line is easy to find.
        let ramp = |ui: &mut Ui, f: &dyn Fn(f32) -> Color32| {
            const PILLS: usize = 10;
            let (bar, _) =
                ui.allocate_exact_size(vec2(PILLS as f32 * 21.0 - 3.0, 10.0), Sense::hover());
            for j in 0..PILLS {
                let r = Rect::from_min_size(
                    pos2(bar.left() + j as f32 * 21.0, bar.top()),
                    vec2(18.0, 10.0),
                );
                let t = j as f32 / (PILLS - 1) as f32;
                ui.painter().rect_filled(r, CornerRadius::same(5), f(t));
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
                ui.label(
                    RichText::new(format!("{:.0} {u}", crate::units::speed(lo)))
                        .size(12.5)
                        .color(SECONDARY),
                );
                ramp(ui, &super::track_speed_color);
                ui.label(
                    RichText::new(format!("{:.0} {u}", crate::units::speed(hi)))
                        .size(12.5)
                        .color(SECONDARY),
                );
            }
            ColorBy::Rpm => {
                if let Some(ch) = rpm {
                    let v = model.traces[trace]
                        .t
                        .iter()
                        .filter_map(|t| super::sample_at(ch, start_ms + (t * 1000.0) as i64));
                    let (lo, hi) = super::robust_range(v);
                    ui.label(
                        RichText::new(format!("{lo:.0} rpm"))
                            .size(12.5)
                            .color(SECONDARY),
                    );
                    ramp(ui, &super::rpm_color);
                    ui.label(
                        RichText::new(format!("{hi:.0} rpm"))
                            .size(12.5)
                            .color(SECONDARY),
                    );
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
    ui.label(
        RichText::new(format!("Lap {}", tr.number))
            .font(semibold(18.0))
            .color(TEXT),
    );
    ui.label(
        RichText::new("against the best of every corner")
            .size(12.5)
            .color(DIM),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{left:.2} s"))
                .font(semibold(22.0))
                .color(theme::FAVORITE),
        );
        ui.label(
            RichText::new("left on this lap")
                .size(13.0)
                .color(SECONDARY),
        );
    });
    ui.add_space(8.0);
    for k in 0..model.segments.len() {
        let best = model.best_of[k] == trace;
        let loss = model.loss(trace, k);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), SURFACE_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(
            rect.left_center() + vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("Corner {}", k + 1),
            regular(13.5),
            TEXT,
        );
        let (text, color) = if best {
            ("best".to_string(), theme::GOOD)
        } else {
            (
                format!("+{loss:.2}"),
                if loss >= 0.005 { BRAKE } else { SECONDARY },
            )
        };
        ui.painter().text(
            rect.right_center() - vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            text,
            medium(13.5),
            color,
        );
        if resp.clicked() {
            *go = Some(Some(k));
        }
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new("Pick a corner to zoom in on it.")
            .size(12.0)
            .color(DIM),
    );
}

/// One corner's numbers: this lap against the best you drove it, as in Corners' up close.
fn corner_numbers(
    ui: &mut Ui,
    model: &CornerModel,
    trace: usize,
    corner: usize,
    rpm: Option<&Channel>,
    laps: &[aim_xrk::Lap],
    all_laps: bool,
) {
    corner_heading(ui, model, trace, corner);
    ui.add_space(6.0);
    let rows = number_rows(model, trace, corner, rpm, laps);
    number_grid(ui, model, trace, corner, rows);
    if all_laps {
        line_spread(ui, model, corner);
    } else {
        ui.add(
            egui::Label::new(
                RichText::new("Turn on All laps to see how repeatable the line is here.")
                    .size(12.0)
                    .color(DIM),
            )
            .wrap(),
        );
    }
}

/// The corner's number, the laps compared, and the time lost there.
fn corner_heading(ui: &mut Ui, model: &CornerModel, trace: usize, corner: usize) {
    let bi = model.best_of[corner];
    let own = bi == trace;
    let (a, b) = (&model.stats[trace][corner], &model.stats[bi][corner]);
    let d = a.time_s - b.time_s;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Corner {}", corner + 1))
                .font(semibold(18.0))
                .color(TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = if own {
                format!("lap {}: your best here", model.traces[trace].number)
            } else {
                format!(
                    "lap {} vs {}",
                    model.traces[trace].number, model.traces[bi].number
                )
            };
            ui.label(RichText::new(text).size(12.0).color(DIM));
        });
    });
    ui.horizontal(|ui| {
        if own {
            ui.label(
                RichText::new("best")
                    .font(semibold(22.0))
                    .color(theme::GOOD),
            );
        } else {
            ui.label(
                RichText::new(format!("+{d:.2} s"))
                    .font(semibold(22.0))
                    .color(if d >= 0.005 { BRAKE } else { SECONDARY }),
            );
            ui.label(RichText::new("lost here").size(13.0).color(SECONDARY));
        }
    });
}

/// A row of a corner's numbers: (label, this lap, the best, higher is better,
/// decimals, unit, big).
type NumberRow = (&'static str, f64, f64, bool, usize, String, bool);

/// The corner's numbers for this lap and the best of it: speeds, where the
/// slowing starts, braking, and (where recorded) RPM, steering and cornering.
fn number_rows(
    model: &CornerModel,
    trace: usize,
    corner: usize,
    rpm: Option<&Channel>,
    laps: &[aim_xrk::Lap],
) -> Vec<NumberRow> {
    let bi = model.best_of[corner];
    let (a, b) = (&model.stats[trace][corner], &model.stats[bi][corner]);
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
        let (i0, i1) = (
            (s.from / STEP_M) as usize,
            ((s.to / STEP_M) as usize).min(tr.cornering.len() - 1),
        );
        tr.cornering[i0..=i1]
            .iter()
            .fold(0.0_f64, |m, g| m.max(g.abs()))
    };
    let mut rows: Vec<NumberRow> = vec![
        (
            "Entry",
            speed(a.entry_kmh),
            speed(b.entry_kmh),
            true,
            1,
            su.to_string(),
            true,
        ),
        (
            "Apex",
            speed(a.min_kmh),
            speed(b.min_kmh),
            true,
            1,
            su.to_string(),
            true,
        ),
        (
            "Down the straight",
            speed(a.exit_kmh),
            speed(b.exit_kmh),
            true,
            1,
            su.to_string(),
            true,
        ),
        (
            "From apex",
            crate::units::distance(a.slowing_m()),
            crate::units::distance(b.slowing_m()),
            true,
            0,
            du.to_string(),
            false,
        ),
        (
            "On the brakes",
            a.brake_s,
            b.brake_s,
            false,
            2,
            "s".into(),
            false,
        ),
    ];
    if let (Some(x), Some(y)) = (apex_rpm(trace, a), apex_rpm(bi, b)) {
        rows.push(("Apex RPM", x, y, true, 0, "".into(), false));
    }
    if let (Some((x, _)), Some((y, _))) = (a.steering, b.steering) {
        rows.push(("Steering", x, y, false, 0, "°".into(), false));
    }
    rows.push((
        "Cornering",
        peak_g(trace),
        peak_g(bi),
        true,
        2,
        "g".into(),
        false,
    ));
    rows
}

/// The numbers in a grid: this lap's, and the best's where it's worse.
fn number_grid(
    ui: &mut Ui,
    model: &CornerModel,
    trace: usize,
    corner: usize,
    rows: Vec<NumberRow>,
) {
    let bi = model.best_of[corner];
    let own = bi == trace;
    let brake_m = model.stats[trace][corner].brake_m;
    egui::Grid::new(("map_numbers", corner))
        .num_columns(3)
        .spacing(vec2(12.0, 6.0))
        .show(ui, |ui| {
            ui.label("");
            ui.label(
                RichText::new(format!("LAP {}", model.traces[trace].number))
                    .font(medium(10.5))
                    .color(DIM),
            );
            ui.label(
                RichText::new(if own {
                    String::new()
                } else {
                    format!("LAP {}", model.traces[bi].number)
                })
                .font(medium(10.5))
                .color(DIM),
            );
            ui.end_row();
            for (label, x, y, higher, dec, unit, big) in rows {
                let name = ui.label(RichText::new(label).size(13.0).color(SECONDARY));
                if matches!(label, "Entry" | "Apex") {
                    name.on_hover_text(APEX_NOTE);
                }
                let size = if big { 17.0 } else { 13.5 };
                let value = ui.label(
                    RichText::new(format!("{x:.dec$} {unit}"))
                        .font(if big { semibold(size) } else { regular(size) })
                        .color(TEXT),
                );
                if label == "From apex" {
                    dwell_note(
                        ui,
                        value.rect,
                        value.id.with("lap_position"),
                        &lap_position(brake_m),
                    );
                }
                if own {
                    ui.label("");
                } else {
                    // Slowing closer to the apex isn't better or worse in itself.
                    let worse = label != "From apex" && if higher { x < y } else { x > y };
                    let close = (x - y).abs() < 0.5 * 10f64.powi(-(dec as i32));
                    ui.label(RichText::new(format!("{y:.dec$}")).size(13.5).color(
                        if close || !worse {
                            DIM
                        } else {
                            theme::LOSE_TEXT
                        },
                    ));
                }
                ui.end_row();
            }
        });
}

/// How repeatable the line is at the apex: the spread of where each counting lap went.
fn line_spread(ui: &mut Ui, model: &CornerModel, corner: usize) {
    let s = model.segments[corner];
    let best_xy = &model.traces[model.best].xy;
    let i = ((s.apex / STEP_M).round() as usize).min(best_xy.len() - 2);
    let (p, q) = (best_xy[i.saturating_sub(1)], best_xy[i + 1]);
    let (dx, dy) = (q.0 - p.0, q.1 - p.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let offs: Vec<f64> = (0..model.traces.len())
        .filter(|&t| model.counts[t])
        .map(|t| {
            let a = model.traces[t].xy[i];
            ((a.0 - best_xy[i].0) * -dy + (a.1 - best_xy[i].1) * dx) / len
        })
        .collect();
    if offs.len() > 1 {
        let mean = offs.iter().sum::<f64>() / offs.len() as f64;
        let sd = (offs.iter().map(|o| (o - mean).powi(2)).sum::<f64>() / offs.len() as f64).sqrt();
        ui.add_space(4.0);
        // In words: GPS can't tell lines much closer than half a meter apart.
        let how = if sd < 0.5 {
            "was consistent"
        } else if sd < 1.2 {
            "varied a little"
        } else {
            "varied a lot"
        };
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "Across {} laps, your line at the apex {how}.",
                    offs.len()
                ))
                .size(12.5)
                .color(TEXT),
            )
            .wrap(),
        )
        .on_hover_text(
            "From the MyChron's GPS, which can't tell lines less than about half a meter apart.",
        );
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
    let speeds = || {
        [trace, reference].into_iter().flat_map(|t| {
            ks.iter()
                .map(move |&k| crate::units::speed(model.traces[t].kmh[k]))
        })
    };
    let (lo, hi) = (
        speeds().fold(f64::MAX, f64::min) - 1.0,
        speeds().fold(f64::MIN, f64::max) + 1.0,
    );
    let x = |j: usize| plot.left() + j as f32 / (ks.len() - 1).max(1) as f32 * plot.width();
    let y = |v: f64| plot.bottom() - ((v - lo) / (hi - lo)) as f32 * plot.height();
    let p = ui.painter_at(rect);
    p.text(
        pos2(rect.left(), plot.top() + 4.0),
        egui::Align2::LEFT_CENTER,
        format!("{hi:.0}"),
        regular(10.5),
        DIM,
    );
    p.text(
        pos2(rect.left(), plot.bottom()),
        egui::Align2::LEFT_CENTER,
        format!("{lo:.0}"),
        regular(10.5),
        DIM,
    );
    p.text(
        pos2(rect.left(), plot.center().y),
        egui::Align2::LEFT_CENTER,
        crate::units::speed_unit(),
        regular(10.5),
        DIM,
    );
    let apex_j = ks
        .iter()
        .position(|&k| k >= idx(model.stats[trace][corner].min_m))
        .unwrap_or(0);
    p.extend(egui::Shape::dashed_line(
        &[pos2(x(apex_j), plot.top()), pos2(x(apex_j), plot.bottom())],
        Stroke::new(1.0, DIM),
        3.0,
        3.0,
    ));
    p.text(
        pos2(x(apex_j) + 4.0, plot.top() + 4.0),
        egui::Align2::LEFT_CENTER,
        "apex",
        regular(10.5),
        DIM,
    );
    let line = |t: usize| -> Vec<egui::Pos2> {
        ks.iter()
            .enumerate()
            .map(|(j, &k)| pos2(x(j), y(crate::units::speed(model.traces[t].kmh[k]))))
            .collect()
    };
    if reference != trace {
        p.extend(egui::Shape::dashed_line(
            &line(reference),
            Stroke::new(1.5, Color32::from_white_alpha(200)),
            5.0,
            4.0,
        ));
    }
    p.add(egui::Shape::line(
        line(trace),
        Stroke::new(2.2, theme::ACCENT),
    ));
    // How each slowed, as strips under the chart.
    for (row, t) in [trace, reference].into_iter().enumerate() {
        let top = plot.bottom() + 8.0 + row as f32 * 14.0;
        p.text(
            pos2(rect.left(), top + 4.5),
            egui::Align2::LEFT_CENTER,
            format!("lap {}", model.traces[t].number),
            regular(10.5),
            DIM,
        );
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
            p.rect_filled(
                Rect::from_min_max(pos2(x(j), top), pos2(x(j + 1) + 0.5, top + 9.0)),
                0.0,
                c,
            );
        }
        if reference == trace {
            break;
        }
    }
}

/// An on/off switch.
fn toggle(ui: &mut Ui, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, on, 0.12);
    let p = ui.painter();
    p.rect_filled(
        rect,
        CornerRadius::same(255),
        super::mix(SURFACE_ACTIVE, theme::ACCENT, t),
    );
    let x = rect.left() + 11.0 + (rect.width() - 22.0) * t;
    p.circle_filled(pos2(x, rect.center().y), 8.0, TEXT);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Where a corner's number goes: the point on `line` at index `i`, and one
/// a meter off it on the outside of the corner (away from the middle of the
/// lap's shape).
fn corner_label(line: &[(f64, f64)], i: usize) -> ((f64, f64), (f64, f64)) {
    let (a, b) = (line[i.saturating_sub(3)], line[(i + 3).min(line.len() - 1)]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let mid = line[i];
    let center = line.iter().fold((0.0, 0.0), |s, p| (s.0 + p.0, s.1 + p.1));
    let n = line.len() as f64;
    let out = if (mid.0 - center.0 / n) * (-dy / len) + (mid.1 - center.1 / n) * (dx / len) > 0.0 {
        1.0
    } else {
        -1.0
    };
    (mid, (mid.0 - dy / len * out, mid.1 + dx / len * out))
}

/// How wide to draw a track (m) without imagery: as wide as the settings say
/// it is (2 to 40 m), else kart-track wide, or wider when AiM lists it as a
/// race track (karts do run on car circuits).
fn track_width(
    set: &std::collections::BTreeMap<String, f64>,
    name: &str,
    kind: aim_track::TrackKind,
) -> f64 {
    set.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, &w)| w)
        .filter(|w| (2.0..=40.0).contains(w))
        .unwrap_or(if kind == aim_track::TrackKind::RaceTrack {
            12.0
        } else {
            8.0
        })
}

/// The track to draw without imagery, `width` meters wide, from where the laps
/// went: its middle (meters, like the traces) and whether it's a loop.
///
/// Fast laps hug the inside of every corner, so in corners the inside edge sits
/// just inside the tightest lines (ignoring the odd stray lap) and the track
/// extends outward from there; on straights it's centered on the spread of the
/// lines; in between, a blend by how sharply the line turns. A lap that
/// doesn't end where it started (track learning off, a GPS dropout at the line)
/// is drawn as it is, not joined up.
fn track_shape(model: &CornerModel, width: f64) -> (Vec<(f64, f64)>, bool) {
    let b = &model.traces[model.best].xy;
    let n = b.len();
    if n < 40 {
        return (b.clone(), false);
    }
    // Where the line would go next, and how far that is from where the lap started.
    let next = (2.0 * b[n - 1].0 - b[n - 2].0, 2.0 * b[n - 1].1 - b[n - 2].1);
    let gap = (b[0].0 - next.0, b[0].1 - next.1);
    let closed = (gap.0 * gap.0 + gap.1 * gap.1).sqrt() < 25.0;
    // A loop's end eased onto its start: GPS rarely puts the lap's last point
    // exactly where its first was, which would leave a step at start/finish.
    let best: Vec<(f64, f64)> = if closed {
        let k = 15;
        (0..n)
            .map(|i| {
                // Half the gap closed from each side, faded in over k points.
                let f = if i + k >= n {
                    0.5 * (i + k + 1 - n) as f64 / k as f64
                } else if i < k {
                    -0.5 * (k - i) as f64 / k as f64
                } else {
                    0.0
                };
                (b[i].0 + gap.0 * f, b[i].1 + gap.1 * f)
            })
            .collect()
    } else {
        b.clone()
    };
    let best = &best;
    // Round the loop, or held at the ends of a line that isn't one.
    let idx = |i: isize| {
        if closed {
            i.rem_euclid(n as isize) as usize
        } else {
            i.clamp(0, n as isize - 1) as usize
        }
    };
    let lines: Vec<&Vec<(f64, f64)>> = (0..model.traces.len())
        .filter(|&t| model.counts[t] || t == model.best)
        .map(|t| &model.traces[t].xy)
        .collect();
    // Of sorted values, the one `p` of the way up.
    let pct = |v: &[f64], p: f64| v[((v.len() - 1) as f64 * p).round() as usize];

    let mut normals = Vec::with_capacity(n);
    let mut turn = Vec::with_capacity(n);
    let mut sides = Vec::with_capacity(n);
    for i in 0..n as isize {
        let dir = |a: isize, c: isize| {
            let (p, q) = (best[idx(a)], best[idx(c)]);
            (q.1 - p.1).atan2(q.0 - p.0)
        };
        let h = dir(i - 2, i + 2);
        normals.push((-h.sin(), h.cos()));
        // How sharply the line turns (1/m, left positive), over 10 m either side.
        let d = (dir(i, i + 5) - dir(i - 5, i) + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        turn.push(d / (5.0 * STEP_M));
        // Each lap's line, meters to the left of the best lap's.
        let p = best[i as usize];
        let (nx, ny) = normals[i as usize];
        let mut offs: Vec<f64> = lines
            .iter()
            .filter_map(|l| l.get(i as usize))
            .map(|q| (q.0 - p.0) * nx + (q.1 - p.1) * ny)
            .collect();
        offs.sort_by(f64::total_cmp);
        sides.push((pct(&offs, 0.1), pct(&offs, 0.9)));
    }

    // Where the middle is, meters left of the best lap's line.
    let shift: Vec<f64> = (0..n)
        .map(|i| {
            let (lo, hi) = sides[i];
            let straight = (lo + hi) / 2.0;
            let corner = if turn[i] > 0.0 {
                hi + 0.5 - width / 2.0
            } else {
                lo - 0.5 + width / 2.0
            };
            // Fully a corner at a 30 m radius or tighter.
            let w = (turn[i].abs() * 30.0).min(1.0);
            w * corner + (1.0 - w) * straight
        })
        .collect();
    // Smoothed (about 16 m either side), so the edges flow.
    let smooth = |v: &[f64]| -> Vec<f64> {
        (0..n as isize)
            .map(|i| (-8..=8).map(|k| v[idx(i + k)]).sum::<f64>() / 17.0)
            .collect()
    };
    let shift = smooth(&smooth(&shift));
    let middle = (0..n)
        .map(|i| {
            (
                best[i].0 + normals[i].0 * shift[i],
                best[i].1 + normals[i].1 * shift[i],
            )
        })
        .collect();
    (middle, closed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_number_sits_outside_the_corner() {
        // A circle of radius 50 m around the origin: outside is farther from it.
        let line: Vec<(f64, f64)> = (0..100)
            .map(|k| {
                let a = k as f64 / 100.0 * std::f64::consts::TAU;
                (50.0 * a.cos(), 50.0 * a.sin())
            })
            .collect();
        for i in [10, 40, 75] {
            let (mid, off) = corner_label(&line, i);
            assert_eq!(mid, line[i]);
            let r = |p: (f64, f64)| (p.0 * p.0 + p.1 * p.1).sqrt();
            assert!((r(off) - 51.0).abs() < 0.01, "{} m out", r(off) - 50.0);
        }
    }

    #[test]
    fn a_track_is_as_wide_as_the_settings_say() {
        use aim_track::TrackKind;
        let set = std::collections::BTreeMap::from([
            ("Club Ring".to_string(), 10.5),
            ("Odd".to_string(), 90.0),
        ]);
        assert_eq!(track_width(&set, "club ring", TrackKind::KartTrack), 10.5);
        // A width out of reason, or none set: kart-track wide, or race-track wide.
        assert_eq!(track_width(&set, "Odd", TrackKind::KartTrack), 8.0);
        assert_eq!(track_width(&set, "Elsewhere", TrackKind::RaceTrack), 12.0);
    }
}
