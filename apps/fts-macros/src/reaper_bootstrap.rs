//! REAPER bootstrap for the fts-macros CLAP plugin.
//!
//! Exports `ReaperPluginEntry` so the REAPER extension can eagerly load this
//! plugin's .dylib and initialize REAPER API access within the plugin's address
//! space. This follows the Helgobox pattern: each .dylib gets its own copy of
//! Rust statics, so the plugin needs its own initialized `reaper-high::Reaper`,
//! `TaskSupport`, and `daw-reaper` setup.
//!
//! # Flow
//!
//! 1. Extension calls `ReaperPluginEntry` on this .dylib during REAPER startup
//! 2. We initialize reaper-rs, TaskSupport, daw-reaper, and build a RoutedHandler
//! 3. A LocalCaller creates an in-process loopback to the handler
//! 4. DawSync wraps the caller for synchronous access
//! 5. A timer callback polls macro params and applies mappings at ~30Hz
//!
//! # Graceful Degradation
//!
//! If the extension doesn't eagerly load us (extension not installed),
//! `daw_sync()` returns `None` and the plugin operates without DAW control.

use crossbeam_channel::{Receiver, Sender};
use daw_control_sync::{DawSync, LocalCaller};
use reaper_high::{MainTaskMiddleware, MainThreadTask, Reaper as HighReaper, TaskSupport};
use reaper_low::PluginContext;
use std::error::Error;
use std::sync::{Arc, Mutex, OnceLock};
use tracing::{info, warn};

use crate::mapping;
use crate::routed_handler::RoutedHandler;
use crate::MacroParams;
use crate::NUM_MACROS;

// Service dispatchers for building the RoutedHandler
use daw_proto::{
    AudioEngineServiceDispatcher, ExtStateServiceDispatcher, FxServiceDispatcher,
    HealthServiceDispatcher, LiveMidiServiceDispatcher, MarkerServiceDispatcher,
    MidiAnalysisServiceDispatcher, MidiServiceDispatcher, ProjectServiceDispatcher,
    RegionServiceDispatcher, RoutingServiceDispatcher, TempoMapServiceDispatcher,
    TrackServiceDispatcher, TransportServiceDispatcher,
};

/// Plugin-side bootstrap state, initialized by `ReaperPluginEntry`.
struct Bootstrap {
    daw_sync: DawSync,
    _task_support: TaskSupport,
    /// Keeps the LocalCaller's server task alive.
    _runtime: tokio::runtime::Runtime,
}

static BOOTSTRAP: OnceLock<Bootstrap> = OnceLock::new();

/// Task middleware for draining main-thread tasks in the timer callback.
static MIDDLEWARE: OnceLock<Mutex<MainTaskMiddleware>> = OnceLock::new();

// ── Direct parameter queue (process() → timer callback) ──────────────────

/// A pending FX parameter change queued from process() on the audio thread.
struct PendingParamChange {
    track_idx: u32,
    fx_idx: u32,
    param_idx: u32,
    value: f64,
}

/// Queue of parameter changes pushed by process(), drained by the timer callback.
static PENDING_CHANGES: Mutex<Vec<PendingParamChange>> = Mutex::new(Vec::new());

/// Queue an FX parameter change from the audio thread.
/// Applied on REAPER's main thread during the next timer callback (~30Hz).
pub fn queue_param_change(track_idx: u32, fx_idx: u32, param_idx: u32, value: f64) {
    if let Ok(mut changes) = PENDING_CHANGES.lock() {
        changes.push(PendingParamChange {
            track_idx,
            fx_idx,
            param_idx,
            value,
        });
    }
}

/// Drain and apply all pending parameter changes. Called on the main thread.
fn apply_pending_changes() {
    let changes = {
        let Ok(mut pending) = PENDING_CHANGES.lock() else {
            return;
        };
        std::mem::take(&mut *pending)
    };

    if changes.is_empty() {
        return;
    }

    let reaper = HighReaper::get();
    let project = reaper.current_project();

    for change in changes {
        let Some(track) = project.track_by_index(change.track_idx) else {
            continue;
        };
        let chain = track.normal_fx_chain();
        let Some(fx) = chain.fx_by_index(change.fx_idx) else {
            continue;
        };
        let param = fx.parameter_by_index(change.param_idx);
        let norm = reaper_medium::ReaperNormalizedFxParamValue::new(change.value);
        let _ = param.set_reaper_normalized_value(norm);
    }
}

// ── Timer-based macro poller (ExtState source for tests) ─────────────────

