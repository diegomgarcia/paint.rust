/// A Paint.NET-style category used to group effects in the Effects menu.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EffectCategory {
    Artistic,
    Blurs,
    Distort,
    Noise,
    Photo,
    Render,
    Stylize,
}

impl EffectCategory {
    /// Categories in Paint.NET menu order.
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
            Self::GaussianBlur { .. } => "Gaussian Blur",
        }
    }

    pub const fn category(self) -> EffectCategory {
        match self {
            Self::Grayscale | Self::Sepia => EffectCategory::Photo,
            Self::GaussianBlur { .. } => EffectCategory::Blurs,
        }
    }

    /// History entry text, including the configured blur radius when present.
    pub fn history_label(self) -> String {
        match self {
            Self::GaussianBlur { radius } => format!("Gaussian Blur (radius {radius})"),
            Self::Grayscale => Self::Grayscale.name().to_owned(),
            Self::Sepia => Self::Sepia.name().to_owned(),
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
            Effect::GaussianBlur { radius: 4 }.category(),
            EffectCategory::Blurs
        );
        assert_eq!(Effect::Grayscale.history_label(), "Grayscale");
        assert_eq!(
            Effect::GaussianBlur { radius: 4 }.history_label(),
            "Gaussian Blur (radius 4)"
        );
        assert_eq!(EffectCategory::ALL.len(), 7);
    }
}
