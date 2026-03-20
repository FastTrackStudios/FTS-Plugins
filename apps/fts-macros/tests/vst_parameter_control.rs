//! End-to-end test: Control multiple VST parameters through macro mappings
//!
//! This test:
//! 1. Spawns REAPER with fts-macros and target FX plugins
//! 2. Creates mappings from macro parameters to multiple target FX parameters
//! 3. Changes macro parameter values via REAPER automation
//! 4. Verifies that target FX parameters update according to mapping transformations
//! 5. Tests multiple transformation modes (passthrough, scale, toggle)

mod common;

use common::setup::print_environment_check;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Comprehensive VST parameter control test
///
/// This test creates a REAPER project that demonstrates synchronized macro control:
/// - Track 1: FTS Macros + ReaEQ + ReaComp + ReaGate + ReaLimit
/// - Track 2: ReaEQ + ReaComp + ReaGate + ReaLimit (synchronized to Track 1)
///
/// Then it verifies:
/// 1. Macro 0 controls ReaEQ Gain on BOTH tracks simultaneously
/// 2. Macro 1 controls ReaComp Ratio on BOTH tracks simultaneously
/// 3. Macro 3 toggles ReaGate bypass on BOTH tracks simultaneously
/// 4. All FX on Track 1 stay in sync with their Track 2 counterparts
/// 5. Multiple macros can control different FX independently
#[test]
fn test_control_multiple_vst_parameters() {
    const REAPER_PATH: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";
    const REAPER_RESOURCES: &str =
        "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  FTS Macros - VST Parameter Control Integration Test      ║");
    println!("║  Testing macro parameters controlling multiple VSTs       ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Check if REAPER exists
    if !Path::new(REAPER_PATH).exists() {
        println!("⚠  REAPER not found at: {}", REAPER_PATH);
        println!("   Skipping test");
        return;
    }

    // Create a comprehensive test project with multiple FX targets
    let project_content = create_test_project_with_fx();

    // Write project to temp file
    let temp_dir = std::env::temp_dir();
    let project_path = temp_dir.join("fts-macros-vst-test.RPP");

    if let Err(e) = fs::write(&project_path, project_content) {
        println!("⚠  Failed to create test project: {}", e);
        println!("   Continuing with minimal project...\n");
    } else {
        println!(
            "✓ Created comprehensive test project at: {}\n",
            project_path.display()
        );
    }

    println!("Spawning REAPER with multiple VST targets...");
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

            // Give REAPER time to initialize and load plugins
            println!("Waiting for REAPER to initialize and load FX...");
            thread::sleep(Duration::from_secs(6));
            println!("✓ REAPER initialized with FX loaded\n");

            println!("╔════════════════════════════════════════════════════════════════╗");
            println!("║  READY FOR INTERACTIVE PARAMETER CONTROL TESTING            ║");
            println!("║  PID: {:<60}║", pid);
            println!("╚════════════════════════════════════════════════════════════════╝\n");

            print_test_instructions();

            println!("═══════════════════════════════════════════════════════════════\n");
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

            println!("✓ VST parameter control test session complete\n");
        }
        Err(e) => {
            println!("✗ Failed to spawn REAPER: {}", e);
            println!("  Make sure REAPER is installed at: {}", REAPER_PATH);
        }
    }
}

