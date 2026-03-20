//! REAPER integration test: load FTS Guide plugin and verify params.
//!
//! Run with: `cargo xtask reaper-test guide_load`
//! Or keep open for inspection: `cargo xtask reaper-test --keep-open guide_load`

use reaper_test::reaper_test;

/// CLAP browser name for FTS Guide in REAPER.
const FTS_GUIDE_CLAP: &str = "CLAP: FTS Guide";

/// Expected parameter names and their default normalized values.
/// BoolParam defaults: true=1.0, false=0.0
/// FloatParam (gain-based) defaults: 0 dB = ~0.827 normalized (skewed)
const EXPECTED_PARAMS: &[&str] = &[
    "Gain",
    "Click Volume",
    "Count Volume",
    "Guide Volume",
    "Enable Beat",
    "Enable Eighth",
    "Enable Sixteenth",
    "Enable Triplet",
    "Measure Accent",
    "Enable Count",
    "Enable Guide",
    "Click Sound",
    "Guide Replaces Beat 1",
    "Offset Count By One",
    "Extend SONGEND Count",
    "Full Count for Odd Time",
];

#[reaper_test(isolated)]
async fn guide_load(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    // Create a track and load FTS Guide
    let track = project.tracks().add("Guide Test", None).await?;
    ctx.log("Created track: Guide Test");

    let fx = match track.fx_chain().add(FTS_GUIDE_CLAP).await {
        Ok(fx) => fx,
        Err(e) => {
            ctx.log(&format!("FAILED to add FX '{}': {:?}", FTS_GUIDE_CLAP, e));
            return Err(eyre::eyre!("Failed to add FX: {:?}", e));
        }
    };
    ctx.log(&format!("Loaded FTS Guide plugin: {:?}", fx));

    // Verify it's on the chain
    let fx_count = track.fx_chain().count().await?;
    assert_eq!(fx_count, 1, "Expected exactly 1 FX on track");
    ctx.log(&format!("FX count on track: {fx_count}"));

    // Enumerate all parameters
    let params = fx.parameters().await?;
    ctx.log(&format!("Total parameters: {}", params.len()));

    for p in &params {
        ctx.log(&format!(
            "  [{:>2}] {:<30} = {:.4}",
            p.index, p.name, p.value
        ));
    }

    // Verify expected params exist
    for expected_name in EXPECTED_PARAMS {
        let found = params.iter().any(|p| p.name == *expected_name);
        assert!(
            found,
            "Expected parameter '{}' not found. Available: {:?}",
            expected_name,
            params.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
    ctx.log(&format!(
        "All {} expected parameters found",
        EXPECTED_PARAMS.len()
    ));

    // Verify boolean defaults
    let enable_beat = params.iter().find(|p| p.name == "Enable Beat").unwrap();
    assert!(
        enable_beat.value > 0.5,
        "Enable Beat should default to true (got {})",
        enable_beat.value
    );

    let enable_eighth = params.iter().find(|p| p.name == "Enable Eighth").unwrap();
    assert!(
        enable_eighth.value < 0.5,
        "Enable Eighth should default to false (got {})",
        enable_eighth.value
    );

    // Verify click sound param exists and has correct default
    let click_sound = params.iter().find(|p| p.name == "Click Sound").unwrap();
    ctx.log(&format!(
        "Click Sound param index: {}, default normalized: {:.4}",
        click_sound.index, click_sound.value
    ));
    assert!(
        click_sound.value < 0.01,
        "Click Sound should default to Blip (0.0), got {}",
        click_sound.value
    );

    // Verify volume params have expected default (~0.71 normalized = 0 dB)
    let gain = params.iter().find(|p| p.name == "Gain").unwrap();
    assert!(
        (gain.value - 0.7078).abs() < 0.01,
        "Gain default should be ~0.71 (0 dB), got {}",
        gain.value
    );
    ctx.log(&format!(
        "Gain default: {:.4} (expected ~0.7078)",
        gain.value
    ));

    ctx.log("guide_load: PASSED");
    Ok(())
}
