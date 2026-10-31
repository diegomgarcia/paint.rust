#![doc = "Desktop editor UI, real headless trace execution, and semantic usability-test hooks."]

use paint_core::Bgra;

mod app;
mod checkerboard;
mod colors;
pub mod headless;
mod icons;
mod layers;
#[cfg(target_os = "macos")]
mod menu;
pub mod native;
mod rulers;
mod selection;

pub use app::{
    COLORS_PANEL_LESS_SIZE, COLORS_PANEL_MORE_SIZE, COLORS_PANEL_TITLE, HISTORY_PANEL_SIZE,
    HISTORY_PANEL_TITLE, LAYER_ITEM_HEIGHT, LAYERS_PANEL_SIZE, LAYERS_PANEL_TITLE,
    LayerListGeometry, LayerRowGeometry, PaintApp, PanelLayout, TOOLS_PANEL_TITLE,
    TOOLS_PANEL_WIDTH, ToolboxEntry, default_panel_layout,
};
pub use rulers::{RULER_THICKNESS, RulerLayout, RulerTick};

/// Converts hue in degrees, saturation, value, and alpha into canonical BGRA.
pub fn color_from_hsv(hue_degrees: f32, saturation: f32, value: f32, alpha: u8) -> Bgra {
    let hue = hue_degrees.rem_euclid(360.0) / 60.0;
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let chroma = value * saturation;
    let secondary = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (red, green, blue) = match hue.floor() as i32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let match_value = value - chroma;
    let channel = |component: f32| ((component + match_value) * 255.0).round() as u8;
    Bgra::new(channel(blue), channel(green), channel(red), alpha)
}

/// Parses `RRGGBB` or `RRGGBBAA` with an optional leading `#`.
pub fn color_from_hex(text: &str) -> Result<Bgra, String> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if digits.len() != 6 && digits.len() != 8 {
        return Err("hex color must contain 6 or 8 digits".to_owned());
    }
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("hex color contains a non-hexadecimal digit".to_owned());
    }
    let pair = |start: usize| {
        u8::from_str_radix(&digits[start..start + 2], 16)
            .map_err(|_| "hex color contains an invalid byte".to_owned())
    };
    let red = pair(0)?;
    let green = pair(2)?;
    let blue = pair(4)?;
    let alpha = if digits.len() == 8 { pair(6)? } else { u8::MAX };
    Ok(Bgra::new(blue, green, red, alpha))
}
