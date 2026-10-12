//! Events page: sessions grouped into events (RaceStudio's week + track +
//! championship rule, or events you name and plan), with each driver's stats.

use super::*;
use crate::events::{self, Event, EventDef, Events};

/// One driver's numbers for an event, from on-track sessions only.
struct DriverStats {
    name: String,
    karts: Vec<String>,
    sessions: usize,
    warm_ups: usize,
    laps: u32,
    time_ms: u64,
    /// (lap time, session index, lap number)
    fastest: Option<(u64, usize, u32)>,
    peak_rpm: Option<u32>,
    top_speed: Option<f64>,
    /// None while a session is still being read.
    peak_egt: Option<Option<f64>>,
    /// (session index, best lap) per on-track session with a best lap, oldest first.
    bests: Vec<(usize, u64)>,
}

impl App {
    pub(super) fn events_page(&mut self, ui: &mut Ui) {
        let lib = self.library.take().unwrap_or_else(Library::load);
        let profiles = self.profiles.take().unwrap_or_else(crate::profiles::Profiles::load);
        let mut defs = self.events.take().unwrap_or_else(Events::load);
        if let Some(rx) = &self.peak_egt_rx {
            while let Ok((path, v)) = rx.try_recv() {
                self.peak_egt.insert(path, v);
            }
        }

        let all = events::group(&lib, &profiles, &defs);
        let today = events::today();
        let mut championships: Vec<String> = all.iter().map(|e| e.championship.clone()).filter(|c| !c.trim().is_empty()).collect();
        championships.sort();
        championships.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let shown: Vec<&Event> = all
            .iter()
            .filter(|e| self.event_champ.as_ref().is_none_or(|c| e.championship.eq_ignore_ascii_case(c)))
            .collect();
        let upcoming = |e: &Event| e.start > today || (e.sessions.is_empty() && e.end >= today);

        // Header.
        // MYCHRONIX_EVENT_DIALOG opens the plan dialog (for screenshots).
        let mut plan = self.event_dialog.is_none() && self.frames < 3 && std::env::var_os("MYCHRONIX_EVENT_DIALOG").is_some();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Events").font(semibold(28.0)).color(TEXT));
                let n_up = all.iter().filter(|e| upcoming(e)).count();
                let mut sub = format!("{} event{}", all.len(), if all.len() == 1 { "" } else { "s" });
                if n_up > 0 {
                    sub.push_str(&format!("  ·  {n_up} coming up"));
                }
                if !championships.is_empty() {
                    sub.push_str(&format!("  ·  {} championship{}", championships.len(), if championships.len() == 1 { "" } else { "s" }));
                }
                ui.label(RichText::new(sub).font(regular(14.5)).color(SECONDARY));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, Some(Icon::Calendar), "Plan an event", true).clicked() {
                    plan = true;
                }
            });
        });
        ui.add_space(14.0);
        if !championships.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                if chip(ui, "All championships", self.event_champ.is_none()).clicked() {
                    self.event_champ = None;
                }
                for c in &championships {
                    let on = self.event_champ.as_ref().is_some_and(|x| x.eq_ignore_ascii_case(c));
                    if chip(ui, c, on).clicked() {
                        self.event_champ = Some(c.clone());
                    }
                }
            });
            ui.add_space(12.0);
        }

        if all.is_empty() {
            card(ui, |ui| {
                ui.label(RichText::new("No events yet").font(semibold(17.0)).color(TEXT));
                ui.label(
                    RichText::new("Sessions you download are grouped into events by week, track and championship. You can also plan an event ahead of time.")
                        .color(SECONDARY),
                );
            });
        } else {
            // MYCHRONIX_EVENT=<name> preselects an event, MYCHRONIX_CHAMPIONSHIP a championship (for screenshots).
            if self.frames < 3 {
                if let Ok(c) = std::env::var("MYCHRONIX_CHAMPIONSHIP") {
                    self.event_champ = Some(c);
                }
                if let Ok(want) = std::env::var("MYCHRONIX_EVENT") {
                    self.event_sel = shown.iter().find(|e| e.title().eq_ignore_ascii_case(&want)).map(|e| e.key());
                }
            }
            // Keep the selection; default to the newest event that has sessions.
            if !shown.iter().any(|e| Some(e.key()) == self.event_sel) {
                self.event_sel = shown.iter().find(|e| !e.sessions.is_empty()).or(shown.first()).map(|e| e.key());
            }
            let gap = 14.0;
            let height = ui.available_height();
            let list_w = 380.0_f32.min(ui.available_width() * 0.36);
            let detail_w = ui.available_width() - list_w - gap;
            let column = Layout::top_down(Align::Min);
            let mut action = None;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.allocate_ui_with_layout(vec2(list_w, height), column, |ui| {
                    ui.set_width(list_w);
                    egui::ScrollArea::vertical().id_salt("event_list").auto_shrink([false, false]).show(ui, |ui| {
                        if let Some(champ) = &self.event_champ {
                            season_card(ui, &lib, &profiles, champ, &shown);
                            ui.add_space(12.0);
                        }
                        for (label, up) in [("Coming up", true), ("Past events", false)] {
                            let group: Vec<&&Event> = shown.iter().filter(|e| upcoming(e) == up).collect();
                            if group.is_empty() {
                                continue;
                            }
                            ui.label(RichText::new(label).font(medium(13.0)).color(DIM));
                            ui.add_space(4.0);
                            // Soonest first for upcoming events.
                            let order: Vec<&&Event> = if up { group.into_iter().rev().collect() } else { group };
                            for e in order {
                                let selected = Some(e.key()) == self.event_sel;
                                if event_row(ui, &lib, &profiles, e, selected, today).clicked() {
                                    self.event_sel = Some(e.key());
                                }
                            }
                            ui.add_space(12.0);
                        }
                        if shown.is_empty() {
                            ui.label(RichText::new("No events in this championship.").color(DIM));
                        }
                    });
                });
                ui.allocate_ui_with_layout(vec2(detail_w, height), column, |ui| {
                    ui.set_width(detail_w);
                    if let Some(e) = shown.iter().find(|e| Some(e.key()) == self.event_sel) {
                        egui::ScrollArea::vertical().id_salt("event_detail").auto_shrink([false, false]).show(ui, |ui| {
                            action = self.event_detail(ui, &lib, &profiles, e, today);
                        });
                    }
                });
            });
            match action {
                Some(DetailAction::Open(path, lap)) => {
                    self.library = Some(lib);
                    self.profiles = Some(profiles);
                    self.events = Some(defs);
                    self.load(path);
                    if let (Some(n), Some(s)) = (lap, &self.session) {
                        if let Some(i) = s.laps.iter().position(|l| l.number as u32 == n) {
                            self.lap = Some(i);
                            self.cursor_s = Some(s.laps[i].duration_ms() as f64 / 2000.0);
                        }
                    }
                    return;
                }
                Some(DetailAction::Edit(def)) => self.event_dialog = Some(def),
                None => {}
            }
        }
        if plan {
            let start = events::iso(events::from_days(events::days(today) + 7));
            self.event_dialog = Some(EventDef {
                start: start.clone(),
                end: start,
                championship: self.event_champ.clone().unwrap_or_default(),
                ..Default::default()
            });
        }
        self.event_dialog(ui.ctx(), &lib, &profiles, &mut defs);
        self.library = Some(lib);
        self.profiles = Some(profiles);
        self.events = Some(defs);
    }

    /// The selected event: title, a card per driver, best laps through the event, sessions by day.
    fn event_detail(&mut self, ui: &mut Ui, lib: &Library, profiles: &crate::profiles::Profiles, e: &Event, today: aim_xrk::summary::Date) -> Option<DetailAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(e.title()).font(semibold(24.0)).color(TEXT));
                let mut sub = vec![date_range(e.start, e.end)];
                if e.def.as_ref().is_some_and(|d| !d.name.trim().is_empty()) {
                    sub.insert(0, e.track.clone());
                }
                if !e.championship.trim().is_empty() {
                    sub.push(e.championship.clone());
                }
                ui.label(RichText::new(sub.join("  ·  ")).font(regular(14.5)).color(SECONDARY));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (label, def) = match &e.def {
                    Some(d) => ("Edit event", d.clone()),
                    None => (
                        "Name this event",
                        EventDef {
                            track: e.track.clone(),
                            start: events::iso(e.start),
                            end: events::iso(e.end),
                            championship: e.championship.clone(),
                            ..Default::default()
                        },
                    ),
                };
                if button(ui, None, label, false).clicked() {
                    action = Some(DetailAction::Edit(def));
                }
            });
        });
        if let Some(notes) = e.def.as_ref().map(|d| d.notes.trim()).filter(|n| !n.is_empty()) {
            ui.add_space(4.0);
            ui.label(RichText::new(notes).color(SECONDARY));
        }
        ui.add_space(14.0);

        if e.sessions.is_empty() {
            card(ui, |ui| {
                let until = events::days(e.start) - events::days(today);
                let when = match until {
                    0 => "Today".to_string(),
                    1 => "Tomorrow".to_string(),
                    n if n > 1 => format!("In {n} days"),
                    _ => "No sessions were downloaded for this event".to_string(),
                };
                ui.label(RichText::new(when).font(semibold(17.0)).color(TEXT));
                // Previous best at this track, any driver.
                let best = lib
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| events::session_key(lib, *i, profiles).is_some_and(|k| k.track.eq_ignore_ascii_case(&e.track)))
                    .filter_map(|(_, s)| s.summary.best_lap_ms.map(|b| (b, s)))
                    .min_by_key(|(b, _)| *b);
                match best {
                    Some((b, s)) => {
                        ui.label(
                            RichText::new(format!(
                                "Best lap here so far: {} by {}, {}.",
                                format_lap_time(b as i64),
                                driver_for(s, profiles),
                                s.summary.date.map(long_date).unwrap_or_default()
                            ))
                            .color(SECONDARY),
                        );
                    }
                    None => {
                        ui.label(RichText::new(format!("No laps at {} yet.", e.track)).color(SECONDARY));
                    }
                }
                ui.label(
                    RichText::new(format!("Sessions you download from {} between these dates join this event automatically.", e.track))
                        .size(12.5)
                        .color(DIM),
                );
            });
            return action;
        }

        // Peak EGT comes from the session files; ask for any not read yet.
        let need: Vec<PathBuf> = e
            .sessions
            .iter()
            .map(|&i| lib.sessions[i].path.clone())
            .filter(|p| !self.peak_egt.contains_key(p) && !self.peak_egt_asked.contains(p))
            .collect();
        if !need.is_empty() {
            self.peak_egt_asked.extend(need.iter().cloned());
            let rx = events::spawn_peak_egt(need, ui.ctx().clone());
            // Merge with any reader still running.
            let old = self.peak_egt_rx.take();
            let (tx, merged) = std::sync::mpsc::channel();
            for r in [old, Some(rx)].into_iter().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    while let Ok(m) = r.recv() {
                        if tx.send(m).is_err() {
                            break;
                        }
                    }
                });
            }
            self.peak_egt_rx = Some(merged);
        }

        let stats = driver_stats(lib, profiles, e, &self.peak_egt);
        let setups = crate::setup::resolve(lib, self.setup_baselines.get_or_insert_with(crate::setup::Baselines::load));
        for (k, d) in stats.iter().enumerate() {
            // Setup at the start of the event, then each change during it.
            let mine: Vec<usize> = e.sessions.iter().copied().filter(|&i| driver_for(&lib.sessions[i], profiles).eq_ignore_ascii_case(&d.name)).collect();
            let mut setup_lines = Vec::new();
            if let Some(&first) = mine.first() {
                let start = setups[first].setup.summary();
                if !start.is_empty() {
                    setup_lines.push((false, format!("Setup: {start}")));
                }
                for &i in &mine[1..] {
                    if !setups[i].changes.is_empty() {
                        setup_lines.push((true, format!("Session {}: {}", lib.sessions[i].summary.number.unwrap_or(0), setups[i].changes.join(", "))));
                    }
                }
                if !setups[first].changes.is_empty() {
                    setup_lines.insert(1.min(setup_lines.len()), (true, format!("Changed for this event: {}", setups[first].changes.join(", "))));
                }
            }
            if let Some(a) = driver_card(ui, lib, d, SERIES[k % SERIES.len()], &setup_lines) {
                action = Some(a);
            }
            ui.add_space(12.0);
        }

        // Best lap per session, one line per driver.
        let with_bests: Vec<(usize, &DriverStats)> = stats.iter().enumerate().filter(|(_, d)| d.bests.len() >= 2).collect();
        if !with_bests.is_empty() {
            // Only sessions with a lap time, so warm-ups don't leave gaps.
            let order: Vec<usize> = e.sessions.iter().copied().filter(|i| stats.iter().any(|d| d.bests.iter().any(|(x, _)| x == i))).collect();
            let numbers: Vec<u32> = order.iter().map(|&i| lib.sessions[i].summary.number.unwrap_or(0)).collect();
            graph_card(ui, 240.0, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Best lap by session").font(medium(14.5)).color(TEXT));
                    ui.label(RichText::new("s").size(13.0).color(DIM));
                });
                Plot::new(("event_bests", e.key()))
                    .height(ui.available_height())
                    .show_background(false)
                    .grid_color(GRID)
                    .y_axis_min_width(46.0)
                    .allow_zoom(false)
                    .allow_drag(false)
                    .allow_scroll(false)
                    .x_axis_formatter(move |mark, _| {
                        let i = mark.value.round() as i64;
                        if (mark.value - i as f64).abs() < 1e-6 && i >= 0 && (i as usize) < numbers.len() {
                            format!("S{}", numbers[i as usize])
                        } else {
                            String::new()
                        }
                    })
                    .show_x(false)
                    .show_y(false)
                    .show(ui, |plot| {
                        for (k, d) in &with_bests {
                            let pts: Vec<[f64; 2]> = d
                                .bests
                                .iter()
                                .filter_map(|(i, b)| order.iter().position(|x| x == i).map(|x| [x as f64, *b as f64 / 1000.0]))
                                .collect();
                            let color = SERIES[*k % SERIES.len()];
                            plot.line(Line::new(d.name.clone(), PlotPoints::from(pts.clone())).color(color).width(2.0));
                            plot.points(egui_plot::Points::new(format!("{} laps", d.name), PlotPoints::from(pts)).color(color).radius(4.0));
                        }
                    });
            });
            ui.add_space(14.0);
        }

        // Sessions by day, with setup changes flagged.
        let setups = crate::setup::resolve(lib, self.setup_baselines.get_or_insert_with(crate::setup::Baselines::load));
        let mut days: Vec<aim_xrk::summary::Date> = e.sessions.iter().filter_map(|&i| lib.sessions[i].summary.date).collect();
        days.dedup();
        for day in days {
            let in_day: Vec<usize> = e.sessions.iter().copied().filter(|&i| lib.sessions[i].summary.date == Some(day)).collect();
            let best = in_day
                .iter()
                .filter(|&&i| !events::is_warm_up(&lib.sessions[i].summary))
                .filter_map(|&i| lib.sessions[i].summary.best_lap_ms)
                .min();
            ui.horizontal(|ui| {
                ui.add(Icon::Calendar.image(SECONDARY, 16.0));
                ui.label(RichText::new(long_date(day)).font(semibold(16.0)).color(TEXT));
                ui.label(RichText::new(format!("{} session{}", in_day.len(), if in_day.len() == 1 { "" } else { "s" })).color(SECONDARY));
            });
            ui.add_space(6.0);
            // Newest first, like the Sessions page.
            for &i in in_day.iter().rev() {
                let s = &lib.sessions[i];
                let mut who = driver_for(s, profiles);
                if let Some(t) = s.meta.as_ref().map(|m| m.session_type.trim()).filter(|t| !t.is_empty()) {
                    who = format!("{t}  ·  {who}");
                }
                if events::is_warm_up(&s.summary) {
                    who = format!("Warm-up  ·  {who}");
                }
                if session_row(ui, lib, i, &who, best, !setups[i].changes.is_empty()).clicked() {
                    action = Some(DetailAction::Open(s.path.clone(), None));
                }
            }
            ui.add_space(12.0);
        }
        action
    }

    /// Plan, name or edit an event.
    fn event_dialog(&mut self, ctx: &egui::Context, lib: &Library, profiles: &crate::profiles::Profiles, defs: &mut Events) {
        let Some(mut def) = self.event_dialog.take() else { return };
        let s = Suggestions::gather(lib, profiles);
        let mut championships = s.championships.clone();
        for c in lib.sessions.iter().enumerate().filter_map(|(i, _)| events::session_key(lib, i, profiles)).map(|k| k.championship) {
            if !c.trim().is_empty() && !championships.iter().any(|x| x.eq_ignore_ascii_case(&c)) {
                championships.push(c);
            }
        }
        let existing = defs.events.iter().any(|d| d.id == def.id && def.id != 0);
        let title = if existing { "Edit event" } else if def.track.is_empty() { "Plan an event" } else { "Name this event" };
        let (mut open, mut save, mut cancel, mut delete) = (true, false, false, false);
        let field_w = 400.0;
        egui::Window::new(RichText::new(title).font(semibold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.add_space(6.0);
                egui::Grid::new("event_fields").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    let label = |ui: &mut Ui, t: &str| ui.label(RichText::new(t).font(medium(15.0)).color(SECONDARY));
                    label(ui, "Name");
                    suggest_field(ui, "ev_name", &mut def.name, "e.g. New England Championship round 3", &[], field_w);
                    ui.end_row();
                    label(ui, "Track");
                    suggest_field(ui, "ev_track", &mut def.track, "Track name as your MyChron uses it", &s.tracks, field_w);
                    ui.end_row();
                    label(ui, "Starts");
                    suggest_field(ui, "ev_start", &mut def.start, "YYYY-MM-DD", &[], field_w);
                    ui.end_row();
                    label(ui, "Ends");
                    suggest_field(ui, "ev_end", &mut def.end, "YYYY-MM-DD", &[], field_w);
                    ui.end_row();
                    label(ui, "Championship");
                    suggest_field(ui, "ev_champ", &mut def.championship, "Optional", &championships, field_w);
                    ui.end_row();
                    label(ui, "Notes");
                    ui.add(
                        egui::TextEdit::multiline(&mut def.notes)
                            .font(regular(15.0))
                            .margin(Margin::symmetric(12, 9))
                            .desired_width(field_w)
                            .desired_rows(3)
                            .hint_text("Entry, class, tyres, anything to remember"),
                    );
                    ui.end_row();
                });
                ui.add_space(10.0);
                let problem = if def.name.trim().is_empty() {
                    Some("Give the event a name.")
                } else if def.track.trim().is_empty() {
                    Some("Choose a track.")
                } else if events::parse_date(&def.start).is_none() || events::parse_date(&def.end).is_none() {
                    Some("Dates look like 2026-10-17.")
                } else {
                    None
                };
                ui.label(
                    RichText::new(problem.unwrap_or("Sessions downloaded from this track between these dates join this event automatically."))
                        .size(12.5)
                        .color(if problem.is_some() { SECONDARY } else { DIM }),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if existing && button(ui, None, "Delete event", false).clicked() {
                        delete = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(problem.is_none(), |ui| {
                            if button(ui, Some(Icon::Check), "Save", true).clicked() {
                                save = true;
                            }
                        });
                        if button(ui, None, "Cancel", false).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if save {
            // Store dates in one form.
            for d in [&mut def.start, &mut def.end] {
                if let Some(p) = events::parse_date(d) {
                    *d = events::iso(p);
                }
            }
            let id = defs.upsert(def);
            self.event_sel = Some(format!("def:{id}"));
            if let Err(e) = defs.save() {
                self.error = Some(format!("Couldn't save events: {e}"));
            }
        } else if delete {
            defs.events.retain(|d| d.id != def.id);
            self.event_sel = None;
            if let Err(e) = defs.save() {
                self.error = Some(format!("Couldn't save events: {e}"));
            }
        } else if open && !cancel {
            self.event_dialog = Some(def);
        }
    }
}

enum DetailAction {
    /// Open a session, optionally on a lap number.
    Open(PathBuf, Option<u32>),
    Edit(EventDef),
}

/// The session's driver: entered at download, else the logger's saved name.
fn driver_for(s: &tracks::LocalSession, profiles: &crate::profiles::Profiles) -> String {
    s.meta
        .as_ref()
        .map(|m| m.driver.clone())
        .filter(|d| !d.trim().is_empty())
        .or_else(|| profiles.get(&s.logger).map(|p| p.driver.clone()).filter(|d| !d.trim().is_empty()))
        .unwrap_or_else(|| format!("Logger {}", s.logger))
}

fn driver_stats(
    lib: &Library,
    profiles: &crate::profiles::Profiles,
    e: &Event,
    peak_egt: &std::collections::HashMap<PathBuf, Option<f64>>,
) -> Vec<DriverStats> {
    let mut out: Vec<DriverStats> = Vec::new();
    for &i in &e.sessions {
        let s = &lib.sessions[i];
        let name = driver_for(s, profiles);
        let d = match out.iter_mut().position(|d| d.name.eq_ignore_ascii_case(&name)) {
            Some(k) => &mut out[k],
            None => {
                out.push(DriverStats {
                    name,
                    karts: vec![],
                    sessions: 0,
                    warm_ups: 0,
                    laps: 0,
                    time_ms: 0,
                    fastest: None,
                    peak_rpm: None,
                    top_speed: None,
                    peak_egt: Some(None),
                    bests: vec![],
                });
                out.last_mut().unwrap()
            }
        };
        let kart = s
            .meta
            .as_ref()
            .map(|m| m.kart.clone())
            .filter(|k| !k.trim().is_empty())
            .or_else(|| profiles.get(&s.logger).map(|p| p.kart.clone()).filter(|k| !k.trim().is_empty()));
        if let Some(k) = kart.filter(|k| !d.karts.contains(k)) {
            d.karts.push(k);
        }
        let sum = &s.summary;
        if events::is_warm_up(sum) {
            d.warm_ups += 1;
            continue;
        }
        d.sessions += 1;
        d.laps += sum.laps.unwrap_or(0);
        d.time_ms += sum.duration_ms.unwrap_or(0);
        if let Some(b) = sum.best_lap_ms {
            d.bests.push((i, b));
            if d.fastest.is_none_or(|(f, _, _)| b < f) {
                d.fastest = Some((b, i, sum.best_lap_number.unwrap_or(0)));
            }
        }
        d.peak_rpm = d.peak_rpm.max(sum.max_rpm);
        if let Some(v) = sum.max_speed {
            d.top_speed = Some(d.top_speed.map_or(v, |t: f64| t.max(v)));
        }
        d.peak_egt = match (d.peak_egt, peak_egt.get(&s.path)) {
            (None, _) | (_, None) => None,
            (Some(a), Some(b)) => Some(match (a, *b) {
                (Some(x), Some(y)) => Some(x.max(y)),
                (x, y) => x.or(y),
            }),
        };
    }
    out
}

/// A driver's card: name and kart, then fastest lap, sessions, laps, time, peaks.
fn driver_card(ui: &mut Ui, lib: &Library, d: &DriverStats, color: Color32, setup_lines: &[(bool, String)]) -> Option<DetailAction> {
    let mut action = None;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            let (sw, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(sw.center(), 5.0, color);
            ui.label(RichText::new(&d.name).font(semibold(17.0)).color(TEXT));
            if !d.karts.is_empty() {
                ui.label(RichText::new(d.karts.join(", ")).color(SECONDARY));
            }
        });
        ui.add_space(10.0);
        let dash = || "—".to_string();
        let warm = if d.warm_ups > 0 { format!("+ {} warm-up{}", d.warm_ups, if d.warm_ups == 1 { "" } else { "s" }) } else { String::new() };
        let fastest_note = d
            .fastest
            .map(|(_, i, lap)| format!("Session {}, lap {lap}", lib.sessions[i].summary.number.unwrap_or(0)))
            .unwrap_or_default();
        let egt = match d.peak_egt {
            None => "…".to_string(),
            Some(Some(v)) => format!("{:.0} {}", crate::units::temp(v), crate::units::temp_unit()),
            Some(None) => dash(),
        };
        let tiles: [(&str, String, String, bool); 7] = [
            ("Fastest lap", d.fastest.map(|(b, _, _)| format_lap_time(b as i64)).unwrap_or_else(dash), fastest_note, true),
            ("Sessions", d.sessions.to_string(), warm, false),
            ("Laps", d.laps.to_string(), String::new(), false),
            ("On track", if d.time_ms > 0 { format_duration(d.time_ms as i64) } else { dash() }, String::new(), false),
            ("Peak RPM", d.peak_rpm.map(|r| group_thousands(r as usize)).unwrap_or_else(dash), String::new(), false),
            ("Peak EGT", egt, String::new(), false),
            ("Top speed", d.top_speed.map(|v| format!("{:.1} {}", crate::units::speed(v), crate::units::speed_unit())).unwrap_or_else(dash), String::new(), false),
        ];
        let w = ui.available_width();
        // All in one row when there's room, else 4 + 3.
        let per_row = if w > 1100.0 { 7 } else if w > 520.0 { 4 } else { 3 };
        let tile_w = (w - 8.0 * (per_row - 1) as f32) / per_row as f32;
        for chunk in tiles.chunks(per_row) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (label, value, note, is_fastest) in chunk {
                    let clickable = *is_fastest && d.fastest.is_some();
                    let (rect, resp) = ui.allocate_exact_size(vec2(tile_w, 66.0), if clickable { Sense::click() } else { Sense::hover() });
                    let p = ui.painter();
                    p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL + 2), if clickable && resp.hovered() { SURFACE_HOVER } else { SURFACE_ACTIVE });
                    p.text(rect.left_top() + vec2(12.0, 16.0), egui::Align2::LEFT_CENTER, *label, regular(12.5), DIM);
                    let vc = if *is_fastest && d.fastest.is_some() { theme::GOOD } else { TEXT };
                    p.text(rect.left_top() + vec2(12.0, 38.0), egui::Align2::LEFT_CENTER, value, semibold(18.0), vc);
                    if !note.is_empty() {
                        p.text(rect.left_top() + vec2(12.0, 56.0), egui::Align2::LEFT_CENTER, note, regular(11.5), SECONDARY);
                    }
                    if clickable {
                        let resp = resp.on_hover_text("Open this lap");
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            let (_, i, lap) = d.fastest.unwrap();
                            action = Some(DetailAction::Open(lib.sessions[i].path.clone(), Some(lap)));
                        }
                    }
                }
            });
            ui.add_space(8.0);
        }
        for (changed, line) in setup_lines {
            ui.label(RichText::new(line).size(12.5).color(if *changed { theme::FAVORITE } else { SECONDARY }));
        }
    });
    action
}

