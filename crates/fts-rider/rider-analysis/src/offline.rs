//! Offline rider analyzer — computes ideal gain curves with perfect lookahead.
//!
//! Reads an entire track's audio and produces a gain ride curve using
//! bidirectional smoothing. Because the full audio is available, the offline
//! analyzer produces superior results to real-time by looking both forward
//! and backward, eliminating overcorrection and pumping artifacts.

use fts_dsp::gain_curve::GainCurve;
use rider_dsp::detector::{DetectMode, LevelDetector};

// r[impl rider.offline.full-track]
// r[impl rider.offline.lookahead-advantage]
// r[impl offline.rider.analysis]
/// Configuration for the offline rider analyzer.
#[derive(Debug, Clone)]
pub struct OfflineRiderConfig {
    /// Target level in dB.
    pub target_db: f64,
    /// Maximum boost in dB.
    pub max_boost_db: f64,
    /// Maximum cut in dB.
    pub max_cut_db: f64,
    /// Detection mode.
    pub detect_mode: DetectMode,
    /// Detection window in milliseconds.
    pub window_ms: f64,
    /// Voice activity threshold in dB.
    pub activity_threshold_db: f64,
    /// Smoothing time in milliseconds (applied bidirectionally).
    pub smoothing_ms: f64,
    /// Point interval in milliseconds for the output GainCurve (0 = per-sample).
    pub interval_ms: f64,
    /// Lookahead / pre-comp in milliseconds (shifts curve backward in time).
    pub precomp_ms: f64,
    /// Sample rate.
    pub sample_rate: f64,
}

impl Default for OfflineRiderConfig {
    fn default() -> Self {
        Self {
            target_db: -18.0,
            max_boost_db: 12.0,
            max_cut_db: 12.0,
            detect_mode: DetectMode::KWeighted,
            window_ms: 50.0,
            activity_threshold_db: -50.0,
            smoothing_ms: 100.0,
            interval_ms: 5.0,
            precomp_ms: 0.0,
            sample_rate: 48000.0,
        }
    }
}

/// Result of offline analysis.
pub struct RiderAnalysis {
    /// The gain curve, ready for DAW automation or audio application.
    pub curve: GainCurve,
    /// Detected levels in dB, one per input sample (for waveform display).
    pub level_db: Vec<f64>,
}

