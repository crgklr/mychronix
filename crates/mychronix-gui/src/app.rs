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
mod splits_ui;
mod tracks_ui;
mod device_page;
mod compare_ui;
mod window_ui;

pub(crate) use sessions_filter_ui::{session_driver, session_track};
pub(crate) use window_ui::OWN_CONTROLS as OWN_WINDOW_CONTROLS;

use compare_ui::gain_color;
use device_page::model_from_ssid;
use tracks_ui::SpeedLap;

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
    /// Laps being compared (the first is the reference), and the compare tab.
    compare: Vec<crate::compare::CompareLap>,
    analysis_view: AnalysisView,
    report: report_ui::ReportState,
    splits: splits_ui::SplitsState,
    corners: corners_ui::CornersState,
    sensors: sensor_ui::SensorState,
    key: datakey_ui::KeyState,
    /// The "Add a MyChron" window, while open.
    add_mychron: Option<device_ui::AddMyChron>,
    track_pick: Option<track_pick_ui::TrackPick>,
    /// When the last touchscreen touch was seen, and whether touch is in use.
    last_touch: Option<std::time::Instant>,
    touch_mode: bool,
    charts: charts_ui::ChartsState,
    /// Shared cursor on the compare tab, meters along the reference lap.
    compare_cursor_m: Option<f64>,
    /// The reference length the compare graphs last showed (as bits), to reset their range.
    compare_window: Option<u64>,
    /// Compare laps, or one driver's best against another's.
    compare_mode: compare_drivers_ui::CompareMode,
    driver_pick: compare_drivers_ui::DriverPick,
    /// Where the icon rail is (for opening it on hover), the session details and channel picker.
    rail_rect: Option<Rect>,
    /// The session panel (opened from the pill or the rail), and where the pill is.
    session_panel: bool,
    pill_rect: Option<Rect>,
    /// The Map view: corner or whole track, line colors, imagery, its own view of the map.
    map_view: map_ui::MapState,
    channel_picker_open: bool,
    /// Events page: saved events, selection, championship filter, dialog, peak EGT per session.
    events: Option<crate::events::Events>,
    event_sel: Option<String>,
    event_champ: Option<String>,
    event_dialog: Option<crate::events::EventDef>,
    peak_egt: std::collections::HashMap<PathBuf, Option<f64>>,
    peak_egt_rx: Option<std::sync::mpsc::Receiver<(PathBuf, Option<f64>)>>,
    peak_egt_asked: std::collections::HashSet<PathBuf>,
    /// Setup sheets: baselines, the editor, the open session's measured RPM per km/h,
    /// and a sheet to write once a download finishes.
    setup_baselines: Option<crate::setup::Baselines>,
    setup_edit: Option<setup_ui::SetupEdit>,
    /// The Garage's driver and kart dialogs.
    driver_edit: Option<garage_ui::DriverEdit>,
    kart_edit: Option<garage_ui::KartEdit>,
    /// Setting up a MyChron that's new to the Garage.
    mychron_setup: Option<garage_manage_ui::MyChronSetup>,
    /// Deleting a driver, or removing a kart or MyChron, waiting for "Are you sure?".
    garage_confirm: Option<garage_manage_ui::Confirm>,
    /// MyChrons put off with "Not now" while connected (asked again once reconnected).
    snoozed_while_on: std::collections::BTreeSet<String>,
    /// Which page of drivers the Garage shows (two drivers to a page).
    kart_page: usize,
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
    pending_setup: Option<(PathBuf, crate::setup::Setup)>,
    /// Channel that colors the track map (None: RPM if present).
    map_channel: Option<usize>,
    /// Color the Graphs map by where the open lap gains and loses time against the best lap.
    map_time_compare: bool,
    pub screenshot_to: Option<PathBuf>,
    frames: u32,

    // Device page
    dev: Option<crate::device::Worker>,
    dev_logger: Option<aim_link::Beacon>,
    dev_networks: Vec<(String, u8)>,
    dev_current_wifi: Option<String>,
    /// The AiM network Mychronix joined (so it can leave it).
    dev_joined: Option<String>,
    /// The connected MyChron is plugged in by USB rather than on WiFi.
    dev_usb: bool,
    /// Linux: an AiM USB device is plugged in that Mychronix may not open
    /// (checked now and then), and the answer to "Allow USB access" while it runs.
    usb_blocked: bool,
    usb_checked: f64,
    usb_allow: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// The last error and device message written to the log.
    logged_error: Option<String>,
    logged_dev_message: Option<(bool, String)>,
    /// Frames seen by the input hook (for scripted test clicks).
    input_frames: u32,
    /// The Garage: a session to open after the frame, and Events' plan dialog to open.
    pending_open: Option<PathBuf>,
    plan_on_open: bool,
    /// The Analysis session picker's driver filter and warm-up toggle.
    picker_logger: Option<String>,
    picker_warmups: bool,
    /// When the WiFi was last checked from another page.
    wifi_check: f64,
    dev_files: Vec<aim_link::DirEntry>,
    dev_summaries: Vec<aim_xrk::SessionSummary>,
    dev_selected: std::collections::BTreeSet<String>,
    /// What the worker is doing, shown while busy.
    dev_busy: Option<String>,
    dev_progress: Option<(usize, usize, String, u64, u64)>,
    dev_message: Option<(bool, String)>,
    dev_last_scan: f64,
    /// The Settings page's last word on connecting an AI assistant.
    mcp_message: Option<(bool, String)>,
    /// A scan is with the worker: don't queue another (one takes several seconds).
    dev_scanning: bool,
    /// The download dialog, while open.
    dev_dialog: Option<DownloadDialog>,

    // Sessions page
    profiles: Option<crate::profiles::Profiles>,
    /// Logger id to show, or all.
    /// The Sessions page's filters (right column).
    session_filter: sessions_filter_ui::SessionFilter,
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
        if let Some(k) = &kart {
            if blank(&session.info.vehicle) {
                session.info.vehicle = nonempty(&k.name);
            }
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
        self.compare.truncate(3);
        self.analysis_view = AnalysisView::Compare;
        self.compare_cursor_m = Some(120.0);
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
                self.library = None;
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
    /// "frame:x,y:dy" scrolls the wheel there by dy points instead (for testing).
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
        let Ok(spec) = std::env::var("MYCHRONIX_CLICKS") else { return };
        for item in spec.split(';') {
            let Some((f, xy)) = item.split_once(':') else { continue };
            let (xy, scroll) = match xy.split_once(':') {
                Some((xy, dy)) => (xy, dy.trim().parse::<f32>().ok()),
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
            match self.input_frames.checked_sub(f) {
                Some(0) => raw.events.push(egui::Event::PointerMoved(pos)),
                Some(1) => raw.events.push(button(true)),
                Some(2) => raw.events.push(button(false)),
                _ => {}
            }
        }
        // MYCHRONIX_DROP="frame:path" drops that file or folder on the window then;
        // MYCHRONIX_DRAG="frame" holds a dragged file over it from then on (for testing).
        if let Some((f, path)) = std::env::var("MYCHRONIX_DROP").ok().and_then(|v| v.split_once(':').map(|(f, p)| (f.parse::<u32>().ok(), p.to_string()))) {
            if f == Some(self.input_frames) {
                raw.dropped_files.push(std::sync::Arc::new(TestDrop(PathBuf::from(path))));
            }
        }
        if std::env::var("MYCHRONIX_DRAG").ok().and_then(|v| v.parse::<u32>().ok()).is_some_and(|f| self.input_frames >= f) {
            raw.hovered_files.push(egui::HoveredFile::default());
        }
    }

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
        // Some desktops ignore maximize at creation; ask again once the window is up.
        if self.frames < 2 && self.screenshot_to.is_none() && std::env::var_os("MYCHRONIX_WINDOWED").is_none() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        if self.screenshot_to.is_none() {
            self.frames = self.frames.saturating_add(1).min(10);
        }
        // Away from the Device page, still notice leaving the MyChron's network
        // and turn the other networks' auto-connect back on.
        let now = ui.ctx().input(|i| i.time);
        if self.page != Page::Device && now - self.wifi_check > 10.0 {
            self.wifi_check = now;
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
        if self.dev_message != self.logged_dev_message {
            if let Some((ok, m)) = &self.dev_message {
                if *ok { log::info!("device: {m}") } else { log::warn!("device: {m}") }
            }
            self.logged_dev_message = self.dev_message.clone();
        }
        // A damaged settings file was moved aside: say so once.
        let warnings = crate::storage::take_warnings();
        if !warnings.is_empty() {
            self.error = Some(warnings.join("\n"));
        }
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
        if self.frames == 3 {
            if let Ok(id) = std::env::var("MYCHRONIX_SENSOR_CHECK") {
                let paths: Vec<PathBuf> = self.library.get_or_insert_with(Library::load).sessions.iter().filter(|s| s.logger == id).map(|s| s.path.clone()).collect();
                self.check_downloaded(paths, &id, &ui.ctx().clone());
            }
        }
        self.key_poll(&ui.ctx().clone());
        self.sensor_dialog(&ui.ctx().clone());
        self.track_pick_dialog(&ui.ctx().clone());

        // Put the session back, unless the page opened another one this frame.
        if self.session.is_none() {
            self.session = session;
        }
        self.screenshot(ui);
    }
}

