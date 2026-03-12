# FTS-Macros Integration Guide for fts-control

This guide explains how fts-control can integrate with the fts-macros plugin to configure and manage macro mappings.

## Overview

fts-control can configure fts-macros mappings through:
1. **Configuration Interface** — Allow users to define mappings
2. **Serialization** — Convert mappings to JSONL format
3. **Plugin Communication** — Send mappings to plugin (Phase 4)
4. **Real-time Verification** — Read back parameter values via REAPER API

## Mapping Configuration

### User Interface

fts-control should provide a UI for users to:
1. Select a macro parameter (Macro 1-8)
2. Specify a target track (by index or name)
3. Specify a target FX (by name or plugin ID)
4. Specify a target parameter (by name or index)
5. Configure a transformation mode
6. Preview the mapping effect

### Example User Workflow

```
User selects "Macro 1"
  → Chooses "Drums" track
  → Selects "ReaComp" plugin
  → Picks "Ratio" parameter
  → Selects "PassThrough" mode
  → Clicks "Add Mapping"

UI creates and displays:
{
  "source_param": 0,
  "target_track": {"type": "by-name", "value": "Drums"},
  "target_fx": {"type": "by-plugin-name", "value": "ReaComp"},
  "target_param_index": 2,
  "mode": "passthrough"
}
```

## Building Mapping JSON

### Create MacroMappingBank

```rust
use fts_macros::mapping::{
    MacroMappingBank, MacroMapping, TrackDescriptor, FxDescriptor, MapMode,
};

// Create empty bank
let mut bank = MacroMappingBank::new();

// Add a mapping
bank.add_mapping(MacroMapping {
    source_param: 0,  // Macro 1
    target_track: TrackDescriptor::ByName("Drums".to_string()),
    target_fx: FxDescriptor::ByPluginName("ReaComp".to_string()),
    target_param_index: 2,
    mode: MapMode::PassThrough,
}).expect("mapping failed");

// Serialize to JSON
let json_str = bank.to_json().expect("serialization failed");

// Serialize to plugin state string (base64-encoded)
let state_string = bank.to_state_string().expect("state encoding failed");
```

### Serialize to JSONL (for saving/loading configuration files)

```rust
// Save to file
let mappings_json = bank.to_json()?;
std::fs::write("my_mappings.jsonl", mappings_json)?;

// Load from file
let json_content = std::fs::read_to_string("my_mappings.jsonl")?;
let loaded_bank = MacroMappingBank::from_json(&json_content)?;
```

## Sending Mappings to Plugin

### Phase 4: RPC Communication (Future)

Once hot-reload is implemented, fts-control can send mapping updates via:

```
fts-control → [RPC message] → fts-macros plugin
  {
    "action": "set_mappings",
    "mappings_json": "<JSONL mapping data>",
    "timestamp": 1234567890
  }

Plugin receives → Deserializes → Swaps mapping bank → Next buffer applies new mappings
```

**Benefits:**
- Changes take effect immediately
- No plugin reload required
- Mappings persist in project when saved

### Current (Phase 1-3): Manual Setup

Currently, users must manually configure mappings in the REAPER project:
1. Create mappings in fts-control configuration
2. Export as JSONL
3. Save project (mappings stored in plugin state)
4. REAPER persists mappings on close

## Reading Back Parameter Values

### Verify Mappings Work

Once fts-control has sent mappings to fts-macros, verify they work by:

1. **Read Macro Parameter**
   ```rust
   // Get current value of Macro 0
   let value = reaper.get_fx_param(track, fx_index, 0)?;
   ```

2. **Read Target Parameter**
   ```rust
   // Get current value of target FX parameter
   let target_value = reaper.get_fx_param(target_track, target_fx, target_param)?;
   ```

3. **Verify Transformation**
   ```rust
   let expected = apply_mode(macro_value, &mode);
   assert_eq!(target_value, expected);
   ```

### Example Verification Loop

```rust
// After sending mappings to plugin...
thread::sleep(Duration::from_millis(100)); // Wait for plugin processing

for mapping in &bank.mappings {
    // Read source value
    let source = reaper.get_fx_param(
        track, fx_index, mapping.source_param as u32
    )?;

    // Read target value
    let target = reaper.get_fx_param(
        target_track, target_fx, mapping.target_param_index
    )?;

    // Compute expected
    let expected = mapping.mode.apply(source);

    // Verify
    if (target - expected).abs() < 0.01 {
        println!("✓ Mapping verified: Macro {} → FX param {}",
                 mapping.source_param, mapping.target_param_index);
    } else {
        println!("✗ Mapping failed: expected {}, got {}", expected, target);
    }
}
```

## Error Handling

### Validation Before Sending

Before sending mappings to plugin, validate them:

