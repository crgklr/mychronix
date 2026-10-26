//! The window's own controls on Linux, where Mychronix draws no title bar:
//! maximize/restore and close buttons in the top-right corner, the top edge and the sidebar to
//! move the window (double-click to maximize), the edges to resize it, and a
//! thin outline that sets it apart from the windows behind it.
//! On a Mac the system's title bar stays.

use eframe::egui::{self, CornerRadius, CursorIcon, Rect, ResizeDirection, Sense, Stroke, StrokeKind, ViewportCommand, pos2, vec2};

use super::App;
use crate::theme::{self, Icon, SECONDARY, SURFACE_HOVER, TEXT};

/// Mychronix draws its own window controls (no title bar).
pub(crate) const OWN_CONTROLS: bool = cfg!(target_os = "linux");

/// Room the top-right corner needs for the buttons: anything else in that
/// corner (the update and error bars) keeps this much clear.
pub(super) const CORNER_W: f32 = if OWN_CONTROLS { 66.0 } else { 0.0 };

/// How far in from the window's edge a press resizes it.
const EDGE: f32 = 8.0;

impl App {
    pub(super) fn window_controls(&mut self, ctx: &egui::Context) {
        if !OWN_CONTROLS {
            return;
        }
        let screen = ctx.content_rect();
        let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
        // Kept short so they stay clear of the pages' own top-right buttons.
        let close = Rect::from_min_size(pos2(screen.right() - 30.0, screen.top() + 3.0), vec2(26.0, 22.0));
        let size = close.translate(vec2(-28.0, 0.0));

        // The top edge moves the window, as the title bar did; double-click to maximize.
        let rail_right = self.rail_rect.map_or(screen.left(), |r| r.right());
        let top = Rect::from_min_max(pos2(rail_right, screen.top()), pos2(size.left() - 4.0, screen.top() + 18.0));
        self.on_top(ctx, "window_move", top, |_, resp| move_or_maximize(ctx, &resp, maximized));

        // Maximized: a "–" that restores the window. Otherwise: a box that maximizes it.
        self.on_top(ctx, "window_size", size, |ui, resp| {
            let color = button_back(ui, &resp, size);
            let c = size.center();
            if maximized {
                ui.painter().line_segment([pos2(c.x - 6.0, c.y), pos2(c.x + 6.0, c.y)], Stroke::new(1.6, color));
            } else {
                let square = Rect::from_center_size(c, vec2(11.0, 11.0));
                ui.painter().rect_stroke(square, CornerRadius::same(2), Stroke::new(1.5, color), StrokeKind::Middle);
            }
            let resp = resp.on_hover_text(if maximized { "Restore down" } else { "Maximize" });
            if resp.clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            }
        });

        // The close button.
        self.on_top(ctx, "window_close", close, |ui, resp| {
            let color = button_back(ui, &resp, close);
            let glyph = Rect::from_center_size(close.center(), vec2(14.0, 14.0));
            Icon::X.image(color, 14.0).paint_at(ui, glyph);
            let resp = resp.on_hover_text("Close Mychronix");
            if resp.clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        });

        // The edges resize the window, unless it fills the screen.
        if maximized || fullscreen {
            return;
        }
        // A 1-pixel outline: Wayland leaves window shadows to the app, and a shadow
        // would throw off snapping and tiling (see the commit that added this).
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("window_outline")));
        let px = 1.0 / ctx.pixels_per_point();
        painter.rect_stroke(screen, CornerRadius::ZERO, Stroke::new(px, theme::SURFACE_ACTIVE), StrokeKind::Inside);
        let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
        let edges = [
            (ResizeDirection::West, Rect::from_min_max(pos2(l, t + EDGE), pos2(l + EDGE, b - EDGE)), CursorIcon::ResizeWest),
            (ResizeDirection::East, Rect::from_min_max(pos2(r - EDGE, t + EDGE), pos2(r, b - EDGE)), CursorIcon::ResizeEast),
            (ResizeDirection::North, Rect::from_min_max(pos2(l + EDGE, t), pos2(r - EDGE, t + EDGE)), CursorIcon::ResizeNorth),
            (ResizeDirection::South, Rect::from_min_max(pos2(l + EDGE, b - EDGE), pos2(r - EDGE, b)), CursorIcon::ResizeSouth),
            (ResizeDirection::NorthWest, Rect::from_min_size(pos2(l, t), vec2(EDGE * 2.0, EDGE * 2.0)), CursorIcon::ResizeNorthWest),
            (ResizeDirection::NorthEast, Rect::from_min_size(pos2(r - EDGE * 2.0, t), vec2(EDGE * 2.0, EDGE * 2.0)), CursorIcon::ResizeNorthEast),
            (ResizeDirection::SouthWest, Rect::from_min_size(pos2(l, b - EDGE * 2.0), vec2(EDGE * 2.0, EDGE * 2.0)), CursorIcon::ResizeSouthWest),
            (ResizeDirection::SouthEast, Rect::from_min_size(pos2(r - EDGE * 2.0, b - EDGE * 2.0), vec2(EDGE * 2.0, EDGE * 2.0)), CursorIcon::ResizeSouthEast),
        ];
        for (direction, rect, cursor) in edges {
            self.on_top(ctx, ("window_resize", format!("{direction:?}")), rect, |ui, resp| {
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                // On the press itself: the desktop takes the resize from there.
                if pressed_on(ui, &resp) {
                    log::debug!("window: resize {direction:?}");
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
        }
    }

    /// Runs `f` with a click-and-drag area over `rect`, above the pages.
    fn on_top(&self, ctx: &egui::Context, id: impl std::hash::Hash + std::fmt::Debug, rect: Rect, f: impl FnOnce(&mut egui::Ui, egui::Response)) {
        let id = egui::Id::new(id);
        egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(rect.min).interactable(true).show(ctx, |ui| {
            ui.set_min_size(rect.size());
            let resp = ui.interact(rect, id.with("hit"), Sense::click_and_drag());
            f(ui, resp);
        });
    }
}

/// A window button's hover background; returns the color for its glyph.
fn button_back(ui: &egui::Ui, resp: &egui::Response, rect: Rect) -> egui::Color32 {
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL), SURFACE_HOVER);
        TEXT
    } else {
        SECONDARY
    }
}

/// Dragging moves the window; a double-click maximizes it or restores it.
pub(super) fn move_or_maximize(ctx: &egui::Context, resp: &egui::Response, maximized: bool) {
    if !OWN_CONTROLS {
        return;
    }
    if resp.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started() {
        log::debug!("window: move");
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// The primary button went down on `resp` this frame.
fn pressed_on(ui: &egui::Ui, resp: &egui::Response) -> bool {
    resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
}
