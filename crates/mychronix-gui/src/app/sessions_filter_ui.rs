//! The Sessions page's filters, in its right column: when (presets or a date
//! range with a calendar), which drivers, which tracks (searchable, for
//! people who've raced many), and whether to show untimed sessions.

use super::*;
use aim_xrk::summary::Date;
use jiff::civil::Date as Day;
use std::collections::BTreeSet;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Range {
    #[default]
    All,
    ThisYear,
    Last30,
    LastDay,
    Custom,
}

#[derive(Default)]
pub(super) struct SessionFilter {
    /// Drivers to show (empty: all).
    drivers: BTreeSet<String>,
    /// Library tracks to show (empty: all).
    tracks: BTreeSet<usize>,
    range: Range,
    from: Option<Day>,
    to: Option<Day>,
    /// Only sessions with a timed lap (no warm-ups, no untimed runs).
    timed_only: bool,
    track_search: String,
    /// The open calendar: 0 for From, 1 for To, and the month it shows.
    calendar: Option<(u8, Day)>,
}

/// A session's driver: the one entered for it, else its kart's (the kart
/// its MyChron was on when it was recorded); "" when neither.
pub(super) fn session_driver(s: &tracks::LocalSession, profiles: &crate::profiles::Profiles) -> String {
    let kart = s.kart.and_then(|id| profiles.karts.iter().find(|k| k.id == id));
    s.meta
        .as_ref()
        .map(|m| m.driver.trim().to_string())
        .filter(|d| !d.is_empty())
        .or_else(|| kart.and_then(|k| profiles.driver_name(k.driver)).map(str::to_string))
        .unwrap_or_default()
}

/// A session's track name: the one it's assigned to, else the one its MyChron recorded.
pub(super) fn session_track(lib: &Library, i: usize) -> Option<String> {
    let s = &lib.sessions[i];
    s.track.map(|t| lib.tracks[t].name.clone()).or_else(|| s.summary.track.clone())
}

fn naive(d: Date) -> Option<Day> {
    Day::new(d.year as i16, d.month as i8, d.day as i8).ok()
}

const NO_DRIVER: &str = "No driver";

impl SessionFilter {
    /// The dates shown, from and to (inclusive): from the preset or range.
    fn bounds(&self, newest: Option<Day>) -> (Option<Day>, Option<Day>) {
        let today = naive(crate::events::today());
        match self.range {
            Range::All => (None, None),
            Range::ThisYear => (today.and_then(|t| Day::new(t.year(), 1, 1).ok()), None),
            Range::Last30 => (today.and_then(|t| t.checked_sub(jiff::Span::new().days(29)).ok()), None),
            Range::LastDay => (newest, newest),
            Range::Custom => (self.from, self.to),
        }
    }

    pub(super) fn keeps(&self, lib: &Library, i: usize, profiles: &crate::profiles::Profiles, newest: Option<Day>) -> bool {
        let s = &lib.sessions[i];
        let driver = session_driver(s, profiles);
        let driver = if driver.is_empty() { NO_DRIVER.to_string() } else { driver };
        let (from, to) = self.bounds(newest);
        let day = s.summary.date.and_then(naive);
        (self.drivers.is_empty() || self.drivers.contains(&driver))
            && (self.tracks.is_empty() || s.track.is_some_and(|t| self.tracks.contains(&t)))
            && from.is_none_or(|f| day.is_some_and(|d| d >= f))
            && to.is_none_or(|t| day.is_some_and(|d| d <= t))
            && (!self.timed_only || s.summary.best_lap_ms.is_some())
    }

    fn active(&self) -> bool {
        !self.drivers.is_empty() || !self.tracks.is_empty() || self.range != Range::All || self.timed_only
    }
}

