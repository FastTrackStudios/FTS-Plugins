# FTS Macros Integration Tests

Integration tests for the fts-macros plugin, verifying the macro parameter surface and integration with the macro system pipeline.

## Structure

- `macro_pipeline.rs` — Main test suite for fts-macros plugin
- `common/mod.rs` — Shared test utilities (setup, mock types, constants)

## Requirements

To run integration tests, you need:

1. **REAPER instance** (automatically spawned by tests)
   - Path: `/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER`
   - Tests verify availability before running

2. **fts-macros plugin installed**
   - Location: `~/Music/FastTrackStudio/Reaper/FTS-TRACKS/UserPlugins/FX/fts-macros.clap`
   - Install via: `cd fts-plugins && just install`

3. **DAW control RPC** (future: when reaper-test integration added)
   - Socket: `/tmp/fts-daw-{pid}.sock`

## Running Tests

### Check environment setup
```bash
cargo test -p fts-macros print_test_info -- --nocapture
```

Output shows:
- REAPER availability
- Plugin installation status
- Installation instructions if needed

### Run all fts-macros tests (stub tests show what's being tested)
```bash
cargo test -p fts-macros -- --nocapture
```

### Run specific test category
```bash
cargo test -p fts-macros test_macro_parameters -- --ignored --nocapture
cargo test -p fts-macros test_macro_automation -- --ignored --nocapture
```

## Test Pipeline

Tests verify this complete macro parameter surface:

```
REAPER UI / Automation / MIDI Learn
        ↓
fts-macros parameter[0..7]  (FloatParam, 0.0–1.0)
        ↓
REAPER FX parameter API
        ↓
fts-control macro polling
        ↓
macro_registry::get_targets(knob_id)
        ↓
target plugin parameter automation
```

## Test Categories

### 1. **Parameter Surface Tests** (`test_macro_parameters_accessible`)
- Load fts-macros on a track
- Verify all 8 macro parameters present
- Set/read parameter values via REAPER FX API
- Test edge cases (0.0, 1.0, 0.5)

### 2. **Automation Tests** (`test_macro_automation`)
- Create automation envelopes on macro parameters
- Verify parameter follows envelope shape
- Test smooth parameter transitions

### 3. **Parameter Independence Tests** (`test_macro_parameter_independence`)
- Verify each macro can be set independently
- Changing one macro doesn't affect others
- Test concurrent parameter changes

### 4. **MIDI Learn Tests** (`test_macro_midi_learn`)
- Bind MIDI CC messages to macro parameters
- Verify CC messages drive parameter changes
- Test multiple MIDI bindings

### 5. **Macro Registry Routing** (`test_macro_registry_routing`)
- Load target plugin that uses macro registry
- Map macro knobs to target parameters
- Verify macro changes drive target plugin automation

### 6. **Robustness Tests** (`test_macro_parameter_robustness`)
- Random parameter value combinations
- Stress test: rapid parameter changes
- Verify no crashes or undefined behavior

## Test Architecture

### Mock Implementation (Current)
Tests use `common::mock` types for offline verification:
- `MockTrack` — Simulates a REAPER track
- `MockFx` — Simulates a plugin with parameters
- `MockParam` — Simulates a parameter with value clamping

### Real REAPER Integration (Future)
When reaper-test framework is available:
1. Use `ReaperProcess::spawn()` to launch fresh REAPER instance
2. Connect via `daw-control` RPC
3. Create real tracks and load real plugins
4. Verify parameters through actual REAPER API

## Future: Main Workspace Integration

Once nih_plug fork updates are complete:
1. Move tests to `FastTrackStudio/apps/tests/plugin-macros/`
2. Use main workspace's `reaper-test` framework directly
3. Add end-to-end tests with macro system integration
4. Create atomic releases (plugins + core system)

## Notes

- Stub tests are marked `#[ignore]` (require running REAPER)
- Run with `-- --ignored` to execute them
- Use `--nocapture` to see println! output
- Environment check runs without `#[ignore]` to verify setup

## Troubleshooting

### "Plugin not installed"
```bash
cd /Users/codywright/Documents/Development/FastTrackStudio/fts-plugins
just install
```

### "REAPER not found"
Check path: `/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER`

### Socket connection errors (future)
Ensure REAPER is running and RPC port is available:
```bash
ls /tmp/fts-daw-*.sock
```
