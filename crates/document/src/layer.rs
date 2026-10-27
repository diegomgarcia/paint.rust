use paint_core::Surface;
use serde::{Deserialize, Serialize};

/// A stable layer identifier. IDs are never reused within a document.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct LayerId(pub u64);

impl LayerId {
    pub const MAX: Self = Self(u64::MAX);

    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(id) => Some(Self(id)),
            None => None,
        }
    }
}

/// A monotonically increasing layer or document revision.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct Revision(pub u64);

impl Revision {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u64::MAX);

    pub const fn new(revision: u64) -> Self {
        Self(revision)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(revision) => Some(Self(revision)),
            None => None,
        }
    }
}

/// Layer blend modes known to the model. Only `Normal` is implemented in this preview.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
}

/// One ordered document layer. Pixel mutation is mediated by `PixelPatch`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layer {
    id: LayerId,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) background: bool,
    pub(crate) blend_mode: BlendMode,
    pub(crate) revision: Revision,
    surface: Surface,
}

impl Layer {
    pub fn new(id: LayerId, name: impl Into<String>, surface: Surface) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            opacity: u8::MAX,
            background: false,
            blend_mode: BlendMode::Normal,
            revision: Revision::ZERO,
            surface,
        }
    }

    pub fn with_visibility(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_opacity(mut self, opacity: u8) -> Self {
        self.opacity = opacity;
        self
    }

    pub fn with_background(mut self, background: bool) -> Self {
        self.background = background;
        self
    }

    pub const fn id(&self) -> LayerId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Layer-wide opacity on a 0–255 scale.
    pub const fn opacity(&self) -> u8 {
        self.opacity
    }

    pub const fn is_background(&self) -> bool {
        self.background
    }

    pub const fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub const fn surface(&self) -> &Surface {
        &self.surface
    }

    pub(crate) const fn surface_mut(&mut self) -> &mut Surface {
        &mut self.surface
    }
}
