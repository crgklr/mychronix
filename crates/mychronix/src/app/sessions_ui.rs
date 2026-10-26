//! The Sessions page: every downloaded session by day, with filters, and the
//! session rows other pages reuse (sparklines and badges).

use super::*;

impl App {
    pub(super) fn sessions_page(&mut self, ui: &mut Ui) {
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
        if open.is_none() && self.frames == 5 && let Some(n) = crate::hooks::get().click_session {
            open = lib.sessions.iter().find(|s| s.summary.number == Some(n)).map(|s| s.path.clone());
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
pub(super) fn sparkline(p: &egui::Painter, rect: Rect, laps: &[(i32, i64)]) {
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
pub(super) fn session_row(ui: &mut Ui, lib: &Library, i: usize, driver: &str, day_best: Option<u64>, setup_changed: bool) -> egui::Response {
    session_row_with(ui, lib, i, driver, day_best, setup_changed, None)
}

/// [`session_row`] with a sparkline and badges.
pub(super) fn session_row_with(ui: &mut Ui, lib: &Library, i: usize, driver: &str, day_best: Option<u64>, setup_changed: bool, extras: Option<&RowExtras>) -> egui::Response {
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
        if let Some(pos) = response.hover_pos() && let Some((_, tip)) = tips.iter().find(|(r, _)| r.contains(pos)) {
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), response.id.with("badge"), egui::PopupAnchor::Pointer).show(|ui| {
                ui.label(tip);
            });
        }
        // Lap times, first to last, left of the numbers.
        if let Some(x) = extras {
            let w = 110.0;
            let spark = Rect::from_min_size(pos2(r - 300.0 - w, rect.top() + 12.0), vec2(w, rect.height() - 24.0));
            if spark.left() > bx + 20.0 {
                sparkline(p, spark, &x.laps);
            }
        }
        let track = s.track_name(&lib.tracks).unwrap_or_else(|| "No track".into());
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
pub(super) fn same_channel<'a>(session: &'a Session, name: &str) -> Option<&'a aim_xrk::Channel> {
    crate::report::channel(session, name)
}

/// "Saturday, October 10, 2026".
pub(super) fn long_date(d: aim_xrk::summary::Date) -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    // Sakamoto's day-of-week method.
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if d.month < 3 { d.year as i32 - 1 } else { d.year as i32 };
    let dow = (y + y / 4 - y / 100 + y / 400 + t[d.month as usize - 1] + d.day as i32).rem_euclid(7) as usize;
    format!("{}, {} {}, {}", DAYS[dow], MONTHS[d.month as usize - 1], d.day, d.year)
}
