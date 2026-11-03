//! Setting a start/finish line on a session's GPS trace: for a session the
//! MyChron recorded as one long lap where no line is known (a track not in
//! AiM's list, track learning off), or to move a line. A click on the trace
//! puts the line there; then it's saved as a track file (`aim_track::write`):
//! one of your tracks nearby with its line moved (the original backed up), or
//! a new track made from the session's fastest lap. Sessions there are then
//! timed from it.

use super::*;

use aim_track::write::LapPoint;

/// Points of a new track's outline about this far apart, as a MyChron spaces them.
const POINT_EVERY_M: f64 = 4.0;

pub(super) struct LinePick {
    path: PathBuf,
    session: std::sync::Arc<Session>,
    /// The whole GPS trace (every lap), to click on.
    trace: Vec<aim_track::LatLon>,
    /// The track's outline from its track file, when it has one.
    outline: Vec<aim_track::LatLon>,
    /// The racing line: the fastest lap, from the chosen line once there is one.
    racing: Vec<aim_track::LatLon>,
    /// The point picked on the trace.
    chosen: Option<usize>,
    /// Your tracks near the session, and which one to change (None: save a new track).
    near: Vec<crate::tracks::NearTrack>,
    update: Option<usize>,
    /// A new track's name.
    name: String,
    memory: walkers::MapMemory,
    fitted: bool,
}

impl LinePick {
    pub fn new(
        path: PathBuf,
        track: String,
        session: std::sync::Arc<Session>,
        outline: Vec<aim_track::LatLon>,
    ) -> Option<Self> {
        let gps = session.gps.as_ref()?;
        let n = gps.latitude.len().min(gps.longitude.len());
        if n < 100 {
            return None;
        }
        // At most ~4,000 points: plenty to click on, quick to draw.
        let step = (n / 4000).max(1);
        let trace: Vec<aim_track::LatLon> = (0..n)
            .step_by(step)
            .map(|i| aim_track::LatLon {
                lat: gps.latitude[i],
                lon: gps.longitude[i],
            })
            .filter(|p| p.lat.is_finite() && p.lon.is_finite() && (p.lat != 0.0 || p.lon != 0.0))
            .collect();
        let first = *trace.first()?;
        let near = crate::tracks::tracks_near(first.lat, first.lon, 3_000.0);
        let update = (!near.is_empty()).then_some(0);
        // The session's own fastest lap, when the logger (or GPS) timed it.
        let racing = session
            .best_lap(aim_xrk::laps::MIN_LAP_MS)
            .map(|l| lap_path(gps, l.start_ms, l.end_ms))
            .unwrap_or_default();
        Some(LinePick {
            path,
            session,
            trace,
            outline,
            racing,
            chosen: None,
            near,
            update,
            name: track,
            memory: walkers::MapMemory::default(),
            fitted: false,
        })
    }
}

/// The GPS points between two times.
fn lap_path(gps: &aim_xrk::GpsTrack, from_ms: i64, to_ms: i64) -> Vec<aim_track::LatLon> {
    (0..gps.times_ms.len())
        .filter(|&i| gps.times_ms[i] >= from_ms && gps.times_ms[i] <= to_ms)
        .map(|i| aim_track::LatLon {
            lat: gps.latitude[i],
            lon: gps.longitude[i],
        })
        .collect()
}

/// What the map draws, and where a click landed (read after the map draws).
struct LineOverlay {
    trace: Vec<aim_track::LatLon>,
    outline: Vec<aim_track::LatLon>,
    racing: Vec<aim_track::LatLon>,
    chosen: Option<usize>,
    clicked: std::rc::Rc<std::cell::Cell<Option<usize>>>,
}

/// Colors for the map's layers (also the legend's).
const TRACE: Color32 = Color32::from_rgba_premultiplied(56, 57, 59, 70);
const OUTLINE: Color32 = Color32::from_rgb(0xf2, 0xf4, 0xf6);

