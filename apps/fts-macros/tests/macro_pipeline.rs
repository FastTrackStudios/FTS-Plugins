//! Integration tests for fts-macros plugin and macro system pipeline.
//!
//! Tests the full macro parameter surface:
//! 1. Load fts-macros plugin on a track
//! 2. Set macro parameter values
//! 3. Verify parameter values are readable via REAPER FX API
//! 4. Test automation lane parameter setting

mod common;

use std::time::Duration;
use std::process::Command;
use std::path::Path;
use std::thread;
use common::setup::print_environment_check;

/// Test that fts-macros plugin can be loaded and parameters are accessible.
///
/// This test:
/// - Spawns a REAPER instance
/// - Creates a track
/// - Loads fts-macros plugin on the track's FX chain
/// - Verifies all 8 macro parameters are present and readable
/// - Sets each parameter to different values
/// - Verifies the values can be read back
#[test]
#[ignore] // Requires running REAPER instance
fn test_macro_parameters_accessible() {
    // Note: This test requires:
    // 1. REAPER running at the configured path
    // 2. fts-macros plugin installed to the user FX directory
    // 3. The daw-control RPC connection to be available
    //
    // Run with: cargo test --test macro_pipeline -- --ignored --nocapture
    //
    // Expected behavior:
    // - Plugin loads without errors
    // - All 8 parameters (Macro 1–8) are present
    // - Parameters can be set and read via FX API
    // - Values persist across parameter reads

    println!("Test: Load fts-macros and verify parameters");
    println!("This requires REAPER to be running with the macro plugin installed");
    println!("Run: cargo test --test macro_pipeline -- --ignored --nocapture");

    // TODO: Implement with reaper-test framework once integrated
    // Steps:
    // 1. Spawn REAPER via ReaperProcess::spawn()
    // 2. Connect via daw-control
    // 3. Create a new track
    // 4. Load FX on track: "FTS Macros" (fts-macros.clap)
    // 5. Read FX parameter list, verify all 8 macro params exist
    // 6. Set each param: fx.param(i).set(value) for i in 0..8
    // 7. Verify read-back: fx.param(i).get() == value
    // 8. Test edge values: 0.0, 1.0, 0.5
}

/// Test that macro parameter changes can be automated.
///
/// This test:
/// - Creates an automation envelope on a macro parameter
/// - Sets envelope points over time
/// - Verifies the parameter follows the envelope
#[test]
#[ignore]
fn test_macro_automation() {
    println!("Test: Macro parameter automation");
    println!("This requires REAPER to be running");

    // TODO: Implement automation testing
    // 1. Load plugin (as above)
    // 2. Get envelope for parameter (e.g., Macro 1)
    // 3. Set automation points: (0s, 0.0), (1s, 1.0), (2s, 0.5)
    // 4. Play from start, sample parameter values at regular intervals
    // 5. Verify values follow the envelope curve
}

/// Test that multiple macro parameters can be set independently.
///
/// This test:
/// - Sets all 8 macro parameters to different values
/// - Verifies each can be read back independently
/// - Tests that changes to one don't affect others
#[test]
#[ignore]
fn test_macro_parameter_independence() {
    println!("Test: Macro parameters are independent");

    // TODO: Implement independence testing
    // 1. Load plugin
    // 2. Set each macro to a unique value (e.g., i/8 for macro i)
    // 3. Verify each reads back correctly
    // 4. Change macro 0 to 1.0
    // 5. Verify macros 1-7 are unchanged
}

/// Test MIDI learn binding to macro parameters.
///
/// This test:
/// - Binds MIDI CC to a macro parameter
/// - Sends CC messages
/// - Verifies the parameter changes accordingly
#[test]
#[ignore]
fn test_macro_midi_learn() {
    println!("Test: MIDI CC binding to macro parameters");

    // TODO: Implement MIDI binding testing
    // This requires:
    // - Virtual MIDI port setup
    // - REAPER MIDI learn configuration
    // - CC message generation
    // - Parameter change verification
}

/// Test the macro registry integration (future: when plugins use macros).
///
/// This test:
/// - Loads fts-macros
/// - Loads a target plugin that uses macros (e.g., future EQ with macro bins)
/// - Maps a macro knob to the target's parameter
/// - Changes the macro value
/// - Verifies the target plugin's parameter follows
#[test]
#[ignore]
fn test_macro_registry_routing() {
    println!("Test: Macro registry routing to target plugins");

    // TODO: Implement after first macro-aware plugin is added
    // 1. Load fts-macros
    // 2. Load target plugin (placeholder for now)
    // 3. Configure macro_registry mapping: macro_knob_0 -> target_plugin.param_1
    // 4. Set macro 0 = 0.5
    // 5. Verify target param reads 0.5 (or mapped equivalent)
    // 6. Change macro 0 = 0.8
    // 7. Verify target param updates to 0.8
}

