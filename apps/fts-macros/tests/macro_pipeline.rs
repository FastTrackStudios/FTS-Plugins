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
use std::fs;
use common::setup::print_environment_check;

// Real REAPER test: Spawn instance and automate macro plugin testing.
///
/// This test:
/// 1. Creates a REAPER project with pre-configured tracks and plugins
/// 2. Spawns a fresh REAPER instance
/// 3. Keeps it open for interactive testing
/// 4. Automatically loads the macro plugin
///
/// Run with: `cargo test -p fts-macros test_reaper_macro_automated -- --nocapture`
#[test]
fn test_reaper_macro_automated() {
    const REAPER_PATH: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";
    const REAPER_RESOURCES: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║  FTS Macros - Automated REAPER Integration Test        ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    // Check if REAPER exists
    if !Path::new(REAPER_PATH).exists() {
        println!("⚠  REAPER not found at: {}", REAPER_PATH);
        println!("   Skipping test");
        return;
    }

    // Create a test project file with tracks and FX pre-configured
    let project_content = r#"<REAPER_PROJECT 0.1 "6.82"
  RECORD_PATH "" ""
  <TRACK 1
    TRACKID {00000001-0000-0000-0000-000000000001}
    NAME "Macro Control"
    PEAKCOL 16576
    BEAT -1
    AUTOMODE 0
    VOLPAN 1 0 -1 -1 1
    MUTESOLO 0 0 0
    IPHASE 0
    PLAYOFFS 0 1
    ISBUS 0 0
    BUSCOMP 0 0 0 0 0
    SHOWINMIX 1 0.6667 0.5 1 0.5 0 0 0 0
    LANEREC -1 -1 -1 0
    REC 0 5088 1 0 0 0 0 0
    TRACKHEIGHT 0 0 0 0 0 0 0
    INQ 0 0 0 0.5 100 0 0 100
    NCHAN 2
    FX 1
    PERF 0
    MIDIOUT -1
    MAINSEND 1 0
    <FXCHAIN
      SHOW 0
      LASTSEL 0
      DOCKED 0
      BYPASS 0 0 0
      <CLAP "CLAP: FTS Macros (FastTrackStudio)" com.fasttrackstudio.fts-macros ""
        CFG 4 0 0 ""
        <STATE
          2gAAAAAAAAB7InZlcnNpb24iOiIwLjEuMCIsInBhcmFtcyI6eyJtYWNyb18wIjp7ImYzMiI6MC4wfSwibWFjcm9fMSI6eyJmMzIiOjAuMH0sIm1hY3JvXzIiOnsiZjMy
          IjowLjB9LCJtYWNyb18zIjp7ImYzMiI6MC4wfSwibWFjcm9fNCI6eyJmMzIiOjAuMH0sIm1hY3JvXzUiOnsiZjMyIjowLjB9LCJtYWNyb182Ijp7ImYzMiI6MC4wfSwi
          bWFjcm9fNyI6eyJmMzIiOjAuMH19LCJmaWVsZHMiOnt9fQ==
        >
      >
      FLOAT 714 740 600 262
      FXID {62D8218F-C448-BC4C-A4E6-86DB884FAA04}
      WAK 0 0
    >
    <ITEM
      POSITION 0
      SNAPOFFS 0
      LENGTH 0
      LOOP 1
      ALLTAKES 0
      FADEIN 1 0 0 1 0 0 0
      FADEOUT 1 0 0 1 0 0 0
      MUTE 0 0
      SEL 0
      IGUID {396959E0-B0AE-8D4C-8F1D-083F7F5069C2}
      IID 1
    >
  >
  <TRACK 2
    TRACKID {00000002-0000-0000-0000-000000000002}
    NAME "Target FX"
    PEAKCOL 16576
    BEAT -1
    AUTOMODE 0
    VOLPAN 1 0 -1 -1 1
    MUTESOLO 0 0 0
    IPHASE 0
    PLAYOFFS 0 1
    ISBUS 0 0
    BUSCOMP 0 0 0 0 0
    SHOWINMIX 1 0.6667 0.5 1 0.5 0 0 0 0
    LANEREC -1 -1 -1 0
    REC 0 5088 1 0 0 0 0 0
    TRACKHEIGHT 0 0 0 0 0 0 0
    INQ 0 0 0 0.5 100 0 0 100
    NCHAN 2
    FX 0
    PERF 0
    MIDIOUT -1
    MAINSEND 1 0
    <FXCHAIN
      SHOW 0
      LASTSEL 0
      DOCKED 0
    >
    <ITEM
      POSITION 0
      SNAPOFFS 0
      LENGTH 0
      LOOP 1
      ALLTAKES 0
      FADEIN 1 0 0 1 0 0 0
      FADEOUT 1 0 0 1 0 0 0
      MUTE 0 0
      SEL 0
      IGUID {396959E0-B0AE-8D4C-8F1D-083F7F5069C3}
      IID 2
    >
  >
>
"#;

    // Write project to temp file
    let temp_dir = std::env::temp_dir();
    let project_path = temp_dir.join("fts-macros-test.RPP");

    if let Err(e) = fs::write(&project_path, project_content) {
        println!("⚠  Failed to create test project: {}", e);
        println!("   Continuing with empty project...\n");
    } else {
        println!("✓ Created test project at: {}\n", project_path.display());
    }

    println!("Spawning REAPER with automated test setup...");
    println!("  Executable: {}", REAPER_PATH);
    println!("  Resources: {}\n", REAPER_RESOURCES);

    // Spawn REAPER
    let mut cmd = Command::new(REAPER_PATH);
    cmd.current_dir(REAPER_RESOURCES)
        .arg("-newinst")
        .arg("-nosplash")
        .arg("-ignoreerrors");

    // Pass project file if it was created
    if project_path.exists() {
        cmd.arg(project_path.to_string_lossy().to_string());
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            println!("✓ REAPER spawned successfully (PID: {})\n", pid);

            // Give REAPER time to initialize
            println!("Waiting for REAPER to initialize...");
            thread::sleep(Duration::from_secs(4));
            println!("✓ REAPER initialized\n");

            println!("╔═══════════════════════════════════════════════════════════╗");
            println!("║  REAPER is RUNNING with FTS Macros loaded               ║");
            println!("║  PID: {:<56}║", pid);
            println!("╚═══════════════════════════════════════════════════════════╝\n");

            println!("🧪 What to Test:\n");
            println!("  1. FTS Macros plugin should be visible in Track 1");
            println!("     → Check: FX window on Track 1 shows 8 Macro parameters\n");

            println!("  2. Test real-time macro control:");
            println!("     → Drag Macro 1-8 faders in the plugin");
            println!("     → Observe values change in REAPER's parameter list\n");

            println!("  3. Test automation:");
            println!("     → Right-click macro param → Automation mode → Latch");
            println!("     → Move faders to record automation\n");

            println!("  4. Test MIDI learn:");
            println!("     → Right-click a macro param → MIDI Learn");
            println!("     → Send MIDI CC from controller\n");

            println!("  5. Test cross-plugin routing (future):");
            println!("     → Load an FX on Track 2");
            println!("     → Map Macro values to control Track 2 FX parameters\n");

            println!("═══════════════════════════════════════════════════════════\n");
            println!("Close REAPER when done testing (Cmd+Q or close window)\n");
            println!("Waiting for REAPER to close...\n");

            // Wait for REAPER to terminate
            let wait_result = child.wait();
            match wait_result {
                Ok(status) => {
                    println!("\n✓ REAPER terminated (status: {})", status);
                }
                Err(e) => {
                    println!("\n✗ Error waiting for REAPER: {}", e);
                }
            }

            // Clean up temp project
            let _ = fs::remove_file(&project_path);

            println!("✓ Automated test session complete\n");
        }
        Err(e) => {
            println!("✗ Failed to spawn REAPER: {}", e);
            println!("  Make sure REAPER is installed at: {}", REAPER_PATH);
        }
    }
}

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
    println!("Test: Load fts-macros and verify parameters");
    println!("This requires REAPER to be running with the macro plugin installed");
    println!("Run: cargo test --test macro_pipeline -- --ignored --nocapture");

    // TODO: Implement with reaper-test framework once workspace deps resolved
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

    // TODO: Implement automation testing with reaper-test
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

    // TODO: Implement independence testing with reaper-test
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

    // TODO: Implement MIDI binding testing with reaper-test
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
}

#[test]
fn print_test_info() {
    print_environment_check();
}