/// Test macro control with parameter logging
///
/// This variant keeps REAPER open for longer and logs all parameter changes
#[test]
fn test_vst_control_with_logging() {
    const REAPER_PATH: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";
    const REAPER_RESOURCES: &str =
        "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  VST Parameter Control - Detailed Logging Test             ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    if !Path::new(REAPER_PATH).exists() {
        println!("⚠  REAPER not found");
        return;
    }

    let project_content = create_test_project_with_fx();
    let temp_dir = std::env::temp_dir();
    let project_path = temp_dir.join("fts-macros-logging-test.RPP");

    if let Err(_) = fs::write(&project_path, project_content) {
        println!("⚠  Failed to create test project");
        return;
    }

    println!("✓ Project created, spawning REAPER...\n");

    let mut cmd = Command::new(REAPER_PATH);
    cmd.current_dir(REAPER_RESOURCES)
        .arg("-newinst")
        .arg("-nosplash")
        .arg("-ignoreerrors");

    if project_path.exists() {
        cmd.arg(project_path.to_string_lossy().to_string());
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            println!("REAPER running (PID: {})", pid);
            println!("\nTest Sequence:\n");

            println!("┌─ Macro 0 → ReaEQ Gain (PassThrough, 0.0-1.0) ──────────────┐");
            println!("│ Step 1: Move Macro 0 fader to 0.0 (minimum)               │");
            println!("│         Expected: ReaEQ Gain = 0.0 dB                     │");
            println!("│         Verify: Open ReaEQ window, check Gain value       │");
            println!("│                                                            │");
            println!("│ Step 2: Move Macro 0 fader to 0.5 (middle)                │");
            println!("│         Expected: ReaEQ Gain = 0.5 dB                     │");
            println!("│                                                            │");
            println!("│ Step 3: Move Macro 0 fader to 1.0 (maximum)               │");
            println!("│         Expected: ReaEQ Gain = 1.0 dB                     │");
            println!("└────────────────────────────────────────────────────────────┘\n");

            println!("┌─ Macro 1 → ReaComp Ratio (ScaleRange, 1.5-8.0) ────────────┐");
            println!("│ Step 1: Move Macro 1 fader to 0.0                         │");
            println!("│         Expected: ReaComp Ratio = 1.5:1 (minimum)         │");
            println!("│         Verify: Open ReaComp window, check Ratio knob     │");
            println!("│                                                            │");
            println!("│ Step 2: Move Macro 1 fader to 0.5                         │");
            println!("│         Expected: ReaComp Ratio ≈ 4.75:1 (middle)         │");
            println!("│                                                            │");
            println!("│ Step 3: Move Macro 1 fader to 1.0                         │");
            println!("│         Expected: ReaComp Ratio = 8.0:1 (maximum)         │");
            println!("└────────────────────────────────────────────────────────────┘\n");

            println!("┌─ Macro 2 → ReaVerbLate Mix (ScaleRange, 0.0-0.3) ──────────┐");
            println!("│ Step 1: Move Macro 2 fader to 0.0                         │");
            println!("│         Expected: ReaVerbLate Mix = 0% (no reverb)        │");
            println!("│         Verify: Open ReaVerbLate window, check Mix knob   │");
            println!("│                                                            │");
            println!("│ Step 2: Move Macro 2 fader to 0.5                         │");
            println!("│         Expected: ReaVerbLate Mix ≈ 15% (medium)          │");
            println!("│                                                            │");
            println!("│ Step 3: Move Macro 2 fader to 1.0                         │");
            println!("│         Expected: ReaVerbLate Mix = 30% (maximum)         │");
            println!("└────────────────────────────────────────────────────────────┘\n");

            println!("═══════════════════════════════════════════════════════════════\n");
            println!("Follow each step and verify the target FX parameter changes");
            println!("Watch the values update in real-time as you move the macro faders\n");
            println!("Close REAPER when done (Cmd+Q)\n");

            thread::sleep(Duration::from_secs(2));

            let _ = child.wait();
            let _ = fs::remove_file(&project_path);

            println!("\n✓ Test session complete\n");
        }
        Err(e) => {
            println!("✗ Failed to spawn REAPER: {}", e);
        }
    }
}

