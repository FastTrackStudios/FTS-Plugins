//! Compressor DSP tests — verify gain reduction, ballistics, and signal integrity.

use comp_dsp::chain::CompChain;
use comp_dsp::compressor::Compressor;
use fts_dsp::{AudioConfig, Processor};

const SAMPLE_RATE: f64 = 48000.0;

fn make_comp() -> Compressor {
    let mut c = Compressor::new();
    c.threshold_db = -20.0;
    c.ratio = 4.0;
    c.attack_ms = 1.0; // Fast for testing
    c.release_ms = 50.0;
    c.knee_db = 0.0; // hard knee for testing
    c.feedback = 0.0;
    c.channel_link = 1.0;
    c.inertia = 0.0;
    c.ceiling = 1.0;
    c.fold = 0.0;
    c.input_gain_db = 0.0;
    c.output_gain_db = 0.0;
    c.update(SAMPLE_RATE);
    c
}

// ── Basic gain reduction ────────────────────────────────────────────────

#[test]
fn loud_signal_is_reduced() {
    let mut comp = make_comp();
    // Feed a loud signal (0 dBFS = well above -20 dB threshold)
    let amplitude = 1.0_f64; // 0 dBFS
    let mut peak_out = 0.0_f64;

    for _ in 0..4096 {
        let mut l = amplitude;
        let mut r = amplitude;
        comp.process_sample(&mut l, &mut r);
        peak_out = peak_out.max(l.abs());
    }

    // Output should be significantly reduced
    assert!(
        peak_out < amplitude * 0.8,
        "Loud signal should be compressed: peak_out={peak_out:.4} (expected < {:.4})",
        amplitude * 0.8
    );
}

#[test]
fn quiet_signal_passes_through() {
    let mut comp = make_comp();
    // Feed a quiet signal (well below threshold)
    let amplitude = 0.01; // ~ -40 dBFS, well below -20 dB threshold

    let mut max_diff = 0.0_f64;
    for _ in 0..2048 {
        let mut l = amplitude;
        let mut r = amplitude;
        comp.process_sample(&mut l, &mut r);
        // Should be very close to input (only tanh saturation)
        max_diff = max_diff.max((l - amplitude).abs());
    }

    assert!(
        max_diff < 0.001,
        "Quiet signal should pass through: max_diff={max_diff:.6}"
    );
}

#[test]
fn silence_produces_silence() {
    let mut comp = make_comp();
    for _ in 0..1024 {
        let mut l = 0.0;
        let mut r = 0.0;
        comp.process_sample(&mut l, &mut r);
        assert!(l == 0.0 && r == 0.0, "Silence should produce silence");
    }
}

// ── Ratio behavior ──────────────────────────────────────────────────────

#[test]
fn higher_ratio_more_reduction() {
    let mut comp_low = make_comp();
    comp_low.ratio = 2.0;
    comp_low.update(SAMPLE_RATE);

    let mut comp_high = make_comp();
    comp_high.ratio = 20.0;
    comp_high.update(SAMPLE_RATE);

    let amplitude = 1.0;
    let mut peak_low = 0.0_f64;
    let mut peak_high = 0.0_f64;

    for _ in 0..4096 {
        let mut l1 = amplitude;
        let mut r1 = amplitude;
        comp_low.process_sample(&mut l1, &mut r1);
        peak_low = peak_low.max(l1.abs());

        let mut l2 = amplitude;
        let mut r2 = amplitude;
        comp_high.process_sample(&mut l2, &mut r2);
        peak_high = peak_high.max(l2.abs());
    }

    assert!(
        peak_high < peak_low,
        "Higher ratio should compress more: ratio=2 peak={peak_low:.4}, ratio=20 peak={peak_high:.4}"
    );
}

// ── Attack/Release behavior ─────────────────────────────────────────────

#[test]
fn attack_time_affects_transient() {
    // Fast attack should reduce the first loud samples more quickly after a level jump
    let mut comp_fast = make_comp();
    comp_fast.attack_ms = 0.1;
    comp_fast.update(SAMPLE_RATE);

    let mut comp_slow = make_comp();
    comp_slow.attack_ms = 100.0;
    comp_slow.update(SAMPLE_RATE);

    // Warm up with a quiet signal so the detector is initialized at a low level
    let quiet = 0.01; // ~-40 dB, below threshold
    for _ in 0..480 {
        let (mut l, mut r) = (quiet, quiet);
        comp_fast.process_sample(&mut l, &mut r);
        let (mut l, mut r) = (quiet, quiet);
        comp_slow.process_sample(&mut l, &mut r);
    }

    // Now jump to a loud signal and measure the first 100 samples
    let amplitude = 1.0;
    let mut fast_sum = 0.0_f64;
    let mut slow_sum = 0.0_f64;

    for _ in 0..100 {
        let mut l1 = amplitude;
        let mut r1 = amplitude;
        comp_fast.process_sample(&mut l1, &mut r1);
        fast_sum += l1.abs();

        let mut l2 = amplitude;
        let mut r2 = amplitude;
        comp_slow.process_sample(&mut l2, &mut r2);
        slow_sum += l2.abs();
    }

    assert!(
        fast_sum < slow_sum,
        "Fast attack should reduce transients sooner: fast_sum={fast_sum:.4}, slow_sum={slow_sum:.4}"
    );
}

// ── Channel linking ─────────────────────────────────────────────────────

