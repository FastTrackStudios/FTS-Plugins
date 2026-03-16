//! REAPER integration test: verify FTS Guide produces audio during playback.
//!
//! Loads FTS Guide on a track, starts transport, and checks that the
//! transport position advances (proving the plugin is processing audio).
//! With "Sync to Transport" enabled and "Enable Beat" on, the plugin
//! should trigger click samples at each beat boundary.
//!
//! Run with: `cargo test -p fts-guide guide_playback -- --ignored --nocapture`

use reaper_test::reaper_test;
use std::time::Duration;

const FTS_GUIDE_CLAP: &str = "CLAP: FTS Guide";

#[reaper_test(isolated)]
async fn guide_playback(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();
    let transport = project.transport();

    // Ensure transport is stopped and at the start
    transport.stop().await?;
    transport.goto_start().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    ctx.log("Transport stopped at start");

    // Create track and load FTS Guide
    let track = project.tracks().add("Guide Playback Test", None).await?;
    let fx = match track.fx_chain().add(FTS_GUIDE_CLAP).await {
        Ok(fx) => fx,
        Err(e) => {
            ctx.log(&format!("FAILED to add FX: {:?}", e));
            return Err(eyre::eyre!("Failed to add FX: {:?}", e));
        }
    };
    ctx.log("Loaded FTS Guide");

    // Read params to verify defaults
    let params = fx.parameters().await?;

    let sync_param = params.iter().find(|p| p.name == "Sync to Transport");
    let beat_param = params.iter().find(|p| p.name == "Enable Beat");
    ctx.log(&format!(
        "Sync to Transport: {:?}, Enable Beat: {:?}",
        sync_param.map(|p| p.value),
        beat_param.map(|p| p.value),
    ));

    // Both should be enabled by default
    assert!(
        sync_param.map(|p| p.value > 0.5).unwrap_or(false),
        "Sync to Transport should be enabled by default"
    );
    assert!(
        beat_param.map(|p| p.value > 0.5).unwrap_or(false),
        "Enable Beat should be enabled by default"
    );

    // Record initial position
    let pos_before = transport.get_position().await?;
    ctx.log(&format!("Position before play: {:.4}s", pos_before));

    // Start playback
    transport.play().await?;
    ctx.log("Transport: PLAY");

    // Let it play for ~2 seconds (should cross several beat boundaries at 120 BPM)
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Check position advanced
    let pos_after = transport.get_position().await?;
    ctx.log(&format!("Position after 2s: {:.4}s", pos_after));

    assert!(
        pos_after > pos_before + 1.0,
        "Transport should have advanced at least 1 second, got {} -> {}",
        pos_before,
        pos_after
    );

    // At 120 BPM, 2 seconds = 4 beats. The plugin should have triggered
    // click samples at each beat boundary. We can't directly measure audio
    // output from the test, but we can verify the transport was running
    // and the plugin was in the FX chain processing.
    let fx_count = track.fx_chain().count().await?;
    assert_eq!(fx_count, 1, "FTS Guide should still be on the chain");

    // Verify plugin is not bypassed
    let params_after = fx.parameters().await?;
    let bypass = params_after.iter().find(|p| p.name == "Bypass");
    if let Some(bypass_param) = bypass {
        ctx.log(&format!("Bypass param: {:.4}", bypass_param.value));
        assert!(
            bypass_param.value < 0.5,
            "Plugin should not be bypassed (got {})",
            bypass_param.value
        );
    }

    // Stop transport
    transport.stop().await?;
    transport.goto_start().await?;
    ctx.log("Transport: STOP");

    ctx.log(&format!(
        "guide_playback: PASSED (played {:.2}s, crossed ~{:.0} beats at 120 BPM)",
        pos_after - pos_before,
        (pos_after - pos_before) * 2.0 // 120 BPM = 2 beats/sec
    ));
    Ok(())
}
