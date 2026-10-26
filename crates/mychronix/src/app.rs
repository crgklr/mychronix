use std::collections::BTreeMap;
use std::path::PathBuf;

use aim_xrk::{Channel, Session, format_lap_time};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, Sense, Stroke, Ui,
    Vec2, pos2, vec2,
};
use egui_plot::{Line, Plot, PlotPoints, VLine};

mod analysis_ui;
mod charts_ui;
mod corners_ui;
mod map_ui;
mod sensor_ui;
mod datakey_ui;
mod device_ui;
mod track_pick_ui;
mod compare_drivers_ui;
mod compare_panel_ui;
mod events_page;
mod garage_manage_ui;
mod garage_ui;
mod report_ui;
mod sessions_filter_ui;
mod setup_ui;
mod welcome_ui;
mod settings_ui;
mod library_load;
mod chrome_ui;
mod format;
mod graphs_ui;
mod sessions_ui;
mod widgets;
use format::*;
use graphs_ui::*;
use sessions_ui::*;
use widgets::*;
// Used outside the pages too (the assistant server, reports).
pub(crate) use format::slow_laps;
pub(crate) use sessions_ui::is_calculated;
mod splits_ui;
mod tracks_ui;
mod device_page;
mod compare_ui;
mod window_ui;

pub(crate) use sessions_filter_ui::{session_driver, session_track};
pub(crate) use window_ui::OWN_CONTROLS as OWN_WINDOW_CONTROLS;

use compare_ui::gain_color;
use device_page::model_from_ssid;

use crate::splits;
use crate::tracks::{self, Kind, Library};
use crate::theme::{
    self, DIM, GRID, Icon, ON_TEXT, ROW_HEIGHT, SECONDARY, SERIES, SIDEBAR, SURFACE,
    SURFACE_ACTIVE, SURFACE_HOVER, TEXT, WINDOW, medium, regular, semibold,
};

/// Points drawn per channel; longer series are min/max-decimated to this many buckets.
const MAX_POINTS: usize = 4000;
/// Laps shorter than this are out/in laps cut by the session start or end.
pub(crate) const MIN_LAP_MS: i64 = 10_000;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Page {
    Garage,
    Sessions,
    Events,
    #[default]
    Analysis,
    Tracks,
    Device,
    Settings,
}

/// The download dialog's state.
struct DownloadDialog {
    meta: crate::device::SessionMeta,
    /// Tracks the logger recorded for the selected sessions; more than one means
    /// each session keeps its own.
    detected: Vec<String>,
    /// Setup for these sessions, starting from the logger's latest; saved on the
    /// earliest one only when it changed or has notes.
    setup: crate::setup::Setup,
    setup_before: crate::setup::Setup,
    setup_open: bool,
    /// Sessions picked that are already on this computer: replace those
    /// copies (true), or keep them and save these beside them.
    replace: bool,
}

