//! REAPER integration test: load FTS Compressor plugin and verify params.
//!
//! Run with: `cargo xtask reaper-test comp_load`

use reaper_test::reaper_test;

/// CLAP browser name for FTS Compressor in REAPER.
const FTS_COMP_CLAP: &str = "CLAP: FTS Compressor";

/// Expected parameter names (must match FtsCompParams field names in lib.rs).
const EXPECTED_PARAMS: &[&str] = &[
    "Threshold",
    "Ratio",
    "Attack",
    "Release",
    "Knee",
    "Feedback",
    "Stereo Link",
    "Inertia",
    "Inertia Decay",
    "Ceiling",
    "Mix",
    "Input",
    "Output",
    "SC HPF",
];

#[reaper_test(isolated)]
async fn comp_load(ctx: &reaper_test::ReaperTestContext) -> eyre::Result<()> {
    let project = ctx.project().clone();

    // Create a track and load the compressor
    let track = project.tracks().add("Comp Test", None).await?;
    ctx.log("Created track: Comp Test");

    let fx = match track.fx_chain().add(FTS_COMP_CLAP).await {
        Ok(fx) => fx,
        Err(e) => {
            ctx.log(&format!("FAILED to add FX '{}': {:?}", FTS_COMP_CLAP, e));
            return Err(eyre::eyre!("Failed to add FX: {:?}", e));
        }
    };
    ctx.log(&format!("Loaded FTS Compressor: {:?}", fx));

    // Verify it's on the chain
    let fx_count = track.fx_chain().count().await?;
    assert_eq!(fx_count, 1, "Expected exactly 1 FX on track");

    // Enumerate all parameters
    let params = fx.parameters().await?;
    ctx.log(&format!("Total parameters: {}", params.len()));

    for p in &params {
        ctx.log(&format!(
            "  [{:>2}] {:<20} = {:.4}",
            p.index, p.name, p.value
        ));
    }

    // Verify all expected params exist
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

    // ── Verify defaults ─────────────────────────────────────────────

    // Threshold: default 0 dB → normalized 1.0 (linear -60..0)
    let threshold = params.iter().find(|p| p.name == "Threshold").unwrap();
    assert!(
        (threshold.value - 1.0).abs() < 0.01,
        "Threshold should default to 1.0 (0 dB), got {}",
        threshold.value
    );
    ctx.log(&format!(
        "Threshold default: {:.4} (expected ~1.0)",
        threshold.value
    ));

    // Ratio: default 4:1 on skewed range 1..20
    let ratio = params.iter().find(|p| p.name == "Ratio").unwrap();
    assert!(
        ratio.value > 0.0 && ratio.value < 1.0,
        "Ratio should be between 0 and 1 normalized, got {}",
        ratio.value
    );
    ctx.log(&format!("Ratio default: {:.4}", ratio.value));

    // Feedback: default 0%
    let feedback = params.iter().find(|p| p.name == "Feedback").unwrap();
    assert!(
        feedback.value < 0.01,
        "Feedback should default to 0%, got {}",
        feedback.value
    );

    // Stereo Link: default 100%
    let link = params.iter().find(|p| p.name == "Stereo Link").unwrap();
    assert!(
        (link.value - 1.0).abs() < 0.01,
        "Stereo Link should default to 100%, got {}",
        link.value
    );

    // Input/Output gain: default 0 dB → normalized 0.5 (linear -24..24)
    let input = params.iter().find(|p| p.name == "Input").unwrap();
    assert!(
        (input.value - 0.5).abs() < 0.01,
        "Input gain should default to 0.5 (0 dB), got {}",
        input.value
    );

    let output = params.iter().find(|p| p.name == "Output").unwrap();
    assert!(
        (output.value - 0.5).abs() < 0.01,
        "Output gain should default to 0.5 (0 dB), got {}",
        output.value
    );

    // SC HPF: default 0 Hz → normalized 0.0
    let sc_hpf = params.iter().find(|p| p.name == "SC HPF").unwrap();
    assert!(
        sc_hpf.value < 0.01,
        "SC HPF should default to 0 Hz, got {}",
        sc_hpf.value
    );

    ctx.log("comp_load: PASSED");
    Ok(())
}
