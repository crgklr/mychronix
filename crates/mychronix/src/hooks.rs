//! Development hooks: environment variables that set the app up for
//! screenshots and checks without a MyChron or a mouse. Read once, here, so
//! the pages ask [`get`] rather than the environment. None of them is for
//! people using Mychronix (that's `MYCHRONIX_LOG`, in `logging`).

use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Default)]
pub struct Hooks {
    /// MYCHRONIX_SCREENSHOT=<file>: save a screenshot there and quit…
    pub screenshot: Option<PathBuf>,
    /// …MYCHRONIX_SCREENSHOT_FRAME=<n> frames in (default 20).
    pub screenshot_frame: u32,
    /// MYCHRONIX_DEMO: made-up names in place of the real ones.
    pub demo: bool,
    /// MYCHRONIX_WINDOWED: open at the normal window size, not maximized.
    pub windowed: bool,
    /// MYCHRONIX_PAGE=<page>: start on that page.
    pub page: Option<String>,
    /// With a session to open: MYCHRONIX_VIEW=<view>, MYCHRONIX_DISTANCE (the
    /// distance axis), MYCHRONIX_SPLITS, MYCHRONIX_REPORT, MYCHRONIX_COMPARE.
    pub view: Option<String>,
    pub distance: bool,
    pub splits: bool,
    pub report: bool,
    pub compare: bool,
    /// MYCHRONIX_CLICKS="frame:x,y;…": input on those frames (see `App::raw_input_hook`).
    pub clicks: Option<String>,
    /// MYCHRONIX_DROP="frame:path" drops a file on the window; MYCHRONIX_DRAG=<frame>
    /// holds a dragged file over it from then on.
    pub drop: Option<(u32, PathBuf)>,
    pub drag: Option<u32>,
    /// MYCHRONIX_DEBUG_SCALE: print what egui thinks the display is.
    pub debug_scale: bool,
    /// MYCHRONIX_SENSOR_CHECK=<logger id>: the sensor check, as if just downloaded.
    pub sensor_check: Option<String>,
    /// MYCHRONIX_AIM_SYNC: fetch AiM's tracks even while taking a screenshot.
    pub aim_sync: bool,
    /// MYCHRONIX_IMPORT=<folder>: import it at start.
    pub import: Option<PathBuf>,
    /// MYCHRONIX_CLICK_SESSION=<n>: open that session on the Sessions page.
    pub click_session: Option<u32>,
    /// MYCHRONIX_CHANNEL_SEARCH=<text>: open the channel picker with it typed.
    pub channel_search: Option<String>,
    /// Tracks page: MYCHRONIX_TRACK=<name> selected, MYCHRONIX_MAP=splits|elevation,
    /// MYCHRONIX_SATELLITE, MYCHRONIX_DETAIL_SCROLL=<px>, MYCHRONIX_TRACKS_COLLAPSED.
    pub track: Option<String>,
    pub map: Option<String>,
    pub satellite: bool,
    pub detail_scroll: Option<f32>,
    pub tracks_collapsed: bool,
    /// Events page: MYCHRONIX_EVENT=<title>, MYCHRONIX_CHAMPIONSHIP=<name>,
    /// MYCHRONIX_EVENT_DIALOG (the plan-an-event window).
    pub event: Option<String>,
    pub championship: Option<String>,
    pub event_dialog: bool,
    /// Device page: MYCHRONIX_FAKE_DEVICE=<logger id> shows that logger's
    /// downloaded sessions as if it were connected; MYCHRONIX_FAKE_DIALOG
    /// (or =multi) opens the download window; MYCHRONIX_FAKE_USB_BLOCKED;
    /// MYCHRONIX_ADD_MYCHRON=wifi|key opens "Add a MyChron" at that step.
    pub fake_device: Option<u32>,
    pub fake_dialog: Option<String>,
    pub fake_usb_blocked: bool,
    pub add_mychron: Option<String>,
    /// MYCHRONIX_UPDATE_TEST=<file>: a release, as GitHub would describe it.
    pub update_test: Option<PathBuf>,
}

pub fn get() -> &'static Hooks {
    static HOOKS: LazyLock<Hooks> = LazyLock::new(Hooks::from_env);
    &HOOKS
}

impl Hooks {
    fn from_env() -> Self {
        let text = |name: &str| std::env::var(name).ok();
        let set = |name: &str| std::env::var_os(name).is_some();
        let path = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Hooks {
            screenshot: path("MYCHRONIX_SCREENSHOT"),
            screenshot_frame: text("MYCHRONIX_SCREENSHOT_FRAME").and_then(|v| v.parse().ok()).unwrap_or(20),
            demo: set("MYCHRONIX_DEMO"),
            windowed: set("MYCHRONIX_WINDOWED"),
            page: text("MYCHRONIX_PAGE"),
            view: text("MYCHRONIX_VIEW"),
            distance: set("MYCHRONIX_DISTANCE"),
            splits: set("MYCHRONIX_SPLITS"),
            report: set("MYCHRONIX_REPORT"),
            compare: set("MYCHRONIX_COMPARE"),
            clicks: text("MYCHRONIX_CLICKS"),
            drop: text("MYCHRONIX_DROP").and_then(|v| v.split_once(':').and_then(|(f, p)| Some((f.parse().ok()?, PathBuf::from(p))))),
            drag: text("MYCHRONIX_DRAG").and_then(|v| v.parse().ok()),
            debug_scale: set("MYCHRONIX_DEBUG_SCALE"),
            sensor_check: text("MYCHRONIX_SENSOR_CHECK"),
            aim_sync: set("MYCHRONIX_AIM_SYNC"),
            import: path("MYCHRONIX_IMPORT"),
            click_session: text("MYCHRONIX_CLICK_SESSION").and_then(|v| v.parse().ok()),
            channel_search: text("MYCHRONIX_CHANNEL_SEARCH"),
            track: text("MYCHRONIX_TRACK"),
            map: text("MYCHRONIX_MAP"),
            satellite: set("MYCHRONIX_SATELLITE"),
            detail_scroll: text("MYCHRONIX_DETAIL_SCROLL").and_then(|v| v.parse().ok()),
            tracks_collapsed: set("MYCHRONIX_TRACKS_COLLAPSED"),
            event: text("MYCHRONIX_EVENT"),
            championship: text("MYCHRONIX_CHAMPIONSHIP"),
            event_dialog: set("MYCHRONIX_EVENT_DIALOG"),
            fake_device: text("MYCHRONIX_FAKE_DEVICE").and_then(|v| v.parse().ok()),
            fake_dialog: text("MYCHRONIX_FAKE_DIALOG"),
            fake_usb_blocked: set("MYCHRONIX_FAKE_USB_BLOCKED"),
            add_mychron: text("MYCHRONIX_ADD_MYCHRON"),
            update_test: path("MYCHRONIX_UPDATE_TEST"),
        }
    }
}