impl walkers::Plugin for LineOverlay {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        response: &egui::Response,
        projector: &walkers::Projector,
        _memory: &walkers::MapMemory,
    ) {
        ui.set_clip_rect(response.rect.intersect(ui.clip_rect()));
        let painter = ui.painter();
        let screen =
            |p: &aim_track::LatLon| projector.project(walkers::lat_lon(p.lat, p.lon)).to_pos2();
        let casing = |w: f32| Stroke::new(w, Color32::from_black_alpha(130));
        // The track's outline, then every lap faintly, then the racing line on top.
        if self.outline.len() > 2 {
            let mut pts: Vec<egui::Pos2> = self.outline.iter().map(screen).collect();
            pts.push(pts[0]);
            painter.add(egui::Shape::line(pts.clone(), casing(9.0)));
            painter.add(egui::Shape::line(
                pts,
                Stroke::new(5.0, OUTLINE.gamma_multiply(0.55)),
            ));
        }
        let trace: Vec<egui::Pos2> = self.trace.iter().map(screen).collect();
        painter.add(egui::Shape::line(trace.clone(), Stroke::new(1.5, TRACE)));
        if self.racing.len() > 2 {
            let pts: Vec<egui::Pos2> = self.racing.iter().map(screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing(6.0)));
            painter.add(egui::Shape::line(pts, Stroke::new(3.0, theme::ACCENT)));
        }
        // The nearest point of the trace to the pointer, when it's close.
        let near = |pos: egui::Pos2| {
            trace
                .iter()
                .enumerate()
                .map(|(i, q)| (i, q.distance(pos)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|(_, d)| *d < 20.0)
                .map(|(i, _)| i)
        };
        if let Some(i) = response.hover_pos().and_then(near) {
            painter.circle_filled(trace[i], 6.0, Color32::from_black_alpha(160));
            painter.circle_filled(trace[i], 4.5, TEXT);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.clicked
                .set(response.interact_pointer_pos().and_then(near));
        }
        // The line: across the track at the chosen point.
        if let Some(i) = self.chosen {
            let (a, b) = (
                trace[i.saturating_sub(3)],
                trace[(i + 3).min(trace.len() - 1)],
            );
            let along = (b - a).normalized();
            let across = vec2(-along.y, along.x) * 22.0;
            painter.line_segment(
                [trace[i] - across, trace[i] + across],
                Stroke::new(8.0, Color32::from_black_alpha(160)),
            );
            painter.line_segment(
                [trace[i] - across, trace[i] + across],
                Stroke::new(5.0, theme::FAVORITE),
            );
            painter.text(
                trace[i] + across * 1.35,
                egui::Align2::CENTER_CENTER,
                "S/F",
                semibold(12.0),
                TEXT,
            );
        }
    }
}

/// The session's fastest lap timed from a line at `at`, as a new track's
/// outline (a point every ~4 m, with the speed there), and its length.
fn lap_outline(session: &Session, at: (f64, f64)) -> Option<(Vec<LapPoint>, f32)> {
    let (_, laps) = aim_xrk::relap::retime(session, &[at])?;
    let lap = laps
        .get(1..laps.len().saturating_sub(1))?
        .iter()
        .min_by_key(|l| l.duration_ms())?;
    let gps = session.gps.as_ref()?;
    let mut points: Vec<LapPoint> = Vec::new();
    let mut length = 0.0;
    for i in (0..gps.times_ms.len())
        .filter(|&i| gps.times_ms[i] >= lap.start_ms && gps.times_ms[i] <= lap.end_ms)
    {
        let here = aim_track::LatLon {
            lat: gps.latitude[i],
            lon: gps.longitude[i],
        };
        let gap = points.last().map_or(f64::MAX, |p| p.at.distance_m(&here));
        if gap >= POINT_EVERY_M {
            length += if points.is_empty() { 0.0 } else { gap };
            points.push(LapPoint {
                at: here,
                kmh: gps.speed_kmh.get(i).copied().unwrap_or(0.0) as f32,
            });
        }
    }
    (points.len() > 10).then_some((points, length as f32))
}

