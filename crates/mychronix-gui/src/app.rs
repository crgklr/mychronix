use std::collections::BTreeMap;
use std::path::PathBuf;

use aim_xrk::{Channel, Session, format_lap_time};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, Sense, Stroke, Ui,
    Vec2, pos2, vec2,
};
use egui_plot::{Line, Plot, PlotPoints, VLine};

use crate::splits;
use crate::tracks::{self, Kind, Library};
use crate::theme::{
    self, DIM, GRID, Icon, ON_TEXT, ROW_HEIGHT, SECONDARY, SERIES, SIDEBAR, SURFACE,
    SURFACE_ACTIVE, SURFACE_HOVER, TEXT, WINDOW, medium, regular, semibold,
};

/// Points drawn per channel; longer series are min/max-decimated to this many buckets.
const MAX_POINTS: usize = 4000;
/// Laps shorter than this are out/in laps cut by the session start or end.
const MIN_LAP_MS: i64 = 10_000;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Page {
    Sessions,
    #[default]
    Analysis,
    Tracks,
    Device,
}

/// The download dialog's state.
struct DownloadDialog {
    meta: crate::device::SessionMeta,
    /// Tracks the logger recorded for the selected sessions; more than one means
    /// each session keeps its own.
    detected: Vec<String>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum MapMode {
    #[default]
    Speed,
    Splits,
    Elevation,
    Outline,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum TrackView {
    #[default]
    All,
    Favorites,
    Custom,
    Raced,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum ChannelView {
    #[default]
    All,
    Graphed,
}

#[derive(Default)]
pub struct App {
    page: Page,
    path: Option<PathBuf>,
    session: Option<Session>,
    error: Option<String>,
    /// Selected channels and the colour slot each keeps while selected.
    selected: BTreeMap<usize, usize>,
    lap: Option<usize>,
    filter: String,
    channel_view: ChannelView,
    /// Shared cursor, in seconds from the start of the current window.
    cursor_s: Option<f64>,
    scroll_to_lap: bool,
    /// Channel that colours the track map (None: RPM if present).
    map_channel: Option<usize>,
    pub screenshot_to: Option<PathBuf>,
    frames: u32,

    // Device page
    dev: Option<crate::device::Worker>,
    dev_logger: Option<aim_link::Beacon>,
    dev_networks: Vec<(String, u8)>,
    dev_current_wifi: Option<String>,
    /// The AiM network Mychronix joined (so it can leave it).
    dev_joined: Option<String>,
    dev_files: Vec<aim_link::DirEntry>,
    dev_summaries: Vec<aim_xrk::SessionSummary>,
    dev_selected: std::collections::BTreeSet<String>,
    /// What the worker is doing, shown while busy.
    dev_busy: Option<String>,
    dev_progress: Option<(usize, usize, String, u64, u64)>,
    dev_message: Option<(bool, String)>,
    dev_last_scan: f64,
    /// The download dialog, while open.
    dev_dialog: Option<DownloadDialog>,

    // Sessions page
    profiles: Option<crate::profiles::Profiles>,
    /// Logger id to show, or all.
    session_logger: Option<String>,
    /// Track index to show, or all.
    session_track: Option<usize>,
    session_search: String,

    // Tracks page
    library: Option<Library>,
    track_sel: Option<usize>,
    track_filter: String,
    track_view: TrackView,
    favorites: Option<tracks::Favorites>,
    /// Expanded groups in the track list: country codes, or `YOURS`.
    track_groups_open: std::collections::HashSet<String>,
    /// Set once the list has opened the system country and scrolled to it.
    track_list_ready: bool,
    track_details: Option<std::sync::mpsc::Receiver<(usize, tracks::Details)>>,
    /// The selected track's full file (logo, contact details), read on selection.
    track_file: Option<(usize, aim_track::TrackFile)>,
    map_tiles: Option<walkers::HttpTiles>,
    sat_tiles: Option<walkers::HttpTiles>,
    /// Satellite imagery under the Tracks map instead of OpenStreetMap.
    map_satellite: bool,
    map_memory: walkers::MapMemory,
    /// Your fastest lap at each track (GPS path and speed), by track guid; None when there's none.
    speed_laps: std::collections::HashMap<String, Option<SpeedLap>>,
    /// What the Tracks map shows.
    track_map: MapMode,
    /// Number of sectors on the Splits view.
    split_count: usize,
    /// Set when a track is selected; the map zooms to fit it on the next draw.
    fit_map: bool,
}

impl App {
    pub fn load(&mut self, path: PathBuf) {
        match Session::open(&path) {
            Ok(mut session) => {
                // The logger leaves driver and kart blank; use the names saved for it.
                let logger = session.info.logger_id.map(|id| id.to_string());
                let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
                if let Some(p) = logger.as_deref().and_then(|l| profiles.get(l)) {
                    if session.info.driver.as_deref().unwrap_or("").is_empty() {
                        session.info.driver = Some(p.driver.clone()).filter(|d| !d.is_empty());
                    }
                    if session.info.vehicle.as_deref().unwrap_or("").is_empty() {
                        session.info.vehicle = Some(p.kart.clone()).filter(|k| !k.is_empty());
                    }
                }
                self.selected.clear();
                for i in default_channels(&session) {
                    self.select(i);
                }
                self.lap = None;
                self.cursor_s = None;
                self.map_channel = None;
                self.error = None;
                self.session = Some(session);
                self.path = Some(path);
                self.page = Page::Analysis;
            }
            Err(e) => self.error = Some(format!("Couldn't open {}: {e}", file_name(&path))),
        }
    }

    /// Opens on the best lap with the cursor 40% in, for screenshots.
    pub fn demo(&mut self) {
        let Some(session) = &self.session else { return };
        let Some(best) = session.best_lap(MIN_LAP_MS).copied() else { return };
        self.lap = session.laps.iter().position(|l| *l == best);
        self.cursor_s = Some(best.duration_ms() as f64 / 1000.0 * 0.4);
        self.scroll_to_lap = true;
    }

    pub fn show_device_page(&mut self) {
        self.page = Page::Device;
    }

    pub fn show_tracks_page(&mut self) {
        self.page = Page::Tracks;
    }

    pub fn show_sessions_page(&mut self) {
        self.page = Page::Sessions;
    }

    fn select(&mut self, channel: usize) {
        let used: Vec<usize> = self.selected.values().copied().collect();
        if let Some(slot) = (0..SERIES.len()).find(|s| !used.contains(s)) {
            self.selected.insert(channel, slot);
        }
    }

    /// Time window in logger ms: the selected lap, or the whole session.
    fn window(&self, session: &Session) -> (i64, i64) {
        if let Some(lap) = self.lap.and_then(|i| session.laps.get(i)) {
            return (lap.start_ms, lap.end_ms);
        }
        let start = session.channels.iter().filter_map(|c| c.times_ms.first()).min();
        let end = session.channels.iter().filter_map(|c| c.times_ms.last()).max();
        (start.copied().unwrap_or(0), end.copied().unwrap_or(0))
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AiM session", &["xrk", "xrz", "XRK", "XRZ"])
            .pick_file()
        {
            self.load(path);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // MYCHRONIX_DEBUG_SCALE=1 prints what egui thinks the display is (development only).
        if std::env::var("MYCHRONIX_DEBUG_SCALE").is_ok() && self.frames % 60 == 5 {
            let ctx = ui.ctx();
            let (native, inner, outer, monitor) = ctx.input(|i| {
                let v = i.viewport();
                (v.native_pixels_per_point, v.inner_rect, v.outer_rect, v.monitor_size)
            });
            eprintln!(
                "ppp={} native_ppp={:?} screen={:?} inner={:?} outer={:?} monitor={:?} maximized={:?}",
                ctx.pixels_per_point(),
                native,
                ctx.content_rect(),
                inner,
                outer,
                monitor,
                ctx.input(|i| i.viewport().maximized)
            );
        }
        // Some desktops ignore maximise at creation; ask again once the window is up.
        if self.frames < 2 && self.screenshot_to.is_none() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if self.screenshot_to.is_none() {
            self.frames = self.frames.saturating_add(1).min(10);
        }
        let session = self.session.take();
        let window = session.as_ref().map(|s| self.window(s));

        self.status_bar(ui, session.as_ref());

        egui::Panel::left("sidebar")
            .exact_size(304.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(SIDEBAR).inner_margin(Margin { left: 16, right: 16, top: 18, bottom: 12 }))
            .show(ui, |ui| {
                wordmark(ui);
                ui.add_space(20.0);
                self.nav(ui);
                if let (Page::Analysis, Some(s), Some((start, _))) = (self.page, &session, window) {
                    ui.add_space(14.0);
                    hairline(ui);
                    ui.add_space(14.0);
                    self.lap_list(ui, s);
                    ui.add_space(16.0);
                    self.channel_list(ui, s, start);
                }
            });

        egui::CentralPanel::default()
            .frame(Frame::new().fill(WINDOW).inner_margin(Margin { left: 28, right: 28, top: 22, bottom: 16 }))
            .show(ui, |ui| match (self.page, &session, window) {
                (Page::Analysis, Some(s), Some((start, end))) => self.analysis(ui, s, start, end),
                (Page::Analysis, _, _) => self.empty_state(ui),
                (Page::Sessions, _, _) => self.sessions_page(ui),
                (Page::Tracks, _, _) => self.tracks_page(ui),
                (Page::Device, _, _) => self.device_page(ui),
            });

        self.session = session;
        self.screenshot(ui);
    }
}

// ---------------------------------------------------------------------------
// Chrome: sidebar navigation, status bar, empty states

impl App {
    fn nav(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (page, icon, label) in [
            (Page::Sessions, Icon::List, "Sessions"),
            (Page::Analysis, Icon::ChartLine, "Analysis"),
            (Page::Tracks, Icon::MapPin, "Tracks"),
            (Page::Device, Icon::Gauge, "Device"),
        ] {
            let selected = self.page == page;
            let response = row(ui, selected, |ui| {
                let tint = if selected { TEXT } else { SECONDARY };
                ui.add(icon.image(tint, 18.0));
                ui.add_space(4.0);
                ui.label(RichText::new(label).font(medium(14.5)).color(tint));
            });
            if response.clicked() {
                self.page = page;
            }
        }
    }

    fn status_bar(&self, ui: &mut Ui, session: Option<&Session>) {
        egui::Panel::bottom("status")
            .exact_size(30.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(SIDEBAR).inner_margin(Margin::symmetric(16, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    match &self.dev_logger {
                        Some(b) => {
                            ui.add(Icon::Wifi.image(theme::GOOD, 14.0));
                            let id = b.logger_id.map(|i| format!("logger {i}")).unwrap_or_else(|| "logger".into());
                            ui.label(RichText::new(format!("MyChron connected  ·  {id}")).size(12.0).color(SECONDARY));
                        }
                        None => {
                            ui.add(Icon::Wifi.image(DIM, 14.0));
                            ui.label(RichText::new("No logger connected").size(12.0).color(DIM));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let (Some(s), Some(path)) = (session, &self.path) {
                            let samples: usize = s.channels.iter().map(|c| c.values.len()).sum();
                            ui.label(RichText::new(format!("{} channels  ·  {} samples", s.channels.len(), group_thousands(samples))).size(12.0).color(DIM));
                            ui.add_space(10.0);
                            ui.label(RichText::new(file_name(path)).size(12.0).color(DIM));
                        }
                        if let Some(err) = &self.error {
                            ui.label(RichText::new(err).size(12.0).color(ui.visuals().error_fg_color));
                        }
                    });
                });
            });
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.add_space((ui.available_height() * 0.5 - 120.0).max(24.0));
            ui.add(Icon::ChartLine.image(DIM, 44.0));
            ui.add_space(14.0);
            ui.label(RichText::new("Open a session").font(semibold(24.0)).color(TEXT));
            ui.add_space(2.0);
            ui.label(RichText::new("Pick a .xrk or .xrz file recorded by your MyChron to see laps and channels.").color(SECONDARY));
            ui.add_space(18.0);
            if button(ui, Some(Icon::FolderOpen), "Open session…", true).clicked() {
                self.open_dialog();
            }
        });
    }

    fn screenshot(&mut self, ui: &mut Ui) {
        let Some(path) = self.screenshot_to.clone() else { return };
        self.frames += 1;
        // MYCHRONIX_SCREENSHOT_FRAME delays the shot, e.g. to let map tiles load.
        let at = std::env::var("MYCHRONIX_SCREENSHOT_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
        if self.frames == at {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let image = ui.ctx().input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec())
                && let Err(e) = buf.save(&path)
            {
                eprintln!("screenshot: {e}");
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------
// Sessions page

impl App {
    fn sessions_page(&mut self, ui: &mut Ui) {
        let lib = self.library.take().unwrap_or_else(Library::load);
        let mut profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let loggers: Vec<String> = {
            let mut l: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
            l.sort();
            l.dedup();
            l
        };
        let driver_name = |logger: &str, profiles: &crate::profiles::Profiles| {
            profiles.get(logger).map(|p| p.driver.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| format!("Logger {logger}"))
        };

        let raced: Vec<usize> = {
            let mut v: Vec<usize> = lib.sessions.iter().filter_map(|s| s.track).collect();
            v.sort();
            v.dedup();
            v
        };
        ui.label(RichText::new("Sessions").font(semibold(28.0)).color(TEXT));
        ui.label(
            RichText::new(format!(
                "{} sessions  ·  {} track{}  ·  {} logger{}",
                lib.sessions.len(),
                raced.len(),
                if raced.len() == 1 { "" } else { "s" },
                loggers.len(),
                if loggers.len() == 1 { "" } else { "s" }
            ))
            .font(regular(14.5))
            .color(SECONDARY),
        );
        ui.add_space(18.0);

        if lib.sessions.is_empty() {
            card(ui, |ui| {
                ui.label(RichText::new("No sessions yet").font(semibold(17.0)).color(TEXT));
                ui.label(RichText::new("Download from your MyChron (Device page, or `mychronix device pull`) and they'll appear here.").color(SECONDARY));
            });
            self.library = Some(lib);
            self.profiles = Some(profiles);
            return;
        }

        let gap = 14.0;
        let height = ui.available_height();
        let side_w = 340.0_f32.min(ui.available_width() * 0.32);
        let list_w = ui.available_width() - side_w - gap;
        let mut open = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(list_w, height), column, |ui| {
                ui.set_width(list_w);
                // Filters: driver, track, search.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if chip(ui, "All drivers", self.session_logger.is_none()).clicked() {
                        self.session_logger = None;
                    }
                    for l in &loggers {
                        if chip(ui, &driver_name(l, &profiles), self.session_logger.as_deref() == Some(l.as_str())).clicked() {
                            self.session_logger = Some(l.clone());
                        }
                    }
                    ui.add_space(12.0);
                    if chip(ui, "All tracks", self.session_track.is_none()).clicked() {
                        self.session_track = None;
                    }
                    for &ti in &raced {
                        if chip(ui, &lib.tracks[ti].name, self.session_track == Some(ti)).clicked() {
                            self.session_track = Some(ti);
                        }
                    }
                });
                ui.add_space(8.0);
                search_field(ui, &mut self.session_search, "Search by session number, date or track");
                ui.add_space(10.0);

                let needle = self.session_search.to_lowercase();
                let mut shown: Vec<usize> = (0..lib.sessions.len())
                    .filter(|&i| {
                        let s = &lib.sessions[i];
                        self.session_logger.as_ref().is_none_or(|l| *l == s.logger)
                            && self.session_track.is_none_or(|t| s.track == Some(t))
                            && (needle.is_empty() || {
                                let sum = &s.summary;
                                let hay = format!(
                                    "{} {} {} {}",
                                    sum.number.unwrap_or(0),
                                    sum.date.map(|d| d.to_string()).unwrap_or_default(),
                                    sum.track.clone().unwrap_or_default(),
                                    s.track.map(|t| lib.tracks[t].full_name.clone().unwrap_or_default()).unwrap_or_default()
                                )
                                .to_lowercase();
                                hay.contains(&needle)
                            })
                    })
                    .collect();
                // Newest first.
                shown.sort_by_key(|&i| {
                    let s = &lib.sessions[i].summary;
                    std::cmp::Reverse((s.date, s.start_time.clone(), s.number))
                });

                egui::ScrollArea::vertical().id_salt("sessions_list").auto_shrink([false, false]).show(ui, |ui| {
                    let mut k = 0;
                    while k < shown.len() {
                        // One group per race day.
                        let date = lib.sessions[shown[k]].summary.date;
                        let mut end = k;
                        while end < shown.len() && lib.sessions[shown[end]].summary.date == date {
                            end += 1;
                        }
                        let day = &shown[k..end];
                        let mut tracks: Vec<String> = day
                            .iter()
                            .filter_map(|&i| lib.sessions[i].summary.track.clone())
                            .collect();
                        tracks.dedup();
                        let best = day.iter().filter_map(|&i| lib.sessions[i].summary.best_lap_ms).min();
                        ui.add_space(if k == 0 { 0.0 } else { 14.0 });
                        ui.horizontal(|ui| {
                            ui.add(Icon::Calendar.image(SECONDARY, 16.0));
                            let heading = date.map(long_date).unwrap_or_else(|| "Unknown date".into());
                            ui.label(RichText::new(heading).font(semibold(16.0)).color(TEXT));
                            ui.label(RichText::new(format!("{}  ·  {} session{}", tracks.join(", "), day.len(), if day.len() == 1 { "" } else { "s" })).color(SECONDARY));
                            if let Some(b) = best {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(RichText::new(format_lap_time(b as i64)).font(semibold(15.0)).color(theme::GOOD));
                                    ui.label(RichText::new("Day best").size(12.5).color(DIM));
                                });
                            }
                        });
                        ui.add_space(6.0);
                        for &i in day {
                            let s = &lib.sessions[i];
                            let driver = s.meta.as_ref().map(|m| m.driver.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| driver_name(&s.logger, &profiles));
                            if session_row(ui, &lib, i, &driver, best).clicked() {
                                open = Some(lib.sessions[i].path.clone());
                            }
                        }
                        k = end;
                    }
                    if shown.is_empty() {
                        ui.label(RichText::new("No sessions match.").color(DIM));
                    }
                });
            });

