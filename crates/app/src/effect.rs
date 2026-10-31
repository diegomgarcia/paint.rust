/// A Paint.NET-style category used to group adjustments and effects in menus.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EffectCategory {
    Adjustment,
    Artistic,
    Blurs,
    Distort,
    Noise,
    Photo,
    Render,
    Stylize,
}

impl EffectCategory {
    /// Categories shown in the Paint.NET Effects menu. Adjustment has its own top-level menu.
    pub const ALL: [Self; 7] = [
        Self::Artistic,
        Self::Blurs,
        Self::Distort,
        Self::Noise,
        Self::Photo,
        Self::Render,
        Self::Stylize,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Adjustment => "Adjustment",
            Self::Artistic => "Artistic",
            Self::Blurs => "Blurs",
            Self::Distort => "Distort",
            Self::Noise => "Noise",
            Self::Photo => "Photo",
            Self::Render => "Render",
            Self::Stylize => "Stylize",
        }
    }
}

/// One implemented, selection-aware raster effect and its configuration.
///
/// Every effect is applied to the active layer as one document mutation, so a single undo
/// reverts the complete effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    Grayscale,
    Sepia,
    /// Brightness and contrast deltas in -100..=100.
    BrightnessContrast {
        brightness: i32,
        contrast: i32,
    },
    /// HSL hue rotation in degrees and saturation/lightness deltas in -100..=100.
    HueSaturation {
        hue: i32,
        saturation: i32,
        lightness: i32,
    },
    /// Per-channel selection-driven 0..=255 stretch.
    AutoLevel,
    /// Separable Gaussian blur with a radius in pixels.
    GaussianBlur {
        radius: u32,
    },
}

impl Effect {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Grayscale => "Grayscale",
            Self::Sepia => "Sepia",
            Self::BrightnessContrast { .. } => "Brightness / Contrast",
            Self::HueSaturation { .. } => "Hue / Saturation",
            Self::AutoLevel => "Auto-Level",
            Self::GaussianBlur { .. } => "Gaussian Blur",
        }
    }

    pub const fn category(self) -> EffectCategory {
        match self {
            Self::Grayscale | Self::Sepia => EffectCategory::Photo,
            Self::BrightnessContrast { .. } | Self::HueSaturation { .. } | Self::AutoLevel => {
                EffectCategory::Adjustment
            }
            Self::GaussianBlur { .. } => EffectCategory::Blurs,
        }
    }

    /// History entry text, including the configured blur radius when present.
    pub fn history_label(self) -> String {
        match self {
            Self::GaussianBlur { radius } => format!("Gaussian Blur (radius {radius})"),
            Self::Grayscale
            | Self::Sepia
            | Self::BrightnessContrast { .. }
            | Self::HueSaturation { .. }
            | Self::AutoLevel => self.name().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect, EffectCategory};

    #[test]
    fn implemented_effects_use_paint_net_categories_and_specific_history_labels() {
        assert_eq!(Effect::Grayscale.category(), EffectCategory::Photo);
        assert_eq!(Effect::Sepia.category(), EffectCategory::Photo);
        assert_eq!(
            Effect::BrightnessContrast {
                brightness: 10,
                contrast: -5
            }
            .category(),
            EffectCategory::Adjustment
        );
        assert_eq!(
            Effect::HueSaturation {
                hue: 20,
                saturation: -10,
                lightness: 5
            }
            .category(),
            EffectCategory::Adjustment
        );
        assert_eq!(Effect::AutoLevel.category(), EffectCategory::Adjustment);
        assert_eq!(
            Effect::GaussianBlur { radius: 4 }.category(),
            EffectCategory::Blurs
        );
        assert_eq!(Effect::Grayscale.history_label(), "Grayscale");
        assert_eq!(
            Effect::BrightnessContrast {
                brightness: 10,
                contrast: -5
            }
            .history_label(),
            "Brightness / Contrast"
        );
        assert_eq!(
            Effect::HueSaturation {
                hue: 20,
                saturation: -10,
                lightness: 5
            }
            .history_label(),
            "Hue / Saturation"
        );
        assert_eq!(Effect::AutoLevel.history_label(), "Auto-Level");
        assert_eq!(
            Effect::GaussianBlur { radius: 4 }.history_label(),
            "Gaussian Blur (radius 4)"
        );
        assert_eq!(EffectCategory::ALL.len(), 7);
    }
}