// ---------------------------------------------------------------------------
// Chrome: sidebar navigation, status bar, empty states

impl App {

    /// A thin bar at the top when a newer release is out: what's new, download, dismiss.
    fn update_bar(&mut self, ui: &mut Ui) {
        // Screenshot runs leave AiM's server alone unless MYCHRONIX_AIM_SYNC=1 (development only).
        let sync = self.screenshot_to.is_none() || std::env::var_os("MYCHRONIX_AIM_SYNC").is_some();
        if self.frames == 1 && self.aim_tracks_rx.is_none() && sync {
            self.aim_tracks_rx = Some(crate::aim_tracks::spawn(ui.ctx().clone()));
        }
        // MYCHRONIX_IMPORT=<file or folder> imports it at start (for testing; the file picker can't be clicked).
        static IMPORT_ONCE: std::sync::Once = std::sync::Once::new();
        if self.frames >= 3 && self.import_rx.is_none() && !IMPORT_ONCE.is_completed() {
            IMPORT_ONCE.call_once(|| {});
            if let Some(p) = std::env::var_os("MYCHRONIX_IMPORT") {
                let lib = self.library.get_or_insert_with(Library::load);
                self.import_rx = Some(crate::import::spawn(vec![PathBuf::from(p)], lib, ui.ctx().clone()));
                self.import_progress = Some((0, 0));
            }
        }
        self.import_poll();
        let (events, ended) = crate::jobs::drain(self.aim_tracks_rx.as_ref());
        // The sync ended without finishing (it crashed): drop its progress bar.
        if ended && self.aim_tracks_progress.is_some() && events.is_empty() {
            self.aim_tracks_progress = None;
        }
        for e in events {
            use crate::aim_tracks::Event;
            match e {
                Event::List => {
                    self.aim_tracks_first = true;
                    self.library = None;
                }
                Event::Progress { done, total } => self.aim_tracks_progress = (done < total).then_some((done, total)),
                Event::Failed { offline } => {
                    self.aim_tracks_progress = None;
                    self.aim_tracks_offline = offline;
                }
                Event::Finished { changed } => {
                    self.aim_tracks_progress = None;
                    self.aim_tracks_offline = false;
                    if changed > 0 {
                        self.library = None; // pick up the new tracks and outlines
                    }
                }
            }
        }
        if self.frames == 1 && self.update_rx.is_none() {
            self.update_rx = Some(crate::update::spawn(ui.ctx().clone()));
        }
        if let Some(r) = self.update_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.update = Some(r);
        }
        let Some(r) = self.update.clone() else { return };
        let mut dismiss = false;
        egui::Panel::top("update")
            .exact_size(38.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(SURFACE).inner_margin(Margin { left: 16, right: 16 + window_ui::CORNER_W as i8, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add(Icon::Check.image(theme::GOOD, 15.0));
                    ui.label(RichText::new(format!("Mychronix {} is available", r.version)).font(medium(13.5)).color(TEXT));
                    ui.label(RichText::new(format!("You have {}.", env!("CARGO_PKG_VERSION"))).size(12.5).color(DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let x = ui.add(Icon::X.image(DIM, 13.0).sense(Sense::click())).on_hover_text("Not now (you'll hear about the next one)");
                        if x.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if x.clicked() {
                            dismiss = true;
                        }
                        if chip(ui, "Download", false).on_hover_text("Open the release page in your browser").clicked() {
                            crate::update::open_url(&r.url);
                        }
                        if !r.notes.trim().is_empty() && chip(ui, "What's new", self.update_notes).clicked() {
                            self.update_notes = !self.update_notes;
                        }
                    });
                });
            });
        if dismiss {
            crate::update::dismiss(&r.version);
            self.update = None;
            self.update_notes = false;
            return;
        }
        if self.update_notes {
            let mut close = false;
            let closed = garage_manage_ui::look::dialog(ui.ctx(), "whats_new", garage_manage_ui::look::WIDTH, |ui| {
                use garage_manage_ui::look;
                look::header(ui, Icon::Download, theme::ACCENT, None, &format!("What's new in {}", r.version), "A newer Mychronix is ready to download.");
                egui::ScrollArea::vertical().max_height(420.0).id_salt("whats_new_body").show(ui, |ui| {
                    look::body(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        for line in r.notes.lines() {
                            let t = line.trim_start_matches('#').trim();
                            if line.starts_with('#') {
                                ui.add_space(6.0);
                                ui.label(RichText::new(t).font(semibold(17.0)).color(TEXT));
                            } else if !line.trim().is_empty() {
                                // "**Lead-in:** the rest", the lead-in in bold.
                                let mut job = egui::text::LayoutJob::default();
                                let mut add = |t: &str, f: egui::FontId, c: Color32| job.append(t, 0.0, egui::TextFormat { font_id: f, color: c, ..Default::default() });
                                match line.trim().strip_prefix("**").and_then(|r| r.split_once("**")) {
                                    Some((lead, rest)) => {
                                        add(lead, semibold(15.0), TEXT);
                                        add(&rest.replace("**", ""), regular(15.0), SECONDARY);
                                    }
                                    None => add(&line.replace("**", ""), regular(15.0), SECONDARY),
                                }
                                job.wrap.max_width = ui.available_width();
                                ui.label(job);
                            }
                        }
                    });
                });
                let (l, d) = look::footer(ui, "Not now", ("Download", Some(Icon::Download), look::Kind::Primary, true));
                if d {
                    crate::update::open_url(&r.url);
                }
                close = l;
            });
            self.update_notes = !closed && !close;
        }
    }

    /// Something went wrong (a save failed, a file couldn't be opened): a bar
    /// across the top that stays until dismissed. Also in the log.
    fn error_bar(&mut self, ui: &mut Ui) {
        let Some(err) = self.error.clone() else { return };
        let mut dismiss = false;
        egui::Panel::top("error")
            .show_separator_line(false)
            .frame(Frame::new().fill(theme::DANGER.gamma_multiply(0.16)).inner_margin(Margin { left: 16, right: 16 + window_ui::CORNER_W as i8, top: 9, bottom: 9 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add(Icon::TriangleAlert.image(theme::DANGER, 15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let x = ui.add(Icon::X.image(SECONDARY, 13.0).sense(Sense::click())).on_hover_text("Dismiss");
                        if x.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        dismiss = x.clicked();
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(err).font(medium(13.5)).color(TEXT)).wrap());
                        });
                    });
                });
            });
        if dismiss {
            self.error = None;
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
                            ui.add(if self.dev_usb { Icon::Usb } else { Icon::Wifi }.image(theme::GOOD, 14.0));
                            let id = b.logger_id.map(|i| format!("logger {i}")).unwrap_or_else(|| "logger".into());
                            ui.label(RichText::new(format!("MyChron connected  ·  {id}")).size(12.0).color(SECONDARY));
                        }
                        None if self.key.present => {
                            ui.add(Icon::Usb.image(theme::GOOD, 14.0));
                            let text = match (&self.key.busy, &self.key.listing) {
                                (Some(b), _) => b.clone(),
                                (None, Some((serial, _, new))) if !new.is_empty() => format!("MyChron 4 data key plugged in  ·  logger {serial}  ·  {} new run{}", new.len(), if new.len() == 1 { "" } else { "s" }),
                                (None, Some((serial, ..))) => format!("MyChron 4 data key plugged in  ·  logger {serial}  ·  everything downloaded"),
                                (None, None) => "MyChron 4 data key plugged in".into(),
                            };
                            ui.label(RichText::new(text).size(12.0).color(SECONDARY));
                        }
                        None => {
                            ui.add(Icon::Wifi.image(DIM, 14.0));
                            ui.label(RichText::new("No MyChron connected").size(12.0).color(DIM));
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let (Some(s), Some(path)) = (session, &self.path) {
                            let samples: usize = s.channels.iter().map(|c| c.values.len()).sum();
                            ui.label(RichText::new(format!("{} channels  ·  {} samples", s.channels.len(), group_thousands(samples))).size(12.0).color(DIM));
                            ui.add_space(10.0);
                            ui.label(RichText::new(file_name(path)).size(12.0).color(DIM));
                        }
                        if let Some((done, total)) = self.import_progress.filter(|p| p.1 > 0) {
                            ui.add_space(10.0);
                            ui.label(RichText::new(format!("{} of {}", group_thousands(done), group_thousands(total))).size(12.0).color(SECONDARY));
                            progress_bar(ui, 180.0, 6.0, done as f32 / total.max(1) as f32);
                            ui.label(RichText::new("Importing sessions").font(medium(12.5)).color(TEXT));
                            ui.add(Icon::FolderOpen.image(TEXT, 14.0));
                        }
                        // Right to left: count, bar, label, icon.
                        if let Some((done, total)) = self.aim_tracks_progress {
                            ui.add_space(10.0);
                            ui.label(RichText::new(format!("{} of {}", group_thousands(done), group_thousands(total))).size(12.0).color(SECONDARY));
                            progress_bar(ui, 180.0, 6.0, done as f32 / total.max(1) as f32);
                            let what = if self.aim_tracks_first { "Downloading AiM's tracks" } else { "Updating AiM's tracks" };
                            ui.label(RichText::new(what).font(medium(12.5)).color(TEXT));
                            ui.add(Icon::Download.image(TEXT, 14.0));
                        }
                    });
                });
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
        let profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let mut shown_rows: Vec<usize> = Vec::new();
        let loggers: Vec<String> = {
            let mut l: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
            l.sort();
            l.dedup();
            l
        };
        let driver_of = |i: usize| sessions_filter_ui::session_driver(&lib.sessions[i], &profiles);
        // The newest session's day, for "Last race day".
        let newest = lib.sessions.iter().filter_map(|s| s.summary.date).max().and_then(|d| jiff::civil::Date::new(d.year as i16, d.month as i8, d.day as i8).ok());

        let setups = crate::setup::resolve(&lib, self.setup_baselines.get_or_insert_with(crate::setup::Baselines::load));
        let raced: Vec<usize> = {
            let mut v: Vec<usize> = lib.sessions.iter().filter_map(|s| s.track).collect();
            v.sort();
            v.dedup();
            v
        };
        let mut import = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Sessions").font(semibold(28.0)).color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.import_progress.is_some();
                if button(ui, Some(Icon::FolderOpen), if busy { "Importing…" } else { "Import a folder…" }, false).clicked() && !busy {
                    import = true;
                }
            });
        });
        if import {
            self.import_dialog(true, &ui.ctx().clone());
        }
        // Session files or folders dropped on the window are imported too.
        self.import_drops(&ui.ctx().clone());
        let matching = (0..lib.sessions.len()).filter(|&i| self.session_filter.keeps(&lib, i, &profiles, newest)).count();
        let count = if matching < lib.sessions.len() { format!("Showing {matching} of {} sessions", lib.sessions.len()) } else { format!("{} sessions", lib.sessions.len()) };
        ui.label(
            RichText::new(format!(
                "{count}  ·  {} track{}  ·  {} MyChron{}",
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
        let side_w = 360.0_f32.min(ui.available_width() * 0.32);
        let list_w = ui.available_width() - side_w - gap;
        let mut open = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(list_w, height), column, |ui| {
                ui.set_width(list_w);
                // Filters are in the right column; search stays here.
                search_field(ui, &mut self.session_search, "Search driver, kart, championship, track, comment…");
                ui.add_space(10.0);

                let needle = self.session_search.to_lowercase();
                let mut shown: Vec<usize> = (0..lib.sessions.len())
                    .filter(|&i| {
                        let s = &lib.sessions[i];
                        self.session_filter.keeps(&lib, i, &profiles, newest)
                            && (needle.is_empty() || {
                                let sum = &s.summary;
                                let m = s.meta.clone().unwrap_or_default();
                                let hay = format!(
                                    "{} {} {} {} {} {} {} {} {} {} {}",
                                    sum.number.unwrap_or(0),
                                    sum.date.map(|d| format!("{d} {}", crate::events::us_short(d))).unwrap_or_default(),
                                    sum.track.clone().unwrap_or_default(),
                                    s.track.map(|t| lib.tracks[t].full_name.clone().unwrap_or_default()).unwrap_or_default(),
                                    driver_of(i),
                                    m.kart,
                                    m.championship,
                                    m.session_type,
                                    m.comment,
                                    s.logger,
                                    sum.driver.clone().unwrap_or_default(),
                                )
                                .to_lowercase();
                                // Every word must match somewhere, as RaceStudio's "ALL words".
                                needle.split_whitespace().all(|w| hay.contains(w))
                            })
                    })
                    .collect();
                // Newest first.
                shown.sort_by_key(|&i| {
                    let s = &lib.sessions[i].summary;
                    std::cmp::Reverse((s.date, s.start_time.clone(), s.number))
                });
                let extras = self.row_extras(&lib, &shown, &ui.ctx().clone());
                shown_rows = shown.clone();

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
                        // The tracks raced that day, as assigned (else as recorded).
                        let mut tracks: Vec<String> = day.iter().filter_map(|&i| sessions_filter_ui::session_track(&lib, i)).collect();
                        tracks.sort();
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
                            let driver = driver_of(i);
                            if session_row_with(ui, &lib, i, &driver, best, !setups[i].changes.is_empty(), extras.get(&i)).clicked() {
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

            // Filters: when, drivers, tracks.
            ui.allocate_ui_with_layout(vec2(side_w, height), column, |ui| {
                ui.set_width(side_w);
                egui::ScrollArea::vertical().id_salt("sessions_filters").auto_shrink([false, false]).show(ui, |ui| {
                    self.sessions_filter_panel(ui, &lib, &profiles, newest);
                });
            });
        });
        // MYCHRONIX_CLICK_SESSION=<n> clicks session n once (for testing).
        if open.is_none() && self.frames == 5 {
            if let Some(n) = std::env::var("MYCHRONIX_CLICK_SESSION").ok().and_then(|v| v.parse::<u32>().ok()) {
                open = lib.sessions.iter().find(|s| s.summary.number == Some(n)).map(|s| s.path.clone());
            }
        }
        self.library = Some(lib);
        self.profiles = Some(profiles);
        self.want_weather(&shown_rows, &ui.ctx().clone());
        if let Some(path) = open {
            self.load(path);
        }
    }
}

/// What a session list knows about a session beyond its summary: its racing
/// laps (for the sparkline) and badges.
#[derive(Default)]
pub(super) struct RowExtras {
    pub laps: Vec<(i32, i64)>,
    /// (label, color, explanation).
    pub badges: Vec<(String, Color32, String)>,
}

impl App {
    /// Sparkline laps and badges for each of `rows` (library indices); digests
    /// not yet known are asked for in the background.
    pub(super) fn row_extras(&mut self, lib: &Library, rows: &[usize], ctx: &egui::Context) -> std::collections::HashMap<usize, RowExtras> {
        self.want_digests(rows.iter().map(|&i| lib.sessions[i].path.clone()), ctx);
        let digests = self.sensors.digests.get_or_insert_with(crate::digest::Digests::load);
        let ignored = self.sensors.ignored.get_or_insert_with(crate::sensor_check::Ignored::load);
        // Personal best: the best lap at a track from this MyChron, among two or more sessions there.
        let mut best_at: std::collections::HashMap<(String, String), (u64, usize)> = std::collections::HashMap::new();
        for s in &lib.sessions {
            if let (Some(b), Some(t)) = (s.summary.best_lap_ms, s.summary.track.clone()) {
                let e = best_at.entry((s.logger.clone(), t)).or_insert((u64::MAX, 0));
                e.0 = e.0.min(b);
                e.1 += 1;
            }
        }
        rows.iter()
            .map(|&i| {
                let s = &lib.sessions[i];
                let sum = &s.summary;
                let mut x = RowExtras::default();
                if let Some(d) = digests.get(&s.path) {
                    x.laps = d.laps.clone();
                    let issues: Vec<String> = digests.reportable(d).into_iter().filter(|i| !ignored.contains(&d.logger, &i.channel)).map(|i| i.channel).collect();
                    if !issues.is_empty() {
                        x.badges.push((format!("⚠ {}", issues.join(", ")), theme::FAVORITE, "A sensor didn't record properly; open the session for what to check".into()));
                    }
                }
                let pb = sum.best_lap_ms.zip(sum.track.clone()).is_some_and(|(b, t)| best_at.get(&(s.logger.clone(), t)).is_some_and(|&(m, n)| n > 1 && b == m));
                if pb {
                    x.badges.insert(0, ("PB".into(), theme::GOOD, "Personal best: the fastest lap from this MyChron at this track".into()));
                }
                if crate::events::is_warm_up(sum) {
                    x.badges.push(("Warm-up".into(), DIM, "The kart never left the stand".into()));
                } else if sum.laps.unwrap_or(0) <= 1 {
                    x.badges.push(("No laps timed".into(), DIM, "Driven without crossing a start/finish line, e.g. practice in a parking lot".into()));
                }
                (i, x)
            })
            .collect()
    }
}

/// A lap-time sparkline in `rect`: one point per racing lap, slower laps
/// higher and faster ones lower (the fastest, at the bottom, dotted green).
fn sparkline(p: &egui::Painter, rect: Rect, laps: &[(i32, i64)]) {
    if laps.len() < 3 {
        return;
    }
    let (lo, hi) = laps.iter().fold((i64::MAX, i64::MIN), |a, l| (a.0.min(l.1), a.1.max(l.1)));
    let span = (hi - lo).max(1) as f32;
    let pts: Vec<egui::Pos2> = laps
        .iter()
        .enumerate()
        .map(|(k, l)| pos2(rect.left() + rect.width() * k as f32 / (laps.len() - 1) as f32, rect.bottom() - rect.height() * (l.1 - lo) as f32 / span))
        .collect();
    p.add(egui::Shape::line(pts.clone(), Stroke::new(1.5, SECONDARY)));
    if let Some(k) = laps.iter().position(|l| l.1 == lo) {
        p.circle_filled(pts[k], 2.5, theme::GOOD);
    }
}

/// One session in the list: number, time, driver; laps and best lap on the right.
fn session_row(ui: &mut Ui, lib: &Library, i: usize, driver: &str, day_best: Option<u64>, setup_changed: bool) -> egui::Response {
    session_row_with(ui, lib, i, driver, day_best, setup_changed, None)
}

/// [`session_row`] with a sparkline and badges.
fn session_row_with(ui: &mut Ui, lib: &Library, i: usize, driver: &str, day_best: Option<u64>, setup_changed: bool, extras: Option<&RowExtras>) -> egui::Response {
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
        let title = p.text(pos2(l, top), egui::Align2::LEFT_CENTER, format!("Session {}", sum.number.unwrap_or(0)), medium(14.5), TEXT);
        // Badges after the title.
        let mut badges: Vec<(String, Color32, String)> = Vec::new();
        if setup_changed {
            badges.push(("Setup changed".into(), theme::FAVORITE, "This session's setup sheet changed something".into()));
        }
        if let Some(x) = extras {
            badges.extend(x.badges.iter().cloned());
        }
        let mut bx = title.right() + 10.0;
        let mut tips = Vec::new();
        for (text, color, tip) in &badges {
            let galley = p.layout_no_wrap(text.clone(), medium(11.5), *color);
            let tag = Rect::from_min_size(pos2(bx, top - galley.size().y / 2.0 - 3.0), galley.size() + vec2(14.0, 6.0));
            p.rect_filled(tag, CornerRadius::same(255), color.gamma_multiply(0.16));
            p.galley(tag.min + vec2(7.0, 3.0), galley, *color);
            tips.push((tag, tip.clone()));
            bx = tag.right() + 6.0;
        }
        if let Some(pos) = response.hover_pos() {
            if let Some((_, tip)) = tips.iter().find(|(r, _)| r.contains(pos)) {
                egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), response.id.with("badge"), egui::PopupAnchor::Pointer).show(|ui| {
                    ui.label(tip);
                });
            }
        }
        // Lap times, first to last, left of the numbers.
        if let Some(x) = extras {
            let w = 110.0;
            let spark = Rect::from_min_size(pos2(r - 300.0 - w, rect.top() + 12.0), vec2(w, rect.height() - 24.0));
            if spark.left() > bx + 20.0 {
                sparkline(p, spark, &x.laps);
            }
        }
        let track = s.track.map(|t| lib.tracks[t].name.clone()).or(sum.track.clone()).unwrap_or_else(|| "No track".into());
        let championship = s.meta.as_ref().map(|m| m.championship.clone()).filter(|c| !c.is_empty());
        let line = [sum.start_time.as_deref().map(crate::events::clock_time), Some(driver.to_string()), Some(track), championship]
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
        let mut facts = vec![plural(laps as usize, "lap")];
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