            // Drivers & karts: names for each logger, saved as you type.
            ui.allocate_ui_with_layout(vec2(side_w, height), column, |ui| {
                ui.set_width(side_w);
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(Icon::User.image(SECONDARY, 16.0));
                        ui.label(RichText::new("Drivers & karts").font(semibold(16.0)).color(TEXT));
                    });
                    ui.label(RichText::new("Your MyChrons don't store names, so Mychronix keeps them here, one per logger.").size(12.5).color(SECONDARY));
                    ui.add_space(8.0);
                    let mut changed = false;
                    for l in &loggers {
                        let entry = profiles.loggers.entry(l.clone()).or_default();
                        ui.label(RichText::new(format!("Logger {l}")).size(12.5).color(DIM));
                        egui::Grid::new(("profile", l.as_str())).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.label(RichText::new("Driver").color(SECONDARY));
                            changed |= ui.add(egui::TextEdit::singleline(&mut entry.driver).desired_width(side_w - 120.0)).changed();
                            ui.end_row();
                            ui.label(RichText::new("Kart").color(SECONDARY));
                            changed |= ui.add(egui::TextEdit::singleline(&mut entry.kart).desired_width(side_w - 120.0)).changed();
                            ui.end_row();
                        });
                        ui.add_space(10.0);
                    }
                    if changed {
                        if let Err(e) = profiles.save() {
                            self.error = Some(format!("Couldn't save names: {e}"));
                        }
                    }
                });
            });
        });
        self.library = Some(lib);
        self.profiles = Some(profiles);
        if let Some(path) = open {
            self.load(path);
        }
    }
}

/// One session in the list: number, time, driver; laps and best lap on the right.
fn session_row(ui: &mut Ui, lib: &Library, i: usize, driver: &str, day_best: Option<u64>) -> egui::Response {
    let s = &lib.sessions[i];
    let sum = &s.summary;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if response.hovered() {
            p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else {
            p.rect_filled(rect.shrink2(vec2(0.0, 1.0)), CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE);
        }
        let (l, r) = (rect.left() + 14.0, rect.right() - 14.0);
        let (top, bottom) = (rect.top() + 17.0, rect.top() + 35.0);
        p.text(pos2(l, top), egui::Align2::LEFT_CENTER, format!("Session {}", sum.number.unwrap_or(0)), medium(14.5), TEXT);
        let track = s.track.map(|t| lib.tracks[t].name.clone()).or(sum.track.clone()).unwrap_or_else(|| "No track".into());
        let championship = s.meta.as_ref().map(|m| m.championship.clone()).filter(|c| !c.is_empty());
        let line = [sum.start_time.clone().map(|t| t.get(..5).unwrap_or(&t).to_string()), Some(driver.to_string()), Some(track), championship]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("  ·  ");
        p.text(pos2(l, bottom), egui::Align2::LEFT_CENTER, line, regular(12.5), DIM);

        // Right side: best lap, then laps / time on track / max RPM.
        let is_day_best = sum.best_lap_ms.is_some() && sum.best_lap_ms == day_best;
        let best = sum.best_lap_ms.map(|b| format_lap_time(b as i64)).unwrap_or_else(|| "—".into());
        p.text(pos2(r, top), egui::Align2::RIGHT_CENTER, best, medium(14.5), if is_day_best { theme::GOOD } else { TEXT });
        let laps = sum.laps.unwrap_or(0);
        let mut facts = vec![format!("{laps} lap{}", if laps == 1 { "" } else { "s" })];
        if let Some(d) = sum.duration_ms {
            facts.push(format_duration(d as i64));
        }
        if let Some(rpm) = sum.max_rpm {
            facts.push(format!("{} rpm max", group_thousands(rpm as usize)));
        }
        p.text(pos2(r, bottom), egui::Align2::RIGHT_CENTER, facts.join("  ·  "), regular(12.5), DIM);
    }
    response
}

/// "Saturday, 10 October 2026".
fn long_date(d: aim_xrk::summary::Date) -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    // Sakamoto's day-of-week method.
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if d.month < 3 { d.year as i32 - 1 } else { d.year as i32 };
    let dow = (y + y / 4 - y / 100 + y / 400 + t[d.month as usize - 1] + d.day as i32).rem_euclid(7) as usize;
    format!("{}, {} {} {}", DAYS[dow], d.day, MONTHS[d.month as usize - 1], d.year)
}

// ---------------------------------------------------------------------------
// Tracks page

