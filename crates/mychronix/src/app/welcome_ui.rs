//! The first thing a new user sees (no sessions, drivers or karts yet): two
//! ways to start, connecting a MyChron or importing session files, and the
//! import's progress and result.

use super::*;
use garage_manage_ui::look::{self, Kind};

/// Session files a file picker offers.
const SESSION_FILES: [&str; 10] = ["xrk", "xrz", "drk", "imd", "mc4", "XRK", "XRZ", "DRK", "IMD", "MC4"];

impl App {
    /// Whether to welcome: nothing in the library or the Garage, and no
    /// MyChron or data key here right now (those get the Garage's own card).
    pub(super) fn first_run(&mut self) -> bool {
        let lib = self.library.get_or_insert_with(Library::load);
        let empty_lib = lib.sessions.is_empty();
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
        empty_lib && profiles.drivers.is_empty() && profiles.garage_karts().next().is_none() && self.device.logger.is_none() && !self.key.present
    }

    /// Asks for session files (or folders) and imports them in the background.
    pub(super) fn import_dialog(&mut self, folders: bool, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new().set_title(if folders { "Import a folder of sessions" } else { "Import session files" });
        let picked = if folders { dialog.pick_folders() } else { dialog.add_filter("AiM sessions (.xrk, .xrz, .drk)", &SESSION_FILES).pick_files() };
        match picked.filter(|p| !p.is_empty()) {
            Some(paths) => self.start_import(paths, ctx),
            None => log::info!("import: the file picker closed without a choice"),
        }
    }

    /// Imports `paths` (files or folders) in the background.
    pub(super) fn start_import(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        if self.import_rx.is_some() {
            return;
        }
        log::info!("import: starting with {paths:?}");
        let lib = self.library.get_or_insert_with(Library::load);
        self.import_rx = Some(crate::import::spawn(paths, lib, ctx.clone()));
        self.import_progress = Some((0, 0));
        self.import_report = None;
    }

    /// Whether this window can receive dropped files: on a Mac and under X11,
    /// but not on Wayland (the windowing library doesn't support drops there).
    pub(super) fn can_drop() -> bool {
        !(cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("WINIT_UNIX_BACKEND").as_deref() != Ok("x11"))
    }

    /// Files or folders dropped on the window, to import. Returns whether
    /// something is being dragged over it right now (to highlight where to drop).
    pub(super) fn import_drops(&mut self, ctx: &egui::Context) -> bool {
        let (dropped, hovering) = ctx.input(|i| (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>(), !i.raw.hovered_files.is_empty()));
        if !dropped.is_empty() {
            self.start_import(dropped, ctx);
        }
        hovering
    }

    /// Follows a running import; when it ends, the library reloads and the
    /// Garage shows what came in.
    pub(super) fn import_poll(&mut self) {
        use std::sync::mpsc::TryRecvError;
        loop {
            let Some(rx) = &self.import_rx else { return };
            match rx.try_recv() {
                Ok(crate::import::Event::Progress { done, total }) => self.import_progress = Some((done, total)),
                Ok(crate::import::Event::Finished(report)) => {
                    self.import_rx = None;
                    self.import_progress = None;
                    self.reload_library();
                    self.import_report = Some(report);
                    self.page = Page::Garage;
                }
                Err(TryRecvError::Empty) => return,
                // Stopped without finishing (it shouldn't): don't wait forever.
                Err(TryRecvError::Disconnected) => {
                    log::warn!("import: stopped without finishing");
                    self.import_rx = None;
                    self.import_progress = None;
                    self.reload_library();
                }
            }
        }
    }