/// Channels Mychronix works out rather than the logger measuring them.
pub(crate) fn is_calculated(name: &str) -> bool {
    name == crate::setup::SPEED_FROM_RPM
}

/// The yellow note under a chart that uses a calculated channel.
pub(super) fn calculated_note(ui: &mut Ui) {
    ui.label(
        RichText::new("Calculated from RPM, gearing and tire size, so it isn't precise (tire wear and clutch slip aren't known).")
            .size(12.0)
            .color(theme::FAVORITE),
    );
}

/// `session`'s channel `name`, or its equivalent under another name (see `report::channel`).
fn same_channel<'a>(session: &'a Session, name: &str) -> Option<&'a aim_xrk::Channel> {
    crate::report::channel(session, name)
}

/// "Saturday, October 10, 2026".
fn long_date(d: aim_xrk::summary::Date) -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    // Sakamoto's day-of-week method.
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if d.month < 3 { d.year as i32 - 1 } else { d.year as i32 };
    let dow = (y + y / 4 - y / 100 + y / 400 + t[d.month as usize - 1] + d.day as i32).rem_euclid(7) as usize;
    format!("{}, {} {}, {}", DAYS[dow], MONTHS[d.month as usize - 1], d.day, d.year)
}

// ---------------------------------------------------------------------------
// Sidebar: laps and channels