impl App {
    fn tracks_page(&mut self, ui: &mut Ui) {
        let mut lib = self.library.take().unwrap_or_else(Library::load);
        // Full names, cities and outlines arrive from a background reader.
        if self.track_details.is_none() {
            self.track_details = Some(tracks::load_details(lib.pending_details(), ui.ctx().clone()));
        }
        if let Some(rx) = &self.track_details {
            let mut got = false;
            while let Ok((i, d)) = rx.try_recv() {
                lib.apply_details(i, d);
                got = true;
            }
            if got {
                ui.ctx().request_repaint();
            }
        }
        if self.track_sel.is_none() && !lib.tracks.is_empty() {
            // MYCHRONIX_TRACK=<name> preselects a track (for screenshots).
            match std::env::var("MYCHRONIX_MAP").as_deref() {
                Ok("splits") => self.track_map = MapMode::Splits,
                Ok("elevation") => self.track_map = MapMode::Elevation,
                _ => {}
            }
            if std::env::var("MYCHRONIX_SATELLITE").is_ok() {
                self.map_satellite = true;
            }
            let wanted = std::env::var("MYCHRONIX_TRACK").ok();
            let i = wanted.and_then(|n| lib.tracks.iter().position(|t| t.name == n)).unwrap_or(0);
            self.select_track(&lib, i);
        }

        let visited = lib.tracks.iter().filter(|t| !t.sessions.is_empty()).count();
        ui.label(RichText::new("Tracks").font(semibold(28.0)).color(TEXT));
        ui.label(
            RichText::new(format!(
                "{} AiM tracks  ·  {} custom  ·  {} with your sessions",
                group_thousands(lib.count(Kind::Aim)),
                lib.count(Kind::Custom),
                visited
            ))
            .font(regular(14.5))
            .color(SECONDARY),
        );
        ui.add_space(18.0);

        if lib.tracks.is_empty() {
            card(ui, |ui| {
                ui.label(RichText::new("No tracks found").font(semibold(17.0)).color(TEXT));
                ui.label(RichText::new("Put AiM track files (.ria, .tkk) in ~/Mychronix/tracks, or install RaceStudio 3 under Wine.").color(SECONDARY));
            });
            self.library = Some(lib);
            return;
        }

        let gap = 14.0;
        let height = ui.available_height();
        let list_w = (ui.available_width() * 0.36).clamp(280.0, 380.0);
        let detail_w = ui.available_width() - list_w - gap;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(list_w, height), column, |ui| {
                ui.set_width(list_w);
                self.track_list(ui, &lib);
            });
            ui.allocate_ui_with_layout(vec2(detail_w, height), column, |ui| {
                ui.set_width(detail_w);
                if let Some(i) = self.track_sel {
                    // The detail scrolls as one column; the map keeps a fixed share of the view.
                    let map_h = (height * 0.5).clamp(300.0, 620.0);
                    let mut scroll = egui::ScrollArea::vertical().id_salt("track_detail").auto_shrink([false, false]);
                    // MYCHRONIX_DETAIL_SCROLL=<px> starts the detail scrolled down (for screenshots).
                    if self.frames < 2 {
                        if let Some(px) = std::env::var("MYCHRONIX_DETAIL_SCROLL").ok().and_then(|v| v.parse().ok()) {
                            scroll = scroll.vertical_scroll_offset(px);
                        }
                    }
                    scroll.show(ui, |ui| {
                        self.track_detail(ui, &lib, i, map_h);
                    });
                }
            });
        });
        self.library = Some(lib);
    }

    fn select_track(&mut self, lib: &Library, i: usize) {
        self.track_sel = Some(i);
        let t = &lib.tracks[i];
        self.track_file = t.tkk.as_ref().and_then(|p| aim_track::read_tkk(p).ok()).map(|f| (i, f));
        if let Some(sf) = t.start_finish {
            self.map_memory.center_at(walkers::lat_lon(sf.lat, sf.lon));
            let _ = self.map_memory.set_zoom(16.5);
        }
        self.fit_map = true;
        // Your fastest lap here, for the speed map.
        if !t.sessions.is_empty() && !self.speed_laps.contains_key(&t.guid) {
            self.speed_laps.insert(t.guid.clone(), speed_lap(lib, t));
        }
    }

    fn track_list(&mut self, ui: &mut Ui, lib: &Library) {
        let favorites = self.favorites.get_or_insert_with(tracks::Favorites::load).clone();
        // Tabs.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let count = |f: &dyn Fn(&tracks::Track) -> bool| lib.tracks.iter().filter(|t| f(t)).count();
            for (view, label, n) in [
                (TrackView::All, "All", lib.tracks.len()),
                (TrackView::Favorites, "Favorites", count(&|t| favorites.contains(&t.guid))),
                (TrackView::Custom, "Custom", count(&|t| t.kind == Kind::Custom)),
                (TrackView::Raced, "Raced", count(&|t| !t.sessions.is_empty())),
            ] {
                if chip(ui, &format!("{label}  {}", group_thousands(n)), self.track_view == view).clicked() {
                    self.track_view = view;
                }
            }
        });
        ui.add_space(8.0);
        search_field(ui, &mut self.track_filter, "Search tracks, cities, regions");
        ui.add_space(8.0);

        let needle = self.track_filter.to_lowercase();
        let searching = !needle.is_empty();
        let in_view = |t: &tracks::Track| match self.track_view {
            TrackView::All => true,
            TrackView::Favorites => favorites.contains(&t.guid),
            TrackView::Custom => t.kind == Kind::Custom,
            TrackView::Raced => !t.sessions.is_empty(),
        };
        let matches = |t: &tracks::Track| {
            in_view(t)
                && (!searching
                    || t.name.to_lowercase().contains(&needle)
                    || t.region.to_lowercase().contains(&needle)
                    || t.city.as_deref().is_some_and(|c| c.to_lowercase().contains(&needle))
                    || t.full_name.as_deref().is_some_and(|c| c.to_lowercase().contains(&needle))
                    || country_label(&t.country).to_lowercase().contains(&needle))
        };
        let by_name = |a: &usize, b: &usize| {
            let key = |i: usize| lib.tracks[i].full_name.clone().unwrap_or_else(|| lib.tracks[i].name.clone()).to_lowercase();
            key(*a).cmp(&key(*b))
        };

        // Rows: the All tab groups by country under collapsible headers; the
        // other tabs are short, flat lists.
        #[derive(Clone)]
        enum Row {
            Header(String, String, usize, bool),
            Track(usize),
        }
        const HEADER_H: f32 = 44.0;
        const TRACK_H: f32 = 76.0;
        let mut rows: Vec<Row> = Vec::new();
        let mut system_header = None;
        let system = crate::countries::system_country();
        if self.track_view == TrackView::All {
            // Open the system country first (MYCHRONIX_TRACKS_COLLAPSED=1 skips that, for screenshots).
            if !self.track_list_ready && std::env::var("MYCHRONIX_TRACKS_COLLAPSED").is_err() {
                if let Some(code) = &system {
                    self.track_groups_open.insert(code.clone());
                }
            }
            let mut by_country: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
            for (i, t) in lib.tracks.iter().enumerate() {
                if matches(t) {
                    by_country.entry(t.country.clone()).or_default().push(i);
                }
            }
            let mut groups: Vec<(String, String, Vec<usize>)> =
                by_country.into_iter().map(|(code, mut v)| { v.sort_by(by_name); let l = country_label(&code); (code, l, v) }).collect();
            groups.sort_by(|a, b| a.1.cmp(&b.1));
            for (code, label, members) in groups {
                let open = searching || self.track_groups_open.contains(&code);
                if system.as_deref() == Some(code.as_str()) {
                    system_header = Some(rows.len());
                }
                rows.push(Row::Header(code.clone(), label, members.len(), open));
                if open {
                    rows.extend(members.into_iter().map(Row::Track));
                }
            }
        } else {
            let mut v: Vec<usize> = (0..lib.tracks.len()).filter(|&i| matches(&lib.tracks[i])).collect();
            v.sort_by(by_name);
            rows.extend(v.into_iter().map(Row::Track));
        }

        // Variable-height rows, drawn only where visible.
        let heights: Vec<f32> = rows.iter().map(|r| if matches!(r, Row::Header(..)) { HEADER_H } else { TRACK_H }).collect();
        let mut tops = Vec::with_capacity(rows.len());
        let mut y = 0.0;
        for h in &heights {
            tops.push(y);
            y += h;
        }
        let total = y;

        let mut scroll = egui::ScrollArea::vertical().id_salt(("track_list", self.track_view as u8)).auto_shrink([false, false]);
        if !self.track_list_ready {
            if let Some(r) = system_header {
                scroll = scroll.vertical_scroll_offset(tops[r]);
            }
            self.track_list_ready = true;
        }

        let mut clicked = None;
        let mut toggled = None;
        let mut starred = None;
        scroll.show_viewport(ui, |ui, viewport| {
            let full = ui.max_rect();
            ui.set_height(total);
            let first = tops.partition_point(|&t| t + TRACK_H < viewport.min.y);
            for k in first..rows.len() {
                if tops[k] > viewport.max.y {
                    break;
                }
                let rect = Rect::from_min_size(pos2(full.left(), full.top() + tops[k]), vec2(full.width(), heights[k]));
                let response = ui.interact(rect, ui.id().with(("track_row", k)), Sense::click());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let p = ui.painter();
                match &rows[k] {
                    Row::Header(code, label, count, open) => {
                        if response.hovered() {
                            p.rect_filled(rect.shrink2(vec2(0.0, 3.0)), CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE_HOVER);
                        }
                        let chevron = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
                        chevron.image(SECONDARY, 16.0).paint_at(ui, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(16.0)));
                        p.text(pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, label, semibold(14.5), TEXT);
                        p.text(pos2(rect.right() - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, group_thousands(*count), regular(12.5), DIM);
                        if response.clicked() && !searching {
                            toggled = Some(code.clone());
                        }
                    }
                    Row::Track(i) => {
                        let i = *i;
                        let t = &lib.tracks[i];
                        let fill = if self.track_sel == Some(i) { Some(SURFACE_ACTIVE) } else if response.hovered() { Some(SURFACE_HOVER) } else { None };
                        if let Some(fill) = fill {
                            p.rect_filled(rect.shrink2(vec2(0.0, 2.0)), CornerRadius::same(theme::RADIUS_SMALL + 2), fill);
                        }
                        let indent = if self.track_view == TrackView::All { 8.0 } else { 0.0 };
                        let thumb = Rect::from_min_size(pos2(rect.left() + 8.0 + indent, rect.top() + 8.0), vec2(64.0, TRACK_H - 16.0));
                        outline_thumbnail(p, thumb, &t.outline, if t.kind == Kind::Custom { SERIES[1] } else { SECONDARY });
                        let left = thumb.right() + 12.0;
                        let title = t.full_name.as_deref().unwrap_or(&t.name);
                        let name_w = (rect.right() - left - 120.0).max(60.0);
                        p.galley(pos2(left, rect.top() + 9.0), p.layout_job(single_line(title, medium(14.0), TEXT, name_w)), TEXT);
                        let place = if self.track_view == TrackView::All {
                            t.locality()
                        } else {
                            [t.locality(), country_label(&t.country)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
                        };
                        p.text(pos2(left, rect.top() + 39.0), egui::Align2::LEFT_CENTER, place, regular(12.5), SECONDARY);
                        p.text(pos2(left, rect.top() + 57.0), egui::Align2::LEFT_CENTER, t.description(), regular(12.0), DIM);
                        // Right: favourite star, then the name on the device; custom tag and sessions below.
                        let star = Rect::from_center_size(pos2(rect.right() - 20.0, rect.top() + 18.0), Vec2::splat(26.0));
                        let star_resp = ui.interact(star, ui.id().with(("track_star", k)), Sense::click());
                        let fav = favorites.contains(&t.guid);
                        let (icon, tint) = if fav { (Icon::StarFilled, theme::FAVORITE) } else if star_resp.hovered() { (Icon::Star, TEXT) } else { (Icon::Star, DIM) };
                        icon.image(tint, 16.0).paint_at(ui, Rect::from_center_size(star.center(), Vec2::splat(16.0)));
                        if star_resp.clicked() {
                            starred = Some(t.guid.clone());
                        }
                        let p = ui.painter();
                        p.text(pos2(star.left() - 4.0, rect.top() + 18.0), egui::Align2::RIGHT_CENTER, &t.name, regular(12.0), DIM);
                        let mut right = rect.right() - 12.0;
                        if !t.sessions.is_empty() {
                            let n = t.sessions.len();
                            let g = p.text(pos2(right, rect.top() + 50.0), egui::Align2::RIGHT_CENTER, format!("{n} session{}", if n == 1 { "" } else { "s" }), regular(12.5), SECONDARY);
                            right = g.left() - 8.0;
                        }
                        if t.kind == Kind::Custom {
                            let galley = p.layout_no_wrap("Custom".into(), semibold(11.0), TEXT);
                            let r = Rect::from_min_size(pos2(right - galley.size().x - 14.0, rect.top() + 40.0), galley.size() + vec2(14.0, 6.0));
                            p.rect_stroke(r, CornerRadius::same(255), Stroke::new(1.0, DIM), egui::StrokeKind::Inside);
                            p.galley(r.center() - galley.size() / 2.0, galley, TEXT);
                        }
                        if response.clicked() && !star_resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                }
            }
        });
        if rows.is_empty() {
            let msg = match self.track_view {
                TrackView::Favorites if !searching => "No favourites yet. Star a track to keep it here.",
                TrackView::Custom if !searching => "No custom tracks yet. They come over from your MyChron when you connect.",
                _ => "No tracks match.",
            };
            ui.label(RichText::new(msg).color(DIM));
        }
        if let Some(code) = toggled {
            if !self.track_groups_open.remove(&code) {
                self.track_groups_open.insert(code);
            }
        }
        if let Some(guid) = starred {
            if let Some(f) = self.favorites.as_mut() {
                if let Err(e) = f.toggle(&guid) {
                    self.error = Some(format!("Couldn't save favourites: {e}"));
                }
            }
        }
        if let Some(i) = clicked {
            self.select_track(lib, i);
        }
    }

    fn track_detail(&mut self, ui: &mut Ui, lib: &Library, i: usize, map_h: f32) {
        let t = &lib.tracks[i];
        let file = self.track_file.as_ref().filter(|(j, _)| *j == i).map(|(_, f)| f);
        let info = file.map(|f| &f.info);

        // AiM's details for the venue, laid out like RaceStudio's track view.
        card(ui, |ui| {
            ui.horizontal_top(|ui| {
                let logo_w = 150.0;
                let text_w = ui.available_width() - logo_w - 12.0;
                ui.allocate_ui_with_layout(vec2(text_w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let title = t.full_name.as_deref().unwrap_or(&t.name);
                    let fav = self.favorites.get_or_insert_with(tracks::Favorites::load).contains(&t.guid);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).font(semibold(22.0)).color(TEXT));
                        let (icon, label) = if fav { (Icon::StarFilled, "Favorite") } else { (Icon::Star, "Add to favorites") };
                        if button(ui, Some(icon), label, false).clicked() {
                            if let Err(e) = self.favorites.as_mut().unwrap().toggle(&t.guid) {
                                self.error = Some(format!("Couldn't save favourites: {e}"));
                            }
                        }
                    });
                    let kind = if t.kind == Kind::Custom { "Custom track" } else { "AiM track" };
                    ui.label(RichText::new(format!("{kind}  ·  name on device: {}", t.name)).size(13.0).color(SECONDARY));
                    ui.add_space(6.0);
                    let address = [
                        info.and_then(|i| i.address.clone()),
                        info.and_then(|i| i.postcode.clone()),
                        info.and_then(|i| i.city.clone()).or(t.city.clone()),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("  ·  ");
                    if !address.is_empty() {
                        ui.label(RichText::new(address).color(TEXT));
                    }
                    let place = [t.region.clone(), country_label(&t.country)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
                    ui.label(RichText::new(place).color(TEXT));
                    if let Some(phone) = info.and_then(|i| i.phone.as_deref()) {
                        ui.label(RichText::new(phone).color(SECONDARY));
                    }
                    if let Some(url) = info.and_then(|i| i.website.as_deref()) {
                        ui.hyperlink_to(RichText::new(url).color(SERIES[0]), url);
                    }
                    ui.add_space(6.0);
                    let desc = t.description();
                    if !desc.is_empty() {
                        ui.label(RichText::new(desc).font(medium(14.0)).color(TEXT));
                    }
                    if let Some(tz) = file.and_then(|f| f.timezone.as_deref()) {
                        ui.label(RichText::new(format!("Time zone: {tz}")).size(13.0).color(SECONDARY));
                    }
                    if let Some(sf) = t.start_finish {
                        let ns = if sf.lat >= 0.0 { "N" } else { "S" };
                        let ew = if sf.lon >= 0.0 { "E" } else { "W" };
                        ui.label(
                            RichText::new(format!("Start/finish: {:.7}° {ns}, {:.7}° {ew}", sf.lat.abs(), sf.lon.abs()))
                                .size(13.0)
                                .color(SECONDARY),
                        );
                    }
                    if let Some(comment) = info.and_then(|i| i.comment.as_deref()) {
                        ui.label(RichText::new(comment).size(13.0).color(SECONDARY));
                    }
                    // Everything AiM stores for the track, verbatim.
                    egui::CollapsingHeader::new(RichText::new("All AiM fields").size(13.0).color(SECONDARY))
                        .id_salt(("aim_fields", i))
                        .show(ui, |ui| {
                            egui::Grid::new(("aim_fields_grid", i)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                                let mut row = |k: &str, v: String| {
                                    ui.label(RichText::new(k).size(12.5).color(DIM));
                                    ui.label(RichText::new(v).size(12.5).color(TEXT));
                                    ui.end_row();
                                };
                                row("AiM id", t.id.clone());
                                row("Track guid", t.guid.clone());
                                if let Some(l) = t.length_m {
                                    row("Length", format!("{l:.1} m ({:.3} mi)", l / 1609.344));
                                }
                                row("Type", t.track_kind.label());
                                if let Some(f) = file {
                                    row("Outline", format!("{} points", f.outline.len()));
                                }
                                for (k, v) in info.map(|i| i.fields.clone()).unwrap_or_default() {
                                    row(&k, v);
                                }
                            });
                        });
                });
                if let Some(png) = file.and_then(|f| f.logo_png.clone()) {
                    ui.add(
                        egui::Image::from_bytes(format!("bytes://track-logo-{}.png", t.id), png)
                            .max_size(vec2(logo_w, 110.0))
                            .corner_radius(CornerRadius::same(6)),
                    );
                }
            });
        });
        ui.add_space(10.0);

        // Map: OpenStreetMap with the track drawn on top.
        let file_outline = self.track_file.as_ref().filter(|(j, _)| *j == i).map(|(_, f)| f.outline.clone()).unwrap_or_default();
        let outline: Vec<aim_track::LatLon> = if !t.outline.is_empty() { t.outline.clone() } else { file_outline };
        let lap = self.speed_laps.get(&t.guid).and_then(Option::as_ref);

        // Speed / splits / outline switch, when there's a lap to show.
        if self.split_count < 2 {
            self.split_count = 3;
        }
        // Elevation comes with the track file's outline (every AiM track has it).
        let elevation: Vec<f32> = self
            .track_file
            .as_ref()
            .filter(|(j, f)| *j == i && f.elevation_m.len() == outline.len())
            .map(|(_, f)| f.elevation_m.clone())
            .unwrap_or_default();
        let mut modes = Vec::new();
        if lap.is_some() {
            modes.extend([(MapMode::Speed, "Your speed"), (MapMode::Splits, "Splits")]);
        }
        if !elevation.is_empty() {
            modes.push((MapMode::Elevation, "Elevation"));
        }
        modes.push((MapMode::Outline, "Outline"));
        if !modes.iter().any(|(m, _)| *m == self.track_map) {
            // e.g. Your speed on a track you haven't raced: fall back to what this track has.
            self.track_map = modes[0].0;
        }
        if modes.len() > 1 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (mode, label) in &modes {
                    if chip(ui, label, self.track_map == *mode).clicked() {
                        self.track_map = *mode;
                    }
                }
                if self.track_map == MapMode::Splits {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for n in (2..=6).rev() {
                            if chip(ui, &n.to_string(), self.split_count == n).clicked() {
                                self.split_count = n;
                            }
                        }
                        ui.label(RichText::new("Sectors").size(12.5).color(DIM));
                    });
                }
            });
            ui.add_space(8.0);
        }
        let mode = self.track_map;
        let show_speed = mode == MapMode::Speed;
        // Splits are placed along the track's own outline when it has one, else along your lap.
        let sectors = (mode == MapMode::Splits)
            .then(|| {
                let reference = if !outline.is_empty() { outline.clone() } else { lap.map(|l| l.points.clone()).unwrap_or_default() };
                splits::Sectors::even(&reference, t.start_finish, self.split_count)
            })
            .flatten();
        let overlay = TrackOverlay {
            outline: if mode == MapMode::Outline {
                if !outline.is_empty() { outline.clone() } else { lap.map(|l| l.points.clone()).unwrap_or_default() }
            } else {
                Vec::new()
            },
            outline_color: if t.kind == Kind::Custom { SERIES[1] } else { SERIES[0] },
            speed: show_speed.then(|| lap.cloned()).flatten(),
            sectors: sectors.as_ref().map(|s| (s.path.clone(), s.boundaries.clone())),
            elevation: (mode == MapMode::Elevation).then(|| (outline.clone(), elevation.clone())),
            start_finish: t.start_finish,
        };

        let options = || walkers::HttpOptions {
            cache: std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/mychronix/tiles")),
            user_agent: Some(walkers::HeaderValue::from_static("Mychronix/0.1 (AiM MyChron data analysis; Linux desktop)")),
            ..Default::default()
        };
        if self.map_tiles.is_none() {
            self.map_tiles = Some(walkers::HttpTiles::with_options(walkers::sources::OpenStreetMap, options(), ui.ctx().clone()));
        }
        if self.map_satellite && self.sat_tiles.is_none() {
            self.sat_tiles = Some(walkers::HttpTiles::with_options(EsriImagery, options(), ui.ctx().clone()));
        }
        let center = t.start_finish.map(|p| walkers::lat_lon(p.lat, p.lon)).unwrap_or(walkers::lat_lon(0.0, 0.0));
        // Zoom to fit the drawn track once per selection.
        if std::mem::take(&mut self.fit_map) {
            let drawn: Vec<aim_track::LatLon> = match (&overlay.speed, &overlay.sectors, &overlay.elevation) {
                (Some(l), _, _) => l.points.clone(),
                (None, Some((path, _)), _) => path.clone(),
                (None, None, Some((path, _))) => path.clone(),
                _ => overlay.outline.clone(),
            };
            if let Some((c, zoom)) = fit_view(&drawn, vec2(ui.available_width(), map_h)) {
                self.map_memory.center_at(walkers::lat_lon(c.lat, c.lon));
                let _ = self.map_memory.set_zoom(zoom);
            }
        }
        let mut toggle = false;
        Frame::new().corner_radius(CornerRadius::same(theme::RADIUS)).fill(SURFACE).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(map_h);
            let tiles: &mut walkers::HttpTiles = if self.map_satellite { self.sat_tiles.as_mut().unwrap() } else { self.map_tiles.as_mut().unwrap() };
            let map = ui.add(walkers::Map::new(Some(tiles), &mut self.map_memory, center).with_plugin(overlay));
            // Map / Satellite switch in the map's top-right corner.
            let corner = Rect::from_min_size(map.rect.right_top() + vec2(-190.0, 10.0), vec2(180.0, 36.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(corner).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if chip(ui, "Satellite", self.map_satellite).clicked() && !self.map_satellite {
                    toggle = true;
                }
                if chip(ui, "Map", !self.map_satellite).clicked() && self.map_satellite {
                    toggle = true;
                }
            });
        });
        if toggle {
            self.map_satellite = !self.map_satellite;
        }
        let credit = if self.map_satellite { "Imagery © Esri, Maxar, Earthstar Geographics." } else { "Map data © OpenStreetMap contributors." };

        ui.add_space(6.0);
        match (mode, lap) {
            (MapMode::Speed, Some(lap)) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:.0}", lap.min_kmh)).size(12.0).color(DIM));
                    let (bar, _) = ui.allocate_exact_size(vec2(160.0, 8.0), Sense::hover());
                    for k in 0..40 {
                        let r = Rect::from_min_size(pos2(bar.left() + bar.width() * k as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                        ui.painter().rect_filled(r, 0.0, speed_color(k as f32 / 39.0));
                    }
                    ui.label(RichText::new(format!("{:.0} km/h", lap.max_kmh)).size(12.0).color(DIM));
                    ui.add_space(14.0);
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 6.0, Color32::from_black_alpha(170));
                    ui.painter().circle_filled(dot.center(), 4.0, TEXT);
                    ui.label(RichText::new("Braking point (peak deceleration)").size(12.0).color(DIM));
                });
                ui.label(RichText::new(format!("{}  {credit}", lap.source)).size(11.5).color(DIM));
            }
            (MapMode::Splits, Some(lap)) => {
                if let Some(sec) = &sectors {
                    ui.horizontal(|ui| {
                        for k in 0..sec.count() {
                            let (sw, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
                            ui.painter().line_segment([sw.left_center(), sw.right_center()], Stroke::new(4.0, SERIES[k % SERIES.len()]));
                            ui.label(RichText::new(format!("S{}", k + 1)).size(12.5).color(SECONDARY));
                            ui.add_space(6.0);
                        }
                    });
                }
                ui.label(
                    RichText::new(format!("Splits placed by Mychronix at equal distances from the start/finish, timed from your GPS.  {credit}"))
                        .size(11.5)
                        .color(DIM),
                );
                ui.add_space(10.0);
                if let Some(sec) = &sectors {
                    split_table(ui, lap, sec);
                }
            }
            (MapMode::Elevation, _) => {
                let (lo, hi) = elevation.iter().fold((f32::MAX, f32::MIN), |(a, b), &e| (a.min(e), b.max(e)));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{lo:.0}")).size(12.0).color(DIM));
                    let (bar, _) = ui.allocate_exact_size(vec2(160.0, 8.0), Sense::hover());
                    for k in 0..40 {
                        let r = Rect::from_min_size(pos2(bar.left() + bar.width() * k as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                        ui.painter().rect_filled(r, 0.0, speed_color(k as f32 / 39.0));
                    }
                    ui.label(RichText::new(format!("{hi:.0} m above sea level  ·  {:.0} m between lowest and highest point", hi - lo)).size(12.0).color(DIM));
                });
                ui.label(RichText::new(format!("Elevation from AiM's track file, as in RaceStudio's track preview.  {credit}")).size(11.5).color(DIM));
            }
            _ => {
                let note = if outline.is_empty() {
                    "AiM tracks give the start/finish only; record a session here to see the shape."
                } else if t.kind == Kind::Custom {
                    "Outline from the custom track file."
                } else {
                    "Outline from AiM's track file."
                };
                ui.label(RichText::new(format!("{note}  {credit}")).size(11.5).color(DIM));
            }
        }
        ui.add_space(10.0);

        // Sessions recorded here.
        let mut open = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Your sessions here").font(semibold(16.0)).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(best) = t.best_lap_ms(lib) {
                        ui.label(RichText::new(format_lap_time(best as i64)).font(semibold(16.0)).color(theme::GOOD));
                    }
                });
            });
            ui.add_space(6.0);
            if t.sessions.is_empty() {
                ui.label(RichText::new("None yet. Sessions you download from your MyChron show up here.").color(SECONDARY));
                return;
            }
            let track_best = t.best_lap_ms(lib);
            egui::ScrollArea::vertical().id_salt("track_sessions").max_height(420.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for &si in &t.sessions {
                    let s = &lib.sessions[si];
                    let sum = &s.summary;
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
                    if ui.is_rect_visible(rect) {
                        let p = ui.painter();
                        if response.hovered() {
                            p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE_HOVER);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let (l, r) = (rect.left() + 10.0, rect.right() - 10.0);
                        let (top, bottom) = (rect.top() + 15.0, rect.top() + 33.0);
                        p.text(pos2(l, top), egui::Align2::LEFT_CENTER, format!("Session {}", sum.number.unwrap_or(0)), medium(14.0), TEXT);
                        let when = [sum.date.map(|d| d.to_string()), sum.start_time.clone()].into_iter().flatten().collect::<Vec<_>>().join("  ");
                        p.text(pos2(l, bottom), egui::Align2::LEFT_CENTER, when, regular(12.5), DIM);
                        let is_best = sum.best_lap_ms.is_some() && sum.best_lap_ms == track_best;
                        let best = sum.best_lap_ms.map(|b| format_lap_time(b as i64)).unwrap_or_else(|| "—".into());
                        p.text(pos2(r, top), egui::Align2::RIGHT_CENTER, best, medium(14.0), if is_best { theme::GOOD } else { TEXT });
                        let laps = sum.laps.unwrap_or(0);
                        p.text(pos2(r, bottom), egui::Align2::RIGHT_CENTER, format!("{laps} lap{}", if laps == 1 { "" } else { "s" }), regular(12.5), DIM);
                    }
                    if response.clicked() {
                        open = Some(s.path.clone());
                    }
                }
            });
        });
        if let Some(path) = open {
            self.load(path);
        }
    }
}

