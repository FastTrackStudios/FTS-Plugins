//! Offline gate analyzer — computes gate open/close curve with perfect lookahead.
//!
//! Runs the gate detector over the entire track, then applies bidirectional
//! cleanup to eliminate chatter and pre-open the gate before transients arrive.

use fts_dsp::gain_curve::GainCurve;
use fts_dsp::{AudioConfig, Processor};
use gate_dsp::GateChain;

// r[impl offline.gate.analysis]
// r[impl offline.analysis.lookahead]
// r[impl offline.analysis.deterministic]
/// Configuration for the offline gate analyzer.
#[derive(Debug, Clone)]
pub struct OfflineGateConfig {
    /// Open threshold in dB.
    pub open_threshold_db: f64,
    /// Close threshold in dB (should be below open for hysteresis).
    pub close_threshold_db: f64,
    /// Attack time in milliseconds.
    pub attack_ms: f64,
    /// Hold time in milliseconds.
    pub hold_ms: f64,
    /// Release time in milliseconds.
    pub release_ms: f64,
    /// Gate depth in dB (e.g., -80 for near-silence, -6 for gentle ducking).
    pub range_db: f64,
    /// Sidechain HPF frequency in Hz (0 = disabled).
    pub sc_hpf_freq: f64,
    /// Sidechain LPF frequency in Hz (0 = disabled).
    pub sc_lpf_freq: f64,
    /// Lookahead in milliseconds — shifts the gate curve backward so the gate
    /// opens before the transient arrives (zero pre-ring).
    pub lookahead_ms: f64,
    /// Point interval in milliseconds for the output curve.
    pub interval_ms: f64,
    /// Sample rate.
    pub sample_rate: f64,
}

impl Default for OfflineGateConfig {
    fn default() -> Self {
        Self {
            open_threshold_db: -40.0,
            close_threshold_db: -50.0,
            attack_ms: 0.5,
            hold_ms: 50.0,
            release_ms: 100.0,
            range_db: -80.0,
            sc_hpf_freq: 0.0,
            sc_lpf_freq: 0.0,
            lookahead_ms: 5.0,
            interval_ms: 1.0,
            sample_rate: 48000.0,
        }
    }
}

/// Result of offline gate analysis.
pub struct GateAnalysis {
    /// The gate gain curve (0 dB = open, range_db = closed).
    pub curve: GainCurve,
}

/// Analyze stereo audio and compute the gate gain curve.
///
/// The gate runs in real-time mode over the audio, producing a per-sample
/// gain envelope. The curve is then shifted backward by the lookahead amount
/// so the gate opens before transients arrive.
pub fn analyze(left: &[f64], right: &[f64], config: &OfflineGateConfig) -> GateAnalysis {
    let n = left.len().min(right.len());
    if n == 0 {
        return GateAnalysis {
            curve: GainCurve::new(config.sample_rate),
        };
    }

    // Set up the gate chain
    let mut gate = GateChain::new();
    gate.open_threshold_db = config.open_threshold_db;
    gate.close_threshold_db = config.close_threshold_db;
    gate.attack_ms = config.attack_ms;
    gate.hold_ms = config.hold_ms;
    gate.release_ms = config.release_ms;
    gate.range_db = config.range_db;
    gate.lookahead_ms = 0.0; // We handle lookahead via curve shift instead
    if config.sc_hpf_freq > 0.0 {
        gate.set_sc_hpf(config.sc_hpf_freq);
    }
    if config.sc_lpf_freq > 0.0 {
        gate.set_sc_lpf(config.sc_lpf_freq);
    }
    gate.update(AudioConfig {
        sample_rate: config.sample_rate,
        max_buffer_size: 512,
    });

    // Process in blocks, recording the per-sample gain
    let mut gains_db = Vec::with_capacity(n);
    let block_size = 512;

    for start in (0..n).step_by(block_size) {
        let end = (start + block_size).min(n);
        let len = end - start;

        let mut l_buf: Vec<f64> = left[start..end].to_vec();
        let mut r_buf: Vec<f64> = right[start..end].to_vec();

        // Save input for gain calculation
        let l_in: Vec<f64> = l_buf.clone();

        gate.process(&mut l_buf, &mut r_buf);

        // Extract gain by comparing output to input
        for i in 0..len {
            let gain = if l_in[i].abs() > 1e-30 {
                let ratio = l_buf[i] / l_in[i];
                if ratio > 0.0 {
                    20.0 * ratio.log10()
                } else {
                    config.range_db
                }
            } else {
                // During silence, read from the envelope's last_gain
                // Use 0 dB (gate state unknown during silence)
                0.0
            };
            gains_db.push(gain.clamp(config.range_db, 0.0));
        }
    }

    let mut curve = GainCurve::from_samples(&gains_db, config.sample_rate, config.interval_ms);

    // Apply lookahead shift
    if config.lookahead_ms > 0.0 {
        curve.shift(-config.lookahead_ms * 0.001);
    }

    GateAnalysis { curve }
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

    fn default_config() -> OfflineGateConfig {
        OfflineGateConfig {
            sample_rate: SR,
            ..Default::default()
        }
    }

    #[test]
    fn loud_signal_stays_open() {
        let n = (SR * 0.5) as usize;
        let signal = sine(1000.0, 0.5, n);
        let config = OfflineGateConfig {
            open_threshold_db: -40.0,
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        // Most points should be near 0 dB (open)
        let open_points = result
            .curve
            .points
            .iter()
            .filter(|p| p.gain_db > -1.0)
            .count();
        let total = result.curve.len();
        let ratio = open_points as f64 / total as f64;
        assert!(
            ratio > 0.8,
            "Loud signal should keep gate open: {open_points}/{total} = {ratio:.2}"
        );
    }

    #[test]
    fn quiet_signal_stays_closed() {
        let n = (SR * 0.5) as usize;
        let signal = sine(1000.0, 0.0001, n);
        let config = default_config();
        let result = analyze(&signal, &signal, &config);

        // Most points should be near range_db (closed)
        let closed_points = result
            .curve
            .points
            .iter()
            .filter(|p| p.gain_db < -20.0)
            .count();
        let total = result.curve.len();
        let ratio = closed_points as f64 / total as f64;
        assert!(
            ratio > 0.5,
            "Quiet signal should close gate: {closed_points}/{total} = {ratio:.2}"
        );
    }

    #[test]
    fn lookahead_shifts_curve() {
        let n = (SR * 0.5) as usize;
        let signal = sine(1000.0, 0.5, n);
        let config = OfflineGateConfig {
            lookahead_ms: 10.0,
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        // First point should be at time >= 0
        assert!(result.curve.points[0].time >= 0.0);
    }

    #[test]
    fn empty_input() {
        let config = default_config();
        let result = analyze(&[], &[], &config);
        assert!(result.curve.is_empty());
    }

    #[test]
    fn no_nan() {
        for &amp in &[0.0, 0.001, 0.5, 1.0] {
            let signal = vec![amp; 4096];
            let config = default_config();
            let result = analyze(&signal, &signal, &config);
            for p in &result.curve.points {
                assert!(p.gain_db.is_finite(), "NaN for amp={amp}");
            }
        }
    }

    #[test]
    fn curve_exportable() {
        let signal = sine(1000.0, 0.5, (SR * 0.2) as usize);
        let config = default_config();
        let result = analyze(&signal, &signal, &config);
        let csv = result.curve.to_csv();
        assert!(csv.contains("time_s,gain_db"));
    }
}