impl App {

    fn channel_list(&mut self, ui: &mut Ui, session: &Session, start_ms: i64) {
        if !self.collapsed_init {
            self.collapsed_init = true;
            self.collapsed_groups.insert("Lap timing");
            self.collapsed_groups.insert("More channels");
            // MYCHRONIX_CHANNEL_SEARCH pre-fills the search (for screenshots).
            if let Ok(q) = std::env::var("MYCHRONIX_CHANNEL_SEARCH") {
                self.filter = q;
            }
        }
        let live = session.channels.iter().filter(|c| !is_dead(c)).count();
        section_header(ui, "Channels", &format!("{} graphed · {live}", self.selected.len()));
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
            let mut toggle_group = None;
            for group in CHANNEL_GROUPS {
                let members: Vec<usize> = session
                    .channels
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| channel_group(c, session.info.imu_mount.is_some()) == group && !is_dead(c))
                    .filter(|(_, c)| needle.is_empty() || c.name.to_lowercase().contains(&needle))
                    .filter(|(i, _)| self.channel_view != ChannelView::Graphed || self.selected.contains_key(i))
                    .map(|(i, _)| i)
                    .collect();
                if members.is_empty() {
                    continue;
                }
                // Searching or showing graphed channels opens every group.
                let open = !needle.is_empty() || self.channel_view == ChannelView::Graphed || !self.collapsed_groups.contains(group);
                let graphed = members.iter().filter(|i| self.selected.contains_key(i)).count();
                let header = ui.horizontal(|ui| {
                    ui.add((if open { Icon::ChevronDown } else { Icon::ChevronRight }).image(DIM, 13.0));
                    ui.label(RichText::new(group).font(medium(13.0)).color(SECONDARY));
                    let detail = if graphed > 0 { format!("{graphed} of {} graphed", members.len()) } else { members.len().to_string() };
                    ui.label(RichText::new(detail).size(12.0).color(DIM));
                });
                let header = ui.interact(header.response.rect, ui.id().with(("group", group)), Sense::click());
                if header.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if header.clicked() {
                    toggle_group = Some(group);
                }
                if !open {
                    ui.add_space(4.0);
                    continue;
                }
                for i in members {
                    let c = &session.channels[i];
                    let slot = self.selected.get(&i).copied();
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
                            if let Some(v) = cursor_ms.and_then(|t| sample_at(c, t)) {
                                ui.label(RichText::new(format_value(v, c.decimals)).font(medium(13.5)).color(TEXT));
                            }
                        });
                    });
                    let response = response.on_hover_text(if slot.is_some() { "Click to remove its graph" } else { "Click to graph" });
                    if response.clicked() {
                        if slot.is_some() {
                            self.selected.remove(&i);
                            self.graph_order.retain(|&c| c != i);
                        } else if !full {
                            self.select(i);
                            // Show the new graph, which goes at the bottom of the stack.
                            self.scroll_to_graph = Some(i);
                        }
                    }
                }
                ui.add_space(6.0);
            }
            if let Some(g) = toggle_group {
                if !self.collapsed_groups.remove(g) {
                    self.collapsed_groups.insert(g);
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Analysis page

impl App {
    fn analysis(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>, start_ms: i64, end_ms: i64) {
        // A view that can't work for this session (no GPS) falls back to Graphs.
        if analysis_ui::view_unavailable(session, self.analysis_view.rail_item()).is_some() {
            self.analysis_view = AnalysisView::Session;
        }
        let pill = self.session_pill(ui, session);
        self.pill_rect = Some(pill);
        if let Some(i) = self.path.as_ref().and_then(|p| self.library.as_ref()?.sessions.iter().position(|s| &s.path == p)) {
            self.want_weather(&[i], &ui.ctx().clone());
        }
        ui.add_space(14.0);
        let title = match self.analysis_view {
            AnalysisView::Report => "Laps",
            AnalysisView::Splits => "Splits",
            AnalysisView::Corners => "Corners",
            AnalysisView::Map => "Map",
            AnalysisView::Compare => "Compare",
            AnalysisView::Session | AnalysisView::Histogram | AnalysisView::Scatter => "Graphs",
        };
        // The map takes the whole view; everything else gets a title.
        if self.analysis_view != AnalysisView::Map {
            self.view_title(ui, title);
            ui.add_space(12.0);
        }
        self.analysis_view_body(ui, session, start_ms, end_ms);
        self.session_panel(&ui.ctx().clone(), session);
        self.setup_dialog(ui.ctx());
    }

    fn analysis_view_body(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>, start_ms: i64, end_ms: i64) {
        match self.analysis_view {
            AnalysisView::Map => {
                self.map_view(ui, session);
                return;
            }
            AnalysisView::Compare => {
                self.compare_view(ui, session);
                return;
            }
            AnalysisView::Report => {
                self.report_view(ui, session);
                return;
            }
            AnalysisView::Corners => {
                self.corners_view(ui, session);
                return;
            }
            AnalysisView::Splits => {
                self.splits_view(ui, session);
                return;
            }
            AnalysisView::Histogram => {
                self.histogram_view(ui, session);
                return;
            }
            AnalysisView::Scatter => {
                self.scatter_view(ui, session);
                return;
            }
            AnalysisView::Session => {}
        }
        self.lap_strip(ui, session, false);
        ui.add_space(12.0);

        let gap = 12.0;
        let height = ui.available_height();
        let map_w = if session.gps.is_some() { (ui.available_width() * 0.4).clamp(280.0, 560.0) } else { 0.0 };
        let graphs_w = ui.available_width() - if map_w > 0.0 { map_w + gap } else { 0.0 };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(graphs_w, height), column, |ui| {
                ui.set_width(graphs_w);
                // Leave room for "+ Add channel" under the graphs.
                let graphs_h = ui.available_height() - 44.0;
                ui.allocate_ui_with_layout(vec2(graphs_w, graphs_h), column, |ui| self.graphs(ui, session, start_ms, end_ms));
                ui.add_space(8.0);
                self.channel_picker(ui, session, start_ms);
            });
            if map_w > 0.0 {
                ui.allocate_ui_with_layout(vec2(map_w, height), column, |ui| {
                    ui.set_width(map_w);
                    self.track_map(ui, session, start_ms, end_ms)
                });
            }
        });
    }


    fn graphs(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        if self.selected.is_empty() {
            card(ui, |ui| {
                ui.set_min_height(180.0);
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Add a channel below to graph it.").color(SECONDARY));
                });
            });
            return;
        }

        // In the order they were added; newest at the bottom.
        let selected: Vec<(usize, usize)> = self.graph_order.iter().filter_map(|c| self.selected.get(c).map(|&s| (*c, s))).collect();
        let gap = 10.0;
        // RaceStudio's Time Compare: time gained or lost against the best lap,
        // along this lap (shown when a lap other than the best is open).
        let reference = self.time_compare_reference(session, start_ms);
        let compare_h = 120.0;
        let n = selected.len() as f32;
        let reserved = if reference.is_some() { compare_h + gap } else { 0.0 };
        let card_h = ((ui.available_height() - reserved - gap * (n - 1.0)) / n).max(110.0);
        let scroll_to = self.scroll_to_graph.take();
        let mut remove = None;
        // Distance mode: meters along the lap from GPS speed (time mode without GPS).
        let profile = if self.graph_distance {
            crate::compare::DistanceProfile::from_gps(session, &aim_xrk::Lap { number: 0, start_ms, end_ms })
        } else {
            None
        };
        let to_x = |t_ms: f64| match &profile {
            Some(p) => p.dist_at(t_ms),
            None => (t_ms - start_ms as f64) / 1000.0,
        };
        let x_max = profile.as_ref().map(|p| p.length()).unwrap_or((end_ms - start_ms) as f64 / 1000.0).max(0.001);
        // Seconds behind the best lap at each point of this one, on this graph's x axis.
        let time_compare: Option<(aim_xrk::Lap, Vec<[f64; 2]>)> = reference.and_then(|best| {
            let points = time_compare_curve(session, best, start_ms, end_ms)?
                .into_iter()
                .map(|(t_ms, d, gap_s)| [if profile.is_some() { d } else { (t_ms - start_ms as f64) / 1000.0 }, gap_s])
                .collect();
            Some((best, points))
        });
        // A new lap (or the whole session, or axis) starts the graphs at its full length.
        let key = (start_ms, end_ms, profile.is_some());
        let window_changed = self.graph_window != Some(key);
        self.graph_window = Some(key);
        let unit = if profile.is_some() { "m" } else { "s" };
        let link = ui.id().with("time");
        let cursor_ms = self.cursor_s.map(|s| start_ms + (s * 1000.0) as i64);
        let mut hovered = None;

        egui::ScrollArea::vertical().id_salt("graphs").show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = gap;
            for (k, &(i, slot)) in selected.iter().enumerate() {
                let c = &session.channels[i];
                let color = SERIES[slot];
                let last = k + 1 == selected.len() && time_compare.is_none();
                let top = ui.cursor().top();
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
                            let x = ui.add(Icon::X.image(DIM, 13.0).sense(Sense::click())).on_hover_text("Remove this graph");
                            if x.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if x.clicked() {
                                remove = Some(i);
                            }
                            ui.add_space(8.0);
                            // The value at the cursor, big, with its units ("6889 rpm").
                            if let Some(v) = cursor_ms.and_then(|t| sample_at(c, t)) {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                if !units.is_empty() {
                                    ui.label(RichText::new(units).font(medium(16.0)).color(SECONDARY));
                                }
                                ui.label(RichText::new(format_value(v, c.decimals)).font(semibold(26.0)).color(TEXT));
                            }
                        });
                    });
                    let mut points = decimate(c, start_ms, end_ms, MAX_POINTS);
                    if profile.is_some() {
                        for p in &mut points {
                            p[0] = to_x(start_ms as f64 + p[0] * 1000.0);
                        }
                    }
                    let mut plot = Plot::new(("channel", i));
                    if window_changed {
                        plot = plot.reset();
                    }
                    let calculated = is_calculated(&c.name);
                    let response = plot
                        .height(ui.available_height() - if calculated { 20.0 } else { 0.0 })
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
                        // Every graph spans the whole lap, even a channel with few samples. A fixed
                        // range (not auto bounds): linked auto bounds feed each other's margins and grow.
                        .auto_bounds([false, true])
                        .default_x_bounds(0.0, x_max)
                        .x_axis_formatter(move |mark, _| if mark.step_size < 1.0 { format!("{:.1} {unit}", mark.value) } else { format!("{:.0} {unit}", mark.value) })
                        .show(ui, |plot| {
                            plot.line(Line::new(c.name.clone(), PlotPoints::from(points)).color(color).width(2.0));
                            if let Some(s) = self.cursor_s {
                                plot.vline(VLine::new("cursor", to_x(start_ms as f64 + s * 1000.0)).color(SECONDARY).width(1.0));
                            }
                            plot.pointer_coordinate()
                        });
                    if calculated {
                        calculated_note(ui);
                    }
                    // The cursor is kept as time, so the map and readouts follow it either way.
                    if let Some(p) = response.inner {
                        hovered = Some(match &profile {
                            Some(pr) => ((pr.time_at(p.x.clamp(0.0, x_max)) - start_ms as f64) / 1000.0).max(0.0),
                            None => p.x.max(0.0),
                        });
                    }
                });
                if scroll_to == Some(i) {
                    let rect = Rect::from_min_max(pos2(ui.min_rect().left(), top), pos2(ui.min_rect().right(), ui.cursor().top()));
                    ui.scroll_to_rect(rect, Some(Align::Max));
                }
            }
            if let Some((best, points)) = &time_compare {
                graph_card(ui, compare_h, |ui| {
                    // The gap at the cursor: above zero, behind the best lap there.
                    let at_cursor = self.cursor_s.and_then(|s| {
                        let x = to_x(start_ms as f64 + s * 1000.0);
                        let i = points.partition_point(|p| p[0] < x);
                        points.get(i.min(points.len().saturating_sub(1))).map(|p| p[1])
                    });
                    ui.horizontal(|ui| {
                        ui.add(Icon::Timer.image(DIM, 14.0));
                        ui.label(RichText::new("Time compare").font(medium(14.5)).color(TEXT));
                        let n = session.laps.iter().position(|l| *l == *best).map(|i| i + 1).unwrap_or(0);
                        ui.label(RichText::new(format!("vs best lap {n}  ·  {}", format_lap_time(best.end_ms - best.start_ms))).size(13.0).color(DIM))
                            .on_hover_text("Above the line: behind the best lap at that point. Below: ahead of it.");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(g) = at_cursor {
                                let (text, color) = if g > 0.0005 {
                                    (format!("+{g:.3} s"), theme::FAVORITE)
                                } else if g < -0.0005 {
                                    (format!("−{:.3} s", -g), theme::GOOD)
                                } else {
                                    ("0.000 s".to_string(), TEXT)
                                };
                                ui.label(RichText::new(text).font(semibold(16.0)).color(color));
                            }
                        });
                    });
                    let mut plot = Plot::new("time_compare");
                    if window_changed {
                        plot = plot.reset();
                    }
                    let response = plot
                        .height(ui.available_height())
                        .show_background(false)
                        .grid_color(GRID)
                        .show_axes([true, true])
                        .y_axis_min_width(46.0)
                        .show_x(false)
                        .show_y(false)
                        .cursor_color(DIM)
                        .link_axis(link, [true, false])
                        .link_cursor(link, [true, false])
                        .allow_zoom([true, false])
                        .allow_drag([true, false])
                        .allow_scroll(false)
                        .auto_bounds([false, true])
                        .default_x_bounds(0.0, x_max)
                        .include_y(0.0)
                        .x_axis_formatter(move |mark, _| if mark.step_size < 1.0 { format!("{:.1} {unit}", mark.value) } else { format!("{:.0} {unit}", mark.value) })
                        .y_axis_formatter(|mark, _| format!("{:+.1} s", mark.value))
                        .show(ui, |plot| {
                            plot.hline(egui_plot::HLine::new("even", 0.0).color(DIM).width(1.0));
                            plot.line(Line::new("Time compare", PlotPoints::from(points.clone())).color(SECONDARY).width(2.0));
                            if let Some(s) = self.cursor_s {
                                plot.vline(VLine::new("cursor", to_x(start_ms as f64 + s * 1000.0)).color(SECONDARY).width(1.0));
                            }
                            plot.pointer_coordinate()
                        });
                    if let Some(p) = response.inner {
                        hovered = Some(match &profile {
                            Some(pr) => ((pr.time_at(p.x.clamp(0.0, x_max)) - start_ms as f64) / 1000.0).max(0.0),
                            None => p.x.max(0.0),
                        });
                    }
                });
            }
        });
        if hovered.is_some() {
            self.cursor_s = hovered;
        }
        if let Some(i) = remove {
            self.selected.remove(&i);
            self.graph_order.retain(|&c| c != i);
        }
    }
}

