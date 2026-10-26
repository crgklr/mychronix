//! Small widgets the pages share: cards, sections, buttons, chips, rows, the
//! search field, suggestion fields, and the color ramps for maps and charts.

use super::*;

/// Values already in use, offered while typing in the download dialog.
pub(super) struct Suggestions {
    pub(super) drivers: Vec<String>,
    pub(super) karts: Vec<String>,
    pub(super) championships: Vec<String>,
    pub(super) session_types: Vec<String>,
    pub(super) tracks: Vec<String>,
}

impl Suggestions {
    pub(super) fn gather(lib: &Library, profiles: &crate::profiles::Profiles) -> Self {
        let mut s = Suggestions { drivers: vec![], karts: vec![], championships: vec![], session_types: vec![], tracks: vec![] };
        s.drivers.extend(profiles.drivers.iter().map(|d| d.name.clone()));
        s.karts.extend(profiles.karts.iter().map(|k| k.name.clone()));
        s.championships.extend(profiles.loggers.values().map(|p| p.championship.clone()));
        for m in lib.sessions.iter().filter_map(|x| x.meta.as_ref()) {
            s.drivers.push(m.driver.clone());
            s.karts.push(m.kart.clone());
            s.championships.push(m.championship.clone());
            s.session_types.push(m.session_type.clone());
            s.tracks.push(m.track.clone());
        }
        // RaceStudio's session types.
        s.session_types.extend(
            [
                "Generic testing", "Practice 1", "Practice 2", "Practice 3", "Practice 4", "Qualifying", "Qualifying 1", "Qualifying 2",
                "Warm-up", "Heat", "Heat 1", "Heat 2", "Heat 3", "Pre-final", "Final", "1st Final", "2nd Final", "Race", "1st Race",
                "2nd Race", "A-Main", "Test", "Engine Test", "Track Outing",
            ]
            .map(String::from),
        );
        // Tracks you've raced or made first, then every AiM track.
        for t in lib.tracks.iter().filter(|t| !t.sessions.is_empty() || t.kind == Kind::Custom) {
            s.tracks.push(t.name.clone());
        }
        for t in &lib.tracks {
            s.tracks.push(t.name.clone());
        }
        for v in [&mut s.drivers, &mut s.karts, &mut s.championships, &mut s.session_types, &mut s.tracks] {
            let mut seen = std::collections::HashSet::new();
            v.retain(|x| !x.trim().is_empty() && seen.insert(x.to_lowercase()));
        }
        s
    }
}

/// A text field that offers matching `suggestions` below it while focused.
/// Picking one fills the field; typing anything else is fine too.
pub(super) fn suggest_field(ui: &mut Ui, id: &str, value: &mut String, hint: &str, suggestions: &[String], width: f32) -> egui::Response {
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(id)
            .hint_text(RichText::new(hint).color(DIM))
            .font(regular(16.5))
            .margin(Margin::symmetric(14, 12))
            .desired_width(width),
    );
    let popup_id = response.id.with("suggestions");
    let ctx = ui.ctx().clone();
    // Stay open while the pointer is over the list, so a click on it lands.
    let last_rect: Option<Rect> = ctx.data(|d| d.get_temp(popup_id));
    let over_list = last_rect.zip(ctx.pointer_hover_pos()).is_some_and(|(r, p)| r.contains(p));
    if !(response.has_focus() || over_list) {
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        return response;
    }
    // Empty, or already one of the suggestions: offer them all (to switch).
    // Otherwise offer the ones containing what's typed.
    let needle = value.to_lowercase();
    let exact = suggestions.iter().any(|s| s.to_lowercase() == needle);
    let matches: Vec<&String> = suggestions
        .iter()
        .filter(|s| needle.is_empty() || exact || s.to_lowercase().contains(&needle))
        .take(40)
        .collect();
    if matches.is_empty() {
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        return response;
    }
    let mut picked = None;
    let area = egui::Area::new(popup_id)
        .order(egui::Order::Foreground)
        .fixed_pos(response.rect.left_bottom() + vec2(0.0, 4.0))
        .show(&ctx, |ui| {
            Frame::new()
                .fill(SURFACE_ACTIVE)
                .corner_radius(CornerRadius::same(theme::RADIUS_SMALL + 2))
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(response.rect.width() - 8.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    // Eight rows show; more scroll.
                    egui::ScrollArea::vertical().max_height(34.0 * 8.0).show(ui, |ui| {
                        for m in &matches {
                            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, CornerRadius::same(theme::RADIUS_SMALL), SURFACE_HOVER);
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            ui.painter().text(r.left_center() + vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, m.as_str(), regular(14.5), TEXT);
                            if resp.clicked() {
                                picked = Some((*m).clone());
                            }
                        }
                    });
                });
        });
    ctx.data_mut(|d| d.insert_temp(popup_id, area.response.rect));
    if let Some(p) = picked {
        *value = p;
        ctx.data_mut(|d| d.remove::<Rect>(popup_id));
        ctx.memory_mut(|m| m.surrender_focus(response.id));
    }
    response
}

