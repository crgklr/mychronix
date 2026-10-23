//! The Device page's MyChrons, and the "Add a MyChron" window: pick the
//! MyChron, follow the steps to connect it while Mychronix watches for it,
//! and see it found.

use super::*;
use garage_manage_ui::look::{self, Kind};

/// How a MyChron downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Way {
    Wifi,
    DataKey,
}

/// The "Add a MyChron" window, while open.
#[derive(Debug, Clone, Copy)]
pub(super) struct AddMyChron {
    /// None until a MyChron is picked (step 1).
    way: Option<Way>,
    /// The series picked, e.g. "MyChron 6".
    series: &'static str,
}

/// Height of the series cards, and of each step's content: every step is
/// as tall as the first (two rows of cards), so the window never resizes.
const CARD_H: f32 = 250.0;
const CARD_GAP: f32 = 16.0;
const STEP_H: f32 = CARD_H * 2.0 + CARD_GAP;

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

/// The series a person can pick: name, models, how it downloads, drawing.
const SERIES: [(&str, &str, Option<Way>, &str, (&str, &[u8])); 4] = [
    ("MyChron 6", "MyChron 6 and 6T", Some(Way::Wifi), "Downloads over WiFi or USB", ("bytes://mychron6.svg", include_bytes!("../../assets/devices/mychron6.svg"))),
    ("MyChron 5", "MyChron 5, 5S, 5 2T and 5S 2T", Some(Way::Wifi), "Downloads over WiFi", ("bytes://mychron5.svg", include_bytes!("../../assets/devices/mychron5.svg"))),
    ("MyChron 4", "MyChron 4, 4 2T and 4 660", Some(Way::DataKey), "Downloads with AiM's data key", ("bytes://mychron4.svg", include_bytes!("../../assets/devices/mychron4.svg"))),
    ("MyChron 3", "MyChron 3 and its variants", None, "Not supported yet (serial cable)", ("bytes://mychron3.svg", include_bytes!("../../assets/devices/mychron3.svg"))),
];