/// Along the lap from `start_ms` to `end_ms`: (logger time ms, meters into
/// the lap, seconds behind `best` there).
fn time_compare_curve(session: &Session, best: aim_xrk::Lap, start_ms: i64, end_ms: i64) -> Option<Vec<(f64, f64, f64)>> {
    let lap = crate::compare::DistanceProfile::from_gps(session, &aim_xrk::Lap { number: 0, start_ms, end_ms })?;
    let reference = crate::compare::DistanceProfile::from_gps(session, &best)?;
    Some(crate::compare::gap_along(&lap, &reference, 600))
}

impl App {
    /// The best lap, when the open lap is another one (what Time Compare measures against).
    fn time_compare_reference(&self, session: &Session, start_ms: i64) -> Option<aim_xrk::Lap> {
        let best = session.best_lap(MIN_LAP_MS).copied();
        self.lap
            .and_then(|i| session.laps.get(i))
            .filter(|l| l.start_ms == start_ms && Some(**l) != best)
            .and(best)
            .filter(|b| b.end_ms - b.start_ms >= MIN_LAP_MS)
    }

    /// The GPS path for the current window, colored by a channel (or by where
    /// time is gained and lost against the best lap), with a dot at the shared
    /// cursor. Hovering the map moves the cursor.
    fn track_map(&mut self, ui: &mut Ui, session: &Session, start_ms: i64, end_ms: i64) {
        let Some(gps) = &session.gps else { return };
        let lo = gps.times_ms.partition_point(|&t| t < start_ms);
        let hi = gps.times_ms.partition_point(|&t| t <= end_ms);
        // RPM by default, or speed when the session has no RPM recorded.
        let default = ["RPM", "GPS Speed"].iter().find_map(|n| session.channels.iter().position(|c| c.name == *n && !is_dead(c)));
        let color_ch = self.map_channel.filter(|&i| session.channels.get(i).is_some_and(|c| !is_dead(c))).or(default).or_else(|| self.selected.keys().next().copied());
        let cursor_ms = self.cursor_s.map(|s| start_ms + (s * 1000.0) as i64);
        // Time compare coloring: how fast the gap to the best lap grows (losing)
        // or shrinks (gaining), per meter, at each point.
        let reference = self.time_compare_reference(session, start_ms);
        let gap_curve = reference.filter(|_| self.map_time_compare).and_then(|best| time_compare_curve(session, best, start_ms, end_ms));
        let gain = gap_curve.as_ref().map(|c| {
            let rate = |k: usize| {
                let (a, b) = (c[k.saturating_sub(1)], c[(k + 1).min(c.len() - 1)]);
                (b.2 - a.2) / (b.1 - a.1).max(1e-6)
            };
            let rates: Vec<(f64, f64)> = (0..c.len()).map(|k| (c[k].0, rate(k))).collect();
            let scale = rates.iter().map(|r| r.1.abs()).fold(0.0_f64, f64::max).max(1e-9) * 0.6;
            (rates, scale)
        });

        Frame::new()
            .fill(SURFACE)
            .corner_radius(CornerRadius::same(theme::RADIUS))
            .inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 12 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height() - 24.0);

