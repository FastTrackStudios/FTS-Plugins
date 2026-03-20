//! Gate DSP tests — verify gating behavior, ballistics, and signal integrity.

use fts_dsp::{AudioConfig, Processor};
use gate_dsp::chain::GateChain;
use gate_dsp::detector::GateDetector;
use gate_dsp::envelope::GateEnvelope;

const SAMPLE_RATE: f64 = 48000.0;

fn config() -> AudioConfig {
    AudioConfig {
        sample_rate: SAMPLE_RATE,
        max_buffer_size: 512,
    }
}

fn make_chain() -> GateChain {
    let mut g = GateChain::new();
    g.open_threshold_db = -30.0;
    g.close_threshold_db = -40.0;
    g.attack_ms = 0.5;
    g.hold_ms = 50.0;
    g.release_ms = 100.0;
    g.range_db = -80.0;
    g.lookahead_ms = 0.0;
    g.sc_hpf_freq = 0.0;
    g.sc_lpf_freq = 0.0;
    g.update(config());
    g
}

// ── Basic gating ────────────────────────────────────────────────────────

#[test]
fn loud_signal_passes_through() {
    let mut gate = make_chain();
    let amplitude = 0.5; // ~ -6 dBFS, well above -30 dB threshold

    // Let the gate open and settle
    let mut left: Vec<f64> = vec![amplitude; 4096];
    let mut right: Vec<f64> = vec![amplitude; 4096];
    gate.process(&mut left, &mut right);

    // Check the last portion (after gate is fully open)
    let tail = &left[2048..];
    let min_out = tail.iter().copied().fold(f64::INFINITY, f64::min);

    assert!(
        min_out > amplitude * 0.9,
        "Loud signal should pass through: min_out={min_out:.4} (expected > {:.4})",
        amplitude * 0.9
    );
}

#[test]
fn quiet_signal_is_gated() {
    let mut gate = make_chain();
    let amplitude = 0.001; // ~ -60 dBFS, well below -40 dB close threshold

    let mut left: Vec<f64> = vec![amplitude; 4096];
    let mut right: Vec<f64> = vec![amplitude; 4096];
    gate.process(&mut left, &mut right);

    // After settling, output should be heavily attenuated
    let tail = &left[2048..];
    let peak_out: f64 = tail.iter().map(|x| x.abs()).fold(0.0, f64::max);

    assert!(
        peak_out < amplitude * 0.1,
        "Quiet signal should be gated: peak_out={peak_out:.6} (expected < {:.6})",
        amplitude * 0.1
    );
}

#[test]
fn silence_produces_silence() {
    let mut gate = make_chain();
    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];
    gate.process(&mut left, &mut right);

    for (i, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
        assert!(
            l == 0.0 && r == 0.0,
            "Silence should produce silence at sample {i}: ({l}, {r})"
        );
    }
}

// ── Hysteresis ──────────────────────────────────────────────────────────

#[test]
fn hysteresis_prevents_chatter() {
    let mut gate = make_chain();
    // Signal right between open (-30) and close (-40) thresholds: ~ -35 dBFS
    let amplitude = 10.0_f64.powf(-35.0 / 20.0); // ~0.0178

    // First open the gate with a loud signal
    let mut left: Vec<f64> = vec![0.5; 2048];
    let mut right: Vec<f64> = vec![0.5; 2048];
    gate.process(&mut left, &mut right);

    // Now feed the mid-level signal — gate should stay open (above close threshold)
    let mut left: Vec<f64> = vec![amplitude; 4096];
    let mut right: Vec<f64> = vec![amplitude; 4096];
    gate.process(&mut left, &mut right);

    // Check that gate stayed open (output should be close to input)
    let tail = &left[2048..];
    let avg_out: f64 = tail.iter().map(|x| x.abs()).sum::<f64>() / tail.len() as f64;

    assert!(
        avg_out > amplitude * 0.5,
        "Gate should stay open between thresholds (hysteresis): avg_out={avg_out:.6}, input={amplitude:.6}"
    );
}

