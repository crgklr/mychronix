//! The window's chrome: the update bar, error bar, status bar, and the
//! screenshot taken for testing.

use super::*;

impl App {

    /// A thin bar at the top when a newer release is out: what's new, download, dismiss.
    pub(super) fn update_bar(&mut self, ui: &mut Ui) {
        // Screenshot runs leave AiM's server alone unless MYCHRONIX_AIM_SYNC=1 (development only).
        let sync = self.screenshot_to.is_none() || crate::hooks::get().aim_sync;
        if self.frames == 1 && self.aim_tracks_rx.is_none() && sync {
            self.aim_tracks_rx = Some(crate::aim_tracks::spawn(ui.ctx().clone()));
        }
        // MYCHRONIX_IMPORT=<file or folder> imports it at start (for testing; the file picker can't be clicked).
        static IMPORT_ONCE: std::sync::Once = std::sync::Once::new();
        if self.frames >= 3 && self.import_rx.is_none() && !IMPORT_ONCE.is_completed() {
            IMPORT_ONCE.call_once(|| {});
            if let Some(p) = crate::hooks::get().import.clone() {
                let lib = self.library.get_or_insert_with(Library::load);
                self.import_rx = Some(crate::import::spawn(vec![p], lib, ui.ctx().clone()));
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
                    self.reload_library();
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
                        self.reload_library(); // pick up the new tracks and outlines
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
    pub(super) fn error_bar(&mut self, ui: &mut Ui) {
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

    pub(super) fn status_bar(&self, ui: &mut Ui, session: Option<&Session>) {
        egui::Panel::bottom("status")
            .exact_size(30.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(SIDEBAR).inner_margin(Margin::symmetric(16, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    match &self.device.logger {
                        Some(b) => {
                            ui.add(if self.device.usb { Icon::Usb } else { Icon::Wifi }.image(theme::GOOD, 14.0));
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

    pub(super) fn screenshot(&mut self, ui: &mut Ui) {
        let Some(path) = self.screenshot_to.clone() else { return };
        self.frames += 1;
        // MYCHRONIX_SCREENSHOT_FRAME delays the shot, e.g. to let map tiles load.
        if self.frames == crate::hooks::get().screenshot_frame {
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
