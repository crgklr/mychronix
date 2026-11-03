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
    /// The trace: degrees, and meters east and north of its first point.
    latlon: Vec<(f64, f64)>,
    xy: Vec<(f64, f64)>,
    /// The point picked on the trace.
    chosen: Option<usize>,
    /// Your tracks near the session, and which one to change (None: save a new track).
    near: Vec<crate::tracks::NearTrack>,
    update: Option<usize>,
    /// A new track's name.
    name: String,
}

impl LinePick {
    pub fn new(path: PathBuf, track: String, session: std::sync::Arc<Session>) -> Option<Self> {
        let gps = session.gps.as_ref()?;
        let n = gps.latitude.len().min(gps.longitude.len());
        if n < 100 {
            return None;
        }
        // At most ~3,000 points: plenty to click on, quick to draw.
        let step = (n / 3000).max(1);
        let latlon: Vec<(f64, f64)> = (0..n).step_by(step).map(|i| (gps.latitude[i], gps.longitude[i])).filter(|(a, o)| a.is_finite() && o.is_finite() && (*a != 0.0 || *o != 0.0)).collect();
        let (lat0, lon0) = *latlon.first()?;
        let m_per_deg_lon = 111_320.0 * lat0.to_radians().cos();
        let xy = latlon.iter().map(|(a, o)| ((o - lon0) * m_per_deg_lon, (a - lat0) * 110_540.0)).collect();
        let near = crate::tracks::tracks_near(lat0, lon0, 3_000.0);
        let update = (!near.is_empty()).then_some(0);
        Some(LinePick { path, session, latlon, xy, chosen: None, near, update, name: track })
    }
}

/// The session's fastest lap timed from a line at `at`, as a new track's
/// outline (a point every ~4 m, with the speed there), and its length.
fn lap_outline(session: &Session, at: (f64, f64)) -> Option<(Vec<LapPoint>, f32)> {
    let (_, laps) = aim_xrk::relap::retime(session, &[at])?;
    let lap = laps.get(1..laps.len().saturating_sub(1))?.iter().min_by_key(|l| l.duration_ms())?;
    let gps = session.gps.as_ref()?;
    let mut points: Vec<LapPoint> = Vec::new();
    let mut length = 0.0;
    for i in (0..gps.times_ms.len()).filter(|&i| gps.times_ms[i] >= lap.start_ms && gps.times_ms[i] <= lap.end_ms) {
        let here = aim_track::LatLon { lat: gps.latitude[i], lon: gps.longitude[i] };
        let gap = points.last().map_or(f64::MAX, |p| p.at.distance_m(&here));
        if gap >= POINT_EVERY_M {
            length += if points.is_empty() { 0.0 } else { gap };
            points.push(LapPoint { at: here, kmh: gps.speed_kmh.get(i).copied().unwrap_or(0.0) as f32 });
        }
    }
    (points.len() > 10).then_some((points, length as f32))
}

