//! REAPER integration test: generate guide MIDI from song structure.
//!
//! Creates regions defining a song structure, then triggers the existing
//! `generate_guide_track` REAPER action which creates the folder structure
//! and MIDI items with click/count/guide notes.
//!
//! Run with:
//!   FTS_HOME=/Users/codywright/Music/Dev/FastTrackStudio \
//!   cargo test -p fts-guide --test reaper_guide_midi_gen -- --ignored --nocapture

use reaper_test::reaper_test;
use std::time::Duration;

/// The REAPER extension action for generating guide tracks.
/// Defined in: FastTrackStudio/apps/reaper-extension/src/local_actions.rs
const GENERATE_GUIDE_TRACK: &str = "_FTS_GUIDE_GENERATE_GUIDE_TRACK";

struct TestSection {
    name: &'static str,
    start: f64,
    end: f64,
}

fn test_song() -> Vec<TestSection> {
    vec![
        TestSection {
            name: "Intro",
            start: 0.0,
            end: 8.0,
        },
        TestSection {
            name: "Verse 1",
            start: 8.0,
            end: 24.0,
        },
        TestSection {
            name: "Pre Chorus",
            start: 24.0,
            end: 28.0,
        },
        TestSection {
            name: "Chorus",
            start: 28.0,
            end: 42.0,
        },
        TestSection {
            name: "Verse 2",
            start: 42.0,
            end: 58.0,
        },
        TestSection {
            name: "Bridge",
            start: 58.0,
            end: 66.0,
        },
        TestSection {
            name: "Chorus 2",
            start: 66.0,
            end: 80.0,
        },
        TestSection {
            name: "Breakdown",
            start: 80.0,
            end: 84.0,
        },
        TestSection {
            name: "Outro",
            start: 84.0,
            end: 92.0,
        },
    ]
}

#[reaper_test(isolated)]
async fn guide_midi_gen(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let song = test_song();

    // ── 1. Create song structure regions ──────────────────────
    ctx.log("=== Creating song structure ===");
    for s in &song {
        project.regions().add(s.start, s.end, s.name).await?;
    }
    let region_count = project.regions().count().await?;
    ctx.log(&format!("{} regions created", region_count));
    assert_eq!(region_count, song.len());

    // ── 2. Run the generate_guide_track action ───────────────
    ctx.log("=== Running generate_guide_track ===");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let ok = project.run_command(GENERATE_GUIDE_TRACK).await?;
    ctx.log(&format!(
        "Action result: {}",
        if ok { "OK" } else { "FAILED" }
    ));

    if !ok {
        ctx.log("Action failed — check if command is registered");
        return Err(eyre::eyre!("generate_guide_track action failed"));
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    // ── 3. Verify track structure ────────────────────────────
    ctx.log("=== Verifying tracks ===");

    let folder = ctx.track_by_name("Click + Guide").await;
    let click = ctx.track_by_name("Click").await;
    let click_native = ctx.track_by_name("Click Native").await;
    let loop_track = ctx.track_by_name("Loop").await;
    let count = ctx.track_by_name("Count").await;
    let guide = ctx.track_by_name("Guide").await;

    ctx.log(&format!(
        "  Folder:       {}",
        if folder.is_ok() { "OK" } else { "MISSING" }
    ));
    ctx.log(&format!(
        "  Click:        {}",
        if click.is_ok() { "OK" } else { "MISSING" }
    ));
    ctx.log(&format!(
        "  Click Native: {}",
        if click_native.is_ok() {
            "OK"
        } else {
            "MISSING"
        }
    ));
    ctx.log(&format!(
        "  Loop:         {}",
        if loop_track.is_ok() { "OK" } else { "MISSING" }
    ));
    ctx.log(&format!(
        "  Count:        {}",
        if count.is_ok() { "OK" } else { "MISSING" }
    ));
    ctx.log(&format!(
        "  Guide:        {}",
        if guide.is_ok() { "OK" } else { "MISSING" }
    ));

    assert!(folder.is_ok(), "Click + Guide folder should exist");
    assert!(click.is_ok(), "Click track should exist");
    assert!(click_native.is_ok(), "Click Native track should exist");
    assert!(loop_track.is_ok(), "Loop track should exist");
    assert!(count.is_ok(), "Count track should exist");
    assert!(guide.is_ok(), "Guide track should exist");

    // ── 4. Verify MIDI items ─────────────────────────────────
    ctx.log("=== Verifying MIDI items ===");

    let click = click.unwrap();
    let click_native = click_native.unwrap();
    let count = count.unwrap();
    let guide = guide.unwrap();

    let click_items = click.items().count().await?;
    let click_native_items = click_native.items().count().await?;
    let count_items = count.items().count().await?;
    let guide_items = guide.items().count().await?;

    ctx.log(&format!("  Click items:        {}", click_items));
    ctx.log(&format!("  Click Native items: {}", click_native_items));
    ctx.log(&format!("  Count items:        {}", count_items));
    ctx.log(&format!("  Guide items:        {}", guide_items));

    // Click folder track: MIDI click items (1 per section)
    assert!(
        click_items >= 1,
        "Click track should have MIDI items (got {})",
        click_items
    );
    // Click Native: 1 REAPER click source item
    assert_eq!(
        click_native_items, 1,
        "Click Native should have 1 click source item (got {})",
        click_native_items
    );
    // Count: short items (1 measure each, at least 1 per section that has a count-in)
    assert!(
        count_items >= 1,
        "Count track should have items (got {})",
        count_items
    );
    // Guide: short items (1 per section)
    assert!(
        guide_items >= 1,
        "Guide track should have items (got {})",
        guide_items
    );

    ctx.log(&format!(
        "guide_midi_gen: PASSED — {} regions → click={} native={} count={} guide={}",
        region_count, click_items, click_native_items, count_items, guide_items
    ));
    Ok(())
}