                // Header: title, color-by picker, readouts at the cursor.
                ui.horizontal(|ui| {
                    ui.add(Icon::MapPin.image(DIM, 16.0));
                    ui.label(RichText::new("Track").font(medium(14.5)).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let name = if gain.is_some() { "Time compare" } else { color_ch.map(|i| session.channels[i].name.as_str()).unwrap_or("—") };
                        egui::ComboBox::from_id_salt("map_channel")
                            .selected_text(RichText::new(name).size(13.0))
                            .width(118.0)
                            .show_ui(ui, |ui| {
                                if reference.is_some() && ui.selectable_label(gain.is_some(), "Time compare").on_hover_text("Where this lap gains and loses time against the best lap").clicked() {
                                    self.map_time_compare = true;
                                }
                                // Only what's worth seeing on the track: engine and movement.
                                let imu = session.info.imu_mount.is_some();
                                for (i, c) in session.channels.iter().enumerate() {
                                    if !is_dead(c)
                                        && matches!(channel_group(c, imu), "Engine" | "Movement")
                                        && ui.selectable_label(gain.is_none() && color_ch == Some(i), &c.name).clicked()
                                    {
                                        self.map_channel = Some(i);
                                        self.map_time_compare = false;
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Channel that colors the track");
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

                // Local flat projection: meters east/north of the window's first point.
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

                // Color scale from the window's values, ignoring the extreme 2% each side
                // (the logger records occasional sensor spikes).
                let channel = color_ch.map(|i| &session.channels[i]);
                let values: Vec<Option<f64>> = (lo..hi)
                    .map(|i| channel.and_then(|c| sample_at(c, gps.times_ms[i])))
                    .collect();
                let (vmin, vmax) = robust_range(values.iter().flatten().copied());
                let colors = channel.map_or(heat_color as fn(f32) -> Color32, channel_ramp);
                let ramp = |v: f64| colors(((v - vmin) / (vmax - vmin).max(f64::EPSILON)) as f32);

                let painter = ui.painter_at(rect);
                for k in 0..pts.len() - 1 {
                    let (i, j) = (lo + k, lo + k + 1);
                    // Don't bridge GPS dropouts.
                    if gps.times_ms[j] - gps.times_ms[i] > 1000 {
                        continue;
                    }
                    let color = match &gain {
                        Some((rates, scale)) => {
                            let t = gps.times_ms[i] as f64;
                            let r = rates[rates.partition_point(|p| p.0 < t).min(rates.len() - 1)].1;
                            gain_color((r / scale) as f32)
                        }
                        None => values[k].map(ramp).unwrap_or(DIM),
                    };
                    painter.line_segment([pts[k], pts[k + 1]], Stroke::new(6.0, color));
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
                    if let Some(c) = &gap_curve {
                        let g = c[c.partition_point(|p| p.0 < t as f64).min(c.len() - 1)].2;
                        line = if g >= 0.0 { format!("+{g:.3} s") } else { format!("−{:.3} s", -g) };
                    } else if let Some(v) = channel.and_then(|c| sample_at(c, t)) {
                        line = format!("{} {units}", format_value(v, dec));
                    }
                    if let Some(i) = gps.index_at(t) {
                        if !line.is_empty() {
                            line.push_str("   ");
                        }
                        line.push_str(&format!("{:.1} {}", crate::units::speed(gps.speed_kmh[i]), crate::units::speed_unit()));
                    }
                    painter.text(rect.left_top() + vec2(2.0, 2.0), egui::Align2::LEFT_TOP, line, medium(14.0), TEXT);
                }

                // Legend: the color ramp and the range it covers.
                ui.add_space(6.0);
                if gain.is_some() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Gaining").size(12.0).color(DIM));
                        let (bar, _) = ui.allocate_exact_size(vec2(140.0, 6.0), Sense::hover());
                        for j in 0..40 {
                            let r = Rect::from_min_size(pos2(bar.left() + bar.width() * j as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                            ui.painter().rect_filled(r, 0.0, gain_color(-1.0 + 2.0 * j as f32 / 39.0));
                        }
                        ui.label(RichText::new("Losing time  ·  vs best lap").size(12.0).color(DIM));
                    });
                    return;
                }
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

// ---------------------------------------------------------------------------
// Widgets

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
        s.drivers.extend(profiles.drivers.iter().map(|d| d.name.clone()));
        s.karts.extend(profiles.karts.iter().map(|k| k.name.clone()));
        s.championships.extend(profiles.loggers.values().map(|p| p.championship.clone()));
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
            .font(regular(16.5))
            .margin(Margin::symmetric(14, 12))
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
        .take(40)
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
                    // Eight rows show; more scroll.
                    egui::ScrollArea::vertical().max_height(34.0 * 8.0).show(ui, |ui| {
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
        });
    ctx.data_mut(|d| d.insert_temp(popup_id, area.response.rect));
    if let Some(p) = picked {
        *value = p;
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        ctx.memory_mut(|m| m.surrender_focus(response.id));
    }
    response
}

/// Colors from the first stop (`t` = 0) to the last (1), evenly spaced.
fn ramp_color(stops: &[Color32], t: f32) -> Color32 {
    let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (x.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], x - i as f32)
}

/// RPM on the track: dark purple when low, through magenta and orange, to
/// yellow when high (plasma), so where the engine bogs and pulls stands out.
pub(super) fn rpm_color(t: f32) -> Color32 {
    ramp_color(
        &[
            Color32::from_rgb(0x2b, 0x0b, 0x6e),
            Color32::from_rgb(0x8a, 0x1c, 0xa8),
            Color32::from_rgb(0xd6, 0x45, 0x7a),
            Color32::from_rgb(0xf7, 0x94, 0x3f),
            Color32::from_rgb(0xf6, 0xe6, 0x4a),
        ],
        t,
    )
}

/// A temperature on the track: blue when low, through yellow and orange, to red when high.
pub(super) fn temp_color(t: f32) -> Color32 {
    ramp_color(&[Color32::from_rgb(0x2f, 0x6b, 0xff), Color32::from_rgb(0xf2, 0xd1, 0x3b), Color32::from_rgb(0xf0, 0x8c, 0x2b), Color32::from_rgb(0xe5, 0x48, 0x4d)], t)
}

/// Speed on the track: red when slow to green when fast.
pub(super) fn track_speed_color(t: f32) -> Color32 {
    ramp_color(&[Color32::from_rgb(0xe5, 0x48, 0x4d), Color32::from_rgb(0xf0, 0xa0, 0x4b), theme::GOOD], t)
}

/// How a channel's values color the track, low to high, by what it measures
/// (whatever the logger calls it): RPM, a temperature, a speed, or anything
/// else on the general ramp.
fn channel_ramp(c: &Channel) -> fn(f32) -> Color32 {
    let name = c.name.to_lowercase();
    let units = pretty_units(&c.units).to_lowercase();
    if name.contains("rpm") || units == "rpm" {
        rpm_color
    } else if units == "°c" || units == "°f" {
        temp_color
    } else if name.contains("speed") || matches!(units.as_str(), "km/h" | "kmh" | "mph") {
        track_speed_color
    } else {
        heat_color
    }
}

/// Low to high on the track: blue, through teal and yellow, to red, so a
/// channel's highs and lows stand apart on the line.
fn heat_color(t: f32) -> Color32 {
    ramp_color(
        &[
            Color32::from_rgb(0x2f, 0x5b, 0xff),
            Color32::from_rgb(0x16, 0xb8, 0xd4),
            Color32::from_rgb(0xf2, 0xd1, 0x3b),
            Color32::from_rgb(0xf0, 0x8c, 0x2b),
            Color32::from_rgb(0xe5, 0x48, 0x4d),
        ],
        t,
    )
}

/// Speed color: viridis, dark purple when slow to yellow when fast. Its
/// lightness rises steadily, so slow and fast stay apart for colorblind
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

/// A titled area without a box: full width, on the page background.
fn section(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
}

/// A rounded progress bar, `fraction` 0–1 filled.
fn progress_bar(ui: &mut Ui, width: f32, height: f32, fraction: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let round = CornerRadius::same((height / 2.0) as u8);
    ui.painter().rect_filled(r, round, SURFACE_ACTIVE);
    let w = (r.width() * fraction.clamp(0.0, 1.0)).max(height);
    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, height)), round, theme::GOOD);
}

impl App {
    /// On the Tracks page: AiM's tracks downloading (with a progress bar), or
    /// a note when they couldn't be fetched because the computer is offline.
    fn aim_tracks_banner(&self, ui: &mut Ui, lib: &Library) {
        let have_list = crate::aim_tracks::list_path().is_some();
        let (icon, title, detail, progress) = match self.aim_tracks_progress {
            Some((done, total)) => (
                Icon::Download,
                if self.aim_tracks_first { "Downloading AiM's track database" } else { "Updating AiM's track database" },
                if self.aim_tracks_first {
                    "First time only: every AiM track with its outline, straight from AiM. Keep using Mychronix meanwhile; from now on it only fetches what changed.".to_string()
                } else {
                    "Fetching the tracks AiM added or changed since last time.".to_string()
                },
                Some((done, total)),
            ),
            None if self.aim_tracks_offline && !have_list => (
                Icon::CloudOff,
                "AiM's tracks will download when you're online",
                if lib.count(Kind::Aim) > 0 {
                    "Until then, tracks come from your MyChron's own list: names and start/finish lines, without outlines.".to_string()
                } else {
                    "Mychronix works offline. Your sessions and custom tracks are all here; AiM's track database downloads the next time Mychronix starts with an internet connection.".to_string()
                },
                None,
            ),
            _ => return,
        };
        Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::same(18)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(10), SURFACE_ACTIVE);
                icon.image(TEXT, 20.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(20.0)));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new(title).font(semibold(16.5)).color(TEXT));
                    ui.label(RichText::new(detail).font(regular(13.5)).color(SECONDARY));
                    if let Some((done, total)) = progress {
                        ui.add_space(6.0);
                        progress_bar(ui, ui.available_width(), 8.0, done as f32 / total.max(1) as f32);
                        ui.label(
                            RichText::new(format!("{} of {} tracks  ·  {}%", group_thousands(done), group_thousands(total), done * 100 / total.max(1)))
                                .font(medium(12.5))
                                .color(SECONDARY),
                        );
                    }
                });
            });
        });
        ui.add_space(14.0);
    }
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