/// A row with a check box, a label and its session count in a pill; true when clicked.
fn check_row(ui: &mut Ui, on: bool, label: &str, count: usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CornerRadius::same(8), SURFACE_ACTIVE);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(8), SURFACE_HOVER);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let bx = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(18.0));
    if on {
        p.rect_filled(bx, CornerRadius::same(5), TEXT);
        Icon::Check.image(ON_TEXT, 14.0).paint_at(ui, bx.shrink(2.0));
    } else {
        p.rect_stroke(bx, CornerRadius::same(5), Stroke::new(1.5, DIM), egui::StrokeKind::Inside);
    }
    p.text(pos2(rect.left() + 42.0, rect.center().y), egui::Align2::LEFT_CENTER, label, medium(15.0), TEXT);
    let n = p.layout_no_wrap(count.to_string(), medium(12.5), SECONDARY);
    let pill = Rect::from_center_size(pos2(rect.right() - 12.0 - (n.size().x + 16.0) / 2.0, rect.center().y), vec2(n.size().x + 16.0, 22.0));
    p.rect_filled(pill, CornerRadius::same(11), if on { SURFACE_HOVER } else { SURFACE_ACTIVE });
    p.galley(pill.center() - n.size() / 2.0, n, SECONDARY);
    resp.clicked()
}

/// A section of the filter panel: its own card, with an icon, a title, a
/// line saying what it's filtering, and a Clear link while it filters.
fn section(ui: &mut Ui, icon: Icon, title: &str, summary: &str, active: bool, add: impl FnOnce(&mut Ui)) -> bool {
    let mut clear = false;
    Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
            ui.painter().rect_filled(r, CornerRadius::same(9), SURFACE_ACTIVE);
            icon.image(if active { TEXT } else { SECONDARY }, 18.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(18.0)));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new(title).font(semibold(16.0)).color(TEXT));
                ui.label(RichText::new(summary).font(regular(13.0)).color(if active { theme::GOOD } else { SECONDARY }));
            });
            if active {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let c = ui.add(egui::Label::new(RichText::new("Clear").font(medium(13.5)).color(SECONDARY)).sense(Sense::click()));
                    if c.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    clear = c.clicked();
                });
            }
        });
        ui.add_space(10.0);
        add(ui);
    });
    ui.add_space(12.0);
    clear
}

/// An even grid of choices, two to a row; the one picked is filled.
fn segments<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) {
    let gap = 8.0;
    let w = (ui.available_width() - gap) / 2.0;
    for row in options.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (v, label) in row {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
                let on = *value == *v;
                let p = ui.painter();
                p.rect_filled(r, CornerRadius::same(8), if on { TEXT } else if resp.hovered() { SURFACE_HOVER } else { SURFACE_ACTIVE });
                p.text(r.center(), egui::Align2::CENTER_CENTER, *label, medium(14.0), if on { ON_TEXT } else { TEXT });
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    *value = *v;
                }
            }
        });
    }
}

/// An on/off switch with a title and an explanation; true when clicked.
fn switch_row(ui: &mut Ui, on: bool, title: &str, detail: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let p = ui.painter();
    let track = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(40.0, 22.0));
    p.rect_filled(track, CornerRadius::same(11), if on { theme::GOOD } else { SURFACE_ACTIVE });
    let knob = if on { track.right() - 11.0 } else { track.left() + 11.0 };
    p.circle_filled(pos2(knob, track.center().y), 8.0, TEXT);
    p.text(pos2(rect.left(), rect.center().y - 9.0), egui::Align2::LEFT_CENTER, title, medium(15.0), TEXT);
    p.text(pos2(rect.left(), rect.center().y + 11.0), egui::Align2::LEFT_CENTER, detail, regular(12.5), SECONDARY);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// A date field that opens a Sunday-first month calendar beside the panel,