/// ExtState section name for macro value overrides.
/// External tools/tests write macro values here; the timer reads them.
const EXT_STATE_SECTION: &str = "FTS_MACROS";

/// ExtState key for injecting mapping configuration at runtime.
/// Tests and future UI write mapping JSON here; the timer picks it up.
const EXT_STATE_MAPPING_KEY: &str = "mapping_config";

/// State for the timer-based macro poller.
struct MacroPollerState {
    mapping_bank: Arc<Mutex<mapping::MacroMappingBank>>,
    prev_values: [f32; NUM_MACROS],
    /// Cached location of the FTS Macros FX: (track_index, fx_index).
    /// Found by scanning tracks for the plugin name.
    cached_location: Option<(u32, u32)>,
}

/// Macro poller state, registered by the plugin during initialize().
static MACRO_POLLER: Mutex<Option<MacroPollerState>> = Mutex::new(None);

/// Register the shared mapping bank for timer-based polling.
///
/// Called from `FtsMacros::initialize()`. The timer callback reads
/// macro values from two sources:
/// 1. REAPER FX params (for UI slider changes)
/// 2. ExtState (for programmatic/test control)
///
/// The `mapping_bank` is the same `Arc<Mutex<>>` stored in `MacroParams`
/// via `#[persist]`, so ExtState updates are visible to both timer and process().
pub fn register_macro_state(
    _params: Arc<MacroParams>,
    mapping_bank: Arc<Mutex<mapping::MacroMappingBank>>,
) {
    if let Ok(mut poller) = MACRO_POLLER.lock() {
        let count = mapping_bank
            .lock()
            .map(|b| b.mappings.len())
            .unwrap_or(0);
        info!(
            "FTS Macros: registering macro poller ({} mappings)",
            count
        );
        *poller = Some(MacroPollerState {
            mapping_bank,
            prev_values: [f32::NAN; NUM_MACROS],
            cached_location: None,
        });
    }
}

/// Read macro values from REAPER ExtState.
///
/// Keys: "macro_0" through "macro_7", values: float strings "0.0" to "1.0".
/// Returns NaN for missing/unparseable keys (no change detected).
fn read_macro_values_from_ext_state() -> [f32; NUM_MACROS] {
    use std::ffi::CString;

    let reaper = HighReaper::get();
    let low = reaper.medium_reaper().low();
    let section = CString::new(EXT_STATE_SECTION).unwrap();

    let mut values = [f32::NAN; NUM_MACROS];
    for i in 0..NUM_MACROS {
        let key = CString::new(format!("macro_{}", i)).unwrap();
        let ptr = unsafe { low.GetExtState(section.as_ptr(), key.as_ptr()) };
        if !ptr.is_null() {
            let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
            if let Ok(s) = cstr.to_str() {
                if let Ok(v) = s.parse::<f32>() {
                    values[i] = v;
                }
            }
        }
    }
    values
}

/// Find the FTS Macros FX instance by scanning tracks for its name.
/// Returns (track_index, fx_index) or None.
fn find_fts_macros_fx() -> Option<(u32, u32)> {
    let reaper = HighReaper::get();
    let project = reaper.current_project();

    for track_idx in 0..project.track_count() {
        let Some(track) = project.track_by_index(track_idx) else {
            continue;
        };
        let chain = track.normal_fx_chain();
        for fx_idx in 0..chain.fx_count() {
            let Some(fx) = chain.fx_by_index(fx_idx) else {
                continue;
            };
            let name = fx.name().to_string();
            if name.contains("FTS Macros") {
                return Some((track_idx, fx_idx));
            }
        }
    }
    None
}

/// Read macro values from REAPER's FX parameter API.
/// Returns NaN for each param if the plugin FX can't be found.
fn read_macro_values_from_fx(
    cached_location: &mut Option<(u32, u32)>,
) -> [f32; NUM_MACROS] {
    let reaper = HighReaper::get();
    let project = reaper.current_project();

    // Find or use cached location
    let (track_idx, fx_idx) = match *cached_location {
        Some(loc) => loc,
        None => {
            if let Some(loc) = find_fts_macros_fx() {
                info!("FTS Macros: found plugin at track={}, fx={}", loc.0, loc.1);
                *cached_location = Some(loc);
                loc
            } else {
                return [f32::NAN; NUM_MACROS];
            }
        }
    };

    let Some(track) = project.track_by_index(track_idx) else {
        *cached_location = None;
        return [f32::NAN; NUM_MACROS];
    };
    let chain = track.normal_fx_chain();
    let Some(fx) = chain.fx_by_index(fx_idx) else {
        *cached_location = None;
        return [f32::NAN; NUM_MACROS];
    };

    let mut values = [f32::NAN; NUM_MACROS];
    for i in 0..NUM_MACROS {
        let param = fx.parameter_by_index(i as u32);
        values[i] = param.reaper_normalized_value().get() as f32;
    }
    values
}

