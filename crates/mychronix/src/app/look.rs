//! The look every window (dialog) shares: a darkened backdrop, a raised
//! panel with generous spacing, a header with an icon, a footer with the
//! actions.

use super::*;

pub const WIDTH: f32 = 600.0;
const PAD: i8 = 32;

/// A modal window: everything behind it dimmed and blocked. True when
/// the person asked to close it (Escape, or a click on the backdrop).
pub fn dialog(ctx: &egui::Context, id: &str, width: f32, body: impl FnOnce(&mut Ui)) -> bool {
    let frame = Frame::new()
        .fill(SIDEBAR)
        .stroke(Stroke::new(1.0, theme::OUTLINE))
        .corner_radius(CornerRadius::same(18))
        .shadow(egui::epaint::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(150) });
    let r = egui::Modal::new(egui::Id::new(id)).backdrop_color(Color32::from_black_alpha(215)).frame(frame).show(ctx, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        body(ui);
    });
    r.should_close()
}

/// The top of a window: a tinted icon badge, an optional small label
/// above the title (e.g. "Step 1 of 3"), the title and a line under it.
pub fn header(ui: &mut Ui, icon: Icon, tint: Color32, eyebrow: Option<&str>, title: &str, subtitle: &str) {
    Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 30, bottom: 8 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 26.0, tint.gamma_multiply(0.18));
            icon.image(tint, 26.0).paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(26.0)));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                if let Some(e) = eyebrow {
                    ui.label(RichText::new(e.to_uppercase()).font(semibold(11.5)).color(DIM).extra_letter_spacing(0.9));
                }
                ui.add(egui::Label::new(RichText::new(title).font(semibold(24.0)).color(TEXT)).wrap());
                if !subtitle.is_empty() {
                    ui.add(egui::Label::new(RichText::new(subtitle).font(regular(15.5)).color(SECONDARY)).wrap());
                }
            });
        });
    });
}

/// The middle of a window.
pub fn body(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 16, bottom: 24 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 10.0;
        add(ui);
    });
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Danger,
    Quiet,
}

/// A large button: 44 points tall.
pub fn big_button(ui: &mut Ui, label: &str, icon: Option<Icon>, kind: Kind, enabled: bool) -> bool {
    let (fill, fg) = match kind {
        Kind::Primary => (TEXT, ON_TEXT),
        Kind::Danger => (theme::DANGER, Color32::WHITE),
        Kind::Quiet => (Color32::TRANSPARENT, TEXT),
    };
    let galley = ui.painter().layout_no_wrap(label.into(), semibold(15.0), fg);
    let icon_w = if icon.is_some() { 26.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2((galley.size().x + 44.0 + icon_w).max(110.0), 44.0), if enabled { Sense::click() } else { Sense::hover() });
    let hovered = enabled && resp.hovered();
    let fill = match kind {
        Kind::Quiet if hovered => SURFACE_HOVER,
        _ if hovered => fill.gamma_multiply(0.88),
        _ => fill,
    };
    let alpha = if enabled { 1.0 } else { 0.35 };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill.gamma_multiply(alpha));
    if kind == Kind::Quiet {
        ui.painter().rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, theme::OUTLINE), egui::StrokeKind::Inside);
    }
    // Icons lead the label, except an arrow forward, which follows it.
    let trailing = icon == Some(Icon::ChevronRight);
    let mut x = rect.center().x - (galley.size().x + icon_w) / 2.0;
    let paint_icon = |ui: &Ui, i: Icon, x: f32| i.image(fg.gamma_multiply(alpha), 18.0).paint_at(ui, Rect::from_min_size(pos2(x, rect.center().y - 9.0), Vec2::splat(18.0)));
    if let Some(i) = icon.filter(|_| !trailing) {
        paint_icon(ui, i, x);
        x += icon_w;
    }
    let text_w = galley.size().x;
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg.gamma_multiply(alpha));
    if let Some(i) = icon.filter(|_| trailing) {
        paint_icon(ui, i, x + text_w + 8.0);
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

/// The bottom of a window: a divider, a quiet button on the left and the
/// main action on the right. Returns (left clicked, right clicked).
pub fn footer(ui: &mut Ui, left: &str, right: (&str, Option<Icon>, Kind, bool)) -> (bool, bool) {
    let (mut l, mut r) = (false, false);
    let top = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, theme::OUTLINE));
    Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 18, bottom: 22 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            l = big_button(ui, left, None, Kind::Quiet, true);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                r = big_button(ui, right.0, right.1, right.2, right.3);
            });
        });
    });
    (l, r)
}