/// Tabs on the Analysis page.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AnalysisView {
    #[default]
    Session,
    Report,
    Splits,
    Corners,
    /// The track up close: the line through each corner (new with the sidebar views).
    Map,
    Histogram,
    Scatter,
    Compare,
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
    session: Option<std::sync::Arc<Session>>,
    error: Option<String>,
    /// Selected channels and the color slot each keeps while selected.
    selected: BTreeMap<usize, usize>,
    /// Graphed channels in the order they were added (graphs stack in this order).
    graph_order: Vec<usize>,
    /// The time window the graphs last showed, to reset their range when it changes.
    graph_window: Option<(i64, i64, bool)>,
    /// Graphs against distance instead of time.
    graph_distance: bool,
    /// Channel groups folded away in the sidebar (Lap timing starts folded).
    collapsed_groups: std::collections::HashSet<&'static str>,
    collapsed_init: bool,
    /// A graph just added, to scroll into view.
    scroll_to_graph: Option<usize>,
    lap: Option<usize>,
    filter: String,
    channel_view: ChannelView,
    /// Shared cursor, in seconds from the start of the current window.
    cursor_s: Option<f64>,
    analysis_view: AnalysisView,
    report: report_ui::ReportState,
    splits: splits_ui::SplitsState,
    corners: corners_ui::CornersState,
    sensors: sensor_ui::SensorState,
    key: datakey_ui::KeyState,
    /// The "Add a MyChron" window, while open.
    add_mychron: Option<device_ui::AddMyChron>,
    track_pick: Option<track_pick_ui::TrackPick>,
    /// Reloading the library in the background (see `library_load`).
    library_load: library_load::LibraryLoad,
    /// Track offers put off this run (by the name you typed), asked again next time.
    offers_closed: std::collections::HashSet<String>,
    /// The track offer on screen and its pick.
    offer_pick: Option<track_pick_ui::OfferPick>,
    /// When the last touchscreen touch was seen, and whether touch is in use.
    last_touch: Option<std::time::Instant>,
    touch_mode: bool,
    charts: charts_ui::ChartsState,
    /// Where the icon rail is (for opening it on hover), the session details and channel picker.
    rail_rect: Option<Rect>,
    /// The session panel (opened from the pill or the rail), and where the pill is.
    session_panel: bool,
    pill_rect: Option<Rect>,
    /// The Map view: corner or whole track, line colors, imagery, its own view of the map.
    map_view: map_ui::MapState,
    channel_picker_open: bool,
    /// Setup sheets: baselines, the editor, the open session's measured RPM per km/h,
    /// and a sheet to write once a download finishes.
    setup_baselines: Option<crate::setup::Baselines>,
    setup_edit: Option<setup_ui::SetupEdit>,
    /// The update check, a newer release found, and its notes window.
    update_rx: Option<std::sync::mpsc::Receiver<crate::update::Release>>,
    /// AiM's track database being brought up to date, and how far along.
    aim_tracks_rx: Option<std::sync::mpsc::Receiver<crate::aim_tracks::Event>>,
    aim_tracks_progress: Option<(usize, usize)>,
    /// Session files being imported, how far along, and what came in.
    import_rx: Option<std::sync::mpsc::Receiver<crate::import::Event>>,
    import_progress: Option<(usize, usize)>,
    import_report: Option<crate::import::Report>,
    /// This is the first download of AiM's tracks (not just changes).
    aim_tracks_first: bool,
    /// The last try couldn't reach AiM's server.
    aim_tracks_offline: bool,
    update: Option<crate::update::Release>,
    update_notes: bool,
    measured_ratio: Option<f64>,
    /// Channel that colors the track map (None: RPM if present).
    map_channel: Option<usize>,
    /// Color the Graphs map by where the open lap gains and loses time against the best lap.
    map_time_compare: bool,
    pub screenshot_to: Option<PathBuf>,
    frames: u32,

    /// The last error and device message written to the log.
    logged_error: Option<String>,
    /// Frames seen by the input hook (for scripted test clicks).
    input_frames: u32,
    /// The Garage: a session to open after the frame, and Events' plan dialog to open.
    pending_open: Option<PathBuf>,
    plan_on_open: bool,
    /// The Analysis session picker's driver filter and warm-up toggle.
    picker_logger: Option<String>,
    picker_warmups: bool,
    /// The Settings page's last word on connecting an AI assistant.
    mcp_message: Option<(bool, String)>,

    profiles: Option<crate::profiles::Profiles>,
    /// The Sessions page's filters (right column).
    session_filter: sessions_filter_ui::SessionFilter,
    session_search: String,

    library: Option<Library>,
    map_tiles: Option<walkers::HttpTiles>,
    sat_tiles: Option<walkers::HttpTiles>,
    /// The Device page: the MyChron found, its sessions, downloading (see `device_page`).
    device: device_page::DeviceState,
    /// The Tracks page: the selected track, the list, the map (see `tracks_ui`).
    track_page: tracks_ui::TrackPageState,
    /// The Events page: saved events, the selection, the plan dialog (see `events_page`).
    events_page: events_page::EventsState,
    /// The Garage: its dialogs and which page of drivers (see `garage_ui`).
    garage: garage_ui::GarageState,
    /// The Compare tab: the laps compared and its cursor (see `compare_ui`).
    comparison: compare_ui::CompareState,
}