```rust
// Validate entire bank
bank.validate()?;

// Check individual mappings
for mapping in &bank.mappings {
    mapping.validate()?;
}
```

### Handle Resolution Failures

The plugin gracefully handles unresolvable mappings:

```
Track not found → Mapping skipped, logged
FX not found → Mapping skipped, logged
Param out of bounds → Mapping skipped, logged
```

fts-control should warn users when mappings fail to resolve:

```rust
// After applying mappings, query plugin for success status
// (Once logging/status API is added in Phase 4)
let failures = plugin.get_resolution_failures()?;
if !failures.is_empty() {
    eprintln!("⚠ {} mappings failed to resolve:", failures.len());
    for failure in failures {
        eprintln!("  - {}", failure);
    }
}
```

## Backward Compatibility

### Version Handling

The MacroMappingBank includes a version field for future compatibility:

```json
{
  "version": "0.1",
  "mappings": [...]
}
```

When loading mappings:
- **v0.1** → Parse directly
- **v0.2+** → Apply upgrade logic (when defined)
- **Unknown** → Warn user, load as empty bank

### Future Schema Evolution

When new features are added (relative mode state, conditions, etc.):

1. Increment version: "0.1" → "0.2"
2. Add migration logic in `MacroMappingBank::from_json()`
3. Maintain backward compatibility by converting old → new format

## Troubleshooting

### Mappings Not Taking Effect

**Checklist:**
- [ ] Macro parameters visible in REAPER? (`FX window → Check parameter list`)
- [ ] Mapping targets valid? (`Check track/FX exist in project`)
- [ ] Plugin loaded on correct track? (`Verify fts-macros is at Track 1`)
- [ ] Parameter indices correct? (`REAPER: Right-click param → Copy value as text`)
- [ ] Mode transformation correct? (`Verify expected range`)

### Plugin Crashes

The plugin uses graceful degradation and should **never crash**:
- Invalid mappings are skipped, not loaded
- Resolution errors are logged, not fatal
- Serialization errors return Result, checked before sending

If crashes occur:
1. Check REAPER error log (`Cmd+Shift+E`)
2. Report with mapping configuration (JSONL)
3. Include system info (macOS/Windows/Linux, REAPER version)

## API Reference

### MacroMappingBank

```rust
impl MacroMappingBank {
    // Create/validate
    pub fn new() -> Self
    pub fn add_mapping(&mut self, m: MacroMapping) -> Result<(), &str>
    pub fn validate(&self) -> Result<(), &str>

    // Query
    pub fn get_mappings_for_param(&self, param: u8) -> Vec<&MacroMapping>

    // Serialization
    pub fn to_json(&self) -> Result<String, serde_json::Error>
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error>
    pub fn to_state_string(&self) -> Result<String, Box<dyn Error>>
    pub fn from_state_string(state: &str) -> Self  // Graceful degradation
}
```

### Descriptors

```rust
// TrackDescriptor
TrackDescriptor::ByIndex(0)
TrackDescriptor::ByName("Drums".to_string())
TrackDescriptor::ByNamePattern("*Drum*".to_string())  // Wildcard

// FxDescriptor
FxDescriptor::ByIndex(0)
FxDescriptor::ByName("ReaComp".to_string())
FxDescriptor::ByPluginName("ReaComp".to_string())  // Plugin ID

// MapMode
MapMode::PassThrough
MapMode::ScaleRange { min: 0.5, max: 1.0 }
MapMode::Relative { step: 0.1 }
MapMode::Toggle
```

## Example: Complete Integration

```rust
use fts_macros::mapping::*;

fn create_test_mappings() -> Result<MacroMappingBank, Box<dyn Error>> {
    let mut bank = MacroMappingBank::new();

    // Map Macro 0 → Compressor Ratio on Drums
    bank.add_mapping(MacroMapping {
        source_param: 0,
        target_track: TrackDescriptor::ByName("Drums".to_string()),
        target_fx: FxDescriptor::ByPluginName("ReaComp".to_string()),
        target_param_index: 2,
        mode: MapMode::ScaleRange {
            min: 1.5,
            max: 8.0,
        },
    })?;

    // Map Macro 1 → Reverb Mix on Master
    bank.add_mapping(MacroMapping {
        source_param: 1,
        target_track: TrackDescriptor::ByName("Master".to_string()),
        target_fx: FxDescriptor::ByPluginName("ReaVerbLate".to_string()),
        target_param_index: 4,
        mode: MapMode::ScaleRange {
            min: 0.0,
            max: 0.3,
        },
    })?;

    // Validate
    bank.validate()?;

    // Export for storage/transmission
    let json = bank.to_json()?;
    let state = bank.to_state_string()?;

    Ok(bank)
}
```

---

**Status**: This guide documents the current architecture (Phase 1-3). Hot-reload support (Phase 4) will extend this with RPC communication patterns once implemented.