/// Check ExtState for a mapping configuration update.
///
/// If the key `FTS_MACROS/mapping_config` contains valid JSON, parse it,
/// update the shared mapping bank, and delete the key. Returns true if
/// mappings were updated (caller should reset prev_values to force
/// re-evaluation of all macros against the new mappings).
fn check_mapping_config(mapping_bank: &Arc<Mutex<mapping::MacroMappingBank>>) -> bool {
    use std::ffi::CString;

    let reaper = HighReaper::get();
    let low = reaper.medium_reaper().low();
    let section = CString::new(EXT_STATE_SECTION).unwrap();
    let key = CString::new(EXT_STATE_MAPPING_KEY).unwrap();

    let ptr = unsafe { low.GetExtState(section.as_ptr(), key.as_ptr()) };
    if ptr.is_null() {
        return false;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    let Ok(json_str) = cstr.to_str() else {
        return false;
    };
    if json_str.is_empty() {
        return false;
    }

    match mapping::MacroMappingBank::from_json(json_str) {
        Ok(bank) => {
            let count = bank.mappings.len();
            if let Ok(mut current) = mapping_bank.lock() {
                *current = bank;
            }
            info!(
                "FTS Macros: loaded {} mappings from ExtState mapping_config",
                count
            );
            // Delete the key so we don't re-process it every tick
            unsafe {
                low.DeleteExtState(section.as_ptr(), key.as_ptr(), true);
            }
            // Write ack so callers can poll for completion instead of sleeping
            let ack_key = CString::new("mapping_config_ack").unwrap();
            let ack_val = CString::new(count.to_string()).unwrap();
            unsafe {
                low.SetExtState(
                    section.as_ptr(),
                    ack_key.as_ptr(),
                    ack_val.as_ptr(),
                    false,
                );
            }
            true
        }
        Err(e) => {
            warn!(
                "FTS Macros: failed to parse mapping_config from ExtState: {}",
                e
            );
            false
        }
    }
}

/// Poll macro parameter values and apply mappings.
/// Called from the timer callback on REAPER's main thread at ~30Hz.
///
/// Reads macro values from two sources:
/// 1. REAPER FX params — picks up UI slider changes (user moves fader)
/// 2. ExtState — picks up programmatic changes (tests, scripts)
///
/// ExtState takes priority when set, allowing tests to override UI values.
fn poll_macros() {
    let Ok(mut poller_guard) = MACRO_POLLER.lock() else {
        return;
    };
    let Some(state) = poller_guard.as_mut() else {
        return;
    };

    // Check for mapping config updates via ExtState IPC.
    // If mappings changed, reset prev_values to force re-evaluation of all
    // macros against the new mappings (otherwise change detection would skip
    // values that haven't changed since before the new mappings were loaded).
    if check_mapping_config(&state.mapping_bank) {
        state.prev_values = [f32::NAN; NUM_MACROS];
    }

    // Read from both sources
    let fx_values = read_macro_values_from_fx(&mut state.cached_location);
    let ext_values = read_macro_values_from_ext_state();

    // Merge: ExtState overrides FX params when set (non-NaN)
    let mut values = fx_values;
    for i in 0..NUM_MACROS {
        if !ext_values[i].is_nan() {
            values[i] = ext_values[i];
        }
    }

    // Lock the mapping bank for the duration of this poll cycle
    let Ok(bank) = state.mapping_bank.lock() else {
        return;
    };

    let reaper = HighReaper::get();
    let project = reaper.current_project();

    for (macro_idx, &value) in values.iter().enumerate() {
        if value.is_nan() {
            continue;
        }

        let prev = state.prev_values[macro_idx];
        if prev == value {
            continue;
        }
        state.prev_values[macro_idx] = value;

        let mappings = bank.get_mappings_for_param(macro_idx as u8);
        if mappings.is_empty() {
            continue;
        }

        info!(
            "FTS Macros: macro {} changed {:.4} → {:.4}, applying {} mappings",
            macro_idx, prev, value, mappings.len()
        );

        for mapping in mappings {
            let track_idx = match &mapping.target_track {
                mapping::TrackDescriptor::ByIndex(idx) => *idx,
                other => {
                    warn!("FTS Macros: unsupported track descriptor: {:?}", other);
                    continue;
                }
            };
            let fx_idx = match &mapping.target_fx {
                mapping::FxDescriptor::ByIndex(idx) => *idx,
                other => {
                    warn!("FTS Macros: unsupported FX descriptor: {:?}", other);
                    continue;
                }
            };

            let transformed = mapping.mode.apply(value);

            let Some(track) = project.track_by_index(track_idx) else {
                warn!("FTS Macros: track {} not found", track_idx);
                continue;
            };
            let target_chain = track.normal_fx_chain();
            let Some(fx) = target_chain.fx_by_index(fx_idx) else {
                warn!("FTS Macros: FX {} not found on track {}", fx_idx, track_idx);
                continue;
            };

            let param = fx.parameter_by_index(mapping.target_param_index);
            let norm = reaper_medium::ReaperNormalizedFxParamValue::new(transformed as f64);
            if let Err(e) = param.set_reaper_normalized_value(norm) {
                warn!(
                    "FTS Macros: set param failed (track={}, fx={}, param={}, val={:.4}): {}",
                    track_idx, fx_idx, mapping.target_param_index, transformed, e
                );
            }
        }
    }
}

/// Get the plugin-side DawSync, if REAPER bootstrap succeeded.
pub fn daw_sync() -> Option<&'static DawSync> {
    BOOTSTRAP.get().map(|b| &b.daw_sync)
}