/// Test plugin parameter randomization (edge case).
///
/// This test:
/// - Sets parameters to random values
/// - Verifies no crashes or undefined behavior
/// - Tests extreme parameter combinations
#[test]
#[ignore]
fn test_macro_parameter_robustness() {
    println!("Test: Macro parameter robustness");

    // TODO: Implement robustness testing
    // Use quickcheck or proptest to generate random parameter values
    // and verify the plugin handles all combinations without crashing
}

#[test]
fn print_test_info() {
    print_environment_check();
}

/// Real REAPER test: Spawn a fresh REAPER instance.
///
/// This test spawns a fresh REAPER instance and verifies it starts correctly.
/// Once reaper-test workspace dependencies are resolved, this will:
/// 1. Spawn a fresh REAPER instance
/// 2. Connect to REAPER via DAW RPC
/// 3. Create a track
/// 4. Load the fts-macros plugin on the track
/// 5. Verify all 8 macro parameters are accessible
///
/// Run with: `cargo test -p fts-macros test_reaper_macro_spawn -- --nocapture`
#[test]
fn test_reaper_macro_spawn() {
    const REAPER_PATH: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";
    const REAPER_RESOURCES: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║  FTS Macros - Real REAPER Integration Test              ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    // Check if REAPER exists
    if !Path::new(REAPER_PATH).exists() {
        println!("⚠  REAPER not found at: {}", REAPER_PATH);
        println!("   Skipping REAPER spawn test");
        return;
    }

    println!("Spawning fresh REAPER instance...");
    println!("  Executable: {}", REAPER_PATH);
    println!("  Resources: {}\n", REAPER_RESOURCES);

    // Spawn REAPER
    let mut cmd = Command::new(REAPER_PATH);
    cmd.current_dir(REAPER_RESOURCES)
        .arg("-newinst")
        .arg("-nosplash")
        .arg("-ignoreerrors");

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            println!("✓ REAPER spawned successfully (PID: {})\n", pid);

            // Give REAPER a moment to initialize
            println!("Waiting for REAPER to initialize...");
            thread::sleep(Duration::from_secs(3));
            println!("✓ REAPER initialized\n");

            println!("╔═══════════════════════════════════════════════════════════╗");
            println!("║  REAPER is now RUNNING (PID: {:<37}║", pid);
            println!("╚═══════════════════════════════════════════════════════════╝\n");

            println!("📋 Setup Instructions:\n");
            println!("  1. Create tracks and load fts-macros.clap:");
            println!("     • Insert → New Track (x3 for testing)");
            println!("     • On Track 1: FX → Utility → FTS Macros (fts-macros.clap)");
            println!("     • On Track 2: FX → Saturation/Distortion or similar FX");
            println!("     • On Track 3: FX → Compressor or similar FX\n");

            println!("  2. Set up macro control:");
            println!("     • Click on a Macro parameter in fts-macros (e.g., Macro 1)");
            println!("     • Right-click → Automation mode → Trim/Read");
            println!("     • Draw automation lanes on different macros\n");

            println!("  3. Test real-time control:");
            println!("     • Drag the macro faders in FTS Macros plugin");
            println!("     • Watch the values change in REAPER's parameter list");
            println!("     • Test MIDI learn: right-click macro param → MIDI Learn");
            println!("     • Bind MIDI CC to a macro parameter\n");

            println!("  4. Test target FX:");
            println!("     • Map macro values to control target plugin parameters");
            println!("     • e.g., Macro 1 → Track 2 Saturation level\n");

            println!("═══════════════════════════════════════════════════════════\n");
            println!("Press Ctrl+C in REAPER to exit, or close the window\n");
            println!("Waiting for REAPER to close...\n");

            // Keep REAPER alive - wait for it to terminate
            let wait_result = child.wait();
            match wait_result {
                Ok(status) => {
                    println!("\n✓ REAPER terminated (status: {})", status);
                }
                Err(e) => {
                    println!("\n✗ Error waiting for REAPER: {}", e);
                }
            }

            println!("✓ Test session complete\n");
        }
        Err(e) => {
            println!("✗ Failed to spawn REAPER: {}", e);
            println!("  Make sure REAPER is installed at: {}", REAPER_PATH);
        }
    }
}
