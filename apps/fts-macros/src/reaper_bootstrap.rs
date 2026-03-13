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
//! 4. DawSync wraps the caller for synchronous access from process()
//! 5. A timer callback drains the main-thread task queue at ~30Hz
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
use std::sync::{Mutex, OnceLock};
use tracing::info;

use crate::routed_handler::RoutedHandler;

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
/// Wrapped in Mutex because OnceLock requires Sync, but we only access
/// from the main thread timer callback (no contention).
static MIDDLEWARE: OnceLock<Mutex<MainTaskMiddleware>> = OnceLock::new();

/// Get the plugin-side DawSync, if REAPER bootstrap succeeded.
///
/// Returns `None` if the extension didn't eagerly load this plugin
/// (i.e., `ReaperPluginEntry` was never called).
pub fn daw_sync() -> Option<&'static DawSync> {
    BOOTSTRAP.get().map(|b| &b.daw_sync)
}

/// Timer callback registered with REAPER to drain the main-thread task queue.
/// Runs at ~30Hz on REAPER's main thread.
extern "C" fn plugin_timer_callback() {
    if let Some(m) = MIDDLEWARE.get() {
        if let Ok(mut mw) = m.lock() {
            mw.run();
        }
    }
}

/// Called by the REAPER extension during eager load.
///
/// The extension finds this .dylib and calls `ReaperPluginEntry` on it,
/// passing the same `PluginContext` that REAPER provided. This initializes
/// the plugin's own copies of reaper-rs statics and DAW services.
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
    info!("FTS Macros: ReaperPluginEntry called, initializing REAPER bootstrap...");

    // 1. Initialize reaper-high in this .dylib's address space
    match HighReaper::load(context).setup() {
        Ok(_) => info!("FTS Macros: reaper-high initialized"),
        Err(_) => {
            // Already initialized (shouldn't happen in a separate .dylib)
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
    let mut session = reaper_medium::ReaperSession::load(context);
    session.plugin_register_add_timer(plugin_timer_callback)?;
    info!("FTS Macros: timer callback registered");

    // 5. Set TaskSupport for daw-reaper (plugin's own copy)
    //    We need a 'static reference, so we store it in the Bootstrap struct
    //    and set it after BOOTSTRAP is initialized. For now, leak a Box.
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
    //    The leaked TaskSupport is fine — it lives for the process lifetime.
    //    We store a dummy in the struct since the real one is leaked.
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
///
/// Same services as the extension's `register_daw_dispatcher()`, but
/// running in the plugin's address space.
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
