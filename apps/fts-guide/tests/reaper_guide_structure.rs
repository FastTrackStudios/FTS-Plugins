//! REAPER integration test: create Click + Guide track structure.
//!
//! Creates a folder track hierarchy with FTS Guide and routed sub-tracks:
//!   Click + Guide/       (folder)
//!     Click              (FTS Guide plugin, 8-channel output)
//!     Loop               (receives ch 3/4 - shaker/loop)
//!     Count              (receives ch 5/6 - count voice)
//!     Guide              (receives ch 7/8 - section announcements)
//!
//! The Click track's channels 1/2 go to the folder parent (normal routing).
//! Channels 3-8 are routed via sends to the sub-tracks.
//!
//! Run with: `cargo test -p fts-guide guide_structure -- --ignored --nocapture`

use reaper_test::reaper_test;
use std::time::Duration;

const FTS_GUIDE_CLAP: &str = "CLAP: FTS Guide";

#[reaper_test(isolated)]
async fn guide_structure(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let tracks = project.tracks();

    // ── Create folder track: "Click + Guide" ─────────────────
    let folder = tracks.add("Click + Guide", None).await?;
    folder.set_folder_depth(1).await?; // Start folder
    ctx.log("Created folder: Click + Guide");

    // ── Create Click track (FTS Guide lives here) ────────────
    let click_track = tracks.add("Click", None).await?;
    let fx = match click_track.fx_chain().add(FTS_GUIDE_CLAP).await {
        Ok(fx) => fx,
        Err(e) => {
            ctx.log(&format!("FAILED to add FX: {:?}", e));
            return Err(eyre::eyre!("Failed to add FX: {:?}", e));
        }
    };
    // Disable parent send — we route channels manually via sends
    click_track.set_parent_send(false).await?;
    ctx.log("Created Click track with FTS Guide, parent send disabled");

    // ── Create Loop track (receives ch 3/4) ──────────────────
    let loop_track = tracks.add("Loop", None).await?;
    ctx.log("Created Loop track");

    // ── Create Count track (receives ch 5/6) ─────────────────
    let count_track = tracks.add("Count", None).await?;
    ctx.log("Created Count track");

    // ── Create Guide track (receives ch 7/8, closes folder) ──
    let guide_track = tracks.add("Guide", None).await?;
    guide_track.set_folder_depth(-1).await?; // End folder
    ctx.log("Created Guide track (folder end)");

    // Small settle for REAPER to process
    tokio::time::sleep(Duration::from_millis(300)).await;

    // ── Create sends from Click track to sub-tracks ──────────
    // Send 1: Click → Loop (for channels 3/4 - shaker)
    let loop_guid = loop_track.guid().to_string();
    let send_loop = click_track.sends().add_to(&loop_guid).await?;
    ctx.log(&format!("Created send: Click → Loop ({:?})", send_loop));

    // Send 2: Click → Count (for channels 5/6)
    let count_guid = count_track.guid().to_string();
    let send_count = click_track.sends().add_to(&count_guid).await?;
    ctx.log(&format!("Created send: Click → Count ({:?})", send_count));

    // Send 3: Click → Guide (for channels 7/8)
    let guide_guid = guide_track.guid().to_string();
    let send_guide = click_track.sends().add_to(&guide_guid).await?;
    ctx.log(&format!("Created send: Click → Guide ({:?})", send_guide));

    tokio::time::sleep(Duration::from_millis(200)).await;

    // ── Verify structure ─────────────────────────────────────
    let track_count = tracks.count().await?;
    ctx.log(&format!("Total tracks: {}", track_count));

    assert!(
        track_count >= 5,
        "Expected at least 5 tracks (folder + 4 children), got {}",
        track_count
    );

    // Verify sends exist
    let sends = click_track.sends().all().await?;
    ctx.log(&format!("Click track sends: {}", sends.len()));
    assert!(
        sends.len() >= 3,
        "Expected at least 3 sends from Click track, got {}",
        sends.len()
    );

    // Verify FX is loaded
    let fx_count = click_track.fx_chain().count().await?;
    assert_eq!(fx_count, 1, "Expected 1 FX on Click track");

    ctx.log("guide_structure: PASSED");
    Ok(())
}
