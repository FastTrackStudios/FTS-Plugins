//! Rider DSP integration tests.

use fts_dsp::{AudioConfig, Processor};
use rider_dsp::detector::{DetectMode, LevelDetector};
use rider_dsp::rider::GainRider;
use rider_dsp::RiderChain;
use std::f64::consts::PI;

const SAMPLE_RATE: f64 = 48000.0;
const CONFIG: AudioConfig = AudioConfig {
    sample_rate: SAMPLE_RATE,
    max_buffer_size: 512,
};

fn sine_block(freq: f64, amp: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| (2.0 * PI * freq * i as f64 / SAMPLE_RATE).sin() * amp)
        .collect()
}

// ── Level Detector ──────────────────────────────────────────────────────

#[test]
fn detector_silence_reads_low() {
    let mut det = LevelDetector::new();
    det.update(SAMPLE_RATE);

    for _ in 0..4096 {
        det.tick(0.0, 0.0);
    }

    assert!(
        det.level_db() < -100.0,
        "Silence should read very low: {:.1} dB",
        det.level_db()
    );
}

#[test]
fn detector_loud_reads_high() {
    let mut det = LevelDetector::new();
    det.mode = DetectMode::Rms;
    det.update(SAMPLE_RATE);

    let n = (SAMPLE_RATE * 0.1) as usize;
    for i in 0..n {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.8;
        det.tick(s, s);
    }

    assert!(
        det.level_db() > -10.0,
        "Loud signal should read high: {:.1} dB",
        det.level_db()
    );
}

#[test]
fn detector_louder_reads_higher() {
    let measure = |amp: f64| -> f64 {
        let mut det = LevelDetector::new();
        det.mode = DetectMode::Rms;
        det.update(SAMPLE_RATE);
        let n = (SAMPLE_RATE * 0.1) as usize;
        for i in 0..n {
            let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * amp;
            det.tick(s, s);
        }
        det.level_db()
    };

    let quiet = measure(0.1);
    let loud = measure(0.8);
    assert!(
        loud > quiet,
        "Louder signal should read higher: loud={loud:.1}, quiet={quiet:.1}"
    );
}

#[test]
fn detector_kweighted_boosts_highs() {
    let measure = |freq: f64| -> f64 {
        let mut det = LevelDetector::new();
        det.mode = DetectMode::KWeighted;
        det.update(SAMPLE_RATE);
        let n = 8192;
        for i in 0..n {
            let s = (2.0 * PI * freq * i as f64 / SAMPLE_RATE).sin() * 0.5;
            det.tick(s, s);
        }
        det.level_db()
    };

    let low = measure(100.0);
    let high = measure(4000.0);
    assert!(
        high > low,
        "K-weighting should boost highs: low={low:.1}, high={high:.1}"
    );
}

#[test]
fn detector_window_affects_response() {
    // Shorter window = faster response to changes
    let mut fast = LevelDetector::new();
    fast.mode = DetectMode::Rms;
    fast.window_ms = 10.0;
    fast.update(SAMPLE_RATE);

    let mut slow = LevelDetector::new();
    slow.mode = DetectMode::Rms;
    slow.window_ms = 200.0;
    slow.update(SAMPLE_RATE);

    // Feed loud signal then silence
    let n = (SAMPLE_RATE * 0.05) as usize;
    for i in 0..n {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.8;
        fast.tick(s, s);
        slow.tick(s, s);
    }
    // Now silence
    for _ in 0..(SAMPLE_RATE * 0.02) as usize {
        fast.tick(0.0, 0.0);
        slow.tick(0.0, 0.0);
    }

    // Fast detector should have dropped more
    assert!(
        fast.level_db() < slow.level_db(),
        "Shorter window should drop faster: fast={:.1}, slow={:.1}",
        fast.level_db(),
        slow.level_db()
    );
}

#[test]
fn detector_reset_clears() {
    let mut det = LevelDetector::new();
    det.update(SAMPLE_RATE);

    for i in 0..4096 {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.8;
        det.tick(s, s);
    }

    det.reset();

    assert!(
        det.level_db() < -100.0,
        "After reset, level should be very low: {:.1}",
        det.level_db()
    );
}

// ── Gain Rider ──────────────────────────────────────────────────────────