/// Colors from the first stop (`t` = 0) to the last (1), evenly spaced.
pub(super) fn ramp_color(stops: &[Color32], t: f32) -> Color32 {
    let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (x.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], x - i as f32)
}

/// RPM on the track: dark purple when low, through magenta and orange, to
/// yellow when high (plasma), so where the engine bogs and pulls stands out.
pub(super) fn rpm_color(t: f32) -> Color32 {
    ramp_color(
        &[
            Color32::from_rgb(0x2b, 0x0b, 0x6e),
            Color32::from_rgb(0x8a, 0x1c, 0xa8),
            Color32::from_rgb(0xd6, 0x45, 0x7a),
            Color32::from_rgb(0xf7, 0x94, 0x3f),
            Color32::from_rgb(0xf6, 0xe6, 0x4a),
        ],
        t,
    )
}

/// A temperature on the track: blue when low, through yellow and orange, to red when high.
pub(super) fn temp_color(t: f32) -> Color32 {
    ramp_color(&[Color32::from_rgb(0x2f, 0x6b, 0xff), Color32::from_rgb(0xf2, 0xd1, 0x3b), Color32::from_rgb(0xf0, 0x8c, 0x2b), Color32::from_rgb(0xe5, 0x48, 0x4d)], t)
}

/// Speed on the track: red when slow to green when fast.
pub(super) fn track_speed_color(t: f32) -> Color32 {
    ramp_color(&[Color32::from_rgb(0xe5, 0x48, 0x4d), Color32::from_rgb(0xf0, 0xa0, 0x4b), theme::GOOD], t)
}

/// How a channel's values color the track, low to high, by what it measures
/// (whatever the logger calls it): RPM, a temperature, a speed, or anything
/// else on the general ramp.
pub(super) fn channel_ramp(c: &Channel) -> fn(f32) -> Color32 {
    let name = c.name.to_lowercase();
    let units = pretty_units(&c.units).to_lowercase();
    if name.contains("rpm") || units == "rpm" {
        rpm_color
    } else if units == "°c" || units == "°f" {
        temp_color
    } else if name.contains("speed") || matches!(units.as_str(), "km/h" | "kmh" | "mph") {
        track_speed_color
    } else {
        heat_color
    }
}

/// Low to high on the track: blue, through teal and yellow, to red, so a
/// channel's highs and lows stand apart on the line.
pub(super) fn heat_color(t: f32) -> Color32 {
    ramp_color(
        &[
            Color32::from_rgb(0x2f, 0x5b, 0xff),
            Color32::from_rgb(0x16, 0xb8, 0xd4),
            Color32::from_rgb(0xf2, 0xd1, 0x3b),
            Color32::from_rgb(0xf0, 0x8c, 0x2b),
            Color32::from_rgb(0xe5, 0x48, 0x4d),
        ],
        t,
    )
}

/// Speed color: viridis, dark purple when slow to yellow when fast. Its
/// lightness rises steadily, so slow and fast stay apart for colorblind
/// viewers too.
pub(super) fn speed_color(t: f32) -> Color32 {
    const STOPS: [Color32; 5] = [
        Color32::from_rgb(0x44, 0x01, 0x54),
        Color32::from_rgb(0x3b, 0x52, 0x8b),
        Color32::from_rgb(0x21, 0x91, 0x8c),
        Color32::from_rgb(0x5e, 0xc9, 0x62),
        Color32::from_rgb(0xfd, 0xe7, 0x25),
    ];
    let x = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let i = (x.floor() as usize).min(STOPS.len() - 2);
    mix(STOPS[i], STOPS[i + 1], x - i as f32)
}