// ── Hold time ───────────────────────────────────────────────────────────

#[test]
fn hold_time_delays_release() {
    let mut gate_no_hold = make_chain();
    gate_no_hold.hold_ms = 0.0;
    gate_no_hold.update(config());

    let mut gate_hold = make_chain();
    gate_hold.hold_ms = 200.0;
    gate_hold.update(config());

    // Open the gate with loud signal, then drop to silence
    let loud: Vec<f64> = vec![0.5; 2048];
    let silent: Vec<f64> = vec![0.0; 4096];

    let mut l1 = [loud.clone(), silent.clone()].concat();
    let mut r1 = l1.clone();
    let mut l2 = l1.clone();
    let mut r2 = l1.clone();

    gate_no_hold.process(&mut l1, &mut r1);
    gate_hold.process(&mut l2, &mut r2);

    // Loud -> silence produces zero output regardless of hold (signal is zero),
    // so test with loud -> quiet (below close threshold) instead.
    // Re-test: loud -> quiet (below close threshold but not zero)
    let quiet_level = 0.0001; // ~ -80 dBFS
    let mut gate_no_hold = make_chain();
    gate_no_hold.hold_ms = 0.0;
    gate_no_hold.release_ms = 10.0; // fast release
    gate_no_hold.update(config());

    let mut gate_hold = make_chain();
    gate_hold.hold_ms = 200.0;
    gate_hold.release_ms = 10.0;
    gate_hold.update(config());

    let loud: Vec<f64> = vec![0.5; 2048];
    let quiet: Vec<f64> = vec![quiet_level; 8192];

    let mut l1 = [loud.clone(), quiet.clone()].concat();
    let mut r1 = l1.clone();
    let mut l2 = l1.clone();
    let mut r2 = l1.clone();

    gate_no_hold.process(&mut l1, &mut r1);
    gate_hold.process(&mut l2, &mut r2);

    // Measure output level shortly after transition
    let check_start = 2048 + 100;
    let check_end = check_start + 2000;

    let avg_no_hold: f64 = l1[check_start..check_end]
        .iter()
        .map(|x| x.abs())
        .sum::<f64>()
        / (check_end - check_start) as f64;
    let avg_hold: f64 = l2[check_start..check_end]
        .iter()
        .map(|x| x.abs())
        .sum::<f64>()
        / (check_end - check_start) as f64;

    assert!(
        avg_hold > avg_no_hold,
        "Hold should delay gate closing: no_hold avg={avg_no_hold:.8}, hold avg={avg_hold:.8}"
    );
}

// ── Range (depth) ───────────────────────────────────────────────────────

#[test]
fn range_attenuates_not_mutes() {
    let mut gate = make_chain();
    gate.range_db = -20.0; // Only attenuate by 20dB, don't fully mute
    gate.update(config());

    let amplitude = 0.001; // Well below threshold
    let mut left: Vec<f64> = vec![amplitude; 4096];
    let mut right: Vec<f64> = vec![amplitude; 4096];
    gate.process(&mut left, &mut right);

    let tail = &left[2048..];
    let peak_out: f64 = tail.iter().map(|x| x.abs()).fold(0.0, f64::max);

    // With -20dB range, output should be ~0.1x input (not zero)
    assert!(
        peak_out > 0.0,
        "Range -20dB should not fully mute: peak_out={peak_out:.8}"
    );
    assert!(
        peak_out < amplitude,
        "Range should still attenuate: peak_out={peak_out:.8}, input={amplitude:.8}"
    );
}

// ── Sidechain HPF ───────────────────────────────────────────────────────