#[test]
fn rider_boosts_quiet_signal() {
    let mut rider = GainRider::new();
    rider.target_db = -18.0;
    rider.detector.mode = DetectMode::Rms;
    rider.attack_ms = 5.0;
    rider.release_ms = 20.0;
    rider.update(SAMPLE_RATE);

    let n = (SAMPLE_RATE * 0.5) as usize;
    for i in 0..n {
        // Quiet signal — well below target
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.01;
        rider.tick(s, s);
    }

    assert!(
        rider.gain_db() > 5.0,
        "Rider should boost quiet signal: gain={:.1} dB",
        rider.gain_db()
    );
}

#[test]
fn rider_cuts_loud_signal() {
    let mut rider = GainRider::new();
    rider.target_db = -18.0;
    rider.detector.mode = DetectMode::Rms;
    rider.attack_ms = 5.0;
    rider.release_ms = 20.0;
    rider.update(SAMPLE_RATE);

    let n = (SAMPLE_RATE * 0.5) as usize;
    for i in 0..n {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.9;
        rider.tick(s, s);
    }

    assert!(
        rider.gain_db() < -5.0,
        "Rider should cut loud signal: gain={:.1} dB",
        rider.gain_db()
    );
}

#[test]
fn rider_freezes_on_silence() {
    let mut rider = GainRider::new();
    rider.target_db = -18.0;
    rider.activity_threshold_db = -50.0;
    rider.detector.mode = DetectMode::Rms;
    rider.attack_ms = 5.0;
    rider.release_ms = 20.0;
    rider.update(SAMPLE_RATE);

    // Feed signal to establish a gain
    let n = (SAMPLE_RATE * 0.3) as usize;
    for i in 0..n {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.1;
        rider.tick(s, s);
    }

    // Flush detector with brief silence to drop below threshold
    for _ in 0..(SAMPLE_RATE * 0.1) as usize {
        rider.tick(0.0, 0.0);
    }

    let gain_before = rider.gain_db();

    // Feed more silence — gain should freeze
    for _ in 0..(SAMPLE_RATE * 0.5) as usize {
        rider.tick(0.0, 0.0);
    }

    let gain_after = rider.gain_db();

    assert!(
        (gain_after - gain_before).abs() < 0.5,
        "Gain should freeze during silence: before={gain_before:.2}, after={gain_after:.2}"
    );
}

#[test]
fn rider_respects_range() {
    let mut rider = GainRider::new();
    rider.target_db = -14.0;
    rider.max_boost_db = 6.0;
    rider.max_cut_db = 6.0;
    rider.detector.mode = DetectMode::Rms;
    rider.attack_ms = 1.0;
    rider.release_ms = 5.0;
    rider.update(SAMPLE_RATE);

    // Very quiet — would want huge boost, but limited to 6 dB
    let n = (SAMPLE_RATE * 1.0) as usize;
    for i in 0..n {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.001;
        rider.tick(s, s);
    }

    assert!(
        rider.gain_db() <= 6.1,
        "Gain should not exceed max_boost_db: {:.1} dB",
        rider.gain_db()
    );
}

#[test]
fn rider_reset_to_unity() {
    let mut rider = GainRider::new();
    rider.update(SAMPLE_RATE);

    let n = (SAMPLE_RATE * 0.2) as usize;
    for i in 0..n {
        let s = (2.0 * PI * 1000.0 * i as f64 / SAMPLE_RATE).sin() * 0.5;
        rider.tick(s, s);
    }

    rider.reset();

    assert!(
        rider.gain_db().abs() < 0.01,
        "After reset, gain should be 0 dB: {:.4}",
        rider.gain_db()
    );
}

// ── RiderChain ──────────────────────────────────────────────────────────