/// Returns true if the REAPER bootstrap completed successfully.
pub fn is_bootstrapped() -> bool {
    BOOTSTRAP.get().is_some()
}

/// Timer callback registered with REAPER to drain the main-thread task queue.
/// Runs at ~30Hz on REAPER's main thread.
extern "C" fn plugin_timer_callback() {
    // 1. Apply parameter changes queued by process() (audio-rate source)
    apply_pending_changes();

    // 2. Poll ExtState for programmatic/test macro values (30Hz source)
    poll_macros();

    // 3. Drain roam/DawSync main-thread tasks
    if let Some(m) = MIDDLEWARE.get() {
        if let Ok(mut mw) = m.lock() {
            mw.run();
        }
    }
}

/// Called by the REAPER extension during eager load.
///
/// # Safety
///
/// `rec` must be a valid pointer to `reaper_plugin_info_t` or null.
/// `h_instance` is the DLL module handle.
#[no_mangle]
pub unsafe extern "C" fn ReaperPluginEntry(
    h_instance: reaper_low::raw::HINSTANCE,
    rec: *mut reaper_low::raw::reaper_plugin_info_t,
) -> std::os::raw::c_int {
    let static_context = reaper_low::static_plugin_context();
    reaper_low::bootstrap_extension_plugin(h_instance, rec, static_context, plugin_init)
}

/// Plugin initialization — called by `bootstrap_extension_plugin` after
/// validating the `PluginContext`.
fn plugin_init(context: PluginContext) -> Result<(), Box<dyn Error>> {
    // Set up tracing to a plugin-specific log file.
    let log_file = std::fs::File::create("/tmp/fts-macros-bootstrap.log")
        .expect("Failed to create /tmp/fts-macros-bootstrap.log");
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .init();

    info!("FTS Macros: ReaperPluginEntry called, initializing REAPER bootstrap...");

    // 1. Initialize reaper-high in this .dylib's address space
    match HighReaper::load(context).setup() {
        Ok(_) => info!("FTS Macros: reaper-high initialized"),
        Err(_) => {
            info!("FTS Macros: reaper-high already initialized");
        }
    }

    // 2. Create TaskSupport channels for main-thread dispatch
    let (task_sender, task_receiver): (Sender<MainThreadTask>, Receiver<MainThreadTask>) =
        crossbeam_channel::unbounded();
    let task_support = TaskSupport::new(task_sender.clone());

    // 3. Create and store the task middleware (for timer callback)
    let middleware = MainTaskMiddleware::new(task_sender, task_receiver);
    MIDDLEWARE
        .set(Mutex::new(middleware))
        .map_err(|_| "Task middleware already initialized")?;

    // 4. Register timer callback to drain tasks on main thread (~30Hz)
    // The session must live forever — dropping it unregisters the timer.
    // ReaperSession isn't Sync so we can't put it in OnceLock; leak it instead.
    let mut session = reaper_medium::ReaperSession::load(context);
    session.plugin_register_add_timer(plugin_timer_callback)?;
    let _ = Box::leak(Box::new(session));
    info!("FTS Macros: timer callback registered (session leaked for lifetime)");

    // 5. Set TaskSupport for daw-reaper (plugin's own copy)
    let task_support_ref: &'static TaskSupport = Box::leak(Box::new(task_support));
    daw_reaper::set_task_support(task_support_ref);
    info!("FTS Macros: daw-reaper TaskSupport configured");

    // 6. Build RoutedHandler with DAW service dispatchers
    let handler = build_daw_handler();

    // 7. Create LocalCaller + DawSync via a temporary multi-thread runtime
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

    let daw_sync = runtime.block_on(async {
        let local_caller = LocalCaller::new(handler).await?;
        DawSync::from_local(local_caller)
    })?;

    info!("FTS Macros: DawSync created via LocalCaller (in-process)");

    // 8. Store bootstrap state
    let (dummy_sender, _) = crossbeam_channel::unbounded();
    BOOTSTRAP
        .set(Bootstrap {
            daw_sync,
            _task_support: TaskSupport::new(dummy_sender),
            _runtime: runtime,
        })
        .map_err(|_| "Bootstrap already initialized")?;

    info!("FTS Macros: REAPER bootstrap complete");
    Ok(())
}