fn country_label(code: &str) -> String {
    match crate::countries::name(code) {
        Some(name) => name.to_string(),
        None if code.is_empty() => "Unknown country".to_string(),
        None => code.to_string(),
    }
}

/// One line of text cut to `width` with an ellipsis.
fn single_line(text: &str, font: egui::FontId, color: Color32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job
}

/// A small drawing of a track's outline, fitted into `rect`.
fn outline_thumbnail(p: &egui::Painter, rect: Rect, outline: &[aim_track::LatLon], color: Color32) {
    p.rect_filled(rect, CornerRadius::same(6), WINDOW);
    if outline.len() < 2 {
        return;
    }
    let lat0 = outline[0].lat;
    let kx = lat0.to_radians().cos();
    let xy: Vec<(f64, f64)> = outline.iter().map(|q| (q.lon * kx, q.lat)).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in &xy {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let area = rect.shrink(6.0);
    let scale = (area.width() as f64 / (x1 - x0).max(1e-9)).min(area.height() as f64 / (y1 - y0).max(1e-9));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let pts: Vec<_> = xy
        .iter()
        .map(|&(x, y)| pos2(area.center().x + ((x - cx) * scale) as f32, area.center().y - ((y - cy) * scale) as f32))
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
}

/// Your fastest lap at a track: where you drove, how fast, and where you braked hardest.
#[derive(Clone)]
struct SpeedLap {
    points: Vec<aim_track::LatLon>,
    kmh: Vec<f64>,
    min_kmh: f64,
    max_kmh: f64,
    brakes: Vec<Brake>,
    /// Which lap this is, for the caption.
    source: String,
    /// The whole session it came from, for timing splits on every lap.
    session_number: u32,
    session_laps: Vec<aim_xrk::Lap>,
    session_times: Vec<i64>,
    session_points: Vec<aim_track::LatLon>,
}

#[derive(Clone, Copy)]
struct Brake {
    /// Index into `points` where braking starts.
    at: usize,
    /// Peak deceleration in g.
    peak_g: f64,
}

/// The best lap of your best session at `t`; if the logger timed no laps
/// there, one loop cut from your longest session.
fn speed_lap(lib: &Library, t: &tracks::Track) -> Option<SpeedLap> {
    let timed = t
        .sessions
        .iter()
        .filter(|&&i| lib.sessions[i].summary.best_lap_ms.is_some())
        .min_by_key(|&&i| lib.sessions[i].summary.best_lap_ms);
    let longest = t.sessions.iter().max_by_key(|&&i| lib.sessions[i].summary.duration_ms.unwrap_or(0));
    let pick = *timed.or(longest)?;
    let session = Session::open(&lib.sessions[pick].path).ok()?;
    let gps = session.gps.as_ref()?;
    let all: Vec<(i64, aim_track::LatLon, f64)> = (0..gps.len())
        .map(|i| (gps.times_ms[i], aim_track::LatLon { lat: gps.latitude[i], lon: gps.longitude[i] }, gps.speed_kmh[i]))
        .collect();
    let number = lib.sessions[pick].summary.number.unwrap_or(0);
    let (range, source) = match session.best_lap(MIN_LAP_MS) {
        Some(lap) => {
            let lo = all.partition_point(|p| p.0 < lap.start_ms);
            let hi = all.partition_point(|p| p.0 <= lap.end_ms);
            (lo..hi, format!("Your best lap here: session {number}, lap {} ({}).", lap.number, format_lap_time(lap.duration_ms())))
        }
        None => (one_loop(&all), format!("One lap from session {number} (no timed laps here).")),
    };
    let lap = &all[range];
    if lap.len() < 10 {
        return None;
    }
    let times: Vec<i64> = lap.iter().map(|p| p.0).collect();
    let kmh = smooth(&lap.iter().map(|p| p.2).collect::<Vec<_>>(), 3);
    let (min_kmh, max_kmh) = robust_range(kmh.iter().copied());
    Some(SpeedLap {
        points: lap.iter().map(|p| p.1).collect(),
        brakes: braking_points(&times, &kmh),
        kmh,
        min_kmh,
        max_kmh,
        source,
        session_number: number,
        session_laps: session.complete_laps().to_vec(),
        session_times: all.iter().map(|p| p.0).collect(),
        session_points: all.iter().map(|p| p.1).collect(),
    })
}

/// Centred moving average over `±half` samples.
fn smooth(v: &[f64], half: usize) -> Vec<f64> {
    (0..v.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(v.len()));
            v[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect()
}

/// Where the hardest braking starts: runs of deceleration above 0.3 g, kept
/// when their peak is at least half the lap's hardest, strongest first (max 8).
fn braking_points(times_ms: &[i64], kmh: &[f64]) -> Vec<Brake> {
    let k = 5; // about 0.2 s either side at the logger's 25 Hz GPS
    let n = kmh.len();
    if n < 2 * k + 2 {
        return Vec::new();
    }
    let decel: Vec<f64> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(k), (i + k).min(n - 1));
            let dt = (times_ms[b] - times_ms[a]) as f64 / 1000.0;
            if dt <= 0.0 { 0.0 } else { (kmh[a] - kmh[b]) / 3.6 / dt / 9.81 }
        })
        .collect();
    let mut runs: Vec<Brake> = Vec::new();
    let mut i = 0;
    while i < n {
        if decel[i] > 0.3 {
            let start = i;
            let mut peak = decel[i];
            while i < n && decel[i] > 0.3 {
                peak = peak.max(decel[i]);
                i += 1;
            }
            runs.push(Brake { at: start, peak_g: peak });
        }
        i += 1;
    }
    let hardest = runs.iter().map(|b| b.peak_g).fold(0.0, f64::max);
    runs.retain(|b| b.peak_g >= hardest * 0.5);
    runs.sort_by(|a, b| b.peak_g.total_cmp(&a.peak_g));
    runs.truncate(8);
    runs
}

