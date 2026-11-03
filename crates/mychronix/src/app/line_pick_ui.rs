//! Set the start/finish line: for a session the MyChron recorded as one long
//! lap where Mychronix knows no line (a track not in AiM's list, with track
//! learning off). The session's GPS trace; a click on it puts the line
//! there; saved for the place (`tracks::MyLines`), so every session there is
//! timed from it.

use super::*;

pub(super) struct LinePick {
    path: PathBuf,
    /// Where the session was, as the line is saved ("" when unknown).
    track: String,
    /// The trace: degrees, and meters east and north of its first point.
    latlon: Vec<(f64, f64)>,
    xy: Vec<(f64, f64)>,
    /// The point picked on the trace.
    chosen: Option<usize>,
}

impl LinePick {
    pub fn new(path: PathBuf, track: String, gps: &aim_xrk::GpsTrack) -> Option<Self> {
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
        Some(LinePick { path, track, latlon, xy, chosen: None })
    }
}

impl App {
    pub(super) fn line_pick_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pick) = self.line_pick.take() else { return };
        let (mut save, mut cancel) = (false, false);
        let closed = look::dialog(ctx, "line_pick", 760.0, |ui| {
            look::header(ui, Icon::Flag, SECONDARY, None, "Set the start/finish line", "Click where the start/finish line crosses the track. Every session here is timed from it.");
            look::body(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 380.0), Sense::click());
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
                let hovered = response.hover_pos().and_then(near);
                if let Some(i) = hovered {
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
                ui.add_space(8.0);
                let note = match pick.chosen {
                    Some(_) => "The line goes across the track there. Saving times this session and every other one recorded here.",
                    None => "Hover over the track and click where the start/finish line is.",
                };
                look::note(ui, note, DIM);
            });
            look::footer_row(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    save = look::big_button(ui, "Save", Some(Icon::Check), look::Kind::Primary, pick.chosen.is_some());
                    cancel = look::big_button(ui, "Cancel", None, look::Kind::Quiet, true);
                });
            });
        });
        if save && let Some(i) = pick.chosen {
            let (lat, lon) = pick.latlon[i];
            match crate::tracks::MyLines::add(lat, lon, &pick.track) {
                Ok(()) => {
                    log::info!("start/finish line set at {lat:.6}, {lon:.6} ({})", pick.track);
                    // Sessions that looked untimed are timed again against it (the
                    // digests' track stamp changed), this one first.
                    crate::digest::Digests::forget(std::slice::from_ref(&pick.path));
                    self.sensors.digests = None;
                    self.sensors.digests_asked.clear();
                    self.reload_library();
                    self.reopen(pick.path.clone());
                    self.notices.success("Start/finish line saved. Sessions here are timed from it.");
                }
                Err(e) => {
                    self.notices.error(format!("Couldn't save the start/finish line: {e}"));
                }
            }
        } else if !closed && !cancel {
            self.line_pick = Some(pick);
        }
    }
}