/// Pill button; `primary` fills it with ink.
fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, primary: bool) -> egui::Response {
    button_with(ui, icon, None, label, primary)
}

/// A button whose icon can have its own color (e.g. a yellow star).
fn button_with(ui: &mut Ui, icon: Option<Icon>, icon_tint: Option<Color32>, label: &str, primary: bool) -> egui::Response {
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
        icon.image(icon_tint.unwrap_or(fg), 16.0).paint_at(ui, Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(16.0)));
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

/// Channel color key: filled dot when graphed, empty ring when not.
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

/// "1 lap", "3 laps".
fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
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
pub(crate) fn slow_laps(session: &Session) -> Vec<bool> {
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

/// Lap-timing channels are stored in milliseconds; show them in seconds.
/// Speeds and temperatures are converted to the display units too.
fn seconds_not_ms(session: &mut Session) {
    // Speeds and temperatures in the chosen units (mph and °F, or metric).
    for c in session.channels.iter_mut() {
        crate::units::convert_channel(c);
    }
    for c in session.channels.iter_mut().filter(|c| c.units.trim() == "ms") {
        for v in &mut c.values {
            *v /= 1000.0;
        }
        c.units = "s".into();
        c.decimals = 3;
    }
}

/// A channel that never changes (e.g. lap comparisons the MyChron isn't set to calculate).
fn is_dead(c: &Channel) -> bool {
    c.values.len() < 2 || c.values.iter().all(|v| (v - c.values[0]).abs() < 1e-9)
}

/// Sidebar groups, in order.
const CHANNEL_GROUPS: [&str; 5] = ["Engine", "Movement", "Lap timing", "Logger", "More channels"];

/// Which group a channel is listed under. `imu`: the session has the IMU's
/// corrected channels, which stand in for the GPS's forward acceleration.
fn channel_group(c: &Channel, imu: bool) -> &'static str {
    let n = c.name.to_lowercase();
    // The IMU's own axes (the logger's, not the kart's) and channels the
    // corrected ones replace: still there, but tucked away.
    if n.starts_with("accelerometer") || n.starts_with("gyro") || (imu && n == "gps lonacc") {
        "More channels"
    } else if n == "forward g" || n == "vertical g" || n == "grip used" {
        "Movement"
    } else if n.contains("predictive") || n.contains("diff") || n.contains("lap time") || n.contains("split") {
        "Lap timing"
    } else if n.contains("rpm") || n.contains("exhaust") || n.contains("egt") || n.contains("water") || n.contains("cht") || n.contains("throttle") || n.contains("lambda") {
        "Engine"
    } else if n.contains("speed") || n.contains("acc") || n.contains("gyro") || n.contains("gps") || n.contains("steer") || n.contains("brake") {
        "Movement"
    } else {
        "Logger"
    }
}

/// Channels shown when a session first opens: ones every MyChron records,
/// falling back to the first few channels with real data.
fn default_channels(session: &Session) -> Vec<usize> {
    const PREFERRED: [&str; 9] = ["RPM", "Engine", "GPS Speed", crate::setup::SPEED_FROM_RPM, "Exhaust Temp", "Speed", "WheelSpeed", "Water Temp", "WT"];
    let mut picks: Vec<usize> = PREFERRED
        .iter()
        .filter_map(|name| session.channels.iter().position(|c| c.name == *name && !is_dead(c)))
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

/// The sample at or just before `t_ms` (no interpolation; see
/// `charts::value_at` for that).
fn sample_at(c: &Channel, t_ms: i64) -> Option<f64> {
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