/// Sector times for every complete lap of the session, best per sector in
/// green, with the ideal lap (best sectors added up).
fn split_table(ui: &mut Ui, lap: &SpeedLap, sectors: &splits::Sectors) {
    let points = sectors.split_points();
    let rows: Vec<(i32, i64, Vec<i64>)> = lap
        .session_laps
        .iter()
        .filter_map(|l| {
            splits::sector_times(&lap.session_times, &lap.session_points, l.start_ms, l.end_ms, &points)
                .map(|t| (l.number, l.duration_ms(), t))
        })
        .collect();
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Sector times  ·  session {}", lap.session_number)).font(semibold(16.0)).color(TEXT));
        });
        ui.add_space(6.0);
        if rows.is_empty() {
            ui.label(RichText::new("No complete laps in this session pass every split.").color(SECONDARY));
            return;
        }
        let n = sectors.count();
        let best: Vec<i64> = (0..n).map(|k| rows.iter().map(|r| r.2[k]).min().unwrap_or(0)).collect();
        let best_lap = rows.iter().map(|r| r.1).min().unwrap_or(0);
        let ideal: i64 = best.iter().sum();
        let secs = |ms: i64| format!("{:.3}", ms as f64 / 1000.0);
        egui::Grid::new("split_table").num_columns(n + 2).spacing([28.0, 6.0]).striped(false).show(ui, |ui| {
            ui.label(RichText::new("Lap").size(12.5).color(DIM));
            for k in 0..n {
                ui.horizontal(|ui| {
                    let (sw, _) = ui.allocate_exact_size(vec2(12.0, 10.0), Sense::hover());
                    ui.painter().line_segment([sw.left_center(), sw.right_center()], Stroke::new(3.0, SERIES[k % SERIES.len()]));
                    ui.label(RichText::new(format!("S{}", k + 1)).size(12.5).color(DIM));
                });
            }
            ui.label(RichText::new("Lap time").size(12.5).color(DIM));
            ui.end_row();
            for (number, total, times) in &rows {
                ui.label(RichText::new(number.to_string()).color(SECONDARY));
                for (k, t) in times.iter().enumerate() {
                    let color = if *t == best[k] { theme::GOOD } else { TEXT };
                    ui.label(RichText::new(secs(*t)).font(medium(14.0)).color(color));
                }
                let color = if *total == best_lap { theme::GOOD } else { TEXT };
                ui.label(RichText::new(format_lap_time(*total)).font(medium(14.0)).color(color));
                ui.end_row();
            }
            ui.label(RichText::new("Ideal").font(semibold(14.0)).color(TEXT));
            for t in &best {
                ui.label(RichText::new(secs(*t)).font(semibold(14.0)).color(theme::GOOD));
            }
            ui.label(RichText::new(format_lap_time(ideal)).font(semibold(14.0)).color(theme::GOOD));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("Ideal lap is {} faster than your best ({}).", secs(best_lap - ideal), format_lap_time(best_lap)))
                .size(12.5)
                .color(SECONDARY),
        );
    });
}

/// Esri World Imagery: satellite and aerial photos. Free for personal,
/// non-commercial use with attribution.
struct EsriImagery;

impl walkers::sources::TileSource for EsriImagery {
    fn tile_url(&self, id: walkers::TileId) -> String {
        format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}", id.zoom, id.y, id.x)
    }

    fn attribution(&self) -> walkers::sources::Attribution {
        walkers::sources::Attribution {
            text: "Esri, Maxar, Earthstar Geographics",
            url: "https://www.esri.com/",
            logo_light: None,
            logo_dark: None,
        }
    }
}

/// Centre and web-map zoom that fit `points` into a `size` pixel view with a margin.
fn fit_view(points: &[aim_track::LatLon], size: Vec2) -> Option<(aim_track::LatLon, f64)> {
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
    let width_m = aim_track::LatLon { lat: c.lat, lon: lon0 }.distance_m(&aim_track::LatLon { lat: c.lat, lon: lon1 }).max(20.0);
    let height_m = aim_track::LatLon { lat: lat0, lon: c.lon }.distance_m(&aim_track::LatLon { lat: lat1, lon: c.lon }).max(20.0);
    // Metres per pixel at zoom z (256-px tiles): 156543.03 × cos(lat) / 2^z.
    let mpp = (width_m / (size.x as f64 * 0.75)).max(height_m / (size.y as f64 * 0.75));
    let zoom = (156_543.03 * c.lat.to_radians().cos() / mpp).log2().clamp(3.0, 19.5);
    Some((c, zoom))
}

/// Speed colour: viridis, dark purple when slow to yellow when fast. Its
/// lightness rises steadily, so slow and fast stay apart for colourblind
/// viewers too.
fn speed_color(t: f32) -> Color32 {
    const STOPS: [Color32; 5] = [
        Color32::from_rgb(0x44, 0x01, 0x54),
        Color32::from_rgb(0x3b, 0x52, 0x8b),
        Color32::from_rgb(0x21, 0x91, 0x8c),
        Color32::from_rgb(0x5e, 0xc9, 0x62),
        Color32::from_rgb(0xfd, 0xe7, 0x25),
    ];
    let x = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let i = (x.floor() as usize).min(STOPS.len() - 2);
    mix(STOPS[i], STOPS[i + 1], x - i as f32)
}

/// The first closed loop in a GPS trace: from the first moment the kart is
/// moving to where it first comes back within 15 m after covering 250 m.
fn one_loop(points: &[(i64, aim_track::LatLon, f64)]) -> std::ops::Range<usize> {
    let Some(start) = points.iter().position(|(_, _, kmh)| *kmh > 20.0) else {
        return 0..points.len();
    };
    let origin = points[start].1;
    let mut travelled = 0.0;
    for j in start + 1..points.len() {
        travelled += points[j - 1].1.distance_m(&points[j].1);
        if travelled > 250.0 && points[j].1.distance_m(&origin) < 15.0 {
            return start..j + 1;
        }
    }
    start..points.len()
}

/// Draws a track on the map: your speed (with braking points) or its outline, and the start/finish.
struct TrackOverlay {
    outline: Vec<aim_track::LatLon>,
    outline_color: Color32,
    speed: Option<SpeedLap>,
    /// Splits view: the reference path and where each sector after the first begins.
    sectors: Option<(Vec<aim_track::LatLon>, Vec<usize>)>,
    /// Elevation view: the outline and each point's height in metres.
    elevation: Option<(Vec<aim_track::LatLon>, Vec<f32>)>,
    start_finish: Option<aim_track::LatLon>,
}

impl walkers::Plugin for TrackOverlay {
    fn run(self: Box<Self>, ui: &mut Ui, response: &egui::Response, projector: &walkers::Projector, _memory: &walkers::MapMemory) {
        // Draw only inside the map's visible area (it may be partly scrolled out of view).
        ui.set_clip_rect(response.rect.intersect(ui.clip_rect()));
        let painter = ui.painter();
        let to_screen = |p: &aim_track::LatLon| projector.project(walkers::lat_lon(p.lat, p.lon)).to_pos2();
        let casing = Stroke::new(8.0, Color32::from_black_alpha(120));
        if let Some(lap) = &self.speed {
            let pts: Vec<_> = lap.points.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing));
            let span = (lap.max_kmh - lap.min_kmh).max(1.0);
            for k in 0..pts.len() - 1 {
                let t = ((lap.kmh[k] - lap.min_kmh) / span) as f32;
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(5.0, speed_color(t)));
            }
            for b in &lap.brakes {
                let c = pts[b.at];
                painter.circle_filled(c, 8.0, Color32::from_black_alpha(170));
                painter.circle_filled(c, 5.5, TEXT);
                let label = format!("{:.1} g", b.peak_g);
                let galley = painter.layout_no_wrap(label, semibold(11.5), TEXT);
                let r = Rect::from_min_size(c + vec2(10.0, -galley.size().y / 2.0 - 3.0), galley.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(190));
                painter.galley(r.min + vec2(5.0, 3.0), galley, TEXT);
            }
        } else if let Some((path, heights)) = &self.elevation {
            let pts: Vec<_> = path.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing));
            let (lo, hi) = heights.iter().fold((f32::MAX, f32::MIN), |(a, b), &e| (a.min(e), b.max(e)));
            let span = (hi - lo).max(0.5);
            for k in 0..pts.len().saturating_sub(1) {
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(5.0, speed_color((heights[k] - lo) / span)));
            }
        } else if let Some((path, boundaries)) = &self.sectors {
            let pts: Vec<_> = path.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing));
            let mut sector = 0;
            for k in 0..pts.len() - 1 {
                while sector < boundaries.len() && k >= boundaries[sector] {
                    sector += 1;
                }
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(5.0, SERIES[sector % SERIES.len()]));
            }
            for (n, &b) in boundaries.iter().enumerate() {
                let c = pts[b];
                painter.circle_filled(c, 8.0, Color32::from_black_alpha(170));
                painter.circle_filled(c, 5.5, TEXT);
                let galley = painter.layout_no_wrap(format!("S{}", n + 2), semibold(11.5), TEXT);
                let r = Rect::from_min_size(c + vec2(10.0, -galley.size().y / 2.0 - 3.0), galley.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(190));
                painter.galley(r.min + vec2(5.0, 3.0), galley, TEXT);
            }
        } else if self.outline.len() > 1 {
            let pts: Vec<_> = self.outline.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(7.0, Color32::from_black_alpha(110))));
            painter.add(egui::Shape::line(pts, Stroke::new(4.0, self.outline_color)));
        }
        if let Some(sf) = &self.start_finish {
            let c = to_screen(sf);
            painter.circle_filled(c, 9.0, Color32::from_black_alpha(140));
            painter.circle_filled(c, 7.0, TEXT);
            Icon::Flag.image(ON_TEXT, 10.0).paint_at(ui, Rect::from_center_size(c, Vec2::splat(10.0)));
        }
    }
}

impl App {
    fn device_page(&mut self, ui: &mut Ui) {
        use crate::device::{Command, Event};
        let ctx = ui.ctx().clone();
        let worker = self.dev.get_or_insert_with(|| crate::device::Worker::spawn(ctx.clone()));

        // Apply whatever the worker has finished.
        let mut events = Vec::new();
        while let Ok(e) = worker.rx.try_recv() {
            events.push(e);
        }
        for e in events {
            match e {
                Event::Scanned { logger, networks, current } => {
                    let newly = logger.is_some() && self.dev_logger.is_none();
                    self.dev_logger = logger;
                    self.dev_networks = networks;
                    self.dev_current_wifi = current;
                    if self.dev_busy.as_deref() == Some("Looking for your MyChron…") {
                        self.dev_busy = None;
                    }
                    if newly {
                        self.dev_busy = Some("Reading the session list…".into());
                        let _ = self.dev.as_ref().unwrap().tx.send(Command::List);
                    }
                }
                Event::Joined { ssid } => {
                    self.dev_joined = Some(ssid);
                    self.dev_busy = Some("Looking for your MyChron…".into());
                    let _ = self.dev.as_ref().unwrap().tx.send(Command::Scan);
                }
                Event::Left => {
                    self.dev_joined = None;
                    self.dev_logger = None;
                    self.dev_files.clear();
                    self.dev_summaries.clear();
                    self.dev_busy = None;
                }
                Event::Listed { files, summaries, new_tracks } => {
                    self.dev_files = files;
                    self.dev_summaries = summaries;
                    self.dev_busy = None;
                    if new_tracks > 0 {
                        self.library = None; // pick up the new custom tracks
                        self.dev_message = Some((true, format!("Copied {new_tracks} custom track{} from the MyChron into your track library.", if new_tracks == 1 { "" } else { "s" })));
                    }
                    self.dev_selected = self.new_on_logger().into_iter().map(|e| e.name).collect();
                }
                Event::Progress { index, count, file, done, total } => {
                    self.dev_progress = Some((index, count, file, done, total));
                }
                Event::Downloaded { saved } => {
                    self.dev_busy = None;
                    self.dev_progress = None;
                    self.dev_message = Some((true, format!("Downloaded {saved} session{}. They're on the Sessions page.", if saved == 1 { "" } else { "s" })));
                    self.library = None; // reload sessions and tracks
                    self.dev_selected.clear();
                }
                Event::Error(msg) => {
                    self.dev_busy = None;
                    self.dev_progress = None;
                    self.dev_message = Some((false, msg));
                }
            }
        }

        // MYCHRONIX_FAKE_DEVICE=<logger id> shows that logger's downloaded sessions as if it were
        // connected, for checking this page's layout without the MyChron (development only).
        if self.dev_logger.is_none() {
            if let Some(id) = std::env::var("MYCHRONIX_FAKE_DEVICE").ok().and_then(|v| v.parse::<u32>().ok()) {
                let mut raw = vec![0u8; 236];
                raw[0] = 0xec;
                raw[84..87].copy_from_slice(b"idn");
                raw[96..100].copy_from_slice(&id.to_le_bytes());
                self.dev_logger = aim_link::Beacon::parse(AP_ADDRESS_FAKE, std::time::Duration::from_millis(3), raw).ok();
                self.dev_current_wifi = Some(format!("AiM-MYC6-{:06}", id % 1_000_000));
                if let Some(dir) = self.logger_dir() {
                    let mut files = Vec::new();
                    let mut summaries = Vec::new();
                    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                        let p = e.path();
                        let name = p.file_name().unwrap().to_string_lossy().into_owned();
                        if name.ends_with(".xrz") {
                            files.push(aim_link::DirEntry { name: name.clone(), size: e.metadata().map(|m| m.len()).unwrap_or(0), date: String::new(), time: String::new() });
                            if let Some(mut sum) = std::fs::read(p.with_extension("xri")).ok().and_then(|b| aim_xrk::parse_xri(&b).ok()) {
                                sum.file = Some(name);
                                summaries.push(sum);
                            }
                        }
                    }
                    // Pretend the newest two aren't downloaded yet.
                    files.sort_by(|a, b| a.name.cmp(&b.name));
                    if let Some(last) = files.last_mut() {
                        last.size += 1;
                    }
                    self.dev_files = files;
                    self.dev_summaries = summaries;
                    self.dev_selected = self.new_on_logger().into_iter().map(|e| e.name).collect();
                }
                self.dev_last_scan = f64::MAX;
            }
        }