/// its right edge at `right`, so both date fields stay in view. Days with sessions get a dot and the range
/// `from..=to` is shaded. Returns the day picked.
fn date_field(ui: &mut Ui, calendar: &mut Option<(u8, Day)>, k: u8, label: &str, value: Day, (from, to): (Day, Day), race_days: &BTreeSet<Day>, right: f32) -> Option<Day> {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let open = calendar.is_some_and(|c| c.0 == k);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(8), if open { SURFACE_ACTIVE } else if resp.hovered() { SURFACE_HOVER } else { theme::WINDOW });
    p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, theme::OUTLINE), egui::StrokeKind::Inside);
    p.text(pos2(rect.left() + 14.0, rect.center().y), egui::Align2::LEFT_CENTER, label, medium(13.5), SECONDARY);
    p.text(pos2(rect.left() + 64.0, rect.center().y), egui::Align2::LEFT_CENTER, value.strftime("%b %-d, %Y").to_string(), medium(15.0), TEXT);
    Icon::Calendar.image(SECONDARY, 16.0).paint_at(ui, Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(16.0)));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *calendar = if open { None } else { Some((k, value.first_of_month())) };
    }
    let Some((_, month)) = calendar.filter(|c| c.0 == k) else { return None };

    let today = naive(crate::events::today());
    let (mut picked, mut nav) = (None, None);
    let cell = 40.0;
    let area = egui::Area::new(ui.id().with(("calendar", k)))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(pos2(right, rect.top()))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(SURFACE_ACTIVE)
                .stroke(Stroke::new(1.0, theme::OUTLINE))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(14))
                .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    // Month, with the previous and next ones either side.
                    let (head, _) = ui.allocate_exact_size(vec2(cell * 7.0, 34.0), Sense::hover());
                    ui.painter().text(head.center(), egui::Align2::CENTER_CENTER, month.strftime("%B %Y").to_string(), semibold(15.5), TEXT);
                    for (step, icon, x) in [(-1, Icon::ChevronLeft, head.left() + 17.0), (1, Icon::ChevronRight, head.right() - 17.0)] {
                        let r = Rect::from_center_size(pos2(x, head.center().y), Vec2::splat(32.0));
                        let b = ui.interact(r, ui.id().with(("month", step)), Sense::click());
                        if b.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(8), SURFACE_HOVER);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        icon.image(SECONDARY, 17.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(17.0)));
                        if b.clicked() {
                            nav = month.checked_add(jiff::Span::new().months(step)).ok();
                        }
                    }
                    let (names, _) = ui.allocate_exact_size(vec2(cell * 7.0, 22.0), Sense::hover());
                    for (i, n) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
                        ui.painter().text(pos2(names.left() + cell * (i as f32 + 0.5), names.center().y), egui::Align2::CENTER_CENTER, *n, medium(12.5), DIM);
                    }
                    // The days, Sunday first.
                    let lead = month.weekday().to_sunday_zero_offset() as i32;
                    let days = month.days_in_month() as i32;
                    let rows = (lead + days + 6) / 7;
                    for row in 0..rows {
                        let (line, _) = ui.allocate_exact_size(vec2(cell * 7.0, cell), Sense::hover());
                        for col in 0..7 {
                            let n = row * 7 + col - lead + 1;
                            if n < 1 || n > days {
                                continue;
                            }
                            let Ok(d) = Day::new(month.year(), month.month(), n as i8) else { continue };
                            let r = Rect::from_min_size(pos2(line.left() + cell * col as f32, line.top()), Vec2::splat(cell));
                            let c = ui.interact(r, ui.id().with(("day", n)), Sense::click());
                            let p = ui.painter();
                            if d >= from && d <= to && from < to {
                                // The range: a band, rounded at its ends.
                                let band = Rect::from_x_y_ranges(if d == from { r.center().x..=r.right() } else { r.left()..=r.right() }, r.center().y - 15.0..=r.center().y + 15.0);
                                let band = if d == to { Rect::from_x_y_ranges(band.left()..=r.center().x, band.y_range()) } else { band };
                                p.rect_filled(band, CornerRadius::ZERO, SURFACE_HOVER);
                            }
                            let ring = r.shrink(4.0);
                            let chosen = d == value;
                            if chosen {
                                p.rect_filled(ring, CornerRadius::same(16), TEXT);
                            } else if c.hovered() {
                                p.rect_filled(ring, CornerRadius::same(16), theme::OUTLINE);
                            } else if Some(d) == today {
                                p.rect_stroke(ring, CornerRadius::same(16), Stroke::new(1.0, DIM), egui::StrokeKind::Inside);
                            }
                            let ink = if chosen { ON_TEXT } else { TEXT };
                            p.text(r.center() - vec2(0.0, 2.0), egui::Align2::CENTER_CENTER, n.to_string(), medium(14.0), ink);
                            if race_days.contains(&d) {
                                p.circle_filled(pos2(r.center().x, r.center().y + 9.0), 2.0, if chosen { ON_TEXT } else { theme::GOOD });
                            }
                            if c.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if c.clicked() {
                                picked = Some(d);
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 3.0, theme::GOOD);
                        ui.label(RichText::new("Race day").font(regular(12.5)).color(SECONDARY));
                    });
                });
        });
    if let Some(m) = nav {
        *calendar = Some((k, m));
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if picked.is_some() || escape || (area.response.clicked_elsewhere() && !resp.clicked()) {
        *calendar = None;
    }
    picked
}

