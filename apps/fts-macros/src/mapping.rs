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
            MapMode::ScaleRange { min, max } => min + clamped * (max - min),
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

    /// Serialize to base64-encoded JSON string for plugin state storage
    ///
    /// This format is used for persistence in CLAP plugin state chunks.
    /// The base64 encoding ensures safe storage without special character issues.
    pub fn to_state_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        let json = self.to_json()?;
        Ok(base64_encode(&json))
    }

    /// Deserialize from base64-encoded JSON string
    ///
    /// Handles graceful degradation - if decoding fails, returns empty bank
    /// so the plugin continues to function without saved mappings.
    pub fn from_state_string(state_str: &str) -> Self {
        // Try to decode and parse
        match base64_decode(state_str) {
            Ok(json) => match Self::from_json(&json) {
                Ok(bank) => bank,
                Err(_) => {
                    // Invalid JSON - return empty bank
                    Self::new()
                }
            },
            Err(_) => {
                // Invalid base64 - return empty bank
                Self::new()
            }
        }
    }
}

/// Encode string to base64
fn base64_encode(s: &str) -> String {
    // Simple base64 implementation using standard algorithm
    const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = s.as_bytes();
    let mut result = String::new();

    let mut i = 0;
    while i < bytes.len() {
        let b1 = bytes[i];
        let b2 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b3 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };

        let n = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);

        result.push(BASE64_CHARS[((n >> 18) & 63) as usize] as char);
        result.push(BASE64_CHARS[((n >> 12) & 63) as usize] as char);
        result.push(if i + 1 < bytes.len() {
            BASE64_CHARS[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if i + 2 < bytes.len() {
            BASE64_CHARS[(n & 63) as usize] as char
        } else {
            '='
        });

        i += 3;
    }

    result
}

/// Decode base64 string
fn base64_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let c1 = base64_char_value(bytes[i])?;
        let c2 = if i + 1 < bytes.len() {
            base64_char_value(bytes[i + 1])?
        } else {
            return Err("incomplete base64".to_string());
        };

        let c3 = if i + 2 < bytes.len() && bytes[i + 2] != b'=' {
            base64_char_value(bytes[i + 2])?
        } else {
            0
        };

        let c4 = if i + 3 < bytes.len() && bytes[i + 3] != b'=' {
            base64_char_value(bytes[i + 3])?
        } else {
            0
        };

        let n = ((c1 as u32) << 18) | ((c2 as u32) << 12) | ((c3 as u32) << 6) | (c4 as u32);

        result.push((n >> 16) as u8);
        if i + 2 < bytes.len() && bytes[i + 2] != b'=' {
            result.push((n >> 8) as u8);
        }
        if i + 3 < bytes.len() && bytes[i + 3] != b'=' {
            result.push(n as u8);
        }

        i += 4;
    }

    String::from_utf8(result).map_err(|_| "invalid utf8".to_string())
}

fn base64_char_value(c: u8) -> Result<u8, String> {
    match c {
        b'A'..=b'Z' => Ok(c - b'A'),
        b'a'..=b'z' => Ok(c - b'a' + 26),
        b'0'..=b'9' => Ok(c - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 char: {}", c as char)),
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
        let mode = MapMode::ScaleRange { min: 0.5, max: 1.0 };
        assert_eq!(mode.apply(0.0), 0.5);
        assert_eq!(mode.apply(1.0), 1.0);
    }

    #[test]
    fn test_map_mode_scale_range_inverted() {
        // Inverted range: macro 0→0.8, macro 1→0.1 (threshold-style)
        let mode = MapMode::ScaleRange { min: 0.8, max: 0.1 };
        assert!((mode.apply(0.0) - 0.8).abs() < f32::EPSILON);
        assert!((mode.apply(1.0) - 0.1).abs() < f32::EPSILON);
        assert!((mode.apply(0.5) - 0.45).abs() < f32::EPSILON);
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

    #[test]
    fn test_base64_encode_decode() {
        let original = "Hello, World!";
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded).expect("decode failed");
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_base64_round_trip_json() {
        let json = r#"{"version":"0.1","mappings":[]}"#;
        let encoded = base64_encode(json);
        let decoded = base64_decode(&encoded).expect("decode failed");
        assert_eq!(json, decoded);
    }

    #[test]
    fn test_mapping_bank_state_string_round_trip() {
        let mut bank = MacroMappingBank::new();
        bank.add_mapping(MacroMapping {
            source_param: 0,
            target_track: TrackDescriptor::ByIndex(1),
            target_fx: FxDescriptor::ByPluginName("ReaEQ".to_string()),
            target_param_index: 3,
            mode: MapMode::ScaleRange { min: 0.5, max: 1.0 },
        })
        .unwrap();

        // Serialize to state string
        let state_string = bank.to_state_string().expect("to_state_string failed");

        // Deserialize from state string
        let restored = MacroMappingBank::from_state_string(&state_string);

        // Verify
        assert_eq!(bank.mappings.len(), restored.mappings.len());
        assert_eq!(
            bank.mappings[0].source_param,
            restored.mappings[0].source_param
        );
        assert_eq!(
            bank.mappings[0].target_param_index,
            restored.mappings[0].target_param_index
        );
    }

    #[test]
    fn test_mapping_bank_graceful_degradation() {
        // Invalid base64
        let bank = MacroMappingBank::from_state_string("!!!invalid base64!!!");
        assert_eq!(bank.mappings.len(), 0); // Returns empty bank, not error

        // Invalid JSON
        let bank = MacroMappingBank::from_state_string(&base64_encode("not valid json"));
        assert_eq!(bank.mappings.len(), 0); // Returns empty bank, not error
    }
}
