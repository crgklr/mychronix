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

mod app;
mod compare;
mod countries;
mod device;
mod profiles;
mod splits;
mod theme;
mod tracks;

use std::path::PathBuf;

use eframe::egui;

/// The MyChron icon, for desktops that take it from the window.
fn app_icon() -> egui::IconData {
    let png = include_bytes!("../assets/mychronix-icon-256.png");
    let image = image::load_from_memory(png).expect("bundled icon is a valid PNG").to_rgba8();
    egui::IconData { width: image.width(), height: image.height(), rgba: image.into_raw() }
}

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let screenshot = std::env::var_os("MYCHRONIX_SCREENSHOT").map(PathBuf::from);
    let demo = std::env::var_os("MYCHRONIX_DEMO").is_some();
    eframe::run_native(
        "Mychronix",
        eframe::NativeOptions {
            // Open maximised (asked again once the window exists, see App::ui); the
            // size is a fallback that fits a 1366×768 laptop screen.
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 720.0])
                .with_min_inner_size([960.0, 600.0])
                .with_maximized(true)
                .with_title("Mychronix")
                // Matches the installed launcher (packaging/linux/mychronix.desktop),
                // which is where GNOME takes the app drawer and Alt+Tab icon from.
                .with_app_id("mychronix")
                .with_icon(app_icon()),
            ..Default::default()
        },
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            // Undo WiFi changes left behind if Mychronix quit while joined to a logger.
            device::restore_after_crash();
            let mut app = app::App::default();
            match path {
                Some(path) => {
                    app.load(path);
                    if demo {
                        app.demo();
                    }
                    if std::env::var_os("MYCHRONIX_COMPARE").is_some() {
                        app.demo_compare();
                    }
                }
                // Without a file, start from the list of sessions.
                None => app.show_sessions_page(),
            }
            match std::env::var("MYCHRONIX_PAGE").as_deref() {
                Ok("device") => app.show_device_page(),
                Ok("tracks") => app.show_tracks_page(),
                Ok("sessions") => app.show_sessions_page(),
                _ => {}
            }
            app.screenshot_to = screenshot;
            Ok(Box::new(app))
        }),
    )
}