        // Keep looking while nothing is connected; check now and then that a logger is still there.
        let now = ctx.input(|i| i.time);
        let every = if self.dev_logger.is_some() { 15.0 } else { 4.0 };
        if self.dev_busy.is_none() && self.dev_last_scan != f64::MAX && now - self.dev_last_scan > every {
            self.dev_last_scan = now;
            let _ = self.dev.as_ref().unwrap().tx.send(Command::Scan);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        ui.label(RichText::new("Device").font(semibold(28.0)).color(TEXT));
        ui.label(RichText::new("Your MyChron, its sessions and downloads.").font(regular(14.5)).color(SECONDARY));
        ui.add_space(18.0);

        if let Some((ok, msg)) = self.dev_message.clone() {
            let color = if ok { theme::GOOD } else { ui.visuals().error_fg_color };
            ui.horizontal(|ui| {
                ui.label(RichText::new(msg).color(color));
                if ui.add(Icon::X.image(DIM, 14.0).sense(Sense::click())).clicked() {
                    self.dev_message = None;
                }
            });
            ui.add_space(8.0);
        }

        if std::env::var("MYCHRONIX_FAKE_DIALOG").is_ok() && self.dev_dialog.is_none() && self.frames < 3 && self.dev_logger.is_some() {
            let multi = std::env::var("MYCHRONIX_FAKE_DIALOG").as_deref() == Ok("multi");
            let id = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
            let p = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).loggers.get(&id).cloned().unwrap_or_default();
            self.dev_dialog = Some(DownloadDialog {
                meta: crate::device::SessionMeta {
                    driver: p.driver,
                    kart: p.kart,
                    championship: p.championship,
                    track: if multi { String::new() } else { "mka".into() },
                    ..Default::default()
                },
                detected: if multi { vec!["NKA".into(), "mka".into()] } else { vec!["mka".into()] },
            });
        }
        self.device_status(ui);
        if self.dev_logger.is_some() {
            ui.add_space(14.0);
            self.device_sessions(ui);
        }
        self.download_dialog(&ctx);
    }

    fn logger_dir(&self) -> Option<PathBuf> {
        let id = self.dev_logger.as_ref()?.logger_id?;
        Some(PathBuf::from(std::env::var_os("HOME")?).join("Mychronix").join("sessions").join(id.to_string()))
    }

    /// Sessions on the logger not yet saved here (same name and size).
    fn new_on_logger(&self) -> Vec<aim_link::DirEntry> {
        let dir = self.logger_dir();
        self.dev_files
            .iter()
            .filter(|e| e.is_session())
            .filter(|e| !dir.as_ref().is_some_and(|d| std::fs::metadata(d.join(&e.name)).is_ok_and(|m| m.len() == e.size)))
            .cloned()
            .collect()
    }

    fn device_status(&mut self, ui: &mut Ui) {
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        card(ui, |ui| {
            match &self.dev_logger {
                Some(b) => {
                    let id = b.logger_id.map(|i| i.to_string()).unwrap_or_default();
                    let model = self.dev_joined.as_deref().or(self.dev_current_wifi.as_deref()).map(model_from_ssid).unwrap_or("MyChron");
                    let who = profiles.get(&id).map(|p| p.driver.clone()).filter(|d| !d.is_empty());
                    ui.horizontal(|ui| {
                        ui.add(Icon::Gauge.image(theme::GOOD, 32.0));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(format!("{model} connected")).font(semibold(17.0)).color(TEXT));
                            let line = [Some(format!("Logger {id}")), who, self.dev_current_wifi.clone(), Some(format!("answered in {} ms", b.round_trip.as_millis()))]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join("  ·  ");
                            ui.label(RichText::new(line).color(SECONDARY));
                        });
                        if self.dev_joined.is_some() {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(ui, Some(Icon::Wifi), "Disconnect", false).clicked() && self.dev_busy.is_none() {
                                    self.dev_busy = Some("Reconnecting to your network…".into());
                                    let _ = self.dev.as_ref().unwrap().tx.send(crate::device::Command::Leave);
                                }
                            });
                        }
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add(Icon::Gauge.image(DIM, 32.0));
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("No logger connected").font(semibold(17.0)).color(TEXT));
                            let hint = match &self.dev_busy {
                                Some(b) => b.clone(),
                                None => "Turn on the MyChron and its WiFi. Mychronix keeps looking.".into(),
                            };
                            ui.label(RichText::new(hint).color(SECONDARY));
                        });
                    });
                    if !self.dev_networks.is_empty() {
                        ui.add_space(12.0);
                        hairline(ui);
                        ui.add_space(10.0);
                        let current = self.dev_current_wifi.clone().unwrap_or_else(|| "your current network".into());
                        ui.label(RichText::new(format!("MyChrons in range. Joining one takes this computer off {current} until you disconnect.")).size(12.5).color(DIM));
                        ui.add_space(6.0);
                        for (ssid, signal) in self.dev_networks.clone() {
                            ui.horizontal(|ui| {
                                ui.add(Icon::Wifi.image(SECONDARY, 18.0));
                                ui.label(RichText::new(model_from_ssid(&ssid)).font(medium(14.5)).color(TEXT));
                                ui.label(RichText::new(format!("{ssid}  ·  {signal}% signal")).color(DIM));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if button(ui, None, "Join", true).clicked() && self.dev_busy.is_none() {
                                        self.dev_busy = Some(format!("Joining {ssid}…"));
                                        self.dev_message = None;
                                        let _ = self.dev.as_ref().unwrap().tx.send(crate::device::Command::Join { ssid: ssid.clone() });
                                    }
                                });
                            });
                        }
                    }
                    ui.add_space(12.0);
                    hairline(ui);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add(Icon::Usb.image(DIM, 16.0));
                        ui.label(RichText::new("USB (MyChron 4) isn't supported yet.").size(12.5).color(DIM));
                    });
                }
            }
        });
    }

    fn device_sessions(&mut self, ui: &mut Ui) {
        let new: std::collections::BTreeSet<String> = self.new_on_logger().into_iter().map(|e| e.name).collect();
        let sessions: Vec<aim_link::DirEntry> = {
            let mut v: Vec<_> = self.dev_files.iter().filter(|e| e.is_session()).cloned().collect();
            v.sort_by(|a, b| b.name.cmp(&a.name));
            v
        };
        let summary_of = |name: &str| self.dev_summaries.iter().find(|s| s.file.as_deref() == Some(name)).cloned();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Sessions on the logger").font(semibold(16.0)).color(TEXT));
                ui.label(RichText::new(format!("{} total  ·  {} new", sessions.len(), new.len())).color(SECONDARY));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = self.dev_selected.len();
                    let label = if n == 0 { "Download".to_string() } else { format!("Download {n}") };
                    if button(ui, Some(Icon::FolderOpen), &label, true).clicked() && n > 0 && self.dev_busy.is_none() {
                        let id = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
                        let p = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).loggers.get(&id).cloned().unwrap_or_default();
                        // The track comes from the logger: one track pre-fills the field, several defer to each session.
                        let mut detected: Vec<String> = self.dev_selected.iter().filter_map(|n| summary_of(n)).filter_map(|s| s.track).filter(|t| !t.is_empty()).collect();
                        detected.sort();
                        detected.dedup();
                        let track = if detected.len() == 1 { detected[0].clone() } else { String::new() };
                        self.dev_dialog = Some(DownloadDialog {
                            meta: crate::device::SessionMeta {
                                driver: p.driver,
                                kart: p.kart,
                                championship: p.championship,
                                track,
                                session_type: String::new(),
                                comment: String::new(),
                            },
                            detected,
                        });
                    }
                    if chip(ui, "None", false).clicked() {
                        self.dev_selected.clear();
                    }
                    if chip(ui, "All new", false).clicked() {
                        self.dev_selected = new.clone();
                    }
                });
            });
            if let Some((index, count, file, done, total)) = &self.dev_progress {
                ui.add_space(8.0);
                let frac = (*index as f32 + *done as f32 / (*total).max(1) as f32) / (*count).max(1) as f32;
                ui.label(RichText::new(format!("Downloading {file}  ({} of {count})", index + 1)).color(SECONDARY));
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                ui.painter().rect_filled(bar, 3.0, SURFACE_ACTIVE);
                ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height())), 3.0, TEXT);
            } else if let Some(b) = &self.dev_busy {
                ui.add_space(6.0);
                ui.label(RichText::new(b).color(SECONDARY));
            }
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("logger_sessions").auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for e in &sessions {
                    let sum = summary_of(&e.name);
                    let is_new = new.contains(&e.name);
                    let mut on = self.dev_selected.contains(&e.name);
                    let response = row(ui, false, |ui| {
                        let (cb, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                        ui.painter().rect_stroke(cb, CornerRadius::same(4), Stroke::new(1.5, if on { TEXT } else { DIM }), egui::StrokeKind::Inside);
                        if on {
                            Icon::Check.image(TEXT, 14.0).paint_at(ui, cb.shrink(2.0));
                        }
                        ui.add_space(6.0);
                        let number = aim_xrk::summary::session_number(&e.name).map(|n| format!("Session {n}")).unwrap_or(e.name.clone());
                        ui.label(RichText::new(number).font(medium(14.0)).color(TEXT));
                        let when = sum.as_ref().map(|s| [s.date.map(|d| d.to_string()), s.start_time.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ")).unwrap_or(e.date.clone());
                        ui.label(RichText::new(when).color(DIM));
                        if let Some(t) = sum.as_ref().and_then(|s| s.track.clone()) {
                            ui.label(RichText::new(t).color(SECONDARY));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if is_new {
                                tag(ui, "New", TEXT);
                            } else {
                                ui.label(RichText::new("Downloaded").size(12.5).color(DIM));
                            }
                            ui.label(RichText::new(format!("{:.1} MB", e.size as f64 / 1e6)).size(12.5).color(DIM));
                            if let Some(s) = &sum {
                                let best = s.best_lap_ms.map(|b| format_lap_time(b as i64)).unwrap_or_else(|| "—".into());
                                ui.label(RichText::new(best).font(medium(14.0)).color(TEXT));
                                let laps = s.laps.unwrap_or(0);
                                ui.label(RichText::new(format!("{laps} lap{}", if laps == 1 { "" } else { "s" })).size(12.5).color(DIM));
                            }
                        });
                    });
                    if response.clicked() {
                        on = !on;
                        if on {
                            self.dev_selected.insert(e.name.clone());
                        } else {
                            self.dev_selected.remove(&e.name);
                        }
                    }
                }
            });
        });
    }

    /// RaceStudio-style dialog: who, what, which championship, session type and
    /// track, before downloading. Every field but the comment suggests values
    /// already in use.
    fn download_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.dev_dialog.take() else { return };
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let s = Suggestions::gather(&lib, &profiles);
        let multiple = dlg.detected.len() > 1;
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        let n = self.dev_selected.len();
        let field_w = 400.0;
        egui::Window::new(RichText::new(format!("Download {n} session{}", if n == 1 { "" } else { "s" })).font(semibold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.add_space(6.0);
                egui::Grid::new("download_meta").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    let label = |ui: &mut Ui, t: &str| ui.label(RichText::new(t).font(medium(15.0)).color(SECONDARY));
                    label(ui, "Driver");
                    suggest_field(ui, "dl_driver", &mut dlg.meta.driver, "Driver name", &s.drivers, field_w);
                    ui.end_row();
                    label(ui, "Kart");
                    suggest_field(ui, "dl_kart", &mut dlg.meta.kart, "Kart or class", &s.karts, field_w);
                    ui.end_row();
                    label(ui, "Championship");
                    suggest_field(ui, "dl_champ", &mut dlg.meta.championship, "e.g. 2026 Club Championship", &s.championships, field_w);
                    ui.end_row();
                    label(ui, "Session type");
                    suggest_field(ui, "dl_type", &mut dlg.meta.session_type, "Practice 1, Heat 2, Final…", &s.session_types, field_w);
                    ui.end_row();
                    label(ui, "Track");
                    if multiple {
                        ui.vertical(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut format!("Multiple tracks detected: {}", dlg.detected.join(", ")))
                                    .font(regular(15.0))
                                    .margin(Margin::symmetric(12, 9))
                                    .desired_width(field_w)
                                    .interactive(false),
                            );
                            ui.label(RichText::new("Each session keeps the track the MyChron recorded for it.").size(12.5).color(DIM));
                        });
                    } else {
                        ui.vertical(|ui| {
                            suggest_field(ui, "dl_track", &mut dlg.meta.track, "Track", &s.tracks, field_w);
                            let from_logger = dlg.detected.first().cloned().unwrap_or_default();
                            let note = if !from_logger.is_empty() && dlg.meta.track == from_logger {
                                "From the MyChron.".to_string()
                            } else if !from_logger.is_empty() {
                                format!("The MyChron recorded {from_logger}; these sessions will be filed under {} instead.", dlg.meta.track)
                            } else {
                                "The MyChron didn't recognise a track for these sessions.".into()
                            };
                            ui.label(RichText::new(note).size(12.5).color(DIM));
                        });
                    }
                    ui.end_row();
                    label(ui, "Comment");
                    ui.add(egui::TextEdit::multiline(&mut dlg.meta.comment).hint_text("Optional: conditions, setup, how it went").font(regular(15.0)).margin(Margin::symmetric(12, 9)).desired_rows(2).desired_width(field_w));
                    ui.end_row();
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if button(ui, Some(Icon::FolderOpen), "Download", true).clicked() {
                        go = true;
                    }
                    if button(ui, None, "Cancel", false).clicked() {
                        cancel = true;
                    }
                });
            });
        self.library = Some(lib);
        if go {
            let id = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
            // Remember these for next time (not the comment or session type, which change every time).
            let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
            let entry = profiles.loggers.entry(id).or_default();
            entry.driver = dlg.meta.driver.clone();
            entry.kart = dlg.meta.kart.clone();
            entry.championship = dlg.meta.championship.clone();
            if !multiple {
                entry.track = dlg.meta.track.clone();
            }
            let _ = profiles.save();
            // Each file gets the dialog's details; with several tracks, its own track from the logger.
            let files: Vec<(aim_link::DirEntry, crate::device::SessionMeta)> = self
                .dev_files
                .iter()
                .filter(|e| self.dev_selected.contains(&e.name))
                .map(|e| {
                    let mut meta = dlg.meta.clone();
                    if multiple || meta.track.is_empty() {
                        meta.track = self.dev_summaries.iter().find(|s| s.file.as_deref() == Some(e.name.as_str())).and_then(|s| s.track.clone()).unwrap_or_default();
                    }
                    (e.clone(), meta)
                })
                .collect();
            if std::env::var("MYCHRONIX_FAKE_DEVICE").is_ok() {
                // Preview mode: there's no MyChron to download from.
                self.dev_message = Some((true, format!(
                    "Preview only: would download {} session{} with these details. Turn on the MyChron to do it for real.",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                )));
            } else if let Some(dir) = self.logger_dir() {
                self.dev_busy = Some("Downloading…".into());
                self.dev_message = None;
                let _ = self.dev.as_ref().unwrap().tx.send(crate::device::Command::Download { files, dir });
            }
        } else if open && !cancel {
            self.dev_dialog = Some(dlg);
        }
    }
}

/// Values already in use, offered while typing in the download dialog.
struct Suggestions {
    drivers: Vec<String>,
    karts: Vec<String>,
    championships: Vec<String>,
    session_types: Vec<String>,
    tracks: Vec<String>,
}

impl Suggestions {
    fn gather(lib: &Library, profiles: &crate::profiles::Profiles) -> Self {
        let mut s = Suggestions { drivers: vec![], karts: vec![], championships: vec![], session_types: vec![], tracks: vec![] };
        for p in profiles.loggers.values() {
            s.drivers.push(p.driver.clone());
            s.karts.push(p.kart.clone());
            s.championships.push(p.championship.clone());
        }
        for m in lib.sessions.iter().filter_map(|x| x.meta.as_ref()) {
            s.drivers.push(m.driver.clone());
            s.karts.push(m.kart.clone());
            s.championships.push(m.championship.clone());
            s.session_types.push(m.session_type.clone());
            s.tracks.push(m.track.clone());
        }
        // RaceStudio's session types.
        s.session_types.extend(
            [
                "Generic testing", "Practice 1", "Practice 2", "Practice 3", "Practice 4", "Qualifying", "Qualifying 1", "Qualifying 2",
                "Warm-up", "Heat", "Heat 1", "Heat 2", "Heat 3", "Pre-final", "Final", "1st Final", "2nd Final", "Race", "1st Race",
                "2nd Race", "A-Main", "Test", "Engine Test", "Track Outing",
            ]
            .map(String::from),
        );
        // Tracks you've raced or made first, then every AiM track.
        for t in lib.tracks.iter().filter(|t| !t.sessions.is_empty() || t.kind == Kind::Custom) {
            s.tracks.push(t.name.clone());
        }
        for t in &lib.tracks {
            s.tracks.push(t.name.clone());
        }
        for v in [&mut s.drivers, &mut s.karts, &mut s.championships, &mut s.session_types, &mut s.tracks] {
            let mut seen = std::collections::HashSet::new();
            v.retain(|x| !x.trim().is_empty() && seen.insert(x.to_lowercase()));
        }
        s
    }
}

