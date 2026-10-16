//! The Device page's guidance: which MyChrons download how, step-by-step
//! instructions with live status, and the MyChrons Mychronix already knows.

use super::*;

/// Which part of the Device page is showing (when no WiFi MyChron is connected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum DeviceView {
    /// The known MyChrons, or the choice below when there are none.
    #[default]
    Home,
    /// "How does your MyChron download?"
    Choose,
    Wifi,
    DataKey,
}

/// The models each way of downloading covers.
const WIFI_MODELS: &str = "MyChron 6 and 5 Series";
const KEY_MODELS: &str = "MyChron 4 Series";

/// A MyChron Mychronix has seen: its id, model and whose it is.
struct Known {
    id: String,
    model: String,
    owner: Option<String>,
    sessions: usize,
    newest: Option<aim_xrk::summary::Date>,
}

impl App {
    fn known_mychrons(&mut self) -> Vec<Known> {
        let lib = self.library.get_or_insert_with(Library::load);
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let digests = self.sensors.digests.get_or_insert_with(crate::digest::Digests::load);
        let mut ids: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
        ids.extend(profiles.loggers.keys().cloned());
        ids.extend(profiles.karts.iter().map(|k| k.logger.clone()));
        ids.retain(|i| !i.is_empty());
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| {
                let mine: Vec<_> = lib.sessions.iter().filter(|s| s.logger == id).collect();
                let model = Some(profiles.for_logger(&id).model)
                    .filter(|m| !m.is_empty())
                    .or_else(|| digests.model(&id))
                    .or_else(|| mine.iter().any(|s| s.path.extension().is_some_and(|e| e == "drk" || e == "mc4")).then(|| "MyChron 4".to_string()))
                    .unwrap_or_else(|| "MyChron".into());
                let owner = profiles
                    .karts
                    .iter()
                    .find(|k| k.logger == id)
                    .and_then(|k| k.driver)
                    .and_then(|d| profiles.driver_name(Some(d)).map(str::to_string))
                    .or_else(|| Some(profiles.for_logger(&id).driver).filter(|d| !d.is_empty()));
                Known { sessions: mine.len(), newest: mine.iter().filter_map(|s| s.summary.date).max(), id, model, owner }
            })
            .collect()
    }

    /// The Device page below the live panels: guidance and known MyChrons.
    pub(super) fn device_guide(&mut self, ui: &mut Ui) {
        let known = self.known_mychrons();
        let view = match self.device_view {
            DeviceView::Home if known.is_empty() => DeviceView::Choose,
            v => v,
        };
        match view {
            DeviceView::Home => self.device_known(ui, &known),
            DeviceView::Choose => self.device_choose(ui, !known.is_empty()),
            DeviceView::Wifi => self.device_wifi_help(ui),
            DeviceView::DataKey => self.device_key_help(ui),
        }
    }

    fn back(&mut self, ui: &mut Ui, to: DeviceView) {
        let r = ui.add(egui::Label::new(RichText::new("‹ Back").size(13.5).color(SECONDARY)).sense(Sense::click()));
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.clicked() {
            self.device_view = to;
        }
        ui.add_space(8.0);
    }

    fn device_choose(&mut self, ui: &mut Ui, has_known: bool) {
        if has_known {
            self.back(ui, DeviceView::Home);
        }
        ui.label(RichText::new("Select your MyChron").font(semibold(20.0)).color(TEXT));
        ui.label(RichText::new("Mychronix will show you how to download from it.").size(13.5).color(SECONDARY));
        ui.add_space(12.0);
        let series: [(&str, &str, Option<DeviceView>, &str, (&str, &[u8])); 4] = [
            ("MyChron 6 Series", "MyChron 6 and 6T", Some(DeviceView::Wifi), "Downloads over WiFi", ("bytes://mychron6.svg", include_bytes!("../../assets/devices/mychron6.svg"))),
            ("MyChron 5 Series", "MyChron 5, 5S, 5 2T and 5S 2T", Some(DeviceView::Wifi), "Downloads over WiFi", ("bytes://mychron5.svg", include_bytes!("../../assets/devices/mychron5.svg"))),
            ("MyChron 4 Series", "MyChron 4, 4 2T and 4 660", Some(DeviceView::DataKey), "Downloads with AiM's data key", ("bytes://mychron4.svg", include_bytes!("../../assets/devices/mychron4.svg"))),
            ("MyChron 3 Series", "MyChron 3 and its variants", None, "Not supported yet (serial cable)", ("bytes://mychron3.svg", include_bytes!("../../assets/devices/mychron3.svg"))),
        ];
        let mut pick = None;
        // Two columns, two rows: a drawing of the series, its name and models under it.
        let gap = 16.0;
        let w = ((ui.available_width() - gap) / 2.0).max(260.0);
        for row in series.chunks(2) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &(name, models, view, how, (uri, svg)) in row {
                    let enabled = view.is_some();
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 280.0), if enabled { Sense::click() } else { Sense::hover() });
                    let hover = enabled && resp.hovered();
                    ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS), if hover { SURFACE_HOVER } else { SURFACE });
                    let cx = rect.center().x;
                    let pic = Rect::from_center_size(pos2(cx, rect.top() + 96.0), vec2(240.0, 140.0));
                    let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(70) };
                    egui::Image::from_bytes(uri, svg).tint(tint).paint_at(ui, pic);
                    let (title, sub) = if enabled { (TEXT, SECONDARY) } else { (DIM, DIM.gamma_multiply(0.8)) };
                    let p = ui.painter();
                    p.text(pos2(cx, rect.top() + 194.0), egui::Align2::CENTER_CENTER, name, semibold(20.0), title);
                    p.text(pos2(cx, rect.top() + 222.0), egui::Align2::CENTER_CENTER, models, regular(14.0), sub);
                    p.text(pos2(cx, rect.top() + 246.0), egui::Align2::CENTER_CENTER, how, regular(13.0), if enabled { DIM } else { sub });
                    if hover {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        pick = view;
                    }
                }
            });
            ui.add_space(gap);
        }
        if let Some(v) = pick {
            self.device_view = v;
        }
    }

    /// Numbered steps.
    fn steps(ui: &mut Ui, steps: &[&str]) {
        for (k, s) in steps.iter().enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 12.0, SURFACE_HOVER);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, (k + 1).to_string(), medium(13.0), TEXT);
                ui.add(egui::Label::new(RichText::new(*s).size(14.0).color(TEXT)).wrap());
            });
            ui.add_space(10.0);
        }
    }

    fn device_wifi_help(&mut self, ui: &mut Ui) {
        self.back(ui, DeviceView::Home);
        ui.label(RichText::new("Download over WiFi").font(semibold(20.0)).color(TEXT));
        ui.label(RichText::new(WIFI_MODELS).size(13.5).color(SECONDARY));
        ui.add_space(14.0);
        Self::steps(
            ui,
            &[
                "Turn the MyChron on and make sure its WiFi is switched on (it's in the MyChron's own menu).",
                "Keep it near this computer. Mychronix looks for its network (named AiM-MYC…) and lists it below; you can also choose it from your computer's WiFi menu.",
                "Join it. Mychronix finds the MyChron, lists its sessions and offers to download the new ones. Disconnect when you're done to get back on your own network.",
            ],
        );
        ui.add_space(4.0);
        self.device_status(ui);
    }

    fn device_key_help(&mut self, ui: &mut Ui) {
        self.back(ui, DeviceView::Home);
        ui.label(RichText::new("Download with the data key").font(semibold(20.0)).color(TEXT));
        ui.label(RichText::new(KEY_MODELS).size(13.5).color(SECONDARY));
        ui.add_space(14.0);
        Self::steps(
            ui,
            &[
                "At the kart, with the MyChron switched off, plug the data key into it, then switch the MyChron on. Its screen says \"Downloading data\" for a few seconds while it copies the new sessions to the key.",
                "Unplug the key from the MyChron and plug its USB end into this computer.",
                "Mychronix finds the key, lists its runs and offers to download the new ones. Nothing on the key or the MyChron is changed.",
            ],
        );
        ui.add_space(4.0);
        if !self.key.present {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(Icon::Usb.image(DIM, 28.0));
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Waiting for the data key…").font(semibold(16.0)).color(TEXT));
                        let note = if cfg!(target_os = "linux") {
                            "If it's plugged in and nothing happens, Linux may need AiM's USB rule; see \"MyChron 4 data key\" in the README."
                        } else {
                            "Plug it into any USB port."
                        };
                        ui.label(RichText::new(note).size(12.5).color(SECONDARY));
                    });
                });
            });
        }
    }

    fn device_known(&mut self, ui: &mut Ui, known: &[Known]) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Your MyChrons").font(semibold(20.0)).color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, Some(Icon::Plus), "Add a MyChron", false).clicked() {
                    self.device_view = DeviceView::Choose;
                }
            });
        });
        ui.add_space(10.0);
        let key_serial = self.key.listing.as_ref().filter(|_| self.key.present).map(|l| l.0.clone());
        let mut go = None;
        for k in known {
            let by_key = k.model == "MyChron 4";
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(Icon::Mychron.image(if key_serial.as_deref() == Some(&k.id) { theme::GOOD } else { SECONDARY }, 32.0));
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let title = match &k.owner {
                            Some(o) => format!("{o}'s {}", k.model),
                            None => k.model.clone(),
                        };
                        ui.label(RichText::new(title).font(semibold(16.0)).color(TEXT));
                        let status = if by_key {
                            if key_serial.as_deref() == Some(&k.id) { "Data key plugged in (above)" } else { "Downloads with the data key" }
                        } else {
                            "Downloads over WiFi · off or out of range"
                        };
                        ui.label(RichText::new(status).size(13.0).color(SECONDARY));
                        let newest = k.newest.map(|d| format!("  ·  newest {}", crate::events::us_short(d))).unwrap_or_default();
                        ui.label(RichText::new(format!("Logger {}  ·  {} sessions{newest}", k.id, k.sessions)).size(12.5).color(DIM));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let r = ui.add(egui::Label::new(RichText::new("How to download").size(13.0).color(TEXT).underline()).sense(Sense::click()));
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if r.clicked() {
                            go = Some(if by_key { DeviceView::DataKey } else { DeviceView::Wifi });
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }
        // WiFi MyChrons in range can be joined from here too.
        if !self.dev_networks.is_empty() {
            self.device_status(ui);
        }
        if let Some(v) = go {
            self.device_view = v;
        }
    }
}