/// Analyze a stereo audio track and compute the ideal gain ride curve.
///
/// Uses bidirectional smoothing for superior results: a forward pass computes
/// the raw gain, then a backward pass smooths it so transients are handled
/// with effective lookahead.
// r[impl offline.analysis.lookahead]
// r[impl offline.analysis.deterministic]
pub fn analyze(left: &[f64], right: &[f64], config: &OfflineRiderConfig) -> RiderAnalysis {
    let n = left.len().min(right.len());
    if n == 0 {
        return RiderAnalysis {
            curve: GainCurve::new(config.sample_rate),
            level_db: Vec::new(),
        };
    }

    // ── Pass 1: Detect levels ───────────────────────────────────────────
    let mut detector = LevelDetector::new();
    detector.mode = config.detect_mode;
    detector.window_ms = config.window_ms;
    detector.update(config.sample_rate);

    let mut levels = Vec::with_capacity(n);
    for i in 0..n {
        let db = detector.tick(left[i], right[i]);
        levels.push(db);
    }

    // ── Pass 2: Compute raw gain ────────────────────────────────────────
    let raw_gain: Vec<f64> = levels
        .iter()
        .map(|&level| {
            if level < config.activity_threshold_db {
                0.0 // freeze at unity during silence
            } else {
                (config.target_db - level).clamp(-config.max_cut_db, config.max_boost_db)
            }
        })
        .collect();

    // ── Pass 3: Bidirectional smoothing ─────────────────────────────────
    let smooth_coeff = if config.smoothing_ms > 0.0 {
        (-1.0 / (config.smoothing_ms * 0.001 * config.sample_rate)).exp()
    } else {
        0.0
    };

    // Forward
    let mut fwd = vec![0.0_f64; n];
    fwd[0] = raw_gain[0];
    for i in 1..n {
        fwd[i] = smooth_coeff * fwd[i - 1] + (1.0 - smooth_coeff) * raw_gain[i];
    }

    // Backward
    let mut bwd = vec![0.0_f64; n];
    bwd[n - 1] = raw_gain[n - 1];
    for i in (0..n - 1).rev() {
        bwd[i] = smooth_coeff * bwd[i + 1] + (1.0 - smooth_coeff) * raw_gain[i];
    }

    // Average the two passes
    let final_gain: Vec<f64> = (0..n).map(|i| (fwd[i] + bwd[i]) * 0.5).collect();

    // ── Build GainCurve with interval thinning ──────────────────────────
    let mut curve = GainCurve::from_samples(&final_gain, config.sample_rate, config.interval_ms);

    // Apply pre-comp (lookahead shift)
    if config.precomp_ms > 0.0 {
        curve.shift(-config.precomp_ms * 0.001);
    }

    RiderAnalysis {
        curve,
        level_db: levels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn sine(freq: f64, amp: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin() * amp)
            .collect()
    }

    fn default_config() -> OfflineRiderConfig {
        OfflineRiderConfig {
            sample_rate: SR,
            ..Default::default()
        }
    }

    #[test]
    fn silence_produces_zero_gain() {
        let n = (SR * 0.5) as usize;
        let silence = vec![0.0; n];
        let config = default_config();
        let result = analyze(&silence, &silence, &config);

        assert!(!result.curve.is_empty());
        // All points should be near zero
        for p in &result.curve.points {
            assert!(
                p.gain_db.abs() < 1.0,
                "Silence gain should be near zero: {:.2}",
                p.gain_db
            );
        }
    }

    #[test]
    fn quiet_signal_gets_boosted() {
        let n = (SR * 1.0) as usize;
        let signal = sine(1000.0, 0.01, n);
        let config = OfflineRiderConfig {
            target_db: -18.0,
            detect_mode: DetectMode::Rms,
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        // Tail points should be positive (boosting)
        let tail: Vec<_> = result
            .curve
            .points
            .iter()
            .filter(|p| p.time > 0.5)
            .collect();
        let avg: f64 = tail.iter().map(|p| p.gain_db).sum::<f64>() / tail.len() as f64;
        assert!(
            avg > 5.0,
            "Quiet signal should get boosted: avg_gain={avg:.1} dB"
        );
    }

    #[test]
    fn loud_signal_gets_cut() {
        let n = (SR * 1.0) as usize;
        let signal = sine(1000.0, 0.9, n);
        let config = OfflineRiderConfig {
            target_db: -18.0,
            detect_mode: DetectMode::Rms,
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        let tail: Vec<_> = result
            .curve
            .points
            .iter()
            .filter(|p| p.time > 0.5)
            .collect();
        let avg: f64 = tail.iter().map(|p| p.gain_db).sum::<f64>() / tail.len() as f64;
        assert!(
            avg < -5.0,
            "Loud signal should get cut: avg_gain={avg:.1} dB"
        );
    }

    #[test]
    fn respects_range_limits() {
        let n = (SR * 1.0) as usize;
        let signal = sine(1000.0, 0.001, n);
        let config = OfflineRiderConfig {
            target_db: -14.0,
            max_boost_db: 6.0,
            max_cut_db: 6.0,
            detect_mode: DetectMode::Rms,
            smoothing_ms: 10.0,
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        for p in &result.curve.points {
            assert!(
                p.gain_db <= 6.1 && p.gain_db >= -6.1,
                "Gain should be within range: {:.2} dB",
                p.gain_db
            );
        }
    }

    #[test]
    fn precomp_shifts_curve() {
        let n = (SR * 1.0) as usize;
        let signal = sine(1000.0, 0.1, n);
        let mut config = default_config();
        config.detect_mode = DetectMode::Rms;
        config.precomp_ms = 50.0;

        let result = analyze(&signal, &signal, &config);

        // First point should be at time 0 (clamped from negative)
        assert!(result.curve.points[0].time >= 0.0);
    }

    #[test]
    fn interval_controls_density() {
        let n = (SR * 1.0) as usize;
        let signal = sine(1000.0, 0.3, n);

        let dense = {
            let mut c = default_config();
            c.interval_ms = 1.0;
            analyze(&signal, &signal, &c)
        };

        let sparse = {
            let mut c = default_config();
            c.interval_ms = 50.0;
            analyze(&signal, &signal, &c)
        };

        assert!(
            dense.curve.len() > sparse.curve.len() * 5,
            "Dense should have many more points: dense={}, sparse={}",
            dense.curve.len(),
            sparse.curve.len()
        );
    }

    #[test]
    fn curve_can_be_exported() {
        let n = (SR * 0.5) as usize;
        let signal = sine(1000.0, 0.3, n);
        let result = analyze(&signal, &signal, &default_config());

        let csv = result.curve.to_csv();
        assert!(csv.contains("time_s,gain_db"));

        let json = result.curve.to_json();
        assert!(json.starts_with('['));
    }

    #[test]
    fn apply_modifies_audio() {
        let n = (SR * 0.5) as usize;
        let signal = sine(1000.0, 0.05, n);
        let config = OfflineRiderConfig {
            target_db: -18.0,
            detect_mode: DetectMode::Rms,
            interval_ms: 0.0, // per-sample for accurate apply
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        let mut left = signal.clone();
        let mut right = signal.clone();
        result.curve.apply(&mut left, &mut right, 0.0);

        let in_rms: f64 = (signal.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt();
        let out_rms: f64 = (left.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt();

        assert!(
            out_rms > in_rms * 1.2,
            "Applied gain should boost quiet signal: in={in_rms:.4}, out={out_rms:.4}"
        );
    }

    #[test]
    fn empty_input_returns_empty() {
        let config = default_config();
        let result = analyze(&[], &[], &config);
        assert!(result.curve.is_empty());
        assert!(result.level_db.is_empty());
    }

    #[test]
    fn no_nan_in_output() {
        let n = 4096;
        for &amp in &[0.0, 0.001, 0.5, 1.0, 10.0] {
            let signal = vec![amp; n];
            let config = default_config();
            let result = analyze(&signal, &signal, &config);
            for p in &result.curve.points {
                assert!(
                    p.gain_db.is_finite(),
                    "NaN for amp={amp}: gain={}",
                    p.gain_db
                );
                assert!(p.time.is_finite(), "NaN for amp={amp}: time={}", p.time);
            }
        }
    }
}
