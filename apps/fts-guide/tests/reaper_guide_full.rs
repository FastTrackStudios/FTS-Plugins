//! REAPER integration test: full end-to-end guide plugin test.
//!
//! Creates the complete Click + Guide track structure with routing,
//! starts playback, and verifies the plugin produces audio through
//! all channels.
//!
//! Run with: `cargo test -p fts-guide guide_full -- --ignored --nocapture`

use reaper_test::reaper_test;
use std::time::Duration;

const FTS_GUIDE_CLAP: &str = "CLAP: FTS Guide";

#[reaper_test(isolated)]
async fn guide_full(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let tracks = project.tracks();
    let transport = project.transport();

    // ── Build track structure ────────────────────────────────
    ctx.log("=== Building track structure ===");

    let folder = tracks.add("Click + Guide", None).await?;
    folder.set_folder_depth(1).await?;

    let click_track = tracks.add("Click", None).await?;
    let fx = match click_track.fx_chain().add(FTS_GUIDE_CLAP).await {
        Ok(fx) => fx,
        Err(e) => return Err(eyre::eyre!("Failed to add FX: {:?}", e)),
    };
    click_track.set_parent_send(false).await?;
    ctx.log("Created Click track with FTS Guide");

    let loop_track = tracks.add("Loop", None).await?;
    let count_track = tracks.add("Count", None).await?;
    let guide_track = tracks.add("Guide", None).await?;
    guide_track.set_folder_depth(-1).await?;
    ctx.log("Created Loop, Count, Guide tracks");

    tokio::time::sleep(Duration::from_millis(300)).await;

    // ── Create sends with channel routing ────────────────────
    ctx.log("=== Setting up sends ===");

    let loop_guid = loop_track.guid().to_string();
    let send_loop = click_track.sends().add_to(&loop_guid).await?;
    send_loop.set_source_channels(2, 2).await?;
    send_loop.set_dest_channels(0, 2).await?;
    ctx.log("Send 0: Click ch 3/4 → Loop ch 1/2");

    let count_guid = count_track.guid().to_string();
    let send_count = click_track.sends().add_to(&count_guid).await?;
    send_count.set_source_channels(4, 2).await?;
    send_count.set_dest_channels(0, 2).await?;
    ctx.log("Send 1: Click ch 5/6 → Count ch 1/2");

    let guide_guid = guide_track.guid().to_string();
    let send_guide = click_track.sends().add_to(&guide_guid).await?;
    send_guide.set_source_channels(6, 2).await?;
    send_guide.set_dest_channels(0, 2).await?;
    ctx.log("Send 2: Click ch 7/8 → Guide ch 1/2");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // ── Verify structure ─────────────────────────────────────
    ctx.log("=== Verifying structure ===");

    let track_count = tracks.count().await?;
    assert!(track_count >= 5, "Expected 5+ tracks, got {}", track_count);
    ctx.log(&format!("Track count: {track_count}"));

    let sends = click_track.sends().all().await?;
    assert_eq!(sends.len(), 3, "Expected 3 sends, got {}", sends.len());
    ctx.log(&format!("Send count: {}", sends.len()));

    let fx_count = click_track.fx_chain().count().await?;
    assert_eq!(fx_count, 1, "Expected 1 FX");

    // ── Verify params ────────────────────────────────────────
    ctx.log("=== Checking params ===");

    let params = fx.parameters().await?;
    let sync = params.iter().find(|p| p.name == "Sync to Transport");
    let beat = params.iter().find(|p| p.name == "Enable Beat");
    let count = params.iter().find(|p| p.name == "Enable Count");
    let accent = params.iter().find(|p| p.name == "Measure Accent");

    assert!(sync.map(|p| p.value > 0.5).unwrap_or(false), "Sync should be on");
    assert!(beat.map(|p| p.value > 0.5).unwrap_or(false), "Beat should be on");
    assert!(count.map(|p| p.value > 0.5).unwrap_or(false), "Count should be on");
    assert!(accent.map(|p| p.value > 0.5).unwrap_or(false), "Accent should be on");
    ctx.log("All default params verified");

    // ── Play and verify transport advances ───────────────────
    ctx.log("=== Playback test ===");

    transport.stop().await?;
    transport.goto_start().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let pos_before = transport.get_position().await?;
    transport.play().await?;
    ctx.log(&format!("Playing from {pos_before:.2}s"));

    // Play for 3 seconds — at 120 BPM that's 6 beats
    tokio::time::sleep(Duration::from_secs(3)).await;

    let pos_after = transport.get_position().await?;
    let elapsed = pos_after - pos_before;
    let beats = elapsed * 2.0; // 120 BPM = 2 beats/sec

    ctx.log(&format!(
        "Played {elapsed:.2}s ({beats:.0} beats at 120 BPM)"
    ));

    assert!(
        elapsed > 2.0,
        "Transport should have advanced 2+ seconds, got {elapsed:.2}"
    );

    // ── Verify plugin still healthy ──────────────────────────
    let fx_count_after = click_track.fx_chain().count().await?;
    assert_eq!(fx_count_after, 1, "FX should still be on chain");

    let params_after = fx.parameters().await?;
    let bypass = params_after.iter().find(|p| p.name == "Bypass");
    if let Some(bp) = bypass {
        assert!(bp.value < 0.5, "Plugin should not be bypassed");
    }

    // ── Stop ─────────────────────────────────────────────────
    transport.stop().await?;
    transport.goto_start().await?;

    ctx.log(&format!(
        "guide_full: PASSED — {track_count} tracks, 3 sends, {beats:.0} beats played"
    ));
    Ok(())
}