/// See `settings_ui::repair_assistants`.
pub fn repair_assistants() {
    settings_ui::repair_assistants();
}

impl App {
    pub fn load(&mut self, path: PathBuf) {
        match crate::jobs::open_session(&path) {
            Ok(mut session) => {
                self.fill_names(&mut session, &path);
                self.measured_ratio = crate::setup::measured_rpm_per_kmh(&session);
                self.selected.clear();
                self.graph_order.clear();
                for i in default_channels(&session) {
                    self.select(i);
                }
                // Open on the best lap with the cursor halfway round, as RaceStudio does.
                self.lap = session.best_lap(MIN_LAP_MS).and_then(|b| session.laps.iter().position(|l| l == b));
                self.cursor_s = self.lap.map(|i| session.laps[i].duration_ms() as f64 / 1000.0 * 0.5);
                self.map_channel = None;
                self.error = None;
                self.session = Some(std::sync::Arc::new(session));
                self.path = Some(path);
                self.page = Page::Analysis;
            }
            Err(e) => {
                log::error!("couldn't open {}: {e}", path.display());
                self.error = Some(format!("Couldn't open {}: {e}", file_name(&path)));
            }
        }
    }

    /// Opens the session on screen again (its laps changed), keeping the
    /// channels graphed.
    pub(super) fn reopen(&mut self, path: PathBuf) {
        let (selected, order, map_channel, page) = (self.selected.clone(), self.graph_order.clone(), self.map_channel, self.page);
        self.load(path);
        if self.error.is_none() {
            (self.selected, self.graph_order, self.map_channel, self.page) = (selected, order, map_channel, page);
        }
    }

    /// Fills in what the logger leaves blank: the details entered for this
    /// session at download, else the names saved for its logger.
    fn fill_names(&mut self, session: &mut Session, path: &std::path::Path) {
        // The kart its MyChron was on when it was recorded (for a file opened
        // from elsewhere: the one its MyChron is on, or was last on).
        let lib = self.library.get_or_insert_with(Library::load);
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
        let kart: Option<crate::profiles::Kart> = match lib.sessions.iter().find(|s| s.path == path) {
            Some(s) => s.kart.and_then(|id| profiles.karts.iter().find(|k| k.id == id)).cloned(),
            None => session.info.logger_id.and_then(|id| profiles.kart_on(&id.to_string())).cloned(),
        };
        let kart_driver = kart.as_ref().and_then(|k| profiles.driver_name(k.driver)).map(str::to_string);
        // Without GPS, speed from RPM and the session's gearing and tires
        // (before units are converted, so it's converted with the rest).
        if session.channel("GPS Speed").is_none() {
            let lib = self.library.get_or_insert_with(Library::load);
            if let Some(i) = lib.sessions.iter().position(|s| s.path == path) {
                let baselines = self.setup_baselines.get_or_insert_with(crate::setup::Baselines::load);
                let resolved = crate::setup::resolve(lib, baselines);
                let engine = kart.as_ref().map(|k| k.engine.clone()).unwrap_or_default();
                let clutch = kart.as_ref().and_then(|k| crate::setup::number(&k.clutch_rpm));
                if let Some(ch) = resolved.get(i).and_then(|r| crate::setup::speed_from_rpm(session, &r.setup, &engine, clutch)) {
                    session.channels.push(ch);
                }
            }
        }
        seconds_not_ms(session);
        let blank = |v: &Option<String>| v.as_deref().unwrap_or("").is_empty();
        let nonempty = |v: &str| Some(v.to_string()).filter(|v| !v.is_empty());
        let meta = crate::device::SessionMeta::load(path);
        // The driver entered for this session wins, then its kart's driver
        // (over the name typed into the logger).
        let entered = meta.as_ref().and_then(|m| nonempty(&m.driver));
        if let Some(d) = entered.or(kart_driver.filter(|d| !d.is_empty())) {
            session.info.driver = Some(d);
        }
        if let Some(m) = meta {
            if blank(&session.info.vehicle) {
                session.info.vehicle = nonempty(&m.kart);
            }
            if blank(&session.info.session) {
                session.info.session = nonempty(&m.session_type);
            }
        }
        if let Some(k) = &kart && blank(&session.info.vehicle) {
            session.info.vehicle = nonempty(&k.name);
        }
    }