/// A new track's id: 8 characters, "mx" and the time in base 36.
fn new_id() -> String {
    let mut n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut s = String::new();
    for _ in 0..6 {
        s.insert(0, char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    format!("mx{s}")
}

/// A new track's guid: 8 bytes from the time and this process.
fn new_guid() -> [u8; 8] {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (t ^ (std::process::id() as u64).rotate_left(40)).to_le_bytes()
}

impl App {
    pub(super) fn line_pick_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pick) = self.line_pick.take() else {
            return;
        };
        let (mut save, mut cancel, mut toggle) = (false, false, false);
        let satellite = self.track_page.satellite;
        let clicked = std::rc::Rc::new(std::cell::Cell::new(None));
        let closed = look::dialog(ctx, "line_pick", 1180.0, |ui| {
            look::header(
                ui,
                Icon::Flag,
                SECONDARY,
                None,
                "Start/finish line",
                "Click where the start/finish line crosses the track, then save it to a track.",
            );
            look::body(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    // The map: satellite or streets, with the track drawn over it.
                    let size = vec2(720.0, 560.0);
                    if !pick.fitted {
                        pick.fitted = true;
                        let fit: &[aim_track::LatLon] = if pick.racing.len() > 2 { &pick.racing } else { &pick.trace };
                        if let Some((c, zoom)) = super::tracks_ui::fit_view(fit, size) {
                            pick.memory.center_at(walkers::lat_lon(c.lat, c.lon));
                            let _ = pick.memory.set_zoom(zoom);
                        }
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        let tiles = super::tracks_ui::tiles(&mut self.map_tiles, &mut self.sat_tiles, ui.ctx(), satellite);
                        let overlay = LineOverlay { trace: pick.trace.clone(), outline: pick.outline.clone(), racing: pick.racing.clone(), chosen: pick.chosen, clicked: clicked.clone() };
                        let center = pick.trace.first().map(|p| walkers::lat_lon(p.lat, p.lon)).unwrap_or(walkers::lat_lon(0.0, 0.0));
                        let map = ui.add_sized(size, walkers::Map::new(super::tracks_ui::shown(tiles), &mut pick.memory, center).with_plugin(overlay));
                        let corner = Rect::from_min_size(map.rect.right_top() + vec2(-190.0, 10.0), vec2(180.0, 36.0));
                        // Over the map's corner, without taking room below it.
                        let mut over = ui.new_child(egui::UiBuilder::new().max_rect(corner).layout(Layout::right_to_left(Align::Center)));
                        over.spacing_mut().item_spacing.x = 4.0;
                        toggle |= chip(&mut over, "Satellite", satellite).clicked() && !satellite;
                        toggle |= chip(&mut over, "Map", !satellite).clicked() && satellite;
                        // The legend, and whose map it is.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let swatch = |ui: &mut Ui, color: Color32, width: f32, label: &str| {
                                let (r, _) = ui.allocate_exact_size(vec2(22.0, 12.0), Sense::hover());
                                ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(width, color));
                                ui.label(RichText::new(label).size(12.5).color(SECONDARY));
                                ui.add_space(10.0);
                            };
                            if pick.racing.len() > 2 {
                                swatch(ui, theme::ACCENT, 3.0, if pick.chosen.is_some() { "Racing line (fastest lap from your line)" } else { "Racing line (fastest lap)" });
                            }
                            if pick.outline.len() > 2 {
                                swatch(ui, OUTLINE.gamma_multiply(0.55), 5.0, "Track outline");
                            }
                            swatch(ui, TRACE, 2.0, "Every lap");
                            swatch(ui, theme::FAVORITE, 4.0, "Start/finish");
                        });
                        let credit = if satellite { "Imagery © Esri, Maxar, Earthstar Geographics." } else { "Map data © OpenStreetMap contributors." };
                        ui.label(RichText::new(format!("Drag to move, scroll to zoom.  {credit}")).size(11.5).color(DIM));
                    });
                    // The steps, and where it's saved.
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 8.0;
                        let step = |ui: &mut Ui, n: &str, text: &str, done: bool| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(n).font(semibold(14.0)).color(if done { theme::GOOD } else { TEXT }));
                                ui.add(egui::Label::new(RichText::new(text).font(regular(14.5)).color(SECONDARY)).wrap());
                            });
                        };
                        step(ui, "1", "Click the track where the start/finish line is. Zoom in to place it exactly.", pick.chosen.is_some());
                        step(ui, "2", "Save it to one of your tracks here, or as a new track.", false);
                        ui.add_space(8.0);
                        if pick.chosen.is_none() {
                            look::note(ui, "Nothing is changed until you save.", DIM);
                            return;
                        }
                        for (k, t) in pick.near.iter().enumerate() {
                            if look::option(ui, pick.update == Some(k), Some(Icon::Track), &format!("Update {}", t.name), "Moves its line here; the original is backed up.") {
                                pick.update = Some(k);
                            }
                        }
                        if look::option(ui, pick.update.is_none(), Some(Icon::Plus), "Save as a new track", "Made from the fastest lap from your line.") {
                            pick.update = None;
                        }
                        if pick.update.is_none() {
                            look::text(ui, &mut pick.name, "Track name", None);
                        }
                    });
                });
            });
            look::footer_row(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ready = pick.chosen.is_some()
                        && (pick.update.is_some() || !pick.name.trim().is_empty());
                    save =
                        look::big_button(ui, "Save", Some(Icon::Check), look::Kind::Primary, ready);
                    cancel = look::big_button(ui, "Cancel", None, look::Kind::Quiet, true);
                });
            });
        });
        if toggle {
            self.track_page.satellite = !satellite;
        }
        // A click on the trace places the line; the racing line follows it.
        if let Some(i) = clicked.get() {
            pick.chosen = Some(i);
            let at = pick.trace[i];
            if let Some((lap, _)) = lap_outline(&pick.session, (at.lat, at.lon)) {
                pick.racing = lap.iter().map(|p| p.at).collect();
            }
        }
        if save && let Some(i) = pick.chosen {
            let (lat, lon) = (pick.trace[i].lat, pick.trace[i].lon);
            let saved = match pick.update.and_then(|k| pick.near.get(k)) {
                Some(t) => self
                    .move_track_line(&t.path, lat, lon)
                    .map(|_| t.name.clone()),
                None => self
                    .save_new_track(&pick, lat, lon)
                    .map(|_| pick.name.trim().to_string()),
            };
            match saved {
                Ok(name) => {
                    log::info!("start/finish line for {name} set at {lat:.6}, {lon:.6}");
                    // This session, and others there that looked untimed (the
                    // tracks' stamp changed), are timed again from it.
                    crate::digest::Digests::forget(std::slice::from_ref(&pick.path));
                    self.sensors.digests = None;
                    self.sensors.digests_asked.clear();
                    self.reload_library();
                    self.act(Action::Reopen(pick.path.clone()));
                    self.notices.success(format!(
                        "Start/finish line saved to {name}. Sessions there are timed from it."
                    ));
                }
                Err(e) => {
                    self.notices
                        .error(format!("Couldn't save the start/finish line: {e}"));
                }
            }
        } else if !closed && !cancel {
            self.line_pick = Some(pick);
        }
    }

    /// Moves the start/finish line of your track at `path`, keeping the original in backups.
    fn move_track_line(
        &mut self,
        path: &std::path::Path,
        lat: f64,
        lon: f64,
    ) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let moved = aim_track::write::with_start_finish(&bytes, aim_track::LatLon { lat, lon })
            .map_err(|e| e.to_string())?;
        let today = crate::dates::today();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let backup = crate::paths::data("backups")
            .unwrap_or_default()
            .join("tracks")
            .join(format!("{stem}-{}.tkk", crate::dates::iso(today)));
        if !backup.exists() {
            crate::storage::write_atomic(&backup, &bytes)
                .map_err(|e| format!("couldn't back it up: {e}"))?;
        }
        crate::storage::write_atomic(path, &moved).map_err(|e| e.to_string())
    }

    /// A new track from the session's fastest lap timed from the line, in
    /// `~/Mychronix/tracks/mychronix/`; the session is put on it.
    fn save_new_track(&mut self, pick: &LinePick, lat: f64, lon: f64) -> Result<(), String> {
        let (lap, length) = lap_outline(&pick.session, (lat, lon)).ok_or("this session has no full lap from that line (pick a point the kart crossed lap after lap)")?;
        let (id, guid, name) = (new_id(), new_guid(), pick.name.trim().to_string());
        let bytes = aim_track::write::from_lap(&name, &id, guid, &lap, length, None)
            .map_err(|e| e.to_string())?;
        let path = crate::paths::data("tracks")
            .ok_or("no home folder")?
            .join("mychronix")
            .join(format!("{id}.tkk"));
        crate::storage::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
        // This session is at that track now.
        let mut meta = crate::device::SessionMeta::load(&pick.path).unwrap_or_default();
        meta.track = name;
        meta.track_guid = Some(guid.iter().map(|b| format!("{b:02x}")).collect());
        meta.save(&pick.path).map_err(|e| e.to_string())
    }
}