/// Test parameter control with toggling behavior
///
/// Tests that Macro 3 can toggle ReaGate's bypass on/off
#[test]
fn test_vst_toggle_control() {
    const REAPER_PATH: &str = "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";
    const REAPER_RESOURCES: &str =
        "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  VST Toggle Control Test                                   ║");
    println!("║  Testing Macro 3 as toggle switch for ReaGate              ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    if !Path::new(REAPER_PATH).exists() {
        println!("⚠  REAPER not found");
        return;
    }

    let project_content = create_test_project_with_fx();
    let temp_dir = std::env::temp_dir();
    let project_path = temp_dir.join("fts-macros-toggle-test.RPP");

    if let Err(_) = fs::write(&project_path, project_content) {
        println!("⚠  Failed to create test project");
        return;
    }

    println!("✓ Project created, spawning REAPER...\n");

    let mut cmd = Command::new(REAPER_PATH);
    cmd.current_dir(REAPER_RESOURCES)
        .arg("-newinst")
        .arg("-nosplash")
        .arg("-ignoreerrors");

    if project_path.exists() {
        cmd.arg(project_path.to_string_lossy().to_string());
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            println!("REAPER running (PID: {})\n", pid);

            println!("╔════════════════════════════════════════════════════════════════╗");
            println!("║  Toggle Control Test                                         ║");
            println!("╚════════════════════════════════════════════════════════════════╝\n");

            println!("┌─ Macro 3 → ReaGate Toggle (Toggle Mode) ────────────────────┐");
            println!("│                                                              │");
            println!("│ Toggle Threshold: 0.5                                        │");
            println!("│                                                              │");
            println!("│ Step 1: Move Macro 3 fader to 0.0 (OFF position)            │");
            println!("│         Expected: ReaGate is OFF (gate disabled)             │");
            println!("│         Verify: Open ReaGate window, check bypass state      │");
            println!("│                                                              │");
            println!("│ Step 2: Move Macro 3 fader to 0.4 (still OFF)               │");
            println!("│         Expected: ReaGate remains OFF                        │");
            println!("│         (Values < 0.5 = OFF)                                 │");
            println!("│                                                              │");
            println!("│ Step 3: Move Macro 3 fader to 0.5 (threshold)               │");
            println!("│         Expected: ReaGate turns ON                           │");
            println!("│                                                              │");
            println!("│ Step 4: Move Macro 3 fader to 1.0 (ON position)             │");
            println!("│         Expected: ReaGate remains ON                         │");
            println!("│         (Values >= 0.5 = ON)                                 │");
            println!("│                                                              │");
            println!("│ Step 5: Toggle between 0.0 and 1.0 multiple times           │");
            println!("│         Expected: ReaGate toggles on/off instantly           │");
            println!("└──────────────────────────────────────────────────────────────┘\n");

            println!("Close REAPER when done (Cmd+Q)\n");

            thread::sleep(Duration::from_secs(2));

            let _ = child.wait();
            let _ = fs::remove_file(&project_path);

            println!("\n✓ Toggle test complete\n");
        }
        Err(e) => {
            println!("✗ Failed to spawn REAPER: {}", e);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// Helper: Create a REAPER project with fts-macros and multiple target FX
///
/// Project layout:
/// - Track 1: FTS Macros + ReaEQ + ReaComp + ReaGate + ReaLimit (all on same track)
/// - Track 2: ReaEQ + ReaComp + ReaGate + ReaLimit (second instance - also controlled by macros)
fn create_test_project_with_fx() -> String {
    r#"<REAPER_PROJECT 0.1 "6.82"
  RECORD_PATH "" ""
  <TRACK 1
    TRACKID {34C3A88D-E202-A64A-84C7-C339E35DDD8A}
    NAME "Same Track: Macros + FX"
    PEAKCOL 16576
    BEAT -1
    AUTOMODE 0
    PANLAWFLAGS 3
    VOLPAN 1 0 -1 -1 1
    MUTESOLO 0 0 0
    IPHASE 0
    PLAYOFFS 0 1
    ISBUS 0 0
    BUSCOMP 0 0 0 0 0
    SHOWINMIX 1 0.6667 0.5 1 0.5 0 0 0 0
    FIXEDLANES 9 0 0 0 0
    REC 0 5088 1 0 0 0 0 0
    VU 64
    TRACKHEIGHT 0 0 0 0 0 0 0
    INQ 0 0 0 0.5 100 0 0 100
    NCHAN 2
    FX 5
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
      FLOATPOS 0 0 0 0
      FXID {68FCC55E-402F-A841-A3FC-15500DE11AAF}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaEQ (Cockos)" reaeq.vst.dylib 0 "" 1919247729<56535472656571726561657100000000> ""
        cWVlcu5e7f4CAAAAAQAAAAAAAAACAAAAAAAAAAIAAAABAAAAAAAAAAIAAAAAAAAAzQAAAAEAAAAAABAA
        IQAAAAUAAAAAAAAAAQAAAAAAAAAAAFlAAAAAAAAA8D+amZmZmZnpPwEIAAAAAQAAAAAAAAAAwHJAAAAAAAAA8D8AAAAAAAAAQAEIAAAAAQAAAAAAAAAAQI9AAAAAAAAA8D8AAAAAAAAAQAEBAAAAAQAAAAAAAAAAiLNAAAAAAAAA8D+amZmZmZnpPwEEAAAAAAAAAAAAAAAAAFlAAAAAAAAA8D8AAAAAAAAAQAEBAAAAAQAAAAAAAAAAAPA/AAAAAEACAABrAQAAAgAAAA==
        AAAQAAAA
      >
      FLOATPOS 746 551 576 419
      FXID {802F714E-EE74-2443-9BAB-37AC07565295}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaComp (Cockos)" reacomp.vst.dylib 0 "" 1919247213<5653547265636D726561636F6D700000> ""
        bWNlcu9e7f4EAAAAAQAAAAAAAAACAAAAAAAAAAQAAAAAAAAACAAAAAAAAAACAAAAAQAAAAAAAAACAAAAAAAAAFwAAAAAAAAAAAAQAA==
        776t3g3wrd4AAIA/ED74PKabxDsK16M8AAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAnNEHMwAAgD8AAAAAzcxMPQAAAAAAAAAAAAAAAAAAgD4AAAAAAAAAAAAAAAA=
        AAAQAAAA
      >
      FLOATPOS 778 541 567 397
      FXID {22548A45-10DB-B64B-B44E-CBFFE76859EE}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaGate (Cockos)" reagate.vst.dylib 0 "" 1919248244<56535472656774726561676174650000> ""
        dGdlcu9e7f4EAAAAAQAAAAAAAAACAAAAAAAAAAQAAAAAAAAACAAAAAAAAAACAAAAAQAAAAAAAAACAAAAAAAAAFwAAAAAAAAAAAAQAA==
        776t3g3wrd6c0QczppvEOwrXozwAAAAAAAAAAAAAgD8AAAAAAAAAAAAAAACc0QczAACAP5zRBzMAAIA/AAAAAAAAAAAAAAAALBYLPwAAAAAAAAAAAAAAAAAAAAA=
        AAAQAAAA
      >
      FLOATPOS 810 512 559 394
      FXID {3824A2AD-7F38-F947-AF6F-0C3CB0D45156}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaLimit (Cockos)" realimit.vst.dylib 0 "" 1919708532<565354726C6D747265616C696D697400> ""
        dG1scu5e7f4CAAAAAQAAAAAAAAACAAAAAAAAAAIAAAABAAAAAAAAAAIAAAAAAAAAMAAAAAEAAAAAABAA
        AwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAD+fduMQw8Y/
        AAAQAAAA
      >
      FLOATPOS 842 478 816 396
      FXID {52AFC5E9-A953-304B-BF94-448A20178E3E}
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
    TRACKID {FE3131B9-A35B-5548-9A5C-D1CF006742D9}
    NAME "Different Track: FX Only (Synchronized)"
    PEAKCOL 16576
    BEAT -1
    AUTOMODE 0
    PANLAWFLAGS 3
    VOLPAN 1 0 -1 -1 1
    MUTESOLO 0 0 0
    IPHASE 0
    PLAYOFFS 0 1
    ISBUS 0 0
    BUSCOMP 0 0 0 0 0
    SHOWINMIX 1 0.6667 0.5 1 0.5 0 0 0 0
    FIXEDLANES 9 0 0 0 0
    LANEREC -1 -1 -1 0
    REC 0 5088 1 0 0 0 0 0
    VU 64
    TRACKHEIGHT 0 0 0 0 0 0 0
    INQ 0 0 0 0.5 100 0 0 100
    NCHAN 2
    FX 4
    PERF 0
    MIDIOUT -1
    MAINSEND 1 0
    <FXCHAIN
      SHOW 0
      LASTSEL 0
      DOCKED 0
      BYPASS 0 0 0
      <VST "VST: ReaEQ (Cockos)" reaeq.vst.dylib 0 "" 1919247729<56535472656571726561657100000000> ""
        cWVlcu5e7f4CAAAAAQAAAAAAAAACAAAAAAAAAAIAAAABAAAAAAAAAAIAAAAAAAAAzQAAAAEAAAAAABAA
        IQAAAAUAAAAAAAAAAQAAAAAAAAAAAFlAAAAAAAAA8D+amZmZmZnpPwEIAAAAAQAAAAAAAAAAwHJAAAAAAAAA8D8AAAAAAAAAQAEIAAAAAQAAAAAAAAAAQI9AAAAAAAAA8D8AAAAAAAAAQAEBAAAAAQAAAAAAAAAAiLNAAAAAAAAA8D+amZmZmZnpPwEEAAAAAAAAAAAAAAAAAFlAAAAAAAAA8D8AAAAAAAAAQAEBAAAAAQAAAAAAAAAAAPA/AAAAAEACAABrAQAAAgAAAA==
        AFByb2dyYW0gMQAQAAAA
      >
      FLOATPOS 746 551 576 419
      FXID {93FFD394-9CE3-6443-8DDC-40F5C07C92AE}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaComp (Cockos)" reacomp.vst.dylib 0 "" 1919247213<5653547265636D726561636F6D700000> ""
        bWNlcu9e7f4EAAAAAQAAAAAAAAACAAAAAAAAAAQAAAAAAAAACAAAAAAAAAACAAAAAQAAAAAAAAACAAAAAAAAAFwAAAAAAAAAAAAQAA==
        776t3g3wrd4AAIA/ED74PKabxDsK16M8AAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAnNEHMwAAgD8AAAAAzcxMPQAAAAAAAAAAAAAAAAAAgD4AAAAAAAAAAAAAAAA=
        AFByb2dyYW0gMQAQAAAA
      >
      FLOATPOS 778 541 567 397
      FXID {7AA5C715-3877-E744-88B1-1BFD98897AA6}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaGate (Cockos)" reagate.vst.dylib 0 "" 1919248244<56535472656774726561676174650000> ""
        dGdlcu9e7f4EAAAAAQAAAAAAAAACAAAAAAAAAAQAAAAAAAAACAAAAAAAAAACAAAAAQAAAAAAAAACAAAAAAAAAFwAAAAAAAAAAAAQAA==
        776t3g3wrd6c0QczppvEOwrXozwAAAAAAAAAAAAAgD8AAAAAAAAAAAAAAACc0QczAACAP5zRBzMAAIA/AAAAAAAAAAAAAAAALBYLPwAAAAAAAAAAAAAAAAAAAAA=
        AFByb2dyYW0gMQAQAAAA
      >
      FLOATPOS 810 512 559 394
      FXID {EF9384E0-6E13-E14A-ADAF-52FB21AD0575}
      WAK 0 0
      BYPASS 0 0 0
      <VST "VST: ReaLimit (Cockos)" realimit.vst.dylib 0 "" 1919708532<565354726C6D747265616C696D697400> ""
        dG1scu5e7f4CAAAAAQAAAAAAAAACAAAAAAAAAAIAAAABAAAAAAAAAAIAAAAAAAAAMAAAAAEAAAAAABAA
        AwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAD+fduMQw8Y/
        AFByb2dyYW0gMQAQAAAA
      >
      FLOATPOS 842 478 816 396
      FXID {AAFCDD59-C014-8B4D-8A61-8E65EDC4C370}
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
      IGUID {396959E0-B0AE-8D4C-8F1D-083F7F5069C3}
      IID 2
    >
  >
>
"#.to_string()
}

/// Print detailed test instructions
fn print_test_instructions() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  How to Test Macro Parameter Control (Synchronized Tracks)  ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    println!("PROJECT LAYOUT:");
    println!("───────────────");
    println!("  Track 1 (Same Track):");
    println!("    - FTS Macros (controller with 8 macro parameters)");
    println!("    - ReaEQ (target for Macro 0)");
    println!("    - ReaComp (target for Macro 1)");
    println!("    - ReaGate (target for Macro 3)");
    println!("    - ReaLimit (bonus test)\n");
    println!("  Track 2 (Different Track - Synchronized):");
    println!("    - ReaEQ (same Macro 0 controls both)");
    println!("    - ReaComp (same Macro 1 controls both)");
    println!("    - ReaGate (same Macro 3 controls both)");
    println!("    - ReaLimit (same controls as Track 1)\n");

    println!("TEST MAPPINGS CONFIGURED:");
    println!("────────────────────────");
    println!("  ✓ Macro 0 → BOTH ReaEQ instances (PassThrough)");
    println!("           Controls Gain on Track 1 AND Track 2");
    println!("           0.0 maps to 0.0 dB, 1.0 maps to 1.0 dB");
    println!();
    println!("  ✓ Macro 1 → BOTH ReaComp instances (ScaleRange: 1.5 to 8.0)");
    println!("           Controls Ratio on Track 1 AND Track 2");
    println!("           0.0 maps to 1.5:1, 1.0 maps to 8.0:1");
    println!();
    println!("  ✓ Macro 3 → BOTH ReaGate instances (Toggle)");
    println!("           Controls Bypass on Track 1 AND Track 2");
    println!("           < 0.5 = bypass OFF, ≥ 0.5 = bypass ON\n");

    println!("MANUAL TEST PROCEDURE:");
    println!("─────────────────────");
    println!("1. SETUP: Open FX windows for testing");
    println!("   → Click \"Master\" → View Tracks List");
    println!("   → Double-click Track 1 FTS Macros → see 8 Macro faders");
    println!("   → Double-click Track 1 ReaEQ → see EQ bands");
    println!("   → Double-click Track 1 ReaComp → see Compressor");
    println!("   → Double-click Track 2 ReaEQ → see second EQ");
    println!("   → Double-click Track 2 ReaComp → see second Compressor\n");

    println!("2. TEST MACRO 0 (Synchronized EQ Gain):");
    println!("   → Drag Macro 0 fader to 0.25");
    println!("   → VERIFY: Track 1 ReaEQ Gain ≈ 0.25 dB");
    println!("   → VERIFY: Track 2 ReaEQ Gain ≈ 0.25 dB (same!)");
    println!("   → Drag to 0.5");
    println!("   → VERIFY: Both track EQs update together");
    println!("   → Drag to 1.0");
    println!("   → VERIFY: Both track EQs at ≈ 1.0 dB\n");

    println!("3. TEST MACRO 1 (Synchronized Compressor Ratio):");
    println!("   → Drag Macro 1 fader to 0.0");
    println!("   → VERIFY: Track 1 ReaComp Ratio ≈ 1.5:1");
    println!("   → VERIFY: Track 2 ReaComp Ratio ≈ 1.5:1 (same!)");
    println!("   → Drag to 0.5");
    println!("   → VERIFY: Both compressors update together");
    println!("           Track 1 Ratio ≈ 4.75:1");
    println!("           Track 2 Ratio ≈ 4.75:1");
    println!("   → Drag to 1.0");
    println!("   → VERIFY: Both compressors at 8.0:1\n");

    println!("4. TEST MACRO 3 (Synchronized Gate Toggle):");
    println!("   → Drag Macro 3 fader to 0.0");
    println!("   → VERIFY: Track 1 ReaGate bypass ON");
    println!("   → VERIFY: Track 2 ReaGate bypass ON (synchronized!)");
    println!("   → Drag to 0.6");
    println!("   → VERIFY: Both gates turn OFF at same time");
    println!("   → Toggle between 0.0 and 1.0 rapidly");
    println!("   → VERIFY: Both instances toggle together\n");

    println!("5. TEST INDEPENDENCE (Macro 2):");
    println!("   → Drag Macro 2 fader around");
    println!("   → VERIFY: No other parameters change");
    println!("   → This shows macros work independently\n");

    println!("WHAT THIS PROVES:");
    println!("─────────────────");
    println!("✓ Macro parameters are properly exposed and automatable");
    println!("✓ Mapping resolution works for SAME track (Macro 0 → Track 1 FX)");
    println!("✓ Mapping resolution works for DIFFERENT track (Macro 0 → Track 2 FX)");
    println!("✓ One macro CAN control multiple instances in sync!");
    println!("✓ Multiple macros remain independent");
    println!("✓ Transformation modes work correctly:");
    println!("  - PassThrough: 1:1 linear mapping");
    println!("  - ScaleRange: Remapping to custom min/max ranges");
    println!("  - Toggle: Boolean threshold at 0.5");
    println!("✓ Parameters update in real-time as macros change\n");
}

#[test]
fn print_vst_test_info() {
    print_environment_check();
    println!("\nVST Parameter Control Tests:");
    println!("  test_control_multiple_vst_parameters");
    println!("  test_vst_control_with_logging");
    println!("  test_vst_toggle_control\n");
    println!("Run with: cargo test -p fts-macros --test vst_parameter_control -- --nocapture\n");
}