/// A text field that offers matching `suggestions` below it while focused.
/// Picking one fills the field; typing anything else is fine too.
fn suggest_field(ui: &mut Ui, id: &str, value: &mut String, hint: &str, suggestions: &[String], width: f32) -> egui::Response {
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(id)
            .hint_text(RichText::new(hint).color(DIM))
            .font(regular(15.0))
            .margin(Margin::symmetric(12, 9))
            .desired_width(width),
    );
    let popup_id = response.id.with("suggestions");
    let ctx = ui.ctx().clone();
    // Stay open while the pointer is over the list, so a click on it lands.
    let last_rect: Option<Rect> = ctx.data(|d| d.get_temp(popup_id));
    let over_list = last_rect.zip(ctx.pointer_hover_pos()).is_some_and(|(r, p)| r.contains(p));
    if !(response.has_focus() || over_list) {
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        return response;
    }
    // Empty, or already one of the suggestions: offer them all (to switch).
    // Otherwise offer the ones containing what's typed.
    let needle = value.to_lowercase();
    let exact = suggestions.iter().any(|s| s.to_lowercase() == needle);
    let matches: Vec<&String> = suggestions
        .iter()
        .filter(|s| needle.is_empty() || exact || s.to_lowercase().contains(&needle))
        .take(8)
        .collect();
    if matches.is_empty() {
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        return response;
    }
    let mut picked = None;
    let area = egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(response.rect.left_bottom() + vec2(0.0, 4.0))
        .show(&ctx, |ui| {
            Frame::new()
                .fill(SURFACE_ACTIVE)
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL + 2))
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(response.rect.width() - 8.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for m in &matches {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(theme::RADIUS_SMALL), SURFACE_HOVER);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.painter().text(r.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, m.as_str(), regular(14.5), TEXT);
                        if resp.clicked() {
                            picked = Some((*m).clone());
                        }
                    }
                });
        });
    ctx.data_mut(|d| d.insert_temp(popup_id, area.response.rect));
    if let Some(p) = picked {
        *value = p;
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        ctx.memory_mut(|m| m.surrender_focus(response.id));
    }
    response
}

const AP_ADDRESS_FAKE: std::net::Ipv4Addr = std::net::Ipv4Addr::new(10, 0, 0, 1);

/// "AiM-MYC6-345678" → "MyChron 6".
fn model_from_ssid(ssid: &str) -> &'static str {
    match ssid.split('-').nth(1).unwrap_or("") {
        "MYC6" => "MyChron 6",
        "MYC5S" => "MyChron 5S",
        "MYC5" => "MyChron 5",
        _ => "AiM logger",
    }
}

// ---------------------------------------------------------------------------
// Sidebar: laps and channels

impl App {
    fn lap_list(&mut self, ui: &mut Ui, session: &Session) {
        section_header(ui, "Laps", &session.laps.len().to_string());
        let best = session.best_lap(MIN_LAP_MS).copied();
        let best_ms = best.map(|b| b.duration_ms()).unwrap_or(0);
        let slow = slow_laps(session);
        // Scale delta bars to the slowest racing lap so out/in laps don't flatten the rest.
        let bar_scale = session
            .laps
            .iter()
            .enumerate()
            .filter(|(i, _)| !slow[*i])
            .map(|(_, l)| l.duration_ms() - best_ms)
            .max()
            .unwrap_or(1)
            .max(1);

        let wanted = (ROW_HEIGHT + 2.0) * (session.laps.len() + 1) as f32;
        let height = wanted.min((ui.available_height() * 0.42).max(ROW_HEIGHT * 4.0));
        let scroll_to = std::mem::take(&mut self.scroll_to_lap);
        let last = session.laps.len().saturating_sub(1);

        egui::ScrollArea::vertical().id_salt("laps").max_height(height).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if row(ui, self.lap.is_none(), |ui| {
                ui.add(Icon::Flag.image(SECONDARY, 15.0));
                ui.add_space(2.0);
                ui.label(RichText::new("Whole session").font(medium(14.0)).color(TEXT));
            })
            .clicked()
            {
                self.lap = None;
                self.cursor_s = None;
            }
            for (i, lap) in session.laps.iter().enumerate() {
                let is_best = Some(*lap) == best;
                let response = row(ui, self.lap == Some(i), |ui| {
                    let (num, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
                    ui.painter().text(num.right_center(), egui::Align2::RIGHT_CENTER, lap.number.to_string(), regular(13.0), DIM);
                    ui.add_space(6.0);
                    let time_color = if is_best { theme::GOOD } else if slow[i] { DIM } else { TEXT };
                    ui.label(RichText::new(format_lap_time(lap.duration_ms())).font(medium(14.0)).color(time_color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let delta = lap.duration_ms() - best_ms;
                        if is_best {
                            tag(ui, "Best", theme::GOOD);
                        } else if slow[i] {
                            let label = match i {
                                0 => "Out lap",
                                i if i == last => "In lap",
                                _ => "Slow",
                            };
                            ui.label(RichText::new(label).size(12.5).color(DIM));
                        } else if best.is_some() {
                            ui.label(RichText::new(format_delta(delta)).size(13.0).color(SECONDARY));
                            let w = 44.0;
                            let (bar, _) = ui.allocate_exact_size(vec2(w, 4.0), Sense::hover());
                            ui.painter().rect_filled(bar, 2.0, SURFACE_ACTIVE);
                            let frac = (delta as f32 / bar_scale as f32).clamp(0.05, 1.0);
                            let fill = Rect::from_min_size(bar.min, vec2(w * frac, bar.height()));
                            ui.painter().rect_filled(fill, 2.0, DIM);
                        }
                    });
                });
                if scroll_to && self.lap == Some(i) {
                    response.scroll_to_me(Some(Align::Center));
                }
                if response.clicked() {
                    self.lap = Some(i);
                    self.cursor_s = None;
                }
            }
        });
    }

    fn channel_list(&mut self, ui: &mut Ui, session: &Session, start_ms: i64) {
        section_header(ui, "Channels", &format!("{} of {}", self.selected.len(), session.channels.len()));
        search_field(ui, &mut self.filter, "Search channels");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (view, label) in [(ChannelView::All, "All"), (ChannelView::Graphed, "Graphed")] {
                if chip(ui, label, self.channel_view == view).clicked() {
                    self.channel_view = view;
                }
            }
        });
        ui.add_space(6.0);

        let needle = self.filter.to_lowercase();
        let cursor_ms = self.cursor_s.map(|s| start_ms + (s * 1000.0) as i64);
        let full = self.selected.len() >= SERIES.len();

        egui::ScrollArea::vertical().id_salt("channels").show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, c) in session.channels.iter().enumerate() {
                let slot = self.selected.get(&i).copied();
                if (!needle.is_empty() && !c.name.to_lowercase().contains(&needle))
                    || (self.channel_view == ChannelView::Graphed && slot.is_none())
                {
                    continue;
                }
                let response = row(ui, false, |ui| {
                    swatch(ui, slot.map(|s| SERIES[s]));
                    ui.add_space(2.0);
                    let name_color = if slot.is_some() { TEXT } else { SECONDARY };
                    ui.label(RichText::new(&c.name).color(name_color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let units = pretty_units(&c.units);
                        if !units.is_empty() {
                            ui.label(RichText::new(units).size(12.0).color(DIM));
                        }
                        if let Some(v) = cursor_ms.and_then(|t| value_at(c, t)) {
                            ui.label(RichText::new(format_value(v, c.decimals)).font(medium(13.5)).color(TEXT));
                        }
                    });
                });
                if response.clicked() {
                    if slot.is_some() {
                        self.selected.remove(&i);
                    } else if !full {
                        self.select(i);
                    }
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Analysis page

impl App {
    fn analysis(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        self.page_header(ui, session);
        ui.add_space(18.0);
        summary_tiles(ui, session);
        ui.add_space(22.0);

        ui.horizontal(|ui| {
            match self.lap.and_then(|i| session.laps.get(i)) {
                Some(l) => {
                    ui.label(RichText::new(format!("Lap {}", l.number)).font(semibold(18.0)).color(TEXT));
                    ui.label(RichText::new(format_lap_time(l.duration_ms())).font(regular(16.0)).color(SECONDARY));
                }
                None => {
                    ui.label(RichText::new("Whole session").font(semibold(18.0)).color(TEXT));
                    ui.label(RichText::new(format_duration(end_ms - start_ms)).font(regular(16.0)).color(SECONDARY));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match self.cursor_s {
                Some(s) => {
                    ui.label(RichText::new(format!("{s:.2} s")).font(medium(14.0)).color(TEXT));
                    ui.add(Icon::Clock.image(DIM, 15.0));
                }
                None => {
                    ui.label(RichText::new("Hover a graph to read values").color(DIM));
                }
            });
        });
        ui.add_space(10.0);

        let gap = 12.0;
        let height = ui.available_height();
        let map_w = if session.gps.is_some() { (ui.available_width() * 0.4).clamp(280.0, 560.0) } else { 0.0 };
        let graphs_w = ui.available_width() - if map_w > 0.0 { map_w + gap } else { 0.0 };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(graphs_w, height), column, |ui| {
                ui.set_width(graphs_w);
                self.graphs(ui, session, start_ms, end_ms)
            });
            if map_w > 0.0 {
                ui.allocate_ui_with_layout(vec2(map_w, height), column, |ui| {
                    ui.set_width(map_w);
                    self.track_map(ui, session, start_ms, end_ms)
                });
            }
        });
    }

    fn page_header(&mut self, ui: &mut Ui, session: &Session) {
        let i = &session.info;
        let title = i.venue.as_deref().filter(|s| !s.is_empty()).unwrap_or("Session");
        let when = match (i.date.as_deref(), i.time.as_deref()) {
            (Some(d), Some(t)) => Some(format!("{d} at {}", t.get(..5).unwrap_or(t))),
            (Some(d), None) => Some(d.to_string()),
            _ => None,
        };
        let sub = [i.session.clone(), when, i.driver.clone(), i.vehicle.clone()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("  ·  ");
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).font(semibold(28.0)).color(TEXT));
                ui.label(RichText::new(sub).font(regular(14.5)).color(SECONDARY));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, Some(Icon::FolderOpen), "Open session…", false).clicked() {
                    self.open_dialog();
                }
            });
        });
    }

    fn graphs(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        if self.selected.is_empty() {
            card(ui, |ui| {
                ui.set_min_height(180.0);
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Pick channels in the sidebar to graph them.").color(SECONDARY));
                });
            });
            return;
        }

        let selected: Vec<(usize, usize)> = self.selected.iter().map(|(&c, &s)| (c, s)).collect();
        let gap = 10.0;
        let n = selected.len() as f32;
        let card_h = ((ui.available_height() - gap * (n - 1.0)) / n).max(140.0);
        let link = ui.id().with("time");
        let cursor_ms = self.cursor_s.map(|s| start_ms + (s * 1000.0) as i64);
        let mut hovered = None;

        egui::ScrollArea::vertical().id_salt("graphs").show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = gap;
            for (k, &(i, slot)) in selected.iter().enumerate() {
                let c = &session.channels[i];
                let color = SERIES[slot];
                let last = k + 1 == selected.len();
                graph_card(ui, card_h, |ui| {
                    ui.horizontal(|ui| {
                        let (key, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
                        ui.painter().line_segment(
                            [pos2(key.left(), key.center().y), pos2(key.right(), key.center().y)],
                            Stroke::new(2.5, color),
                        );
                        ui.label(RichText::new(&c.name).font(medium(14.5)).color(TEXT));
                        let units = pretty_units(&c.units);
                        if !units.is_empty() {
                            ui.label(RichText::new(units).size(13.0).color(DIM));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(v) = cursor_ms.and_then(|t| value_at(c, t)) {
                                ui.label(RichText::new(format_value(v, c.decimals)).font(semibold(16.0)).color(TEXT));
                            }
                        });
                    });
                    let points = decimate(c, start_ms, end_ms, MAX_POINTS);
                    let response = Plot::new(("channel", i))
                        .height(ui.available_height())
                        .show_background(false)
                        .grid_color(GRID)
                        .show_axes([last, true])
                        .y_axis_min_width(46.0)
                        .show_x(false)
                        .show_y(false)
                        .cursor_color(DIM)
                        .link_axis(link, [true, false])
                        .link_cursor(link, [true, false])
                        .allow_zoom([true, false])
                        .allow_drag([true, false])
                        .allow_scroll(false)
                        .x_axis_formatter(|mark, _| format!("{:.0} s", mark.value))
                        .show(ui, |plot| {
                            plot.line(Line::new(c.name.clone(), PlotPoints::from(points)).color(color).width(2.0));
                            if let Some(s) = self.cursor_s {
                                plot.vline(VLine::new("cursor", s).color(SECONDARY).width(1.0));
                            }
                            plot.pointer_coordinate()
                        });
                    if let Some(p) = response.inner {
                        hovered = Some(p.x.max(0.0));
                    }
                });
            }
        });
        if hovered.is_some() {
            self.cursor_s = hovered;
        }
    }
}

impl App {
    /// The GPS path for the current window, coloured by a channel, with a dot
    /// at the shared cursor. Hovering the map moves the cursor.
    fn track_map(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        let Some(gps) = &session.gps else { return };
        let lo = gps.times_ms.partition_point(|&t| t < start_ms);
        let hi = gps.times_ms.partition_point(|&t| t <= end_ms);
        let rpm = session.channels.iter().position(|c| c.name == "RPM");
        let color_ch = self.map_channel.or(rpm).or_else(|| self.selected.keys().next().copied());
        let slot = color_ch.and_then(|i| self.selected.get(&i).copied()).unwrap_or(0);
        let hue = SERIES[slot];
        let cursor_ms = self.cursor_s.map(|s| start_ms + (s * 1000.0) as i64);

        Frame::new()
            .fill(SURFACE)
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 12 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height() - 24.0);

