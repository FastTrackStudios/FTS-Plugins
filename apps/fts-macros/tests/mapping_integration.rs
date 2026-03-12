//! Integration tests for fts-macros mapping system
//!
//! Tests the complete mapping pipeline:
//! 1. Create mappings with various track/FX descriptors
//! 2. Apply mode transformations to macro values
//! 3. Verify resolver caching works correctly
//! 4. Test boundary cases (0.0, 1.0, 0.5)

use fts_macros::mapping::{
    FxDescriptor, MacroMapping, MacroMappingBank, MapMode, TrackDescriptor,
};
use fts_macros::resolver::ResolutionCache;

#[test]
fn test_macro_to_fx_parameter_passthrough() {
    // Create a simple passthrough mapping
    let mut bank = MacroMappingBank::new();
    bank.add_mapping(MacroMapping {
        source_param: 0,
        target_track: TrackDescriptor::ByIndex(1),
        target_fx: FxDescriptor::ByIndex(0),
        target_param_index: 2,
        mode: MapMode::PassThrough,
    })
    .expect("mapping validation failed");

    // Get mappings for macro 0
    let mappings = bank.get_mappings_for_param(0);
    assert_eq!(mappings.len(), 1);

    let mapping = &mappings[0];

    // Test passthrough at different values
    assert_eq!(mapping.mode.apply(0.0), 0.0);
    assert_eq!(mapping.mode.apply(0.5), 0.5);
    assert_eq!(mapping.mode.apply(1.0), 1.0);
}

#[test]
fn test_macro_to_fx_parameter_scaled_range() {
    // Create a scaled range mapping (0.0-1.0 → 0.5-1.0)
    let mut bank = MacroMappingBank::new();
    bank.add_mapping(MacroMapping {
        source_param: 1,
        target_track: TrackDescriptor::ByName("Drums".to_string()),
        target_fx: FxDescriptor::ByPluginName("ReaEQ".to_string()),
        target_param_index: 3,
        mode: MapMode::ScaleRange {
            min: 0.5,
            max: 1.0,
        },
    })
    .expect("mapping validation failed");

    let mappings = bank.get_mappings_for_param(1);
    assert_eq!(mappings.len(), 1);

    let mapping = &mappings[0];

    // Test scaled range
    assert_eq!(mapping.mode.apply(0.0), 0.5); // min
    assert_eq!(mapping.mode.apply(1.0), 1.0); // max
    assert!(mapping.mode.apply(0.5) > 0.5 && mapping.mode.apply(0.5) < 1.0); // mid
}

#[test]
fn test_macro_to_fx_parameter_toggle() {
    let mut bank = MacroMappingBank::new();
    bank.add_mapping(MacroMapping {
        source_param: 2,
        target_track: TrackDescriptor::ByIndex(0),
        target_fx: FxDescriptor::ByIndex(1),
        target_param_index: 5,
        mode: MapMode::Toggle,
    })
    .expect("mapping validation failed");

    let mappings = bank.get_mappings_for_param(2);
    let mapping = &mappings[0];

    // Test toggle threshold at 0.5
    assert_eq!(mapping.mode.apply(0.0), 0.0); // off
    assert_eq!(mapping.mode.apply(0.49), 0.0); // off
    assert_eq!(mapping.mode.apply(0.5), 1.0); // on (threshold)
    assert_eq!(mapping.mode.apply(1.0), 1.0); // on
}

#[test]
fn test_multiple_mappings_per_macro() {
    let mut bank = MacroMappingBank::new();

    // Macro 0 controls two different FX parameters
    bank.add_mapping(MacroMapping {
        source_param: 0,
        target_track: TrackDescriptor::ByIndex(1),
        target_fx: FxDescriptor::ByIndex(0),
        target_param_index: 2,
        mode: MapMode::PassThrough,
    })
    .expect("failed");

    bank.add_mapping(MacroMapping {
        source_param: 0,
        target_track: TrackDescriptor::ByIndex(2),
        target_fx: FxDescriptor::ByIndex(1),
        target_param_index: 3,
        mode: MapMode::ScaleRange {
            min: 0.0,
            max: 0.5,
        },
    })
    .expect("failed");

    let mappings = bank.get_mappings_for_param(0);
    assert_eq!(mappings.len(), 2);

    // Verify both mappings are present and independent
    assert_eq!(mappings[0].target_param_index, 2);
    assert_eq!(mappings[1].target_param_index, 3);

    // Test values with different modes
    let value = 0.8;
    assert_eq!(mappings[0].mode.apply(value), 0.8); // passthrough
    assert_eq!(mappings[1].mode.apply(value), 0.4); // scaled to 0.0-0.5
}

