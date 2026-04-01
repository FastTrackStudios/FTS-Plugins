//! Saturation style definitions — category + variant selection.

/// Top-level saturation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Category {
    Tape = 0,
    Tube = 1,
    Saturation = 2,
    Amp = 3,
    Transformer = 4,
    FX = 5,
}

impl Category {
    pub const COUNT: usize = 6;

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Tape,
            1 => Self::Tube,
            2 => Self::Saturation,
            3 => Self::Amp,
            4 => Self::Transformer,
            5 => Self::FX,
            _ => Self::Tape,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tape => "Tape",
            Self::Tube => "Tube",
            Self::Saturation => "Saturation",
            Self::Amp => "Amp",
            Self::Transformer => "Transformer",
            Self::FX => "FX",
        }
    }

    /// Return the variant names for this category.
    pub fn variants(self) -> &'static [&'static str] {
        match self {
            Self::Tape => &["Subtle", "Clean", "Warm", "Extreme"],
            Self::Tube => &["Subtle", "Clean", "Warm", "Hot", "Broken"],
            Self::Saturation => &["Mojo", "PurestDrive", "Density"],
            Self::Amp => &["Clean", "Crunch", "High Gain", "Overdrive"],
            Self::Transformer => &["Subtle", "Warm", "Colorful"],
            Self::FX => &["Smudge", "Foldback", "Rectify", "Destroy"],
        }
    }

    /// Number of variants in this category.
    pub fn variant_count(self) -> usize {
        self.variants().len()
    }
}

/// A fully-resolved style: category + variant index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub category: Category,
    pub variant: usize,
}

impl Style {
    pub fn new(category: Category, variant: usize) -> Self {
        let variant = variant.min(category.variant_count().saturating_sub(1));
        Self { category, variant }
    }

    pub fn variant_name(&self) -> &'static str {
        let variants = self.category.variants();
        variants[self.variant.min(variants.len() - 1)]
    }

    pub fn display_name(&self) -> String {
        format!("{} — {}", self.category.name(), self.variant_name())
    }
}

impl Default for Style {
    fn default() -> Self {
        Self {
            category: Category::Tape,
            variant: 2, // Tape — Warm (ToTape9)
        }
    }
}