#[test]
fn channel_link_matches_stereo_gr() {
    let mut comp = make_comp();
    comp.channel_link = 1.0; // Full link
    comp.update(SAMPLE_RATE);

    // Feed asymmetric signal: loud left, quiet right
    for _ in 0..2048 {
        let mut l = 1.0;
        let mut r = 0.001;
        comp.process_sample(&mut l, &mut r);
    }

    // With full linking, both channels should have the same GR
    let gr_diff = (comp.last_gr_db[0] - comp.last_gr_db[1]).abs();
    assert!(
        gr_diff < 0.1,
        "Fully linked channels should have matching GR: L={:.2}, R={:.2}",
        comp.last_gr_db[0],
        comp.last_gr_db[1]
    );
}

// ── Parallel mix (fold) ─────────────────────────────────────────────────

#[test]
fn fold_blends_dry_signal() {
    let mut comp_dry = make_comp();
    comp_dry.fold = 1.0; // 100% dry
    comp_dry.update(SAMPLE_RATE);

    let amplitude = 1.0;
    // After settling, output should equal input (dry pass-through)
    let mut l = amplitude;
    let mut r = amplitude;
    for _ in 0..4096 {
        l = amplitude;
        r = amplitude;
        comp_dry.process_sample(&mut l, &mut r);
    }

    assert!(
        (l - amplitude).abs() < 0.01,
        "100% fold should pass dry signal: got {l:.4}"
    );
}

// ── Sidechain HPF ───────────────────────────────────────────────────────

#[test]
fn sidechain_hpf_reduces_bass_pumping() {
    // Without HPF: bass-heavy signal triggers lots of GR
    let mut chain_no_hpf = CompChain::new();
    chain_no_hpf.comp.threshold_db = -20.0;
    chain_no_hpf.comp.ratio = 8.0;
    chain_no_hpf.comp.attack_ms = 1.0;
    chain_no_hpf.comp.release_ms = 50.0;
    chain_no_hpf.set_sidechain_freq(0.0); // Off
    chain_no_hpf.update(AudioConfig {
        sample_rate: SAMPLE_RATE,
        max_buffer_size: 512,
    });

    // With HPF at 300Hz: bass doesn't trigger compression
    let mut chain_hpf = CompChain::new();
    chain_hpf.comp.threshold_db = -20.0;
    chain_hpf.comp.ratio = 8.0;
    chain_hpf.comp.attack_ms = 1.0;
    chain_hpf.comp.release_ms = 50.0;
    chain_hpf.set_sidechain_freq(300.0);
    chain_hpf.update(AudioConfig {
        sample_rate: SAMPLE_RATE,
        max_buffer_size: 512,
    });

    // Generate 50Hz bass signal (loud)
    let len = 4096;
    let mut left_no: Vec<f64> = (0..len)
        .map(|i| (2.0 * std::f64::consts::PI * 50.0 * i as f64 / SAMPLE_RATE).sin() * 0.9)
        .collect();
    let mut right_no = left_no.clone();
    let mut left_hpf = left_no.clone();
    let mut right_hpf = left_no.clone();

    chain_no_hpf.process(&mut left_no, &mut right_no);
    chain_hpf.process(&mut left_hpf, &mut right_hpf);

    let gr_no = chain_no_hpf.comp.gain_reduction_db();
    let gr_hpf = chain_hpf.comp.gain_reduction_db();

    assert!(
        gr_hpf < gr_no,
        "Sidechain HPF should reduce bass-driven GR: without={gr_no:.2}dB, with={gr_hpf:.2}dB"
    );
}

// ── NaN safety ──────────────────────────────────────────────────────────

#[test]
fn no_nan_from_edge_cases() {
    let mut comp = make_comp();

    let signals: Vec<f64> = vec![0.0, 1.0, -1.0, 1e6, -1e6, 1e-30, f64::MIN_POSITIVE];

    for &s in &signals {
        let mut l = s;
        let mut r = s;
        comp.process_sample(&mut l, &mut r);
        assert!(l.is_finite(), "NaN/Inf from input {s}: got {l}");
        assert!(r.is_finite(), "NaN/Inf from input {s}: got {r}");
    }
}

// ── Reset ───────────────────────────────────────────────────────────────

#[test]
fn reset_produces_silence() {
    let mut comp = make_comp();

    // Process loud signal
    for _ in 0..1024 {
        let mut l = 1.0;
        let mut r = 1.0;
        comp.process_sample(&mut l, &mut r);
    }

    comp.reset();

    // Process silence after reset
    for i in 0..1024 {
        let mut l = 0.0;
        let mut r = 0.0;
        comp.process_sample(&mut l, &mut r);
        assert!(
            l == 0.0 && r == 0.0,
            "After reset, silence[{i}] = ({l}, {r})"
        );
    }
}

// ── Deterministic ───────────────────────────────────────────────────────

#[test]
fn deterministic_output() {
    let signal: Vec<f64> = (0..2048)
        .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.8)
        .collect();

    let run = || {
        let mut comp = make_comp();
        signal
            .iter()
            .map(|&s| {
                let mut l = s;
                let mut r = s;
                comp.process_sample(&mut l, &mut r);
                l
            })
            .collect::<Vec<f64>>()
    };

    let out1 = run();
    let out2 = run();

    for (i, (a, b)) in out1.iter().zip(out2.iter()).enumerate() {
        assert!(
            (*a).to_bits() == (*b).to_bits(),
            "Non-deterministic at sample {i}: {a} vs {b}"
        );
    }
}