impl App {
    fn known_mychrons(&mut self) -> Vec<Known> {
        let lib = self.library.get_or_insert_with(Library::load);
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let digests = self.sensors.digests.get_or_insert_with(crate::digest::Digests::load);
        let mut ids: Vec<String> = lib.sessions.iter().map(|s| s.logger.clone()).collect();
        ids.extend(profiles.loggers.keys().cloned());
        ids.extend(profiles.garage_karts().map(|k| k.logger.clone()));
        // Only the Garage's MyChrons (on a kart in it) and new ones: removed
        // ones, and ones a kart had before, aren't shown.
        ids.retain(|i| !i.is_empty() && (profiles.mounted_on(i).is_some() || !profiles.knows_logger(i)));
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| {
                let mine: Vec<_> = lib.sessions.iter().filter(|s| s.logger == id).collect();
                let model = Some(profiles.for_logger(&id).model)
                    .filter(|m| !m.is_empty())
                    .or_else(|| digests.model(&id))
                    .or_else(|| mine.iter().find_map(|s| s.summary.model.clone()))
                    .unwrap_or_else(|| "MyChron".into());
                let owner = profiles
                    .kart_on(&id)
                    .and_then(|k| k.driver)
                    .and_then(|d| profiles.driver_name(Some(d)).map(str::to_string))
                    .or_else(|| Some(profiles.for_logger(&id).driver).filter(|d| !d.is_empty()));
                Known { sessions: mine.len(), newest: mine.iter().filter_map(|s| s.summary.date).max(), id, model, owner }
            })
            .collect()
    }

    /// Opens the "Add a MyChron" window, at the start or at a MyChron's steps.
    pub(super) fn open_add_mychron(&mut self, way: Option<Way>, series: &'static str) {
        self.add_mychron = Some(AddMyChron { way, series });
    }

    /// The Device page when no WiFi MyChron is connected: the MyChrons
    /// Mychronix knows, or a start for the first one.
    pub(super) fn device_guide(&mut self, ui: &mut Ui) {
        let known = self.known_mychrons();
        if known.is_empty() {
            self.device_empty(ui);
        } else {
            self.device_known(ui, &known);
        }
        // WiFi MyChrons in range can be joined from the page too.
        if !self.dev_networks.is_empty() && self.add_mychron.is_none() {
            ui.add_space(4.0);
            self.device_status(ui);
        }
    }

    /// No MyChrons yet: one clear way to start.
    fn device_empty(&mut self, ui: &mut Ui) {
        Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::same(40)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add(Icon::Mychron.image(SECONDARY, 56.0));
                ui.add_space(14.0);
                ui.label(RichText::new("Add your MyChron").font(semibold(22.0)).color(TEXT));
                ui.add_space(4.0);
                ui.label(RichText::new("Mychronix walks you through connecting it and downloads its sessions.").font(regular(15.0)).color(SECONDARY));
                ui.add_space(20.0);
                if look::big_button(ui, "Add a MyChron", Some(Icon::Plus), Kind::Primary, true) {
                    self.open_add_mychron(None, "");
                }
            });
        });
    }

    fn device_known(&mut self, ui: &mut Ui, known: &[Known]) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Your MyChrons").font(semibold(20.0)).color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, Some(Icon::Plus), "Add a MyChron", false).clicked() {
                    self.open_add_mychron(None, "");
                }
            });
        });
        ui.add_space(10.0);
        let key_serial = self.key.listing.as_ref().filter(|_| self.key.present).map(|l| l.0.clone());
        let mut open = None;
        for k in known {
            let by_key = k.model.starts_with("MyChron 4");
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
                            // USB checked on the MyChron 6 only.
                            if k.model.starts_with("MyChron 6") { "Downloads over WiFi or USB · not connected" } else { "Downloads over WiFi · off or out of range" }
                        };
                        ui.label(RichText::new(status).size(13.0).color(SECONDARY));
                        let newest = k.newest.map(|d| format!("  ·  newest {}", crate::events::us_short(d))).unwrap_or_default();
                        ui.label(RichText::new(format!("Logger {}  ·  {} sessions{newest}", k.id, k.sessions)).size(12.5).color(DIM));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(if by_key { Icon::Usb } else { Icon::Wifi }), "Connect", false).clicked() {
                            open = Some(if by_key { (Way::DataKey, "MyChron 4") } else { (Way::Wifi, "MyChron") });
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }
        if let Some((way, series)) = open {
            self.open_add_mychron(Some(way), series);
        }
    }

    /// The "Add a MyChron" window: 1 pick the MyChron, 2 connect it while
    /// Mychronix watches, 3 found.
    pub(super) fn add_mychron_dialog(&mut self, ctx: &egui::Context) {
        let Some(flow) = self.add_mychron else { return };
        let found = match flow.way {
            Some(Way::Wifi) => self.dev_logger.is_some(),
            Some(Way::DataKey) => self.key.present,
            None => false,
        };
        let step = match (flow.way, found) {
            (None, _) => 0,
            (Some(_), false) => 1,
            (Some(_), true) => 2,
        };
        // Found, and not on a kart yet: the window offers to set it up.
        let to_set_up = if step == 2 { self.found_needs_setup(flow.way) } else { None };
        let mut next = Some(flow);
        let mut close = false;
        let mut set_up = false;
        let width = (ctx.content_rect().width() - 160.0).clamp(720.0, 1080.0);
        let dismissed = look::dialog(ctx, "add_mychron", width, |ui| {
            let series = if flow.series.is_empty() { "MyChron" } else { flow.series };
            let (title, subtitle) = match (step, flow.way) {
                (0, _) => ("Which MyChron do you have?".to_string(), "Each one connects a little differently. Mychronix shows you how.".to_string()),
                (1, Some(Way::Wifi)) => (format!("Connect your {series}"), format!("{WIFI_MODELS}: over the MyChron's own WiFi.")),
                (1, _) => ("Plug in the data key".to_string(), format!("{KEY_MODELS}: AiM's data key carries the sessions from the MyChron to this computer.")),
                (_, Some(Way::Wifi)) => ("MyChron connected".to_string(), "Its sessions are on the Device page, ready to download.".to_string()),
                _ => ("Data key found".to_string(), "Its runs are on the Device page, ready to download.".to_string()),
            };
            let (icon, tint) = if step == 2 { (Icon::CircleCheck, theme::GOOD) } else { (Icon::Mychron, TEXT) };
            look::header(ui, icon, tint, Some("Add a MyChron"), &title, &subtitle);
            look::steps(ui, &["Your MyChron", "Connect", "Done"], step, false);
            look::body(ui, |ui| {
                ui.set_min_height(STEP_H);
                match step {
                0 => {
                    if let Some((name, way)) = Self::series_cards(ui) {
                        next = Some(AddMyChron { way: Some(way), series: name });
                    }
                }
                1 if flow.way == Some(Way::Wifi) => {
                    // A Mac doesn't let apps list WiFi networks, so there the person joins it from the menu bar.
                    let steps: [&str; 3] = if cfg!(target_os = "macos") {
                        [
                            "Turn the MyChron on and switch its WiFi on (in the MyChron's own menu).",
                            "Keep it near this Mac and join its network (named AiM-MYC…) from the WiFi menu in the menu bar.",
                            "Mychronix finds the MyChron and lists its sessions. Rejoin your own network when you're done.",
                        ]
                    } else {
                        [
                            "Turn the MyChron on and switch its WiFi on (in the MyChron's own menu).",
                            "Keep it near this computer. Its network (named AiM-MYC…) shows up below.",
                            "Click Join. Mychronix connects, finds the MyChron and lists its sessions. Disconnect when you're done to get back on your own network.",
                        ]
                    };
                    Self::centered(ui, |ui| {
                        Self::numbered(ui, &steps);
                        ui.add_space(6.0);
                        self.wifi_search(ui);
                    });
                }
                1 => Self::centered(ui, |ui| {
                    Self::numbered(
                        ui,
                        &[
                            "At the kart, with the MyChron switched off, plug the data key into it, then switch the MyChron on. Its screen says \"Downloading data\" for a few seconds while it copies the new sessions to the key.",
                            "Unplug the key from the MyChron and plug its USB end into this computer.",
                            "Mychronix finds the key and lists its runs. Nothing on the key or the MyChron is changed.",
                        ],
                    );
                    ui.add_space(6.0);
                    let note = if cfg!(target_os = "linux") {
                        "On Linux, the first time, Mychronix may ask to allow USB access: one click and your password."
                    } else {
                        "Plug it into any USB port."
                    };
                    Self::searching(ui, "Waiting for the data key…", note);
                }),
                _ => self.found_panel(ui, flow.way),
                }
            });
            look::footer_row(ui, |ui| {
                let back = match step {
                    1 => "Back",
                    2 if to_set_up.is_some() => "Skip for now",
                    _ => "Close",
                };
                if look::big_button(ui, back, None, Kind::Quiet, true) {
                    if step == 1 {
                        next = Some(AddMyChron { way: None, series: "" });
                    } else {
                        close = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if step == 2 && to_set_up.is_some() {
                        if look::big_button(ui, "Set it up", Some(Icon::ChevronRight), Kind::Primary, true) {
                            set_up = true;
                        }
                    } else if step == 2 && look::big_button(ui, "See its sessions", Some(Icon::ChevronRight), Kind::Primary, true) {
                        close = true;
                    }
                });
            });
        });
        self.add_mychron = if close || dismissed || set_up { None } else { next };
        // On to "Whose MyChron is it?" and "Which kart?".
        if let (true, Some((id, model))) = (set_up, to_set_up) {
            self.mychron_setup = Some(garage_manage_ui::MyChronSetup::new(&id, &model));
        }
    }

    /// The MyChron just found (its id and model), when it isn't on a kart yet.
    fn found_needs_setup(&mut self, way: Option<Way>) -> Option<(String, String)> {
        let (id, model) = match way {
            Some(Way::Wifi) => (
                self.dev_logger.as_ref().and_then(|b| b.logger_id)?.to_string(),
                self.dev_joined.as_deref().or(self.dev_current_wifi.as_deref()).map(model_from_ssid).unwrap_or("MyChron"),
            ),
            _ => (self.key.listing.as_ref()?.0.clone(), "MyChron 4"),
        };
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
        profiles.mounted_on(&id).is_none().then(|| (id, model.to_string()))
    }

    /// The four series as cards, two to a row. Returns the one picked.
    fn series_cards(ui: &mut Ui) -> Option<(&'static str, Way)> {
        let mut pick = None;
        // Exactly STEP_H tall: two rows and the gap, no extra spacing.
        ui.spacing_mut().item_spacing.y = 0.0;
        let gap = CARD_GAP;
        let w = (ui.available_width() - gap) / 2.0;
        for row in SERIES.chunks(2) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &(name, models, way, how, (uri, svg)) in row {
                    let enabled = way.is_some();
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, CARD_H), if enabled { Sense::click() } else { Sense::hover() });
                    let hover = enabled && resp.hovered();
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius::same(14), if hover { SURFACE_HOVER } else { SURFACE });
                    if hover {
                        p.rect_stroke(rect, CornerRadius::same(14), Stroke::new(1.5, SECONDARY), egui::StrokeKind::Inside);
                    }
                    let cx = rect.center().x;
                    let pic = Rect::from_center_size(pos2(cx, rect.top() + 88.0), vec2(220.0, 128.0));
                    let tint = if enabled { Color32::WHITE } else { Color32::from_white_alpha(70) };
                    egui::Image::from_bytes(uri, svg).tint(tint).paint_at(ui, pic);
                    let (title, sub) = if enabled { (TEXT, SECONDARY) } else { (DIM, DIM.gamma_multiply(0.8)) };
                    let p = ui.painter();
                    p.text(pos2(cx, rect.top() + 174.0), egui::Align2::CENTER_CENTER, format!("{name} Series"), semibold(19.0), title);
                    p.text(pos2(cx, rect.top() + 200.0), egui::Align2::CENTER_CENTER, models, regular(14.0), sub);
                    p.text(pos2(cx, rect.top() + 223.0), egui::Align2::CENTER_CENTER, how, regular(13.0), if enabled { DIM } else { sub });
                    if hover {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        pick = way.map(|w| (name, w));
                    }
                }
            });
            if row.as_ptr() == SERIES[0..2].as_ptr() {
                ui.add_space(gap);
            }
        }
        pick
    }

    /// A step's content as a column of comfortable line length, centered
    /// both ways in the step's fixed space. Its height is measured each
    /// frame and used for the next, so it settles at once.
    fn centered(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
        let id = ui.id().with("centered_height");
        let measured: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(STEP_H);
        let w = ui.available_width().min(780.0);
        let side = (ui.available_width() - w) / 2.0;
        ui.add_space(((STEP_H - measured) / 2.0).max(0.0));
        let r = ui.horizontal_top(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_width(w);
                add(ui);
            })
            .response
        });
        let h = r.inner.rect.height();
        if (h - measured).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, h));
            ui.ctx().request_repaint();
        }
    }

    /// Numbered steps, sized for the window: large text with room between.
    fn numbered(ui: &mut Ui, steps: &[&str]) {
        for (k, s) in steps.iter().enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 19.0, SURFACE_ACTIVE);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, (k + 1).to_string(), semibold(17.0), TEXT);
                ui.vertical(|ui| {
                    // Line the first line up with the middle of the number.
                    ui.add_space(7.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.add(egui::Label::new(RichText::new(*s).font(regular(18.0)).color(TEXT).line_height(Some(27.0))).wrap());
                });
            });
            ui.add_space(20.0);
        }
    }

    /// A live "still looking" panel: a spinner, a title and a line.
    fn searching(ui: &mut Ui, title: &str, detail: &str) {
        Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(14)).inner_margin(Margin::symmetric(26, 24)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 20.0;
                ui.add(egui::Spinner::new().size(32.0).color(SECONDARY));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    ui.label(RichText::new(title).font(semibold(19.0)).color(TEXT));
                    ui.add(egui::Label::new(RichText::new(detail).font(regular(15.5)).color(SECONDARY)).wrap());
                });
            });
        });
    }

    /// Step 2 over WiFi: looking for MyChron networks, then each one with a Join button.
    fn wifi_search(&mut self, ui: &mut Ui) {
        if let Some(busy) = self.dev_busy.clone() {
            Self::searching(ui, &busy, "This takes a few seconds.");
            return;
        }
        if self.dev_networks.is_empty() {
            Self::searching(ui, "Looking for your MyChron…", crate::device::JOIN_HINT);
            if let Some((false, msg)) = &self.dev_message {
                ui.add_space(4.0);
                look::note(ui, msg, ui.visuals().error_fg_color);
            }
            return;
        }
        let current = self.dev_current_wifi.clone().unwrap_or_else(|| "your current network".into());
        ui.label(RichText::new(format!("Found in range. Joining one takes this computer off {current} until you disconnect.")).font(regular(15.5)).color(SECONDARY));
        ui.add_space(4.0);
        for (ssid, signal) in self.dev_networks.clone() {
            Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(14)).inner_margin(Margin::symmetric(24, 18)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    ui.add(Icon::Wifi.image(theme::GOOD, 28.0));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new(model_from_ssid(&ssid)).font(semibold(19.0)).color(TEXT));
                        ui.label(RichText::new(format!("{ssid}  ·  {signal}% signal")).font(regular(15.5)).color(SECONDARY));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if look::big_button(ui, "Join", None, Kind::Primary, self.dev_busy.is_none()) {
                            self.dev_busy = Some(format!("Joining {ssid}…"));
                            self.dev_message = None;
                            if let Some(dev) = &self.dev {
                                let _ = dev.tx.send(crate::device::Command::Join { ssid: ssid.clone() });
                            }
                        }
                    });
                });
            });
        }
    }

    /// Step 3: what was found.
    fn found_panel(&mut self, ui: &mut Ui, way: Option<Way>) {
        let (title, detail) = match way {
            Some(Way::Wifi) => {
                let id = self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()).unwrap_or_default();
                let model = self.dev_joined.as_deref().or(self.dev_current_wifi.as_deref()).map(model_from_ssid).unwrap_or("MyChron");
                let n = self.new_on_logger().len();
                (format!("{model}  ·  logger {id}"), if n == 0 { "Everything on it is already downloaded.".to_string() } else { format!("{n} new session{} to download.", if n == 1 { "" } else { "s" }) })
            }
            _ => match &self.key.listing {
                Some((serial, total, new)) => (format!("MyChron 4  ·  logger {serial}"), format!("{total} runs on the key, {} new.", new.len())),
                None => ("MyChron 4 data key".to_string(), "Reading its runs…".to_string()),
            },
        };
        // Centered in the step's space: the MyChron, what it is, what's on it.
        let new_to_us = {
            let id = match way {
                Some(Way::Wifi) => self.dev_logger.as_ref().and_then(|b| b.logger_id).map(|i| i.to_string()),
                _ => self.key.listing.as_ref().map(|l| l.0.clone()),
            };
            let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load);
            id.is_some_and(|i| !profiles.knows_logger(&i))
        };
        ui.add_space(STEP_H / 2.0 - 120.0);
        ui.vertical_centered(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 48.0, theme::GOOD.gamma_multiply(0.15));
            Icon::Mychron.image(theme::GOOD, 48.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(48.0)));
            ui.add_space(16.0);
            ui.label(RichText::new(title).font(semibold(22.0)).color(TEXT));
            ui.add_space(4.0);
            ui.label(RichText::new(detail).font(regular(16.0)).color(SECONDARY));
            if new_to_us {
                ui.add_space(14.0);
                ui.label(RichText::new("It's new to Mychronix. Next, tell it whose MyChron this is and which kart it's on.").font(regular(14.5)).color(DIM));
            }
        });
    }
}