    pub(super) fn welcome_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let dragging = self.import_drops(&ctx);
        let width = ui.available_width().min(1040.0);
        let side = (ui.available_width() - width) / 2.0;
        egui::ScrollArea::vertical().id_salt("welcome").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.add_space((ui.available_height() * 0.08).clamp(16.0, 70.0));
                    // An import that brought nothing in leaves us here: say why.
                    self.import_banner(ui);
                    ui.vertical_centered(|ui| {
                        // The logo (the wordmark the sidebar shows), 578 × 82 in its drawing.
                        ui.add(egui::Image::new(egui::include_image!("../../assets/mychronix.svg")).tint(TEXT).fit_to_exact_size(vec2(360.0, 360.0 * 82.0 / 578.0)));
                        ui.add_space(18.0);
                        ui.label(RichText::new("Start Analyzing Your Race Data").font(semibold(37.0)).color(TEXT));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Connect to your data source to compare your MyChron sessions.").font(regular(17.0)).color(SECONDARY));
                    });
                    ui.add_space(34.0);

                    let gap = 22.0;
                    let card_w = (width - gap) / 2.0;
                    let card_h = 400.0;
                    let mut connect = false;
                    let mut import = None;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        // 1. A MyChron to connect.
                        Self::start_card(ui, card_w, card_h, |ui| {
                            ui.add(egui::Image::from_bytes("bytes://mychron6.svg", include_bytes!("../../assets/devices/mychron6.svg")).fit_to_exact_size(vec2(230.0, 134.0)));
                            ui.add_space(22.0);
                            ui.label(RichText::new("Connect my MyChron").font(semibold(22.0)).color(TEXT));
                            ui.add_space(8.0);
                            ui.add(egui::Label::new(RichText::new("Download straight from a MyChron 6, 5 or 4. Mychronix walks you through connecting it.").font(regular(15.5)).color(SECONDARY)).wrap());
                        }, |ui| {
                            if look::big_button(ui, "Connect a MyChron", Some(Icon::ChevronRight), Kind::Primary, true) {
                                connect = true;
                            }
                        });
                        // 2. Files to import: a folder from the picker, or anything dropped on the card.
                        let card = Self::start_card(ui, card_w, card_h, |ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(230.0, 134.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 56.0, SURFACE_ACTIVE);
                            Icon::FolderOpen.image(TEXT, 50.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(50.0)));
                            ui.add_space(22.0);
                            ui.label(RichText::new("Import session files").font(semibold(22.0)).color(TEXT));
                            ui.add_space(8.0);
                            let progress = self.import_progress;
                            let text = match progress {
                                Some((_, 0)) => "Looking for sessions…".to_string(),
                                Some((done, total)) => format!("Importing {} of {} files…", group_thousands(done), group_thousands(total)),
                                None if Self::can_drop() => "Already have sessions from RaceStudio, a USB stick or a friend? Choose their folder, or drag files or folders here.".to_string(),
                                None => "Already have sessions from RaceStudio, a USB stick or a friend? Choose their folder: Mychronix finds the sessions in it.".to_string(),
                            };
                            ui.add(egui::Label::new(RichText::new(text).font(regular(15.5)).color(SECONDARY)).wrap());
                        }, |ui| match self.import_progress {
                            Some((done, total)) => {
                                let (bar, _) = ui.allocate_exact_size(vec2(card_w * 0.7, 8.0), Sense::hover());
                                ui.painter().rect_filled(bar, CornerRadius::same(4), SURFACE_ACTIVE);
                                let f = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                                ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2((bar.width() * f).max(8.0), 8.0)), CornerRadius::same(4), theme::GOOD);
                            }
                            None => {
                                if look::big_button(ui, "Choose a folder…", None, Kind::Primary, true) {
                                    import = Some(true);
                                }
                            }
                        });
                        // While files are dragged over the window, the card says where they go.
                        if dragging && Self::can_drop() && self.import_progress.is_none() {
                            ui.painter().rect_filled(card, CornerRadius::same(18), SURFACE_HOVER);
                            ui.painter().rect_stroke(card.shrink(2.0), CornerRadius::same(16), Stroke::new(2.0, theme::GOOD), egui::StrokeKind::Inside);
                            Icon::Download.image(theme::GOOD, 44.0).paint_at(ui, Rect::from_center_size(card.center() - vec2(0.0, 26.0), Vec2::splat(44.0)));
                            ui.painter().text(card.center() + vec2(0.0, 30.0), egui::Align2::CENTER_CENTER, "Drop to import", semibold(20.0), TEXT);
                        }
                    });
                    ui.add_space(26.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Mychronix only reads from your MyChron, and everything stays on this computer.").font(regular(14.0)).color(DIM));
                        ui.add_space(6.0);
                        let tracks = ui.add(egui::Label::new(RichText::new("Just looking around? Browse AiM's tracks").font(medium(14.0)).color(SECONDARY).underline()).sense(Sense::click()));
                        if tracks.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if tracks.clicked() {
                            self.page = Page::Tracks;
                        }
                    });
                    if connect {
                        self.page = Page::Device;
                        self.open_add_mychron(None, "");
                    }
                    if let Some(folders) = import {
                        self.import_dialog(folders, &ctx);
                    }
                });
            });
        });
    }

    /// One of the two ways to start: a large card, its content centered and
    /// its actions along the bottom, so both cards' buttons line up.
    fn start_card(ui: &mut Ui, w: f32, h: f32, body: impl FnOnce(&mut Ui), actions: impl FnOnce(&mut Ui)) -> Rect {
        let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(18), SURFACE);
        let inner = rect.shrink2(vec2(36.0, 40.0));
        let mut top = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Center)));
        body(&mut top);
        let bottom = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 44.0), inner.max);
        let mut foot = ui.new_child(egui::UiBuilder::new().max_rect(bottom).layout(Layout::top_down(Align::Center)));
        actions(&mut foot);
        rect
    }

    /// After an import, on the Garage: what came in and what to do next.
    pub(super) fn import_banner(&mut self, ui: &mut Ui) {
        let Some(r) = self.import_report.clone() else { return };
        let mut close = false;
        Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::symmetric(22, 18)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let ok = r.added > 0;
                let (r0, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                let tint = if ok { theme::GOOD } else { SECONDARY };
                ui.painter().circle_filled(r0.center(), 22.0, tint.gamma_multiply(0.15));
                (if ok { Icon::CircleCheck } else { Icon::FolderOpen }).image(tint, 22.0).paint_at(ui, Rect::from_center_size(r0.center(), Vec2::splat(22.0)));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let title = if ok {
                        format!("Imported {} from {}", plural(r.added, "session"), plural(r.loggers.len(), "MyChron"))
                    } else if r.already > 0 {
                        "Those sessions are already here".to_string()
                    } else if !r.not_sessions.is_empty() && r.searched.is_empty() {
                        if r.not_sessions.len() == 1 { format!("{} isn't a session file", r.not_sessions[0]) } else { "Those aren't session files".to_string() }
                    } else {
                        "No sessions found there".to_string()
                    };
                    ui.label(RichText::new(title).font(semibold(17.0)).color(TEXT));
                    let mut notes = Vec::new();
                    if r.already > 0 {
                        notes.push(if r.already == 1 { "1 session was already in your library".to_string() } else { format!("{} sessions were already in your library", r.already) });
                    }
                    if !r.unreadable.is_empty() {
                        notes.push(format!("{} couldn't be read", plural(r.unreadable.len(), "file")));
                    }
                    // Say plainly what was left alone, so nothing seems "imported" that wasn't.
                    if ok || r.already > 0 {
                        match r.not_sessions.len() {
                            0 => {}
                            1 => notes.push(format!("{} isn't a session file", r.not_sessions[0])),
                            n => notes.push(format!("{n} files weren't session files")),
                        }
                    }
                    if !r.searched.is_empty() {
                        notes.push(format!("Only session files were taken from {}", r.searched.join(", ")));
                    }
                    if ok {
                        notes.push("Next: say whose each MyChron is, below".to_string());
                    } else if r.already == 0 && r.unreadable.is_empty() {
                        notes.push("Session files end in .xrk, .xrz or .drk".to_string());
                    }
                    ui.label(RichText::new(notes.join("  ·  ")).font(regular(14.0)).color(SECONDARY));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Icon::X.image(DIM, 16.0).sense(Sense::click())).clicked() {
                        close = true;
                    }
                });
            });
        });
        ui.add_space(12.0);
        if close {
            self.import_report = None;
        }
    }
}