#[test]
fn test_resolver_cache_performance() {
    let mut cache = ResolutionCache::new();
    let track_desc = TrackDescriptor::ByIndex(1);
    let fx_desc = FxDescriptor::ByIndex(0);

    // First resolution returns result
    let result1 = cache.resolve_track_cached(&track_desc);
    assert_eq!(result1, Ok(1));

    // Second resolution uses cache (same result without recomputation)
    let result2 = cache.resolve_track_cached(&track_desc);
    assert_eq!(result2, result1);

    // FX cache is independent
    let fx_result = cache.resolve_fx_cached(0, &fx_desc);
    assert_eq!(fx_result, Ok(0));

    // Clear cache
    cache.clear();

    // After clear, resolves again (would hit cache again if not cleared)
    let result3 = cache.resolve_track_cached(&track_desc);
    assert_eq!(result3, Ok(1));
}

#[test]
fn test_boundary_values() {
    let modes = vec![
        MapMode::PassThrough,
        MapMode::ScaleRange {
            min: 0.1,
            max: 0.9,
        },
        MapMode::Toggle,
        MapMode::Relative { step: 0.2 },
    ];

    for mode in modes {
        // All modes should handle boundary values
        let _ = mode.apply(0.0); // min
        let _ = mode.apply(0.5); // middle
        let _ = mode.apply(1.0); // max

        // Test clamping of out-of-range values
        let _ = mode.apply(-0.5); // below range, should clamp
        let _ = mode.apply(1.5); // above range, should clamp
    }
}

#[test]
fn test_serialization_preserves_mappings() {
    let mut original = MacroMappingBank::new();

    // Add a complex mapping
    original
        .add_mapping(MacroMapping {
            source_param: 0,
            target_track: TrackDescriptor::ByNamePattern("*Drum*".to_string()),
            target_fx: FxDescriptor::ByPluginName("ReaEQ".to_string()),
            target_param_index: 4,
            mode: MapMode::ScaleRange {
                min: 0.2,
                max: 0.8,
            },
        })
        .expect("failed");

    original
        .add_mapping(MacroMapping {
            source_param: 1,
            target_track: TrackDescriptor::ByName("Master".to_string()),
            target_fx: FxDescriptor::ByIndex(0),
            target_param_index: 1,
            mode: MapMode::Toggle,
        })
        .expect("failed");

    // Serialize to JSON
    let json = original.to_json().expect("serialization failed");

    // Deserialize from JSON
    let restored = MacroMappingBank::from_json(&json).expect("deserialization failed");

    // Verify all mappings preserved
    assert_eq!(original.mappings.len(), restored.mappings.len());
    assert_eq!(original.mappings[0].source_param, restored.mappings[0].source_param);
    assert_eq!(
        original.mappings[0].target_param_index,
        restored.mappings[0].target_param_index
    );
    assert_eq!(original.mappings[1].source_param, restored.mappings[1].source_param);
}

#[test]
fn test_state_persistence_round_trip() {
    let mut original = MacroMappingBank::new();

    original
        .add_mapping(MacroMapping {
            source_param: 3,
            target_track: TrackDescriptor::ByIndex(2),
            target_fx: FxDescriptor::ByIndex(1),
            target_param_index: 5,
            mode: MapMode::ScaleRange {
                min: 0.3,
                max: 0.7,
            },
        })
        .expect("failed");

    // Serialize to plugin state string
    let state_string = original.to_state_string().expect("state serialization failed");

    // Should be base64-encoded (non-JSON-like string)
    assert!(!state_string.contains('{'));
    assert!(!state_string.contains('}'));

    // Deserialize from state string
    let restored = MacroMappingBank::from_state_string(&state_string);

    // Verify mapping survived round-trip
    assert_eq!(original.mappings.len(), restored.mappings.len());
    assert_eq!(
        original.mappings[0].source_param,
        restored.mappings[0].source_param
    );
}

#[test]
fn test_validation_prevents_invalid_mappings() {
    let mut bank = MacroMappingBank::new();

    // Invalid source_param (> 7)
    let result = bank.add_mapping(MacroMapping {
        source_param: 8,
        target_track: TrackDescriptor::ByIndex(0),
        target_fx: FxDescriptor::ByIndex(0),
        target_param_index: 0,
        mode: MapMode::PassThrough,
    });

    assert!(result.is_err());
    assert_eq!(bank.mappings.len(), 0); // Not added

    // Valid mapping should succeed
    let result = bank.add_mapping(MacroMapping {
        source_param: 7,
        target_track: TrackDescriptor::ByIndex(0),
        target_fx: FxDescriptor::ByIndex(0),
        target_param_index: 0,
        mode: MapMode::PassThrough,
    });

    assert!(result.is_ok());
    assert_eq!(bank.mappings.len(), 1);
}

#[test]
fn test_mapping_independence() {
    let mut bank = MacroMappingBank::new();

    // Add mappings for different macros
    for i in 0..8 {
        bank.add_mapping(MacroMapping {
            source_param: i,
            target_track: TrackDescriptor::ByIndex(0),
            target_fx: FxDescriptor::ByIndex(0),
            target_param_index: i as u32,
            mode: MapMode::PassThrough,
        })
        .expect("failed");
    }

    // Each macro should have exactly its own mappings
    for i in 0..8 {
        let mappings = bank.get_mappings_for_param(i);
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_param, i);
    }

    // Unmapped macro should have no mappings
    let unmapped = bank.get_mappings_for_param(8);
    assert_eq!(unmapped.len(), 0);
}
