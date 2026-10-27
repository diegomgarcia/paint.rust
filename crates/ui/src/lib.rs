#![doc = "Desktop editor UI, real headless trace execution, and semantic usability-test hooks."]

use paint_core::Bgra;

mod app;
pub mod headless;
mod icons;
pub mod native;

pub use app::PaintApp;

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