/// The bottom of a window with buttons of its own: a divider and padding.
pub fn footer_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let top = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, theme::OUTLINE));
    Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 18, bottom: 22 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(add);
    });
}

/// One choice in a list: a full-width row with a radio mark (or an icon),
/// a title and a line under it. True when picked.
pub fn option(ui: &mut Ui, selected: bool, icon: Option<Icon>, title: &str, subtitle: &str) -> bool {
    let h = if subtitle.is_empty() { 54.0 } else { 66.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let hovered = resp.hovered();
    let (fill, stroke) = match (selected, hovered) {
        (true, _) => (SURFACE_ACTIVE, Stroke::new(1.5, TEXT)),
        (false, true) => (SURFACE_HOVER, Stroke::new(1.0, theme::OUTLINE)),
        (false, false) => (SURFACE, Stroke::new(1.0, theme::OUTLINE)),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    ui.painter().rect_stroke(rect, CornerRadius::same(12), stroke, egui::StrokeKind::Inside);
    let mark = pos2(rect.left() + 28.0, rect.center().y);
    match icon {
        Some(i) => i.image(if selected { TEXT } else { SECONDARY }, 20.0).paint_at(ui, Rect::from_center_size(mark, Vec2::splat(20.0))),
        None => {
            ui.painter().circle_stroke(mark, 9.0, Stroke::new(2.0, if selected { TEXT } else { DIM }));
            if selected {
                ui.painter().circle_filled(mark, 4.5, TEXT);
            }
        }
    }
    let x = rect.left() + 54.0;
    if subtitle.is_empty() {
        ui.painter().text(pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, title, semibold(16.0), TEXT);
    } else {
        ui.painter().text(pos2(x, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, title, semibold(16.0), TEXT);
        ui.painter().text(pos2(x, rect.center().y + 12.0), egui::Align2::LEFT_CENTER, subtitle, regular(13.5), SECONDARY);
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// A form label beside or above a field.
pub fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(medium(16.0)).color(SECONDARY));
}

/// A text field `width` wide (or the row's width when None).
pub fn text(ui: &mut Ui, value: &mut String, hint: &str, width: Option<f32>) -> egui::Response {
    let w = width.unwrap_or_else(|| ui.available_width());
    ui.add(egui::TextEdit::singleline(value).font(regular(16.5)).margin(Margin::symmetric(14, 12)).desired_width(w).hint_text(hint))
}

/// The steps of a form, as a bar of tabs under the header: done, current
/// and to come. When `jump` is set (editing), any step can be clicked.
pub fn steps(ui: &mut Ui, names: &[&str], current: usize, jump: bool) -> Option<usize> {
    let mut to = None;
    Frame::new().inner_margin(Margin { left: PAD, right: PAD, top: 8, bottom: 4 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let w = (ui.available_width() - 8.0 * (names.len() - 1) as f32) / names.len() as f32;
            for (k, name) in names.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 44.0), if jump { Sense::click() } else { Sense::hover() });
                let on = k == current;
                let done = k < current && !jump;
                let bar = Rect::from_min_size(rect.left_top(), vec2(rect.width(), 4.0));
                ui.painter().rect_filled(bar, CornerRadius::same(2), if on || done { TEXT } else { theme::OUTLINE });
                let color = if on { TEXT } else if jump && resp.hovered() { SECONDARY } else { DIM };
                ui.painter().text(pos2(rect.left(), rect.top() + 24.0), egui::Align2::LEFT_CENTER, format!("{}  {name}", k + 1), semibold(14.0), color);
                if jump && resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if jump && resp.clicked() {
                    to = Some(k);
                }
            }
        });
    });
    to
}

/// A text box of several lines, `width` wide, styled like `text`.
pub fn text_area(ui: &mut Ui, value: &mut String, hint: &str, width: f32, rows: usize) -> egui::Response {
    ui.add(egui::TextEdit::multiline(value).font(regular(16.5)).margin(Margin::symmetric(14, 12)).desired_width(width).desired_rows(rows).hint_text(hint))
}

/// A text field sized for a window.
pub fn field(ui: &mut Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.add(egui::TextEdit::singleline(text).font(regular(16.0)).margin(Margin::symmetric(14, 12)).desired_width(ui.available_width()).hint_text(hint))
}

/// A line of guidance or warning.
pub fn note(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(medium(14.0)).color(color)).wrap());
}

/// A row of what an action touches: an icon and a line.
pub fn fact(ui: &mut Ui, icon: Icon, tint: Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.add(icon.image(tint, 18.0));
        ui.add(egui::Label::new(RichText::new(text).font(regular(15.5)).color(TEXT)).wrap());
    });
}