                // Header: title, colour-by picker, readouts at the cursor.
                ui.horizontal(|ui| {
                    ui.add(Icon::MapPin.image(DIM, 16.0));
                    ui.label(RichText::new("Track").font(medium(14.5)).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let name = color_ch.map(|i| session.channels[i].name.as_str()).unwrap_or("—");
                        egui::ComboBox::from_id_salt("map_channel")
                            .selected_text(RichText::new(name).size(13.0))
                            .width(118.0)
                            .show_ui(ui, |ui| {
                                for (i, c) in session.channels.iter().enumerate() {
                                    if c.values.len() > 1 {
                                        ui.selectable_value(&mut self.map_channel, Some(i), &c.name);
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Channel that colours the track");
                    });
                });

                let legend_h = 34.0;
                let (rect, response) = ui.allocate_exact_size(
                    vec2(ui.available_width(), (ui.available_height() - legend_h).max(80.0)),
                    Sense::hover(),
                );
                if hi <= lo + 1 {
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "No GPS fix in this part of the session", regular(13.0), DIM);
                    return;
                }

                // Local flat projection: metres east/north of the window's first point.
                let (lat0, lon0) = (gps.latitude[lo], gps.longitude[lo]);
                let kx = 111_320.0 * lat0.to_radians().cos();
                let ky = 110_540.0;
                let xy: Vec<(f64, f64)> = (lo..hi)
                    .map(|i| ((gps.longitude[i] - lon0) * kx, (gps.latitude[i] - lat0) * ky))
                    .collect();
                let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                for &(x, y) in &xy {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
                // Leave a strip at the top for the cursor readout.
                let area = Rect::from_min_max(rect.min + vec2(0.0, 26.0), rect.max).shrink(12.0);
                let scale = (area.width() as f64 / (x1 - x0).max(1.0)).min(area.height() as f64 / (y1 - y0).max(1.0));
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let to_screen = |(x, y): (f64, f64)| {
                    pos2(area.center().x + ((x - cx) * scale) as f32, area.center().y - ((y - cy) * scale) as f32)
                };
                let pts: Vec<egui::Pos2> = xy.iter().map(|&p| to_screen(p)).collect();

                // Colour scale from the window's values, ignoring the extreme 2% each side
                // (the logger records occasional sensor spikes).
                let channel = color_ch.map(|i| &session.channels[i]);
                let values: Vec<Option<f64>> = (lo..hi)
                    .map(|i| channel.and_then(|c| value_at(c, gps.times_ms[i])))
                    .collect();
                let (vmin, vmax) = robust_range(values.iter().flatten().copied());
                let ramp = |v: f64| {
                    let t = ((v - vmin) / (vmax - vmin).max(f64::EPSILON)).clamp(0.0, 1.0) as f32;
                    let low = mix(hue, SURFACE, 0.62);
                    let high = mix(hue, Color32::WHITE, 0.6);
                    mix(low, high, t)
                };

                let painter = ui.painter_at(rect);
                for k in 0..pts.len() - 1 {
                    let (i, j) = (lo + k, lo + k + 1);
                    // Don't bridge GPS dropouts.
                    if gps.times_ms[j] - gps.times_ms[i] > 1000 {
                        continue;
                    }
                    let color = values[k].map(ramp).unwrap_or(DIM);
                    painter.line_segment([pts[k], pts[k + 1]], Stroke::new(3.5, color));
                }
                // Where the window starts.
                painter.circle_filled(pts[0], 4.0, DIM);

                // Hover: snap the shared cursor to the nearest point.
                if let Some(pointer) = response.hover_pos() {
                    let (k, d) = pts
                        .iter()
                        .enumerate()
                        .map(|(k, p)| (k, p.distance_sq(pointer)))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .unwrap();
                    if d < 28.0 * 28.0 {
                        self.cursor_s = Some((gps.times_ms[lo + k] - start_ms) as f64 / 1000.0);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                    }
                }
                let units = channel.map(|c| pretty_units(&c.units)).unwrap_or("");
                let dec = channel.map(|c| c.decimals).unwrap_or(0);
                if let Some(t) = cursor_ms {
                    if let Some(k) = gps.index_at(t).filter(|&i| i >= lo && i < hi).map(|i| i - lo) {
                        painter.circle_filled(pts[k], 7.0, SURFACE);
                        painter.circle_filled(pts[k], 5.0, TEXT);
                    }
                    // Readout in the map's top-left corner.
                    let mut line = String::new();
                    if let Some(v) = channel.and_then(|c| value_at(c, t)) {
                        line = format!("{} {units}", format_value(v, dec));
                    }
                    if let Some(i) = gps.index_at(t) {
                        if !line.is_empty() {
                            line.push_str("   ");
                        }
                        line.push_str(&format!("{:.1} km/h", gps.speed_kmh[i]));
                    }
                    painter.text(rect.left_top() + vec2(2.0, 2.0), egui::Align2::LEFT_TOP, line, medium(14.0), TEXT);
                }

                // Legend: the colour ramp and the range it covers.
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format_value(vmin, dec)).size(12.0).color(DIM));
                    let bar_w = (ui.available_width() - 110.0).clamp(60.0, 200.0);
                    let (bar, _) = ui.allocate_exact_size(vec2(bar_w, 6.0), Sense::hover());
                    let steps = 32;
                    for s in 0..steps {
                        let r = Rect::from_min_size(
                            pos2(bar.left() + bar.width() * s as f32 / steps as f32, bar.top()),
                            vec2(bar.width() / steps as f32 + 0.5, bar.height()),
                        );
                        ui.painter().rect_filled(r, 0.0, ramp(vmin + (vmax - vmin) * s as f64 / (steps - 1) as f64));
                    }
                    ui.label(RichText::new(format!("{} {units}", format_value(vmax, dec))).size(12.0).color(DIM));
                });
            });
    }
}

/// Range of `values` without the extreme 2% at each end.
fn robust_range(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let mut v: Vec<f64> = values.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return (0.0, 1.0);
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    let (lo, hi) = (at(0.02), at(0.98));
    if hi > lo { (lo, hi) } else { (v[0], v[v.len() - 1].max(v[0] + 1.0)) }
}

/// Linear blend from `a` (t = 0) to `b` (t = 1).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn summary_tiles(ui: &mut Ui, session: &Session) {
    let slow = slow_laps(session);
    let racing: Vec<_> = session.laps.iter().zip(&slow).filter(|(_, s)| !**s).map(|(l, _)| l).collect();
    let best = session.best_lap(MIN_LAP_MS);
    let avg = (!racing.is_empty()).then(|| racing.iter().map(|l| l.duration_ms()).sum::<i64>() / racing.len() as i64);
    let span = session.laps.first().zip(session.laps.last()).map(|(a, b)| b.end_ms - a.start_ms);
    let dash = || "—".to_string();
    let tiles = [
        (Icon::Timer, "Best lap", best.map(|l| format_lap_time(l.duration_ms())).unwrap_or_else(dash)),
        (Icon::Activity, "Average lap", avg.map(format_lap_time).unwrap_or_else(dash)),
        (Icon::Flag, "Laps", session.laps.len().to_string()),
        (Icon::Clock, "On track", span.map(format_duration).unwrap_or_else(dash)),
    ];

    let gap = 10.0;
    let width = (ui.available_width() - gap * (tiles.len() - 1) as f32) / tiles.len() as f32;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (icon, label, value) in tiles {
            tile(ui, width, icon, label, value);
        }
    });
}

// ---------------------------------------------------------------------------
// Widgets

fn wordmark(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.add(egui::Image::new(egui::include_image!("../assets/mychronix-icon.svg")).fit_to_exact_size(Vec2::splat(38.0)));
        // The wordmark's viewBox is 578 × 82.
        ui.add(egui::Image::new(egui::include_image!("../assets/mychronix.svg")).fit_to_exact_size(vec2(22.0 * 578.0 / 82.0, 22.0)));
    });
}

fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::OUTLINE);
}

fn section_header(ui: &mut Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(semibold(16.0)).color(TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(detail).size(12.5).color(DIM));
        });
    });
    ui.add_space(4.0);
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

fn graph_card(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(height - 20.0);
            add(ui)
        });
}

fn tile(ui: &mut Ui, width: f32, icon: Icon, label: &str, value: String) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin { left: 16, right: 12, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.add(icon.image(DIM, 15.0));
                    ui.add(egui::Label::new(RichText::new(label).size(13.0).color(SECONDARY)).truncate());
                });
                ui.add(egui::Label::new(RichText::new(value).font(semibold(24.0)).color(TEXT)).truncate());
            });
        });
}

/// Pill button; `primary` fills it with ink.
fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, primary: bool) -> egui::Response {
    let (fg, fill, hover) = if primary {
        (ON_TEXT, TEXT, Color32::from_rgb(0xd9, 0xdd, 0xe2))
    } else {
        (TEXT, SURFACE, SURFACE_HOVER)
    };
    let galley = ui.painter().layout_no_wrap(label.into(), medium(14.0), fg);
    let icon_w = if icon.is_some() { 24.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + icon_w + 32.0, 38.0), Sense::click());
    let fill = if response.hovered() { hover } else { fill };
    ui.painter().rect_filled(rect, CornerRadius::same(19), fill);
    let mut x = rect.left() + 16.0;
    if let Some(icon) = icon {
        icon.image(fg, 16.0).paint_at(ui, Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(16.0)));
        x += icon_w;
    }
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

fn chip(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let fg = if selected { ON_TEXT } else { TEXT };
    let galley = ui.painter().layout_no_wrap(label.into(), medium(13.0), fg);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(26.0, 14.0), Sense::click());
    let fill = match (selected, response.hovered()) {
        (true, _) => TEXT,
        (false, true) => SURFACE_HOVER,
        (false, false) => SURFACE,
    };
    ui.painter().rect_filled(rect, CornerRadius::same(255), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

fn tag(ui: &mut Ui, text: &str, fill: Color32) {
    let galley = ui.painter().layout_no_wrap(text.into(), semibold(11.5), ON_TEXT);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(14.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(255), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ON_TEXT);
}

fn search_field(ui: &mut Ui, text: &mut String, hint: &str) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin { left: 12, right: 10, top: 4, bottom: 4 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(Icon::Search.image(DIM, 16.0));
                let clear_w = if text.is_empty() { 0.0 } else { 22.0 };
                ui.add(
                    egui::TextEdit::singleline(text)
                        .frame(Frame::new())
                        .hint_text(RichText::new(hint).color(DIM))
                        .desired_width(ui.available_width() - clear_w),
                );
                if !text.is_empty() && ui.add(Icon::X.image(DIM, 14.0).sense(Sense::click())).clicked() {
                    text.clear();
                }
            });
        });
}

/// Channel colour key: filled dot when graphed, empty ring when not.
fn swatch(ui: &mut Ui, color: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    match color {
        Some(c) => ui.painter().circle_filled(rect.center(), 5.0, c),
        None => ui.painter().circle_stroke(rect.center(), 4.5, Stroke::new(1.2, DIM)),
    };
}

/// A full-width clickable list row with hover and selected states.
fn row(ui: &mut Ui, selected: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Some(SURFACE_ACTIVE)
        } else if response.hovered() {
            Some(SURFACE_HOVER)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL + 2), fill);
        }
        let inner = rect.shrink2(vec2(10.0, 0.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
        add(&mut child);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

// ---------------------------------------------------------------------------
// Formatting and data helpers

fn file_name(path: &std::path::Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn group_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn format_duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s >= 3600 { format!("{}h {:02}m", s / 3600, (s / 60) % 60) } else { format!("{}m {:02}s", s / 60, s % 60) }
}

fn format_delta(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "+" };
    let a = ms.unsigned_abs();
    if a >= 60_000 { format!("{sign}{}", format_lap_time(a as i64)) } else { format!("{sign}{:.3}", a as f64 / 1000.0) }
}

fn format_value(v: f64, decimals: u8) -> String {
    format!("{v:.prec$}", prec = decimals.min(4) as usize)
}

/// Units as the logger stores them, made presentable.
fn pretty_units(units: &str) -> &str {
    match units {
        "C" => "°C",
        "F" => "°F",
        "deg" => "°",
        other => other,
    }
}

/// Laps that aren't timed laps: the out-lap (first) and in-lap (last), which
/// run to or from the ends of the recording, plus any lap far off the pace
/// (cool-downs, cautions): shorter than `MIN_LAP_MS` or 15% slower than the
/// median complete lap.
fn slow_laps(session: &Session) -> Vec<bool> {
    let n = session.laps.len();
    let mut times: Vec<i64> = session.complete_laps().iter().map(|l| l.duration_ms()).filter(|&d| d >= MIN_LAP_MS).collect();
    times.sort_unstable();
    let median = times.get(times.len() / 2).copied().unwrap_or(i64::MAX);
    session
        .laps
        .iter()
        .enumerate()
        .map(|(i, l)| {
            i == 0 || i + 1 == n || l.duration_ms() < MIN_LAP_MS || l.duration_ms() as f64 > median as f64 * 1.15
        })
        .collect()
}

/// Channels shown when a session first opens: ones every MyChron records,
/// falling back to the first few channels with real data.
fn default_channels(session: &Session) -> Vec<usize> {
    const PREFERRED: [&str; 7] = ["RPM", "GPS Speed", "Exhaust Temp", "Speed", "WheelSpeed", "Water Temp", "WT"];
    let mut picks: Vec<usize> = PREFERRED
        .iter()
        .filter_map(|name| session.channels.iter().position(|c| c.name == *name))
        .take(3)
        .collect();
    if picks.is_empty() {
        picks = session
            .channels
            .iter()
            .enumerate()
            .filter(|(_, c)| c.values.len() > 1000)
            .map(|(i, _)| i)
            .take(3)
            .collect();
    }
    picks
}

/// Sample value at or just before `t_ms`.
fn value_at(c: &Channel, t_ms: i64) -> Option<f64> {
    let i = c.times_ms.partition_point(|&t| t <= t_ms);
    i.checked_sub(1).map(|i| c.values[i])
}

/// Samples within [start_ms, end_ms] as (seconds from start, value), reduced to
/// at most ~`max` points by keeping each bucket's min and max so peaks survive.
fn decimate(c: &Channel, start_ms: i64, end_ms: i64, max: usize) -> Vec<[f64; 2]> {
    let lo = c.times_ms.partition_point(|&t| t < start_ms);
    let hi = c.times_ms.partition_point(|&t| t <= end_ms);
    let point = |i: usize| [(c.times_ms[i] - start_ms) as f64 / 1000.0, c.values[i]];
    let n = hi.saturating_sub(lo);
    if n <= max {
        return (lo..hi).map(point).collect();
    }
    let buckets = max / 2;
    let mut out = Vec::with_capacity(max);
    for b in 0..buckets {
        let a = lo + n * b / buckets;
        let z = lo + n * (b + 1) / buckets;
        let (mut min_i, mut max_i) = (a, a);
        for i in a..z {
            if c.values[i] < c.values[min_i] {
                min_i = i;
            }
            if c.values[i] > c.values[max_i] {
                max_i = i;
            }
        }
        let (first, second) = if min_i <= max_i { (min_i, max_i) } else { (max_i, min_i) };
        out.push(point(first));
        if second != first {
            out.push(point(second));
        }
    }
    out
}