#[test]
fn chain_rides_toward_target() {
    let mut chain = RiderChain::new();
    chain.set_target_db(-18.0);
    chain.set_range_db(12.0);
    chain.rider.detector.mode = DetectMode::Rms;
    chain.rider.attack_ms = 5.0;
    chain.rider.release_ms = 20.0;
    chain.update(CONFIG);

    // Quiet signal
    let n = (SAMPLE_RATE * 0.5) as usize;
    let mut left = sine_block(1000.0, 0.02, n);
    let mut right = left.clone();

    chain.process(&mut left, &mut right);

    // Output should be louder than input
    let input_rms = 0.02 / 2.0_f64.sqrt();
    let output_rms: f64 = (left.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt();

    assert!(
        output_rms > input_rms * 1.5,
        "Chain should boost quiet signal: in_rms={input_rms:.4}, out_rms={output_rms:.4}"
    );
}

#[test]
fn chain_sc_listen_outputs_filtered() {
    let mut chain = RiderChain::new();
    chain.set_sidechain_freq(200.0);
    chain.sc_listen = true;
    chain.update(CONFIG);

    // Feed a mix of low and high frequency
    let n = 4096;
    let mut left: Vec<f64> = (0..n)
        .map(|i| {
            let low = (2.0 * PI * 50.0 * i as f64 / SAMPLE_RATE).sin() * 0.5;
            let high = (2.0 * PI * 2000.0 * i as f64 / SAMPLE_RATE).sin() * 0.5;
            low + high
        })
        .collect();
    let mut right = left.clone();

    chain.process(&mut left, &mut right);

    // The HPF at 200Hz should attenuate the 50Hz component significantly.
    // Measure energy in the output — should be less than full input because
    // the low freq is removed.
    let out_rms: f64 = (left.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt();
    let in_rms = (0.5_f64.powi(2) + 0.5_f64.powi(2)).sqrt() / 2.0_f64.sqrt();

    assert!(
        out_rms < in_rms * 0.9,
        "SC listen with HPF should remove lows: in={in_rms:.4}, out={out_rms:.4}"
    );
}

#[test]
fn chain_reset_clears_all() {
    let mut chain = RiderChain::new();
    chain.set_sidechain_freq(100.0);
    chain.update(CONFIG);

    let n = 2048;
    let mut left = sine_block(1000.0, 0.5, n);
    let mut right = left.clone();
    chain.process(&mut left, &mut right);

    chain.reset();

    assert!(
        chain.gain_db().abs() < 0.01,
        "After reset, gain should be 0 dB: {:.4}",
        chain.gain_db()
    );
}

// ── NaN safety ──────────────────────────────────────────────────────────

#[test]
fn detector_no_nan() {
    let mut det = LevelDetector::new();
    det.update(SAMPLE_RATE);

    for &val in &[0.0, 1.0, -1.0, 1e6, -1e6, 1e-30, f64::MIN_POSITIVE] {
        for _ in 0..256 {
            let db = det.tick(val, val);
            assert!(db.is_finite(), "NaN from input {val}: level={db}");
        }
    }
}

#[test]
fn rider_no_nan() {
    let mut rider = GainRider::new();
    rider.update(SAMPLE_RATE);

    for &val in &[0.0, 1.0, -1.0, 1e6, -1e6, 1e-30, f64::MIN_POSITIVE] {
        for _ in 0..256 {
            let g = rider.tick(val, val);
            assert!(g.is_finite(), "NaN from input {val}: gain={g}");
        }
    }
}

#[test]
fn chain_no_nan() {
    let mut chain = RiderChain::new();
    chain.update(CONFIG);

    for &val in &[0.0, 1.0, -1.0, 1e6, -1e6, 1e-30, f64::MIN_POSITIVE] {
        let mut left = vec![val; 256];
        let mut right = vec![val; 256];
        chain.process(&mut left, &mut right);
        for (i, (&l, &r)) in left.iter().zip(right.iter()).enumerate() {
            assert!(l.is_finite(), "NaN from input {val} at [{i}]: left={l}");
            assert!(r.is_finite(), "NaN from input {val} at [{i}]: right={r}");
        }
    }
}

// ── Deterministic ───────────────────────────────────────────────────────

#[test]
fn chain_deterministic() {
    let run = || {
        let mut chain = RiderChain::new();
        chain.set_target_db(-18.0);
        chain.rider.detector.mode = DetectMode::Rms;
        chain.rider.attack_ms = 10.0;
        chain.rider.release_ms = 40.0;
        chain.update(CONFIG);

        let mut left = sine_block(1000.0, 0.3, 2048);
        let mut right = left.clone();
        chain.process(&mut left, &mut right);
        left
    };

    let out1 = run();
    let out2 = run();

    for (i, (a, b)) in out1.iter().zip(out2.iter()).enumerate() {
        assert!(
            a.to_bits() == b.to_bits(),
            "Non-deterministic at [{i}]: {a} vs {b}"
        );
    }
}
