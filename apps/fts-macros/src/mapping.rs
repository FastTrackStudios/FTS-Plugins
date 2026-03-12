//! Macro mapping system - defines source→target parameter mappings
//!
//! A mapping connects a macro parameter (source) to a target FX parameter via a transformation mode.
//! Mappings are stored in the plugin state and applied during audio processing for sample-accuracy.

use serde::{Deserialize, Serialize};

/// A single macro parameter → FX parameter mapping
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroMapping {
    /// Source macro parameter index (0-7)
    pub source_param: u8,
    /// Target track descriptor
    pub target_track: TrackDescriptor,
    /// Target FX descriptor
    pub target_fx: FxDescriptor,
    /// Target FX parameter index
    pub target_param_index: u32,
    /// Value transformation mode
    pub mode: MapMode,
}

impl MacroMapping {
    /// Validate mapping consistency
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.source_param > 7 {
            return Err("source_param must be 0-7");
        }
        self.target_track.validate()?;
        self.target_fx.validate()?;
        Ok(())
    }
}

/// Track descriptor for runtime resolution
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TrackDescriptor {
    /// Track by index (0-based)
    ByIndex(u32),
    /// Track by exact name
    ByName(String),
    /// Track by name pattern (supports * wildcard)
    ByNamePattern(String),
}

impl TrackDescriptor {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            TrackDescriptor::ByIndex(_) => Ok(()),
            TrackDescriptor::ByName(name) => {
                if name.is_empty() {
                    Err("track name cannot be empty")
                } else {
                    Ok(())
                }
            }
            TrackDescriptor::ByNamePattern(pattern) => {
                if pattern.is_empty() {
                    Err("track pattern cannot be empty")
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// FX descriptor for runtime resolution
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FxDescriptor {
    /// FX by index in track FX chain (0-based)
    ByIndex(u32),
    /// FX by exact plugin name
    ByName(String),
    /// FX by plugin ID (e.g., "ReaEQ", "TT_ProReverb")
    ByPluginName(String),
}

impl FxDescriptor {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            FxDescriptor::ByIndex(_) => Ok(()),
            FxDescriptor::ByName(name) => {
                if name.is_empty() {
                    Err("FX name cannot be empty")
                } else {
                    Ok(())
                }
            }
            FxDescriptor::ByPluginName(name) => {
                if name.is_empty() {
                    Err("FX plugin name cannot be empty")
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// Value transformation mode for mapping
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MapMode {
    /// Direct passthrough: source value → target value (0.0-1.0)
    PassThrough,

    /// Scale range: map 0.0-1.0 to [min..max]
    ScaleRange { min: f32, max: f32 },

    /// Relative increment: each change moves target by step amount
    Relative { step: f32 },

    /// Toggle mode: threshold at 0.5, below=off above=on
    Toggle,
}

impl MapMode {
    /// Apply mode transformation to a source value
    pub fn apply(&self, source_value: f32) -> f32 {
        let clamped = source_value.clamp(0.0, 1.0);
        match self {
            MapMode::PassThrough => clamped,
            MapMode::ScaleRange { min, max } => {
                let range = (max - min).abs();
                let start = if min < max { *min } else { *max };
                start + clamped * range
            }
            MapMode::Relative { step } => {
                // In relative mode, we'd need state tracking (not implemented here)
                // For now, return step scaled by source value
                clamped * step.abs()
            }
            MapMode::Toggle => {
                if clamped >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

/// Collection of all mappings for a macro bank
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MacroMappingBank {
    /// Serialization version for future compatibility
    pub version: String,
    /// All active mappings
    pub mappings: Vec<MacroMapping>,
}

impl MacroMappingBank {
    /// Create an empty mapping bank
    pub fn new() -> Self {
        Self {
            version: "0.1".to_string(),
            mappings: Vec::new(),
        }
    }

    /// Add a mapping to the bank
    pub fn add_mapping(&mut self, mapping: MacroMapping) -> Result<(), &'static str> {
        mapping.validate()?;
        self.mappings.push(mapping);
        Ok(())
    }

    /// Get all mappings for a specific macro parameter
    pub fn get_mappings_for_param(&self, param_index: u8) -> Vec<&MacroMapping> {
        self.mappings
            .iter()
            .filter(|m| m.source_param == param_index)
            .collect()
    }

    /// Validate entire bank
    pub fn validate(&self) -> Result<(), &'static str> {
        for mapping in &self.mappings {
            mapping.validate()?;
        }
        Ok(())
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macro_mapping_validation() {
        let mut mapping = MacroMapping {
            source_param: 0,
            target_track: TrackDescriptor::ByIndex(1),
            target_fx: FxDescriptor::ByName("Compressor".to_string()),
            target_param_index: 2,
            mode: MapMode::PassThrough,
        };

        assert!(mapping.validate().is_ok());

        // Invalid source param
        mapping.source_param = 8;
        assert!(mapping.validate().is_err());
    }

    #[test]
    fn test_map_mode_passthrough() {
        assert_eq!(MapMode::PassThrough.apply(0.0), 0.0);
        assert_eq!(MapMode::PassThrough.apply(0.5), 0.5);
        assert_eq!(MapMode::PassThrough.apply(1.0), 1.0);
    }

    #[test]
    fn test_map_mode_scale_range() {
        let mode = MapMode::ScaleRange {
            min: 0.5,
            max: 1.0,
        };
        assert_eq!(mode.apply(0.0), 0.5);
        assert_eq!(mode.apply(1.0), 1.0);
    }

    #[test]
    fn test_map_mode_toggle() {
        let mode = MapMode::Toggle;
        assert_eq!(mode.apply(0.3), 0.0);
        assert_eq!(mode.apply(0.5), 1.0);
        assert_eq!(mode.apply(0.7), 1.0);
    }

    #[test]
    fn test_mapping_bank_serialization() {
        let mut bank = MacroMappingBank::new();
        bank.add_mapping(MacroMapping {
            source_param: 0,
            target_track: TrackDescriptor::ByName("Drums".to_string()),
            target_fx: FxDescriptor::ByPluginName("ReaEQ".to_string()),
            target_param_index: 3,
            mode: MapMode::PassThrough,
        })
        .unwrap();

        let json = bank.to_json().expect("serialization failed");
        let restored = MacroMappingBank::from_json(&json).expect("deserialization failed");

        assert_eq!(bank.mappings.len(), restored.mappings.len());
        assert_eq!(bank.mappings[0], restored.mappings[0]);
    }

    #[test]
    fn test_get_mappings_for_param() {
        let mut bank = MacroMappingBank::new();
        bank.add_mapping(MacroMapping {
            source_param: 0,
            target_track: TrackDescriptor::ByIndex(1),
            target_fx: FxDescriptor::ByIndex(0),
            target_param_index: 2,
            mode: MapMode::PassThrough,
        })
        .unwrap();

        bank.add_mapping(MacroMapping {
            source_param: 0,
            target_track: TrackDescriptor::ByIndex(2),
            target_fx: FxDescriptor::ByIndex(1),
            target_param_index: 3,
            mode: MapMode::PassThrough,
        })
        .unwrap();

        bank.add_mapping(MacroMapping {
            source_param: 1,
            target_track: TrackDescriptor::ByIndex(3),
            target_fx: FxDescriptor::ByIndex(0),
            target_param_index: 4,
            mode: MapMode::PassThrough,
        })
        .unwrap();

        let param_0_mappings = bank.get_mappings_for_param(0);
        assert_eq!(param_0_mappings.len(), 2);

        let param_1_mappings = bank.get_mappings_for_param(1);
        assert_eq!(param_1_mappings.len(), 1);
    }
}