    /// Opens on the best lap with the cursor 40% in, for screenshots.
    pub fn demo(&mut self) {
        let Some(session) = &self.session else { return };
        let Some(best) = session.best_lap(MIN_LAP_MS).copied() else { return };
        self.lap = session.laps.iter().position(|l| *l == best);
        self.cursor_s = Some(best.duration_ms() as f64 / 1000.0 * 0.4);
    }

    /// Compare tab with the best laps at this track, for screenshots.
    pub fn demo_compare(&mut self) {
        self.add_best_laps_here(None);
        self.comparison.laps.truncate(3);
        self.analysis_view = AnalysisView::Compare;
        self.comparison.cursor_m = Some(120.0);
    }

    pub fn show_device_page(&mut self) {
        self.page = Page::Device;
    }

    pub fn show_tracks_page(&mut self) {
        self.page = Page::Tracks;
    }

    /// The setup in force for a logger after its newest session.
    fn latest_setup(&mut self, logger: &str) -> crate::setup::Setup {
        let lib = self.library.get_or_insert_with(Library::load);
        let baselines = self.setup_baselines.get_or_insert_with(crate::setup::Baselines::load);
        let kart = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).mounted_on(logger).map(|k| k.id);
        crate::setup::latest(lib, baselines, logger, kart)
    }

    pub fn show_distance(&mut self) {
        self.graph_distance = true;
    }

    pub fn show_view(&mut self, name: &str) {
        self.analysis_view = match name {
            "histogram" => AnalysisView::Histogram,
            "scatter" => AnalysisView::Scatter,
            _ => self.analysis_view,
        };
    }

    pub fn show_splits(&mut self) {
        self.analysis_view = AnalysisView::Splits;
    }

    pub fn show_report(&mut self) {
        self.analysis_view = AnalysisView::Report;
    }

    pub fn show_analysis_page(&mut self) {
        self.page = Page::Analysis;
    }

    pub fn show_settings_page(&mut self) {
        self.page = Page::Settings;
    }

    pub fn show_garage_page(&mut self) {
        self.page = Page::Garage;
    }

    pub fn show_events_page(&mut self) {
        self.page = Page::Events;
    }

    pub fn show_sessions_page(&mut self) {
        self.page = Page::Sessions;
    }

    fn select(&mut self, channel: usize) {
        let used: Vec<usize> = self.selected.values().copied().collect();
        if let Some(slot) = (0..SERIES.len()).find(|s| !used.contains(s)) {
            self.selected.insert(channel, slot);
            self.graph_order.retain(|&c| c != channel);
            self.graph_order.push(channel);
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
            .add_filter("AiM session", &["xrk", "xrz", "drk", "imd", "mc4", "XRK", "XRZ", "DRK", "IMD", "MC4"])
            .add_filter("MyChron 5/6 (.xrk, .xrz)", &["xrk", "xrz", "XRK", "XRZ"])
            .add_filter("RaceStudio 2 / MyChron 4 (.drk, .imd, .mc4)", &["drk", "imd", "mc4", "DRK", "IMD", "MC4"])
            .pick_file()
        {
            let path = self.import_rs2(path);
            self.load(path);
        }
    }

    /// A RaceStudio 2 session opened from outside the library is copied into
    /// it with its companion files (see `import_rs2_files`). Returns the copy
    /// to open, or `path` as it was.
    fn import_rs2(&mut self, path: PathBuf) -> PathBuf {
        let root = crate::paths::sessions_dir().unwrap_or_default();
        match import_rs2_files(&path, &root) {
            Ok(Some(copy)) => {
                self.reload_library();
                copy
            }
            Ok(None) => path,
            Err(e) => {
                log::warn!("couldn't add {} to the library: {e}", path.display());
                path
            }
        }
    }
}