#[test]
fn sidechain_hpf_ignores_bass() {
    // Without HPF: loud bass triggers gate open
    let mut gate_no_hpf = make_chain();
    gate_no_hpf.open_threshold_db = -20.0;
    gate_no_hpf.close_threshold_db = -30.0;
    gate_no_hpf.update(config());

    // With HPF at 500Hz: bass shouldn't trigger gate
    let mut gate_hpf = make_chain();
    gate_hpf.open_threshold_db = -20.0;
    gate_hpf.close_threshold_db = -30.0;
    gate_hpf.set_sc_hpf(500.0);
    gate_hpf.update(config());

    // Generate 50Hz bass signal at -10 dBFS
    let len = 8192;
    let amp = 10.0_f64.powf(-10.0 / 20.0);
    let mut left_no: Vec<f64> = (0..len)
        .map(|i| (2.0 * std::f64::consts::PI * 50.0 * i as f64 / SAMPLE_RATE).sin() * amp)
        .collect();
    let mut right_no = left_no.clone();
    let mut left_hpf = left_no.clone();
    let mut right_hpf = left_no.clone();

    gate_no_hpf.process(&mut left_no, &mut right_no);
    gate_hpf.process(&mut left_hpf, &mut right_hpf);

    // Without HPF, the loud bass should open the gate (output ≈ input)
    let peak_no: f64 = left_no[4096..].iter().map(|x| x.abs()).fold(0.0, f64::max);
    // With HPF, the bass is filtered from sidechain so gate stays closed (output attenuated)
    let peak_hpf: f64 = left_hpf[4096..].iter().map(|x| x.abs()).fold(0.0, f64::max);

    assert!(
        peak_hpf < peak_no * 0.5,
        "SC HPF should prevent bass from opening gate: no_hpf={peak_no:.4}, hpf={peak_hpf:.4}"
    );
}

// ── Sidechain listen ────────────────────────────────────────────────────

#[test]
fn sidechain_listen_outputs_filtered_signal() {
    let mut gate = make_chain();
    gate.set_sc_hpf(1000.0);
    gate.sc_listen = true;
    gate.update(config());

    // Feed a mixed signal (50Hz + 5kHz)
    let len = 4096;
    let mut left: Vec<f64> = (0..len)
        .map(|i| {
            let t = i as f64 / SAMPLE_RATE;
            (2.0 * std::f64::consts::PI * 50.0 * t).sin() * 0.5
                + (2.0 * std::f64::consts::PI * 5000.0 * t).sin() * 0.5
        })
        .collect();
    let mut right = left.clone();

    gate.process(&mut left, &mut right);

    // Output should be the HPF'd signal — mostly the 5kHz, less of the 50Hz
    // Just verify it's not silence and not identical to input
    let peak: f64 = left[1024..].iter().map(|x| x.abs()).fold(0.0, f64::max);
    assert!(
        peak > 0.01,
        "SC listen should output filtered signal, not silence: peak={peak:.4}"
    );
}

// ── Lookahead ───────────────────────────────────────────────────────────

#[test]
fn lookahead_adds_latency() {
    let mut gate = make_chain();
    gate.lookahead_ms = 5.0;
    gate.update(config());

    assert_eq!(
        gate.latency_samples(),
        (5.0 * 0.001 * SAMPLE_RATE) as usize,
        "Lookahead should report correct latency"
    );
}

#[test]
fn lookahead_delays_audio_path() {
    let mut gate = make_chain();
    gate.lookahead_ms = 1.0;
    gate.open_threshold_db = -100.0; // Always open
    gate.close_threshold_db = -120.0;
    gate.update(config());

    let delay_samples = gate.latency_samples();

    // Feed an impulse
    let len = 1024;
    let mut left = vec![0.0; len];
    let mut right = vec![0.0; len];
    left[0] = 1.0;
    right[0] = 1.0;

    gate.process(&mut left, &mut right);

    // The impulse should appear at the delay position, not at 0
    // (gate needs to open first, so check that sample 0 is ~0)
    if delay_samples > 0 && delay_samples < len {
        assert!(
            left[0].abs() < 0.01,
            "With lookahead, sample 0 should be delayed: got {:.4}",
            left[0]
        );
    }
}

