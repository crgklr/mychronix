//! Mychronix analysis window.
//!
//! Every view follows one shared cursor time (`App::cursor_s`), so the graphs,
//! the channel readouts and later views (track map, video) stay in step.
//!
//! Development aid: `MYCHRONIX_SCREENSHOT=out.png` renders a few frames, saves a
//! screenshot and exits; `MYCHRONIX_DEMO=1` opens on the best lap with a cursor
//! placed, so a screenshot shows a populated view; `MYCHRONIX_PAGE=device` or
//! `tracks` opens on that page, `MYCHRONIX_TRACK=<name>` picks a track there and
//! `MYCHRONIX_MAP=splits` opens its Splits view. `MYCHRONIX_FAKE_DEVICE=<logger id>`
//! previews the Device page with that logger's downloaded sessions
//! (`MYCHRONIX_FAKE_DIALOG=1` also opens the download dialog).

mod aim_tracks;
mod app;
mod import;
mod hooks;
mod jobs;
mod braking;
mod bumps;
mod charts;
mod compare;
mod corner_model;
mod countries;
mod device;
mod digest;
mod logging;
mod startup_notify;
mod mcp;
mod paths;
mod events;
mod profiles;
mod report;
mod settings;
mod setup;
mod split_report;
mod sensor_check;
mod splits;
mod storage;
mod theme;
mod units;
mod usb_access;
mod update;
mod weather;
mod tracks;

use std::path::PathBuf;

use eframe::egui;

/// The MyChron icon, for desktops that take it from the window.
fn app_icon() -> egui::IconData {
    let png = include_bytes!("../assets/mychronix-icon-256.png");
    let image = image::load_from_memory(png).expect("bundled icon is a valid PNG").to_rgba8();
    egui::IconData { width: image.width(), height: image.height(), rgba: image.into_raw() }
}

/// Writes a crash to `crash.log` beside the log (kept under about 1 MB)
/// and, if the app itself is going down, undoes any WiFi changes first.
fn install_crash_handler() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        if let Some(dir) = crate::paths::log_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let log = dir.join("crash.log");
            if std::fs::metadata(&log).is_ok_and(|m| m.len() > 1_000_000) {
                let _ = std::fs::rename(&log, dir.join("crash.log.old"));
            }
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let line = format!("[{secs}] Mychronix {} · thread {thread}: {info}\n", env!("CARGO_PKG_VERSION"));
            log::error!("panic on thread {thread}: {info}");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
                use std::io::Write;
                let _ = f.write_all(line.as_bytes());
            }
        }
        // The window's own thread: the app is going down. Don't leave the
        // laptop stuck on the MyChron's WiFi with the home networks paused.
        if thread == "main" && device::paused_networks() {
            device::restore_wifi_now();
        }
        default(info);
    }));
}

/// The command-line tool's commands (see `mychronix-cli --help`), and asking
/// for help, which once reached it under this program's name.
const CLI_COMMANDS: [&str; 6] = ["info", "sessions", "device", "datakey", "--help", "-h"];

fn main() -> eframe::Result {
    // A Mac's app menu ("About …", "Hide …", "Quit …") is named after the
    // running program's file, mychronix. Name it Mychronix, as the app is.
    // SAFETY: first thing in main, before any other thread exists (setting
    // the process name isn't thread safe); Mychronix keeps no user defaults
    // under the old name.
    #[cfg(target_os = "macos")]
    unsafe {
        objc2_foundation::NSProcessInfo::processInfo().setProcessName(objc2_foundation::ns_string!("Mychronix"));
    }
    // Until 0.1.17 this name was the command-line tool, now mychronix-cli:
    // its commands get pointed there instead of opening the app.
    if let Some(first) = std::env::args().nth(1).filter(|a| CLI_COMMANDS.contains(&a.as_str()) && !std::path::Path::new(a).exists()) {
        let rest: Vec<String> = std::env::args().skip(1).collect();
        eprintln!("The Mychronix command-line tool is now mychronix-cli. Run:\n  mychronix-cli {}", if first.starts_with('-') { "--help".to_string() } else { rest.join(" ") });
        std::process::exit(2);
    }
    let launch_token = startup_notify::take_token();
    logging::init();
    install_crash_handler();
    log::info!(
        "Mychronix {} starting ({} {}), log level from MYCHRONIX_LOG={:?}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("MYCHRONIX_LOG").unwrap_or_default()
    );
    // `--mcp`: the Mychronix Performance Engineer, for an AI assistant on this
    // computer (no window; it talks over stdin and stdout).
    if std::env::args().any(|a| a == "--mcp") {
        units::init();
        mcp::serve();
        return Ok(());
    }
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let hooks = hooks::get();
    let screenshot = hooks.screenshot.clone();
    let demo = hooks.demo;
    eframe::run_native(
        "Mychronix",
        eframe::NativeOptions {
            // Open maximized (asked again once the window exists, see App::ui); the
            // size is a fallback that fits a 1366×768 laptop screen.
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 720.0])
                .with_min_inner_size([960.0, 600.0])
                // MYCHRONIX_WINDOWED=1 opens it at its normal size (for testing the window's edges).
                .with_maximized(!hooks.windowed)
                .with_title("Mychronix")
                // On Linux, Mychronix draws its own close button instead of a title bar.
                .with_decorations(!app::OWN_WINDOW_CONTROLS)
                // Matches the installed launcher (packaging/linux/mychronix.desktop),
                // which is where GNOME takes the app drawer and Alt+Tab icon from.
                .with_app_id("mychronix")
                .with_icon(app_icon()),
            ..Default::default()
        },
        Box::new(move |cc| {
            startup_notify::finish(launch_token, cc, cc);
            theme::install(&cc.egui_ctx);
            units::init();
            log::info!("units: {}", if units::imperial() { "mph and °F" } else { "km/h and °C" });
            // Undo WiFi changes left behind if Mychronix quit while joined to a logger.
            device::restore_after_crash();
            // An assistant set up before an update may run a program that's gone.
            if settings::get().mcp {
                jobs::spawn("assistants", app::repair_assistants);
            }
            let mut app = app::App::default();
            match path {
                Some(path) => {
                    app.load(path);
                    if demo {
                        app.demo();
                    }
                    if hooks.distance {
                        app.show_distance();
                    }
                    if let Some(v) = &hooks.view {
                        app.show_view(v);
                    }
                    if hooks.splits {
                        app.show_splits();
                    }
                    if hooks.report {
                        app.show_report();
                    }
                    if hooks.compare {
                        app.demo_compare();
                    }
                }
                // Without a file, start at home: the Garage.
                None => app.show_garage_page(),
            }
            match hooks.page.as_deref() {
                Some("device") => app.show_device_page(),
                Some("settings") => app.show_settings_page(),
                Some("tracks") => app.show_tracks_page(),
                Some("sessions") => app.show_sessions_page(),
                Some("events") => app.show_events_page(),
                Some("garage") => app.show_garage_page(),
                Some("analysis") => app.show_analysis_page(),
                _ => {}
            }
            app.screenshot_to = screenshot;
            Ok(Box::new(app))
        }),
    )
}
