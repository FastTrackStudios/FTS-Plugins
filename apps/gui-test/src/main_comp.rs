//! Standalone compressor GUI — runs the real editor component without a DAW.
//!
//! This lets you see debug output (eprintln) and iterate on rendering issues.
//!
//! Run with:
//!   cargo run -p gui-test --bin gui-test-comp

use nih_plug_dioxus::SharedState;
use std::sync::Arc;

fn main() {
    eprintln!("Starting FTS Compressor GUI — Standalone");
    eprintln!("All overlay debug output will appear here.");

    use comp_plugin::{CompUiState, FtsCompParams, WAVEFORM_LEN};
    use std::sync::atomic::Ordering;

    let params = Arc::new(FtsCompParams::default());
    let ui_state = Arc::new(CompUiState::new(params));

    // Pre-fill waveform with some fake data so we can see it render
    for i in 0..WAVEFORM_LEN {
        let t = i as f32 / WAVEFORM_LEN as f32;
        // Simulated input level (sine-ish envelope)
        let level = 0.3 + 0.5 * (t * std::f32::consts::TAU * 3.0).sin().abs();
        ui_state.waveform_input[i].store(level, Ordering::Relaxed);
        // Simulated GR that follows peaks
        let gr = if level > 0.5 {
            (level - 0.5) * 1.5
        } else {
            0.0
        };
        ui_state.waveform_gr[i].store(gr, Ordering::Relaxed);
    }
    ui_state.waveform_pos.store(0.0, Ordering::Relaxed);
    ui_state.gain_reduction_db.store(4.2, Ordering::Relaxed);
    ui_state.input_peak_db.store(-12.0, Ordering::Relaxed);

    let shared = SharedState::new(ui_state);

    nih_plug_dioxus::open_standalone_with_state(comp_plugin::editor::App, 900, 620, Some(shared));
}