/// A championship's season so far: per driver, events entered and best lap at each track.
fn season_card(ui: &mut Ui, lib: &Library, profiles: &crate::profiles::Profiles, champ: &str, events: &[&Event]) {
    card(ui, |ui| {
        ui.label(RichText::new(champ).font(semibold(16.0)).color(TEXT));
        let raced: Vec<&&Event> = events.iter().filter(|e| !e.sessions.is_empty()).collect();
        let planned = events.len() - raced.len();
        let mut sub = format!("{} event{} so far", raced.len(), if raced.len() == 1 { "" } else { "s" });
        if planned > 0 {
            sub.push_str(&format!("  ·  {planned} coming up"));
        }
        ui.label(RichText::new(sub).size(12.5).color(SECONDARY));
        ui.add_space(8.0);
        // driver -> (events entered, track -> best lap)
        let mut drivers: Vec<(String, usize, Vec<(String, Option<u64>)>)> = Vec::new();
        for e in &raced {
            let mut seen: Vec<String> = Vec::new();
            for &i in &e.sessions {
                let s = &lib.sessions[i];
                let name = driver_for(s, profiles);
                let k = match drivers.iter().position(|d| d.0.eq_ignore_ascii_case(&name)) {
                    Some(k) => k,
                    None => {
                        drivers.push((name.clone(), 0, vec![]));
                        drivers.len() - 1
                    }
                };
                if !seen.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
                    seen.push(name);
                    drivers[k].1 += 1;
                }
                let best = (!events::is_warm_up(&s.summary)).then_some(s.summary.best_lap_ms).flatten();
                match drivers[k].2.iter_mut().find(|(t, _)| t.eq_ignore_ascii_case(&e.track)) {
                    Some((_, b)) => *b = match (*b, best) {
                        (Some(x), Some(y)) => Some(x.min(y)),
                        (x, y) => x.or(y),
                    },
                    None => drivers[k].2.push((e.track.clone(), best)),
                }
            }
        }
        for (name, n, tracks) in &drivers {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).font(medium(14.0)).color(TEXT));
                ui.label(RichText::new(format!("{n} event{}", if *n == 1 { "" } else { "s" })).size(12.5).color(DIM));
            });
            for (track, best) in tracks {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new(track).size(13.0).color(SECONDARY));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let text = best.map(|b| format_lap_time(b as i64)).unwrap_or_else(|| "no laps".into());
                        ui.label(RichText::new(text).font(medium(13.5)).color(if best.is_some() { TEXT } else { DIM }));
                    });
                });
            }
            ui.add_space(6.0);
        }
    });
}