/// Build a RoutedHandler with all DAW service dispatchers.
fn build_daw_handler() -> RoutedHandler {
    use daw_proto::{
        audio_engine_service_service_descriptor, ext_state_service_service_descriptor,
        fx_service_service_descriptor, health_service_service_descriptor,
        live_midi_service_service_descriptor, marker_service_service_descriptor,
        midi_analysis_service_service_descriptor, midi_service_service_descriptor,
        project_service_service_descriptor, region_service_service_descriptor,
        routing_service_service_descriptor, tempo_map_service_service_descriptor,
        track_service_service_descriptor, transport_service_service_descriptor,
    };

    let transport = daw_reaper::ReaperTransport::new();
    let project = daw_reaper::ReaperProject::new();
    let marker = daw_reaper::ReaperMarker::new();
    let region = daw_reaper::ReaperRegion::new();
    let tempo_map = daw_reaper::ReaperTempoMap::new();
    let audio_engine = daw_reaper::ReaperAudioEngine::new();
    let midi = daw_reaper::ReaperMidi::new();
    let midi_analysis = daw_reaper::ReaperMidiAnalysis::new();
    let fx = daw_reaper::ReaperFx::new();
    let track = daw_reaper::ReaperTrack::new();
    let routing = daw_reaper::ReaperRouting::new();
    let live_midi = daw_reaper::ReaperLiveMidi::new();
    let ext_state = daw_reaper::ReaperExtState::new();
    let health = daw_reaper::ReaperHealth::new();

    RoutedHandler::new()
        .with(
            transport_service_service_descriptor(),
            TransportServiceDispatcher::new(transport),
        )
        .with(
            project_service_service_descriptor(),
            ProjectServiceDispatcher::new(project),
        )
        .with(
            marker_service_service_descriptor(),
            MarkerServiceDispatcher::new(marker),
        )
        .with(
            region_service_service_descriptor(),
            RegionServiceDispatcher::new(region),
        )
        .with(
            tempo_map_service_service_descriptor(),
            TempoMapServiceDispatcher::new(tempo_map),
        )
        .with(
            audio_engine_service_service_descriptor(),
            AudioEngineServiceDispatcher::new(audio_engine),
        )
        .with(
            midi_service_service_descriptor(),
            MidiServiceDispatcher::new(midi),
        )
        .with(
            midi_analysis_service_service_descriptor(),
            MidiAnalysisServiceDispatcher::new(midi_analysis),
        )
        .with(
            fx_service_service_descriptor(),
            FxServiceDispatcher::new(fx),
        )
        .with(
            track_service_service_descriptor(),
            TrackServiceDispatcher::new(track),
        )
        .with(
            routing_service_service_descriptor(),
            RoutingServiceDispatcher::new(routing),
        )
        .with(
            live_midi_service_service_descriptor(),
            LiveMidiServiceDispatcher::new(live_midi),
        )
        .with(
            ext_state_service_service_descriptor(),
            ExtStateServiceDispatcher::new(ext_state),
        )
        .with(
            health_service_service_descriptor(),
            HealthServiceDispatcher::new(health),
        )
}
