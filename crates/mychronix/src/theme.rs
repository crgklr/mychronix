//! Visual design: color tokens, type, icons and the egui style built from them.
//!
//! Everything the UI draws takes its colors from the constants here, so the
//! look can be tuned (or a light theme added) in one place. Fonts, text
//! rendering and the icon loader come from fastframe (github.com/crmne/fastframe),
//! the shared foundation of Spotifast and friends.

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};
use fastframe_fonts::{FontSetup, Monospace, Weight};

// Surfaces, darkest to lightest.
pub const WINDOW: Color32 = Color32::from_rgb(0x0f, 0x11, 0x14);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x14, 0x17, 0x1b);
pub const SURFACE: Color32 = Color32::from_rgb(0x1b, 0x1f, 0x24);
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x24, 0x29, 0x30);
pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x2d, 0x33, 0x3c);
pub const OUTLINE: Color32 = Color32::from_rgb(0x26, 0x2b, 0x32);
pub const GRID: Color32 = Color32::from_rgb(0x27, 0x2c, 0x33);

// Ink.
pub const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf4, 0xf6);
pub const SECONDARY: Color32 = Color32::from_rgb(0xa9, 0xb1, 0xbc);
pub const DIM: Color32 = Color32::from_rgb(0x6e, 0x77, 0x84);
/// Status "good" (best lap). Reserved: never a channel color; always paired with a label.
pub const GOOD: Color32 = Color32::from_rgb(0x34, 0xc7, 0x59);
/// Favorite stars. Not a channel color.
/// Deleting things: buttons and warnings.
pub const DANGER: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
pub const FAVORITE: Color32 = Color32::from_rgb(0xf5, 0xb7, 0x2e);
/// The logo's cyan: marks what's open (the session indicator in the rail).
pub const ACCENT: Color32 = Color32::from_rgb(0x19, 0xd3, 0xf2);
/// Text on a `TEXT`-filled control (selected chip, primary button).
pub const ON_TEXT: Color32 = Color32::from_rgb(0x0f, 0x11, 0x14);

/// Channel colors, assigned in this fixed order and kept with the channel.
/// Validated for the `SURFACE` card color (CVD-adjacent ΔE ≥ 8.4, contrast ≥ 3:1).
/// Selection and focus are drawn in neutral ink so they never look like data.
pub const SERIES: [Color32; 8] = [
    Color32::from_rgb(0x39, 0x87, 0xe5), // blue
    Color32::from_rgb(0xd9, 0x59, 0x26), // orange
    Color32::from_rgb(0x19, 0x9e, 0x70), // aqua
    Color32::from_rgb(0xc9, 0x85, 0x00), // yellow
    Color32::from_rgb(0xd5, 0x51, 0x81), // magenta
    Color32::from_rgb(0x00, 0x83, 0x00), // green
    Color32::from_rgb(0x90, 0x85, 0xe9), // violet
    Color32::from_rgb(0xe6, 0x67, 0x67), // red
];

pub const RADIUS: u8 = 10;
pub const RADIUS_SMALL: u8 = 6;
pub const ROW_HEIGHT: f32 = 36.0;

pub fn regular(size: f32) -> FontId {
    Weight::Regular.font_id(size)
}

pub fn medium(size: f32) -> FontId {
    Weight::Medium.font_id(size)
}

pub fn semibold(size: f32) -> FontId {
    Weight::SemiBold.font_id(size)
}

fastframe_icons::icons! {
    /// Every icon the interface draws: Lucide (ISC), white so egui's tint colors them.
    pub enum Icon {
        prefix: "mychronix-icon-",
        directory: "../assets/icons/",
        Activity => "activity",
        Calendar => "calendar",
        List => "list",
        User => "user",
        ChartLine => "chart-line",
        Flag => "flag",
        FolderOpen => "folder-open",
        Gauge => "gauge",
        MapPin => "map-pin",
        Star => "star",
        StarFilled => "star-filled",
        Timer => "timer",
        Usb => "usb",
        Wifi => "wifi",
        Search => lucide "search",
        X => lucide "x",
        Check => lucide "check",
        ChevronDown => lucide "chevron-down",
        ChevronRight => lucide "chevron-right",
        ChevronLeft => lucide "chevron-left",
        Clock => lucide "clock",
        Settings => lucide "settings",
        Plus => lucide "plus",
        Pencil => lucide "pencil",
        Trash => lucide "trash-2",
        EyeOff => lucide "eye-off",
        CircleCheck => lucide "circle-check",
        Users => lucide "users",
        Download => "download",
        CloudOff => "cloud-off",
        TriangleAlert => "triangle-alert",
        House => "house",
        /// A kart track's outline, for the Map view.
        Track => "track",
        /// An insight worth a look (Lucide's lightbulb).
        Lightbulb => "lightbulb",
        /// A MyChron: its case, screen and two buttons (Mychronix's own, in Lucide's style).
        Mychron => "mychron",
    }
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontSetup::default()
        .weights(&[Weight::Medium, Weight::SemiBold, Weight::Bold])
        .monospace(Monospace::Inter)
        .definitions();
    let rendering = fastframe_text::detect();
    rendering.apply_to(&mut fonts);
    ctx.set_fonts(fonts);

    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        apply(style);
        rendering.apply_to_visuals(&mut style.visuals);
    });
}

fn apply(style: &mut egui::Style) {
    style.text_styles = [
        (TextStyle::Small, regular(11.5)),
        (TextStyle::Body, regular(14.0)),
        (TextStyle::Button, medium(14.0)),
        (TextStyle::Heading, semibold(17.0)),
        (TextStyle::Monospace, regular(13.0)),
    ]
    .into();

    // Labels are UI text, not documents: no text cursor or selection, and clicks
    // go to the row or button they sit on.
    style.interaction.selectable_labels = false;

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(14.0, 7.0);
    s.interact_size = Vec2::new(40.0, 32.0);
    s.window_margin = Margin::same(12);

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = WINDOW;
    v.window_fill = SURFACE;
    v.window_stroke = Stroke::new(1.0, OUTLINE);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.extreme_bg_color = SURFACE;
    // Text fields sit darker than the cards they're on, so they read as fields.
    v.text_edit_bg_color = Some(WINDOW);
    v.faint_bg_color = SURFACE;
    v.hyperlink_color = TEXT;
    v.selection.bg_fill = SURFACE_ACTIVE;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.text_cursor.stroke = Stroke::new(2.0, TEXT);

    let radius = CornerRadius::same(RADIUS_SMALL);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = SURFACE;
    w.noninteractive.weak_bg_fill = SURFACE;
    w.noninteractive.bg_stroke = Stroke::NONE;
    w.noninteractive.fg_stroke = Stroke::new(1.0, SECONDARY);
    w.noninteractive.corner_radius = radius;
    for (state, fill, stroke) in [
        (&mut w.inactive, SURFACE, Stroke::NONE),
        (&mut w.hovered, SURFACE_HOVER, Stroke::NONE),
        (&mut w.active, SURFACE_ACTIVE, Stroke::NONE),
        (&mut w.open, SURFACE_HOVER, Stroke::NONE),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = stroke;
        state.fg_stroke = Stroke::new(1.0, TEXT);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
}