pub(super) fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::OUTLINE);
}

pub(super) fn section_header(ui: &mut Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(semibold(16.0)).color(TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(detail).size(12.5).color(DIM));
        });
    });
    ui.add_space(4.0);
}

/// A titled area without a box: full width, on the page background.
pub(super) fn section(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
}

/// A rounded progress bar, `fraction` 0–1 filled.
pub(super) fn progress_bar(ui: &mut Ui, width: f32, height: f32, fraction: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let round = CornerRadius::same((height / 2.0) as u8);
    ui.painter().rect_filled(r, round, SURFACE_ACTIVE);
    let w = (r.width() * fraction.clamp(0.0, 1.0)).max(height);
    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(w, height)), round, theme::GOOD);
}

pub(super) fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

pub(super) fn graph_card(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(height - 20.0);
            add(ui)
        });
}

/// Pill button; `primary` fills it with ink.
pub(super) fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, primary: bool) -> egui::Response {
    button_with(ui, icon, None, label, primary)
}

/// A button whose icon can have its own color (e.g. a yellow star).
pub(super) fn button_with(ui: &mut Ui, icon: Option<Icon>, icon_tint: Option<Color32>, label: &str, primary: bool) -> egui::Response {
    let (fg, fill, hover) = if primary {
        (ON_TEXT, TEXT, Color32::from_rgb(0xd9, 0xdd, 0xe2))
    } else {
        (TEXT, SURFACE, SURFACE_HOVER)
    };
    let galley = ui.painter().layout_no_wrap(label.into(), medium(14.0), fg);
    let icon_w = if icon.is_some() { 24.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + icon_w + 32.0, 38.0), Sense::click());
    let fill = if response.hovered() { hover } else { fill };
    ui.painter().rect_filled(rect, CornerRadius::same(19), fill);
    let mut x = rect.left() + 16.0;
    if let Some(icon) = icon {
        icon.image(icon_tint.unwrap_or(fg), 16.0).paint_at(ui, Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(16.0)));
        x += icon_w;
    }
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

pub(super) fn chip(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let fg = if selected { ON_TEXT } else { TEXT };
    let galley = ui.painter().layout_no_wrap(label.into(), medium(13.0), fg);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(26.0, 14.0), Sense::click());
    let fill = match (selected, response.hovered()) {
        (true, _) => TEXT,
        (false, true) => SURFACE_HOVER,
        (false, false) => SURFACE,
    };
    ui.painter().rect_filled(rect, CornerRadius::same(255), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

pub(super) fn tag(ui: &mut Ui, text: &str, fill: Color32) {
    let galley = ui.painter().layout_no_wrap(text.into(), semibold(11.5), ON_TEXT);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(14.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(255), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ON_TEXT);
}

pub(super) fn search_field(ui: &mut Ui, text: &mut String, hint: &str) {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin { left: 12, right: 10, top: 4, bottom: 4 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(Icon::Search.image(DIM, 16.0));
                let clear_w = if text.is_empty() { 0.0 } else { 22.0 };
                ui.add(
                    egui::TextEdit::singleline(text)
                        .frame(Frame::new())
                        .hint_text(RichText::new(hint).color(DIM))
                        .desired_width(ui.available_width() - clear_w),
                );
                if !text.is_empty() && ui.add(Icon::X.image(DIM, 14.0).sense(Sense::click())).clicked() {
                    text.clear();
                }
            });
        });
}

/// Channel color key: filled dot when graphed, empty ring when not.
pub(super) fn swatch(ui: &mut Ui, color: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    match color {
        Some(c) => ui.painter().circle_filled(rect.center(), 5.0, c),
        None => ui.painter().circle_stroke(rect.center(), 4.5, Stroke::new(1.2, DIM)),
    };
}

/// A full-width clickable list row with hover and selected states.
pub(super) fn row(ui: &mut Ui, selected: bool, add: impl FnOnce(&mut Ui)) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Some(SURFACE_ACTIVE)
        } else if response.hovered() {
            Some(SURFACE_HOVER)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL + 2), fill);
        }
        let inner = rect.shrink2(vec2(10.0, 0.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
        add(&mut child);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}
