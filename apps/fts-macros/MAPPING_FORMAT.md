# FTS-Macros Mapping Configuration Format

## Overview

The mapping configuration defines how macro parameters control FX parameters. This document specifies the JSONL format used for serialization and communication between the fts-macros plugin and fts-control.

## Format Version

**Current Version:** `0.1`

The `version` field enables forward compatibility. Future versions can update the schema while maintaining support for older mappings.

## Schema

### Root Structure

```jsonl
{
  "version": "0.1",
  "mappings": [<MacroMapping>, ...]
}
```

### MacroMapping

A single source→target mapping with transformation mode.

```jsonl
{
  "source_param": 0,
  "target_track": <TrackDescriptor>,
  "target_fx": <FxDescriptor>,
  "target_param_index": 2,
  "mode": <MapMode>
}
```

**Fields:**
- `source_param` (u8, 0-7): Source macro parameter index
- `target_track` (TrackDescriptor): Target track selector
- `target_fx` (FxDescriptor): Target FX plugin selector
- `target_param_index` (u32): Target FX parameter index (0-based)
- `mode` (MapMode): Value transformation mode

### TrackDescriptor

Selects a track by index, exact name, or name pattern.

```jsonl
// By index (0-based)
{"type": "by-index", "value": 0}

// By exact name (string match)
{"type": "by-name", "value": "Drums"}

// By name pattern (supports * wildcard)
{"type": "by-name-pattern", "value": "*Drum*"}
```

**Types:**
- `by-index`: 0-based track index
- `by-name`: Exact track name (case-sensitive)
- `by-name-pattern`: Wildcard pattern where `*` matches any substring

### FxDescriptor

Selects a plugin by index, name, or plugin identifier.

```jsonl
// By index in track FX chain (0-based)
{"type": "by-index", "value": 1}

// By exact plugin name (UI display name)
{"type": "by-name", "value": "Compressor"}

// By plugin identifier (e.g., ReaEQ, TT_ProReverb)
{"type": "by-plugin-name", "value": "ReaEQ"}
```

**Types:**
- `by-index`: 0-based FX slot index in track's chain
- `by-name`: Exact plugin display name
- `by-plugin-name`: Plugin manufacturer ID (for built-in REAPER plugins)

### MapMode

Defines the transformation applied to the source value before sending to target.

```jsonl
// Direct passthrough (0.0-1.0 → 0.0-1.0)
"passthrough"

// Scale to custom range
{"type": "scale-range", "min": 0.5, "max": 1.0}

// Relative increment (for toggle/increment controls)
{"type": "relative", "step": 0.1}

// Boolean toggle (threshold at 0.5)
"toggle"
```

**Modes:**
- `passthrough`: Source value passed directly to target (0.0-1.0)
- `scale-range`: Map 0.0-1.0 source range to [min..max]
- `relative`: Each source change increments target by step
- `toggle`: Values ≥0.5 → 1.0, <0.5 → 0.0

## Examples

### Example 1: Simple Passthrough

Map Macro 0 to a compressor ratio on the Drums track:

```jsonl
{
  "version": "0.1",
  "mappings": [
    {
      "source_param": 0,
      "target_track": {"type": "by-name", "value": "Drums"},
      "target_fx": {"type": "by-plugin-name", "value": "ReaComp"},
      "target_param_index": 2,
      "mode": "passthrough"
    }
  ]
}
```

### Example 2: Scaled Range

Map Macro 1 to reverb mix, scaling 0.0-1.0 to 10%-30%:

```jsonl
{
  "version": "0.1",
  "mappings": [
    {
      "source_param": 1,
      "target_track": {"type": "by-name", "value": "Master"},
      "target_fx": {"type": "by-plugin-name", "value": "ReaVerbLate"},
      "target_param_index": 4,
      "mode": {"type": "scale-range", "min": 0.1, "max": 0.3}
    }
  ]
}
```

### Example 3: Multiple Mappings

One macro controlling multiple FX parameters:

```jsonl
{
  "version": "0.1",
  "mappings": [
    {
      "source_param": 2,
      "target_track": {"type": "by-index", "value": 0},
      "target_fx": {"type": "by-index", "value": 0},
      "target_param_index": 3,
      "mode": "passthrough"
    },
    {
      "source_param": 2,
      "target_track": {"type": "by-index", "value": 1},
      "target_fx": {"type": "by-plugin-name", "value": "ReaEQ"},
      "target_param_index": 5,
      "mode": {"type": "scale-range", "min": 0.0, "max": 0.8}
    }
  ]
}
```

### Example 4: Pattern-Based Track Selection

Use wildcard patterns for flexible track naming:

```jsonl
{
  "version": "0.1",
  "mappings": [
    {
      "source_param": 3,
      "target_track": {"type": "by-name-pattern", "value": "*Synth*"},
      "target_fx": {"type": "by-plugin-name", "value": "ReaEQ"},
      "target_param_index": 1,
      "mode": "passthrough"
    }
  ]
}
```

## Storage

### In Plugin State

Mappings are stored in the CLAP plugin state as:
1. Serialize to JSON
2. Encode to base64 (for safe binary storage)
3. Store in CLAP state chunk

Example state chunk:
```
eyJ2ZXJzaW9uIjoiMC4xIiwibWFwcGluZ3MiOlt7InNvdXJjZV9wYXJhbSI6MCwi...
```

### Project Portability

When a `.rpp` project is saved, the plugin state includes the base64-encoded mappings. This ensures:
- Mappings survive project saves/loads
- Mappings are portable across machines
- No external configuration files needed

## Validation Rules

When loading a mapping configuration:

1. **source_param** must be 0-7 (8 macro parameters)
2. **target_param_index** must be valid for the target FX (validated at runtime)
3. **Track/FX selectors** must resolve to valid tracks/plugins (fails gracefully)
4. **MapMode values** must be within valid ranges:
   - scale-range: min/max are floats (typically 0.0-1.0 but unrestricted)
   - relative: step is non-zero float

Invalid mappings are logged but don't crash the plugin. They are silently skipped during processing.

## Backward Compatibility

When upgrading to a new format version:
- **v0.1 → v0.2:** The `from_state_string()` method will parse v0.1 and convert/upgrade as needed
- **Unknown versions:** Returns empty mapping bank (graceful degradation)
- **Partial failures:** Invalid individual mappings skipped, valid ones retained

## Future Extensions (v0.2+)

Potential enhancements:
- **Relative mode state tracking:** Current macro state for relative increments
- **Conditioning:** Enable/disable mappings based on conditions
- **Math expressions:** Support EEL for custom transformations
- **Feedback direction:** Control source parameters from target changes
- **Macro banks:** Named preset banks switchable in REAPER

---

## Implementation Notes

- This format is used internally between the plugin and fts-control
- The plugin automatically handles serialization/deserialization
- fts-control can read/write this format to configure macro mappings
- REAPER's project save mechanism preserves the encoded state