/// A new track's id: 8 characters, "mx" and the time in base 36.
fn new_id() -> String {
    let mut n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut s = String::new();
    for _ in 0..6 {
        s.insert(0, char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    format!("mx{s}")
}

/// A new track's guid: 8 bytes from the time and this process.
fn new_guid() -> [u8; 8] {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    (t ^ (std::process::id() as u64).rotate_left(40)).to_le_bytes()
}

impl App {
    pub(super) fn line_pick_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pick) = self.line_pick.take() else { return };
        let (mut save, mut cancel) = (false, false);
        let closed = look::dialog(ctx, "line_pick", 760.0, |ui| {
            look::header(ui, Icon::Flag, SECONDARY, None, "Start/finish line", "Click where the start/finish line crosses the track, then save it to a track.");
            look::body(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 320.0), Sense::click());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, CornerRadius::same(12), SURFACE);
                // Fit the trace, north up, keeping its shape.
                let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                for &(x, y) in &pick.xy {
                    (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
                }
                let inner = rect.shrink(24.0);
                let scale = (inner.width() as f64 / (x1 - x0).max(1.0)).min(inner.height() as f64 / (y1 - y0).max(1.0));
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let at = |(x, y): (f64, f64)| inner.center() + vec2(((x - cx) * scale) as f32, (-(y - cy) * scale) as f32);
                let points: Vec<egui::Pos2> = pick.xy.iter().map(|&q| at(q)).collect();
                p.add(egui::Shape::line(points.clone(), Stroke::new(2.0, SECONDARY)));
                // The nearest point to the pointer, when it's close to the trace.
                let near = |pos: egui::Pos2| points.iter().enumerate().map(|(i, q)| (i, q.distance(pos))).min_by(|a, b| a.1.total_cmp(&b.1)).filter(|(_, d)| *d < 24.0).map(|(i, _)| i);
                if let Some(i) = response.hover_pos().and_then(near) {
                    p.circle_filled(points[i], 5.0, TEXT);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked()
                    && let Some(i) = response.interact_pointer_pos().and_then(near)
                {
                    pick.chosen = Some(i);
                }
                // The line: across the track at the chosen point.
                if let Some(i) = pick.chosen {
                    let (a, b) = (points[i.saturating_sub(2)], points[(i + 2).min(points.len() - 1)]);
                    let along = (b - a).normalized();
                    let across = vec2(-along.y, along.x) * 18.0;
                    p.line_segment([points[i] - across, points[i] + across], Stroke::new(4.0, theme::ACCENT));
                }
                ui.add_space(10.0);
                if pick.chosen.is_none() {
                    look::note(ui, "Hover over the track and click where the start/finish line is.", DIM);
                    return;
                }
                // Where it's saved: one of your tracks here, or a new one.
                for (k, t) in pick.near.iter().enumerate() {
                    if look::option(ui, pick.update == Some(k), Some(Icon::Track), &format!("Update {}", t.name), "Moves this track's start/finish line here. The original is kept in backups.") {
                        pick.update = Some(k);
                    }
                }
                if look::option(ui, pick.update.is_none(), Some(Icon::Plus), "Save as a new track", "A new track file made from this session's fastest lap, with the line here.") {
                    pick.update = None;
                }
                if pick.update.is_none() {
                    ui.add_space(6.0);
                    look::text(ui, &mut pick.name, "Track name", Some(420.0));
                }
            });
            look::footer_row(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let ready = pick.chosen.is_some() && (pick.update.is_some() || !pick.name.trim().is_empty());
                    save = look::big_button(ui, "Save", Some(Icon::Check), look::Kind::Primary, ready);
                    cancel = look::big_button(ui, "Cancel", None, look::Kind::Quiet, true);
                });
            });
        });
        if save && let Some(i) = pick.chosen {
            let (lat, lon) = pick.latlon[i];
            let saved = match pick.update.and_then(|k| pick.near.get(k)) {
                Some(t) => self.move_track_line(&t.path, lat, lon).map(|_| t.name.clone()),
                None => self.save_new_track(&pick, lat, lon).map(|_| pick.name.trim().to_string()),
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
                    self.reopen(pick.path.clone());
                    self.notices.success(format!("Start/finish line saved to {name}. Sessions there are timed from it."));
                }
                Err(e) => {
                    self.notices.error(format!("Couldn't save the start/finish line: {e}"));
                }
            }
        } else if !closed && !cancel {
            self.line_pick = Some(pick);
        }
    }

    /// Moves the start/finish line of your track at `path`, keeping the original in backups.
    fn move_track_line(&mut self, path: &std::path::Path, lat: f64, lon: f64) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let moved = aim_track::write::with_start_finish(&bytes, aim_track::LatLon { lat, lon }).map_err(|e| e.to_string())?;
        let today = crate::dates::today();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let backup = crate::paths::data("backups").unwrap_or_default().join("tracks").join(format!("{stem}-{}.tkk", crate::dates::iso(today)));
        if !backup.exists() {
            crate::storage::write_atomic(&backup, &bytes).map_err(|e| format!("couldn't back it up: {e}"))?;
        }
        crate::storage::write_atomic(path, &moved).map_err(|e| e.to_string())
    }

    /// A new track from the session's fastest lap timed from the line, in
    /// `~/Mychronix/tracks/mychronix/`; the session is put on it.
    fn save_new_track(&mut self, pick: &LinePick, lat: f64, lon: f64) -> Result<(), String> {
        let (lap, length) = lap_outline(&pick.session, (lat, lon)).ok_or("this session has no full lap from that line (pick a point the kart crossed lap after lap)")?;
        let (id, guid, name) = (new_id(), new_guid(), pick.name.trim().to_string());
        let bytes = aim_track::write::from_lap(&name, &id, guid, &lap, length, None).map_err(|e| e.to_string())?;
        let path = crate::paths::data("tracks").ok_or("no home folder")?.join("mychronix").join(format!("{id}.tkk"));
        crate::storage::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
        // This session is at that track now.
        let mut meta = crate::device::SessionMeta::load(&pick.path).unwrap_or_default();
        meta.track = name;
        meta.track_guid = Some(guid.iter().map(|b| format!("{b:02x}")).collect());
        meta.save(&pick.path).map_err(|e| e.to_string())
    }
}