impl App {
    /// The filter panel. `newest`: the newest session's day (for "Last race day").
    pub(super) fn sessions_filter_panel(&mut self, ui: &mut Ui, lib: &Library, profiles: &crate::profiles::Profiles, newest: Option<Day>) {
        let f = &mut self.session_filter;
        let us = |d: Day| d.strftime("%b %-d, %Y").to_string();

        // When.
        let today = naive(crate::events::today()).unwrap_or_default();
        let first = lib.sessions.iter().filter_map(|s| s.summary.date).min().and_then(naive);
        let (bf, bt) = f.bounds(newest);
        // The dates shown are the ones in force: the preset's, else the range picked.
        let shown = [bf.or(first).unwrap_or(today), bt.unwrap_or(today)];
        let summary = match f.range {
            Range::All => "Every session".to_string(),
            Range::LastDay => newest.map(|d| format!("Only {}", us(d))).unwrap_or_default(),
            _ if shown[0] == shown[1] => format!("Only {}", us(shown[0])),
            _ => format!("{} – {}", us(shown[0]), us(shown[1])),
        };
        let race_days: BTreeSet<Day> = lib.sessions.iter().filter_map(|s| s.summary.date.and_then(naive)).collect();
        if section(ui, Icon::Calendar, "When", &summary, f.range != Range::All, |ui| {
            // A custom range leaves none of these picked.
            let mut range = f.range;
            segments(ui, &mut range, &[(Range::All, "All time"), (Range::ThisYear, "This year"), (Range::Last30, "Last 30 days"), (Range::LastDay, "Last race day")]);
            if range != f.range {
                f.range = range;
                f.calendar = None;
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Or pick dates").font(medium(13.0)).color(SECONDARY));
            // The calendar opens to the left of the panel's cards.
            let right = ui.max_rect().left() - 30.0;
            for (k, label) in [(0u8, "From"), (1, "To")] {
                // "All time" has no range to shade.
                let band = if f.range == Range::All { (today, today) } else { (shown[0], shown[1]) };
                if let Some(d) = date_field(ui, &mut f.calendar, k, label, shown[k as usize], band, &race_days, right) {
                    // Picking a date starts a range from what was shown.
                    if f.range != Range::Custom {
                        (f.from, f.to, f.range) = (Some(shown[0]), Some(shown[1]), Range::Custom);
                    }
                    if k == 0 { f.from = Some(d) } else { f.to = Some(d) }
                    // A From after the To swaps them.
                    if let (Some(a), Some(b)) = (f.from, f.to) {
                        if a > b {
                            (f.from, f.to) = (Some(b), Some(a));
                        }
                    }
                }
            }
        }) {
            (f.range, f.from, f.to, f.calendar) = (Range::All, None, None, None);
        }

        // Drivers.
        let mut drivers: Vec<(String, usize)> = Vec::new();
        for s in &lib.sessions {
            let d = session_driver(s, profiles);
            let d = if d.is_empty() { NO_DRIVER.to_string() } else { d };
            match drivers.iter_mut().find(|x| x.0 == d) {
                Some(x) => x.1 += 1,
                None => drivers.push((d, 1)),
            }
        }
        drivers.sort_by(|a, b| (a.0 == NO_DRIVER).cmp(&(b.0 == NO_DRIVER)).then(a.0.cmp(&b.0)));
        let picked: Vec<&str> = drivers.iter().filter(|d| f.drivers.contains(&d.0)).map(|d| d.0.as_str()).collect();
        let summary = if picked.is_empty() { "All drivers".to_string() } else { format!("Only {}", picked.join(", ")) };
        if section(ui, Icon::Users, "Drivers", &summary, !picked.is_empty(), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (d, n) in &drivers {
                if check_row(ui, f.drivers.contains(d), d, *n) && !f.drivers.remove(d) {
                    f.drivers.insert(d.clone());
                }
            }
        }) {
            f.drivers.clear();
        }

        // Tracks, most raced first; searchable once there are many.
        let mut tracks: Vec<(usize, usize)> = Vec::new();
        for s in &lib.sessions {
            if let Some(t) = s.track {
                match tracks.iter_mut().find(|x| x.0 == t) {
                    Some(x) => x.1 += 1,
                    None => tracks.push((t, 1)),
                }
            }
        }
        tracks.sort_by(|a, b| b.1.cmp(&a.1).then(lib.tracks[a.0].name.cmp(&lib.tracks[b.0].name)));
        let picked: Vec<&str> = tracks.iter().filter(|t| f.tracks.contains(&t.0)).map(|t| lib.tracks[t.0].name.as_str()).collect();
        let summary = match picked.len() {
            0 if tracks.len() == 1 => "The 1 track you've raced".to_string(),
            0 => format!("All {} tracks", tracks.len()),
            1..=2 => format!("Only {}", picked.join(", ")),
            n => format!("{n} of {} tracks", tracks.len()),
        };
        if section(ui, Icon::MapPin, "Tracks", &summary, !picked.is_empty(), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if tracks.len() > 6 {
                search_field(ui, &mut f.track_search, "Find a track…");
                ui.add_space(6.0);
            }
            let needle = f.track_search.to_lowercase();
            for (t, n) in &tracks {
                let name = &lib.tracks[*t].name;
                let full = lib.tracks[*t].full_name.clone().unwrap_or_default();
                if !needle.is_empty() && !name.to_lowercase().contains(&needle) && !full.to_lowercase().contains(&needle) {
                    continue;
                }
                if check_row(ui, f.tracks.contains(t), name, *n) && !f.tracks.remove(t) {
                    f.tracks.insert(*t);
                }
            }
        }) {
            f.tracks.clear();
        }

        // Timed laps.
        let summary = if f.timed_only { "Only sessions with timed laps" } else { "Every session, timed or not" };
        if section(ui, Icon::Timer, "Laps", summary, f.timed_only, |ui| {
            if switch_row(ui, f.timed_only, "Timed laps only", "Hides warm-ups and runs with no full lap") {
                f.timed_only = !f.timed_only;
            }
        }) {
            f.timed_only = false;
        }

        // At the end, so nothing above moves when it appears.
        if f.active() && button(ui, Some(Icon::X), "Clear all filters", false).clicked() {
            *f = SessionFilter::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_presets() {
        let newest = Day::new(2026, 10, 11).ok();
        let f = SessionFilter { range: Range::LastDay, ..Default::default() };
        assert_eq!(f.bounds(newest), (newest, newest));
        let f = SessionFilter { range: Range::Custom, from: Day::new(2026, 9, 1).ok(), ..Default::default() };
        assert_eq!(f.bounds(newest), (Day::new(2026, 9, 1).ok(), None));
        assert_eq!(SessionFilter::default().bounds(newest), (None, None));
    }
}