/// A row in the event list.
fn event_row(ui: &mut Ui, lib: &Library, profiles: &crate::profiles::Profiles, e: &Event, selected: bool, today: aim_xrk::summary::Date) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter_at(rect.shrink2(vec2(0.0, 2.0)));
        let fill = if selected {
            SURFACE_ACTIVE
        } else if response.hovered() {
            SURFACE_HOVER
        } else {
            SURFACE
        };
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        p.rect_filled(rect.shrink2(vec2(0.0, 2.0)), CornerRadius::same(theme::RADIUS_SMALL + 2), fill);
        let (l, r) = (rect.left() + 14.0, rect.right() - 14.0);
        let (top, bottom) = (rect.top() + 20.0, rect.top() + 39.0);
        p.text(pos2(l, top), egui::Align2::LEFT_CENTER, e.title(), medium(14.5), TEXT);
        let mut drivers: Vec<String> = e.sessions.iter().map(|&i| driver_for(&lib.sessions[i], profiles)).collect();
        drivers.sort();
        drivers.dedup();
        let mut line = vec![date_range(e.start, e.end)];
        if !e.championship.trim().is_empty() {
            line.push(e.championship.clone());
        }
        if !drivers.is_empty() {
            line.push(drivers.join(", "));
        }
        p.text(pos2(l, bottom), egui::Align2::LEFT_CENTER, line.join("  ·  "), regular(12.5), DIM);
        let best = e
            .sessions
            .iter()
            .filter(|&&i| !events::is_warm_up(&lib.sessions[i].summary))
            .filter_map(|&i| lib.sessions[i].summary.best_lap_ms)
            .min();
        let (right, color) = match (best, e.sessions.is_empty()) {
            (Some(b), _) => (format_lap_time(b as i64), theme::GOOD),
            (None, true) => {
                let n = events::days(e.start) - events::days(today);
                (if n > 1 { format!("in {n} days") } else if n == 1 { "tomorrow".into() } else if n == 0 { "today".into() } else { "planned".into() }, SECONDARY)
            }
            (None, false) => (format!("{} session{}", e.sessions.len(), if e.sessions.len() == 1 { "" } else { "s" }), DIM),
        };
        p.text(pos2(r, top), egui::Align2::RIGHT_CENTER, right, medium(14.0), color);
    }
    response
}

/// "Sat 10 Oct 2026", or "Sat 17 – Sun 18 Oct 2026".
fn date_range(a: aim_xrk::summary::Date, b: aim_xrk::summary::Date) -> String {
    let short = |d: aim_xrk::summary::Date| {
        let long = long_date(d); // "Saturday, 10 October 2026"
        let (day, rest) = long.split_once(", ").unwrap_or(("", &long));
        let mut parts = rest.split(' ');
        let (n, month, year) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        (day.get(..3).unwrap_or(day).to_string(), n.to_string(), month.get(..3).unwrap_or(month).to_string(), year.to_string())
    };
    let (da, na, ma, ya) = short(a);
    if a == b {
        return format!("{da} {na} {ma} {ya}");
    }
    let (db, nb, mb, yb) = short(b);
    if ya != yb {
        format!("{da} {na} {ma} {ya} – {db} {nb} {mb} {yb}")
    } else if ma != mb {
        format!("{da} {na} {ma} – {db} {nb} {mb} {yb}")
    } else {
        format!("{da} {na} – {db} {nb} {mb} {yb}")
    }
}
