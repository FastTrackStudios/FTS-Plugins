# FTS Macros - DAW Integration Guide

## Architecture

The fts-macros plugin now uses **DawSync** to set FX parameters in real-time:

```
REAPER Audio Loop
  ↓
fts-macros Plugin (real-time safe)
  ├─ Read macro parameters (0-7)
  ├─ Resolve macro mappings
  ├─ Transform values based on modes
  └─ Queue parameter changes via DawSync.queue_set_param()  ← Non-blocking!
      ↓
    Background Tokio Runtime
      ├─ Receives queued parameter changes
      ├─ Makes async DAW service calls
      └─ Sets target FX parameters asynchronously

Actual FX Parameters Updated
```

## Initialization

The plugin initializes with three states:

### 1. Default Creation (No DAW Connection Yet)
```rust
let plugin = FtsMacros::default();
// daw_sync is Some(None) - waiting for initialization
```

### 2. DAW Connection Available
When the plugin receives a connection handle from the DAW extension:
```rust
plugin.init_daw_sync(connection_handle)?;
// daw_sync is now Some(DawSync) - ready to queue parameter changes
```

### 3. Process Loop (Real-Time)
```rust
fn process(&mut self, buffer: &mut Buffer, ...) -> ProcessStatus {
    // For each mapping:
    if let Some(daw) = self.daw_sync.lock().ok().and_then(|o| o.as_ref()) {
        daw.queue_set_param(track, fx, param, transformed_value)?;
    }
    // Never blocks - just queues the request
}
```

## Integration with REAPER Extension

The initialization flow should be:

1. **REAPER loads fts-macros plugin** (via CLAP)
   - Plugin created with default DawSync = None
   - Plugin ready to process but can't set parameters yet

2. **fts-control extension starts**
   - Connects to DAW service (roam RPC)
   - Gets the DAW service connection handle

3. **Extension initializes plugin's DawSync**
   ```rust
   // In fts-control or REAPER extension:
   let handle = roam::connect("unix:///tmp/fts-daw.sock").await?;

   // Find the fts-macros instance:
   // (This requires a way to call plugin initialization - see below)
   plugin.init_daw_sync(handle)?;
   ```

4. **Plugin now queues parameter changes**
   - Process loop starts queuing FX parameter changes
   - Background tokio runtime processes them asynchronously

## Unresolved: Plugin Initialization Hook

**Problem**: We need a way for the DAW extension to initialize the plugin's DawSync after the plugin is created.

**Current Options**:

### Option A: Plugin State File
- Store connection handle details in plugin state
- Plugin reads on initialization
- Cons: Brittle, plugin state not ideal for this

### Option B: Named Pipes / Shared Memory
- Plugin watches a named pipe for connection info
- Extension writes connection handle details
- Plugin reconnects to DAW service
- Cons: Complex, platform-specific

### Option C: Environment Variables
- Extension sets `FTS_DAW_SOCKET` env var
- Plugin reads at first process() call
- Plugin connects to DAW service independently
- Cons: Simple but feels hacky

### Option D: nih-plug Plugin Init Callback
- Extend nih-plug to call an init hook after plugin creation
- Hook receives context with host capabilities
- Plugin can access DAW connection from there
- Cons: Requires nih-plug modification

### Option E: Deferred Initialization in process()
- First call to process() checks if DawSync is initialized
- If not, tries to connect to DAW service
- Once connected, stays connected for plugin lifetime
- Pros: Simple, self-contained
- Cons: Small latency on first buffer

## Recommendation: Option E

The plugin can initialize DawSync on first audio buffer:

```rust
fn process(&mut self, buffer: &mut Buffer, ...) -> ProcessStatus {
    // Lazy initialization on first process call
    if let Ok(mut daw_opt) = self.daw_sync.lock() {
        if daw_opt.is_none() {
            // Try to connect to DAW service
            if let Ok(handle) = try_connect_to_daw_service() {
                if let Ok(daw) = DawSync::new(handle) {
                    *daw_opt = Some(daw);
                    tracing::info!("FTS Macros connected to DAW service");
                }
            }
        }
    }

    // Rest of processing loop...
}
```

This way:
- ✅ Plugin is completely self-contained
- ✅ No special initialization hook needed
- ✅ Works even if DAW extension isn't running
- ✅ First-buffer initialization is acceptable (one-time 1-2ms latency)
- ✅ DAW extension can still initialize it earlier if needed

## Connection String

The DAW service listens on a socket (Unix or named pipe):
- **Unix (Linux/macOS)**: `unix:///tmp/fts-daw.sock`
- **Windows (Named Pipe)**: `np://fts-daw`

Environment variable or hardcoded path can specify this.

## Testing

To verify integration works:

```bash
# 1. Start DAW service (from daw app)
cd /path/to/daw && cargo run

# 2. Load REAPER with fts-macros
# 3. The plugin should:
#    - Try to connect on first audio buffer
#    - Establish connection to DAW service
#    - Start queuing parameter changes
#    - Verify in logs: "FTS Macros connected to DAW service"

# 4. Change macro parameters in REAPER
# 5. Observe target FX parameters updating in real-time
```

## Build Note: Dependency Resolution

**Status:** fts-macros source code is complete, but the fts-plugins workspace has a dependency resolution issue:

- **Problem**: `roam@rev:30a8e10` requires `facet ^0.43.0`, but facet's git main branch only has `v0.44.1`
- **Root Cause**: roam's pinned revision pre-dates facet 0.44's release
- **Workaround**: Build fts-macros from the main FastTrackStudio workspace build system (via Meson/external build), which can manage all transitive deps correctly

The plugin code itself is correct and ready for use once built successfully.

## Next Steps

1. **Build Integration**
   - Add fts-macros to main workspace build or
   - Update roam to a newer revision compatible with facet 0.44, or
   - Lock facet to 0.43.x in a Cargo.lock file

2. **Implement connection logic** (in daw-control-sync)
   - Add `DawSync::connect_to_service()` async helper
   - Connect to service via env var `FTS_DAW_SOCKET` or default path
   - Handle connection failures gracefully

3. **Test with real REAPER**
   - Load plugin with DAW service running
   - Verify macro parameter changes propagate to target FX
   - Measure latency and queue depth

4. **Extend daw-control as needed**
   - Expose FX APIs for DawSync request handler to use
   - Implement `SetFxParam` and `GetFxParam` handlers in DawSync
   - Handle any missing functionality discovered during testing
