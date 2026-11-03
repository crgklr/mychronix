//! Notices: every status and problem the app reports, in one place and one
//! look. Pages push them (`self.notices.error(…)`, `.success(…)`); they show
//! as a stack in the window's bottom-right corner, above the status bar.
//! Info and success go after a few seconds; warnings and errors stay until
//! dismissed. Progress (a download's bar) stays on its page: notices are for
//! outcomes. Each is logged once, when pushed.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone)]
pub(super) struct Notice {
    tone: Tone,
    text: String,
    /// A button that goes to a page, e.g. ("Open Sessions", Page::Sessions).
    action: Option<(&'static str, Page)>,
    /// When it was first drawn (egui time), for fading out.
    shown: Option<f64>,
}

/// How long info and success notices stay, in seconds.
const STAYS_S: f64 = 7.0;
/// At most this many at once; the oldest go first.
const MOST: usize = 4;

#[derive(Default)]
pub(super) struct Notices {
    list: Vec<Notice>,
}

impl Notices {
    pub fn push(&mut self, tone: Tone, text: impl Into<String>) -> &mut Self {
        let text = text.into();
        match tone {
            Tone::Info | Tone::Success => log::info!("notice: {text}"),
            Tone::Warning | Tone::Error => log::warn!("notice: {text}"),
        }
        // The same thing again: shown once, afresh.
        self.list.retain(|n| n.text != text);
        self.list.push(Notice {
            tone,
            text,
            action: None,
            shown: None,
        });
        if self.list.len() > MOST {
            self.list.remove(0);
        }
        self
    }

    pub fn info(&mut self, text: impl Into<String>) -> &mut Self {
        self.push(Tone::Info, text)
    }

    pub fn success(&mut self, text: impl Into<String>) -> &mut Self {
        self.push(Tone::Success, text)
    }

    pub fn warning(&mut self, text: impl Into<String>) -> &mut Self {
        self.push(Tone::Warning, text)
    }

    pub fn error(&mut self, text: impl Into<String>) -> &mut Self {
        self.push(Tone::Error, text)
    }

    /// A button on the notice just pushed that goes to `page`.
    pub fn go_to(&mut self, label: &'static str, page: Page) -> &mut Self {
        if let Some(n) = self.list.last_mut() {
            n.action = Some((label, page));
        }
        self
    }
}

impl App {
    /// The notices, bottom right, over everything (dialogs included).
    pub(super) fn notices_overlay(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        for n in &mut self.notices.list {
            n.shown.get_or_insert(now);
        }
        self.notices.list.retain(|n| {
            matches!(n.tone, Tone::Warning | Tone::Error) || now - n.shown.unwrap_or(now) < STAYS_S
        });
        if self.notices.list.is_empty() {
            return;
        }
        // Wake up to fade the next one out.
        if let Some(left) = self
            .notices
            .list
            .iter()
            .filter(|n| matches!(n.tone, Tone::Info | Tone::Success))
            .map(|n| STAYS_S - (now - n.shown.unwrap_or(now)))
            .reduce(f64::min)
        {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.05)));
        }
        let (mut dismiss, mut go) = (None, None);
        egui::Area::new(egui::Id::new("notices"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-18.0, -42.0))
            .interactable(true)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for (k, n) in self.notices.list.iter().enumerate() {
                    let (icon, color) = match n.tone {
                        Tone::Info => (Icon::Activity, theme::ACCENT),
                        Tone::Success => (Icon::CircleCheck, theme::GOOD),
                        Tone::Warning => (Icon::TriangleAlert, theme::FAVORITE),
                        Tone::Error => (Icon::TriangleAlert, theme::DANGER),
                    };
                    Frame::new()
                        .fill(SURFACE_ACTIVE)
                        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
                        .corner_radius(CornerRadius::same(12))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 4],
                            blur: 18,
                            spread: 0,
                            color: Color32::from_black_alpha(90),
                        })
                        .inner_margin(Margin::symmetric(14, 11))
                        .show(ui, |ui| {
                            ui.set_max_width(420.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                ui.add(icon.image(color, 16.0));
                                ui.vertical(|ui| {
                                    ui.set_max_width(340.0);
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&n.text).font(regular(13.5)).color(TEXT),
                                        )
                                        .wrap(),
                                    );
                                    if let Some((label, page)) = n.action
                                        && ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new(label)
                                                        .font(medium(13.5))
                                                        .color(theme::ACCENT),
                                                )
                                                .sense(Sense::click()),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                    {
                                        go = Some(page);
                                        dismiss = Some(k);
                                    }
                                });
                                let x = ui
                                    .add(Icon::X.image(SECONDARY, 12.0).sense(Sense::click()))
                                    .on_hover_text("Dismiss")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if x.clicked() {
                                    dismiss = Some(k);
                                }
                            });
                        });
                }
            });
        if let Some(k) = dismiss {
            self.notices.list.remove(k);
        }
        if let Some(p) = go {
            self.page = p;
        }
    }
}