/// Copies a RaceStudio 2 session (`.drk`, `.imd` or `.mc4`) from outside
/// `root` into `root/<logger serial>/`, with the files RaceStudio 2 keeps
/// beside it: `.imd` (the raw download), `.gpk` and `.rrk` (GPS positions).
/// Files already there are kept. None when it isn't one or is already inside.
fn import_rs2_files(path: &std::path::Path, root: &std::path::Path) -> std::io::Result<Option<PathBuf>> {
    let rs2 = path.extension().is_some_and(|e| ["drk", "imd", "mc4"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    if !rs2 || path.starts_with(root) {
        return Ok(None);
    }
    let Ok(session) = crate::jobs::open_session(path) else { return Ok(None) };
    let folder = root.join(session.info.logger_id.map(|id| id.to_string()).unwrap_or_else(|| "racestudio2".into()));
    let (Some(dir), Some(stem), Some(name)) = (path.parent(), path.file_stem(), path.file_name()) else { return Ok(None) };
    let dir = if dir.as_os_str().is_empty() { std::path::Path::new(".") } else { dir };
    std::fs::create_dir_all(&folder)?;
    let mut copied = 0;
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        let companion = p.file_stem() == Some(stem) && p.extension().is_some_and(|e| ["drk", "imd", "gpk", "rrk", "mc4"].iter().any(|x| e.eq_ignore_ascii_case(x)));
        let to = folder.join(p.file_name().unwrap_or_default());
        if companion && !to.exists() {
            std::fs::copy(&p, &to)?;
            copied += 1;
        }
    }
    log::info!("added {} to the library ({copied} files copied)", path.display());
    Ok(Some(folder.join(name)))
}

/// A file "dropped" by MYCHRONIX_DROP (for testing).
#[derive(Debug)]
struct TestDrop(PathBuf);

impl egui::DroppedFile for TestDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

impl eframe::App for App {
    /// Quitting while joined to a MyChron: turn the other networks'
    /// auto-connect back on and go back to the previous network now, rather
    /// than leaving them paused until Mychronix next starts.
    fn on_exit(&mut self) {
        if crate::device::paused_networks() {
            log::info!("quitting while joined to a MyChron: restoring WiFi");
            crate::device::restore_wifi_on_quit();
        }
        log::info!("Mychronix quitting");
    }

    /// MYCHRONIX_CLICKS="frame:x,y;…" clicks at logical points on those frames;
    /// "frame:x,y:dy" scrolls the wheel there by dy points instead, and
    /// "frame:x,y>x2,y2" presses at one point and lets go at the other, as a
    /// hand does, and "frame:type:text" types text (for testing).
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.input_frames += 1;
        // Touch vs mouse: a touchscreen tap also moves the pointer, so remember
        // when we last saw a real touch (hover effects stand down for a while).
        let touched = raw.events.iter().any(|e| matches!(e, egui::Event::Touch { .. }));
        if touched {
            self.last_touch = Some(std::time::Instant::now());
            self.touch_mode = true;
        } else if self.touch_mode
            && raw.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_)))
            && self.last_touch.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(1))
        {
            // The mouse moved on its own: hover effects are back.
            self.touch_mode = false;
        }
        let Some(spec) = crate::hooks::get().clicks.as_deref() else { return };
        for item in spec.split(';') {
            let Some((f, xy)) = item.split_once(':') else { continue };
            if let Some(text) = xy.strip_prefix("type:") {
                if f.trim().parse::<u32>().ok() == Some(self.input_frames) {
                    raw.events.push(egui::Event::Text(text.to_string()));
                }
                continue;
            }
            let (xy, scroll) = match xy.split_once(':') {
                Some((xy, dy)) => (xy, dy.trim().parse::<f32>().ok()),
                None => (xy, None),
            };
            let (xy, to) = match xy.split_once('>') {
                Some((a, b)) => (a, b.split_once(',').and_then(|(x, y)| Some(pos2(x.trim().parse().ok()?, y.trim().parse().ok()?)))),
                None => (xy, None),
            };
            let (Ok(f), Some((x, y))) = (f.trim().parse::<u32>(), xy.split_once(',')) else { continue };
            let (Ok(x), Ok(y)) = (x.trim().parse::<f32>(), y.trim().parse::<f32>()) else { continue };
            let pos = pos2(x, y);
            let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            if let Some(dy) = scroll {
                match self.input_frames.checked_sub(f) {
                    Some(0) => raw.events.push(egui::Event::PointerMoved(pos)),
                    Some(1) => raw.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, dy),
                        phase: egui::TouchPhase::Move,
                        modifiers: Default::default(),
                    }),
                    _ => {}
                }
                continue;
            }
            if let Some(to) = to {
                match self.input_frames.checked_sub(f) {
                    Some(0) => raw.events.push(egui::Event::PointerMoved(pos)),
                    Some(1) => raw.events.push(button(true)),
                    Some(2..=4) => raw.events.push(egui::Event::PointerMoved(pos + (to - pos) * (self.input_frames - f - 1) as f32 / 3.0)),
                    Some(5) => raw.events.push(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }),
                    _ => {}
                }
                continue;
            }
            match self.input_frames.checked_sub(f) {
                Some(0) => raw.events.push(egui::Event::PointerMoved(pos)),
                Some(1) => raw.events.push(button(true)),
                Some(2) => raw.events.push(button(false)),
                _ => {}
            }
        }
        // MYCHRONIX_DROP="frame:path" drops that file or folder on the window then;
        // MYCHRONIX_DRAG="frame" holds a dragged file over it from then on (for testing).
        if let Some((f, path)) = &crate::hooks::get().drop && *f == self.input_frames {
            raw.dropped_files.push(std::sync::Arc::new(TestDrop(path.clone())));
        }
        if crate::hooks::get().drag.is_some_and(|f| self.input_frames >= f) {
            raw.hovered_files.push(egui::HoveredFile::default());
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // MYCHRONIX_DEBUG_SCALE=1 prints what egui thinks the display is (development only).
        if crate::hooks::get().debug_scale && self.frames % 60 == 5 {
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
        // Some desktops ignore maximize at creation; ask again once the window is up.
        if self.frames < 2 && self.screenshot_to.is_none() && !crate::hooks::get().windowed {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if self.screenshot_to.is_none() {
            self.frames = self.frames.saturating_add(1).min(10);
        }
        // Away from the Device page, still notice leaving the MyChron's network
        // and turn the other networks' auto-connect back on.
        let now = ui.ctx().input(|i| i.time);
        if self.page != Page::Device && now - self.device.wifi_check > 10.0 {
            self.device.wifi_check = now;
            if crate::device::paused_networks() {
                // Does nothing while the computer is still on the MyChron's network.
                crate::jobs::spawn("wifi-restore", crate::device::restore_after_crash);
            }
        }
        // Everything shown as an error or a device message also goes to the log, once.
        if self.error != self.logged_error {
            if let Some(e) = &self.error {
                log::warn!("shown: {e}");
            }
            self.logged_error = self.error.clone();
        }
        if self.device.message != self.device.logged_message {
            if let Some((ok, m)) = &self.device.message {
                if *ok { log::info!("device: {m}") } else { log::warn!("device: {m}") }
            }
            self.device.logged_message = self.device.message.clone();
        }
        // A damaged settings file was moved aside: say so once.
        let warnings = crate::storage::take_warnings();
        if !warnings.is_empty() {
            self.error = Some(warnings.join("\n"));
        }
        self.relap_poll(&ui.ctx().clone());
        let session = self.session.take();
        let window = session.as_ref().map(|s| self.window(s));

        self.status_bar(ui, session.as_deref());
        self.update_bar(ui);
        self.error_bar(ui);

        // The icon rail's space; the rail itself is drawn over the page (it slides out on hover).
        let rail_space = egui::Panel::left("rail")
            .exact_size(analysis_ui::RAIL_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(SIDEBAR))
            .show(ui, |_| {})
            .response
            .rect;

        egui::CentralPanel::default()
            .frame(Frame::new().fill(WINDOW).inner_margin(Margin { left: 28, right: 28, top: 22, bottom: 16 }))
            .show(ui, |ui| match (self.page, &session, window) {
                (Page::Analysis, Some(s), Some((start, end))) => self.analysis(ui, s, start, end),
                (Page::Analysis, _, _) => self.session_picker(ui),
                (Page::Garage, _, _) => self.garage_page(ui),
                (Page::Sessions, _, _) => self.sessions_page(ui),
                (Page::Events, _, _) => self.events_page(ui),
                (Page::Tracks, _, _) => self.tracks_page(ui),
                (Page::Device, _, _) => self.device_page(ui),
                (Page::Settings, _, _) => self.settings_page(ui),
            });

        self.rail(&ui.ctx().clone(), rail_space, session.as_deref());
        self.window_controls(&ui.ctx().clone());
        // MYCHRONIX_SENSOR_CHECK=<logger id> checks that logger's downloaded sessions
        // as if just downloaded (development only).
        if self.frames == 3 && let Some(id) = crate::hooks::get().sensor_check.clone() {
            let paths: Vec<PathBuf> = self.library.get_or_insert_with(Library::load).sessions.iter().filter(|s| s.logger == id).map(|s| s.path.clone()).collect();
            self.check_downloaded(paths, &id, &ui.ctx().clone());
        }
        self.key_poll(&ui.ctx().clone());
        self.sensor_dialog(&ui.ctx().clone());
        self.track_pick_dialog(&ui.ctx().clone());
        self.track_offer_dialog(&ui.ctx().clone());
        // After the pages and dialogs: a change they made starts reloading now.
        self.library_poll(&ui.ctx().clone());

        // Put the session back, unless the page opened another one this frame.
        if self.session.is_none() {
            self.session = session;
        }
        self.screenshot(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-cycle MyChron 4 run from logger 4242 (`.mc4`).
    fn mc4() -> Vec<u8> {
        let mut h = vec![0u8; 256];
        h[0x0E] = 0x02;
        h[0x76..0x78].copy_from_slice(&[0x59, 0x32]);
        h[0x78..0x7C].copy_from_slice(&4242u32.to_le_bytes());
        h[0x7C..0x7E].copy_from_slice(&106u16.to_le_bytes());
        h[0x9A..0xA1].copy_from_slice(&[0x00, 0x30, 0x10, 0x19, 0x05, 0x09, 0x26]);
        let mut lap = vec![0xFFu8; 64];
        lap[4..8].copy_from_slice(&100u32.to_le_bytes());
        let words: Vec<u16> = (0..22).map(|k| 5000 + k).collect();
        let data: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let mut f = b"MC4R".to_vec();
        f.extend(1u32.to_le_bytes());
        for part in [&h[..], &lap[..], &data[..]] {
            f.extend((part.len() as u32).to_le_bytes());
            f.extend(part);
        }
        f
    }

    #[test]
    fn importing_a_racestudio_session_brings_its_companions() {
        let base = std::env::temp_dir().join(format!("mychronix-import-{}", std::process::id()));
        let (outside, root) = (base.join("RaceStudio2"), base.join("sessions"));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("run 7.mc4"), mc4()).unwrap();
        std::fs::write(outside.join("run 7.GPK"), b"gps").unwrap();
        std::fs::write(outside.join("run 7.rrk"), b"raw").unwrap();
        std::fs::write(outside.join("run 8.rrk"), b"another run").unwrap();
        let copy = import_rs2_files(&outside.join("run 7.mc4"), &root).unwrap().unwrap();
        assert_eq!(copy, root.join("4242").join("run 7.mc4"));
        let mut got: Vec<String> = std::fs::read_dir(root.join("4242")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        got.sort();
        assert_eq!(got, ["run 7.GPK", "run 7.mc4", "run 7.rrk"]);
        // Already in the library: opened where it is.
        assert_eq!(import_rs2_files(&copy, &root).unwrap(), None);
        // Not a RaceStudio 2 file.
        assert_eq!(import_rs2_files(&outside.join("run 8.rrk"), &root).unwrap(), None);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
