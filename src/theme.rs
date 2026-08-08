//! Visual theme — a cyan / sky "Phigros" look for egui.

use egui::{Color32, Rounding, Stroke};

/// Primary cyan accent (matches the icon's glowing "P").
pub const ACCENT: Color32 = Color32::from_rgb(0x36, 0xD1, 0xE0);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1E, 0x8A, 0x99);
/// Panel background (semi-transparent so the sky shows through).
pub const PANEL: Color32 = Color32::from_rgba_premultiplied(12, 20, 34, 210);
pub const PANEL_SOLID: Color32 = Color32::from_rgb(12, 20, 34);
pub const CARD: Color32 = Color32::from_rgba_premultiplied(20, 32, 52, 225);
pub const CARD_HOVER: Color32 = Color32::from_rgba_premultiplied(30, 48, 74, 235);
pub const TEXT: Color32 = Color32::from_rgb(0xEA, 0xF4, 0xFF);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9C, 0xB4, 0xCC);

/// Apply the theme to an egui context.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;

    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;
    v.window_fill = PANEL_SOLID;
    v.extreme_bg_color = Color32::from_rgb(8, 14, 24);
    v.faint_bg_color = CARD;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.hyperlink_color = ACCENT;

    let rounding = Rounding::same(10.0);
    v.widgets.noninteractive.rounding = rounding;
    v.widgets.inactive.rounding = rounding;
    v.widgets.hovered.rounding = rounding;
    v.widgets.active.rounding = rounding;
    v.widgets.open.rounding = rounding;

    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_DIM);
    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.weak_bg_fill = CARD_HOVER;
    v.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, TEXT);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.weak_bg_fill = ACCENT_DIM;
    v.widgets.active.fg_stroke = Stroke::new(1.2_f32, TEXT);

    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);

    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(26.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
    ]
    .into();

    ctx.set_style(style);
}