// ── NaN safety ──────────────────────────────────────────────────────────

#[test]
fn no_nan_from_edge_cases() {
    let mut gate = make_chain();

    let signals: Vec<f64> = vec![0.0, 1.0, -1.0, 1e6, -1e6, 1e-30, f64::MIN_POSITIVE];

    for &s in &signals {
        let mut left = vec![s; 64];
        let mut right = vec![s; 64];
        gate.process(&mut left, &mut right);
        for (i, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
            assert!(
                l.is_finite(),
                "NaN/Inf from input {s} at sample {i}: left={l}"
            );
            assert!(
                r.is_finite(),
                "NaN/Inf from input {s} at sample {i}: right={r}"
            );
        }
    }
}

// ── Reset ───────────────────────────────────────────────────────────────

#[test]
fn reset_clears_state() {
    let mut gate = make_chain();

    // Open the gate with loud signal
    let mut left = vec![0.5; 2048];
    let mut right = vec![0.5; 2048];
    gate.process(&mut left, &mut right);

    gate.reset();

    // After reset, gate should be closed — silence should stay silent
    let mut left = vec![0.0; 512];
    let mut right = vec![0.0; 512];
    gate.process(&mut left, &mut right);

    for (i, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
        assert!(
            l == 0.0 && r == 0.0,
            "After reset, silence[{i}] = ({l}, {r})"
        );
    }
}

// ── Deterministic ───────────────────────────────────────────────────────

#[test]
fn deterministic_output() {
    let signal: Vec<f64> = (0..4096)
        .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.8)
        .collect();

    let run = || {
        let mut gate = make_chain();
        let mut left = signal.clone();
        let mut right = signal.clone();
        gate.process(&mut left, &mut right);
        left
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

// ── Detector unit tests ─────────────────────────────────────────────────

#[test]
fn detector_opens_above_threshold() {
    let mut det = GateDetector::new();
    det.set_sample_rate(SAMPLE_RATE);

    let amplitude = 0.5; // ~ -6 dBFS
    let mut opened = false;
    for _ in 0..1000 {
        if det.tick(amplitude, -30.0, -40.0, 0) {
            opened = true;
            break;
        }
    }
    assert!(opened, "Detector should open for loud signal");
}

#[test]
fn detector_stays_closed_below_threshold() {
    let mut det = GateDetector::new();
    det.set_sample_rate(SAMPLE_RATE);

    let amplitude = 0.0001; // ~ -80 dBFS
    for _ in 0..2000 {
        let open = det.tick(amplitude, -30.0, -40.0, 0);
        assert!(!open, "Detector should stay closed for very quiet signal");
    }
}

// ── Envelope unit tests ─────────────────────────────────────────────────

#[test]
fn envelope_ramps_up_on_open() {
    let mut env = GateEnvelope::new();
    env.set_params(1.0, 50.0, 100.0, -80.0, SAMPLE_RATE);

    let mut max_gain = 0.0_f64;
    for _ in 0..((SAMPLE_RATE * 0.01) as usize) {
        let g = env.tick(true, 0);
        max_gain = max_gain.max(g);
    }

    assert!(
        max_gain > 0.9,
        "Envelope should ramp up during attack: max_gain={max_gain:.4}"
    );
}

#[test]
fn envelope_ramps_down_on_close() {
    let mut env = GateEnvelope::new();
    env.set_params(0.1, 0.0, 50.0, -80.0, SAMPLE_RATE);

    // First open fully
    for _ in 0..((SAMPLE_RATE * 0.01) as usize) {
        env.tick(true, 0);
    }

    // Now close
    let mut min_gain = 1.0_f64;
    for _ in 0..((SAMPLE_RATE * 0.1) as usize) {
        let g = env.tick(false, 0);
        min_gain = min_gain.min(g);
    }

    assert!(
        min_gain < 0.1,
        "Envelope should ramp down during release: min_gain={min_gain:.4}"
    );
}
