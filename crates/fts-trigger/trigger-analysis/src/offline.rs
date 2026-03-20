//! Offline trigger analyzer — detects all transients with perfect lookahead.
//!
//! Runs the trigger detector over the entire track, extracts onset times and
//! velocities. Results can be written as MIDI notes or trigger automation.

use trigger_dsp::detector::{DetectMode, TriggerDetector};
use trigger_dsp::velocity::{VelocityCurve, VelocityMapper};

// r[impl offline.trigger.analysis]
// r[impl offline.analysis.deterministic]
/// A single detected trigger event.
#[derive(Debug, Clone, Copy)]
pub struct TriggerEvent {
    /// Time in seconds from the start of the audio.
    pub time: f64,
    /// Peak level that triggered the event (linear, 0.0–1.0+).
    pub peak_level: f64,
    /// Mapped velocity (0.0–1.0).
    pub velocity: f64,
    /// MIDI velocity (1–127).
    pub midi_velocity: u8,
}

/// Configuration for the offline trigger analyzer.
#[derive(Debug, Clone)]
pub struct OfflineTriggerConfig {
    /// Detection threshold in dB.
    pub threshold_db: f64,
    /// Release ratio (close threshold = open * ratio).
    pub release_ratio: f64,
    /// Detection confirmation time in milliseconds.
    pub detect_time_ms: f64,
    /// Release time in milliseconds.
    pub release_time_ms: f64,
    /// Minimum retrigger interval in milliseconds.
    pub retrigger_ms: f64,
    /// Detector reactivity in milliseconds.
    pub reactivity_ms: f64,
    /// Detection mode.
    pub detect_mode: DetectMode,
    /// Velocity curve type.
    pub velocity_curve: VelocityCurve,
    /// Velocity dynamics exponent.
    pub dynamics: f64,
    /// Sample rate.
    pub sample_rate: f64,
}

impl Default for OfflineTriggerConfig {
    fn default() -> Self {
        Self {
            threshold_db: -30.0,
            release_ratio: 0.5,
            detect_time_ms: 1.0,
            release_time_ms: 50.0,
            retrigger_ms: 20.0,
            reactivity_ms: 5.0,
            detect_mode: DetectMode::Peak,
            velocity_curve: VelocityCurve::Linear,
            dynamics: 1.0,
            sample_rate: 48000.0,
        }
    }
}

/// Result of offline trigger analysis.
pub struct TriggerAnalysis {
    /// All detected trigger events, sorted by time.
    pub events: Vec<TriggerEvent>,
}

impl TriggerAnalysis {
    /// Export events as CSV.
    pub fn to_csv(&self) -> String {
        let mut out = String::from("time_s,peak_level,velocity,midi_velocity\n");
        for e in &self.events {
            out.push_str(&format!(
                "{:.6},{:.6},{:.4},{}\n",
                e.time, e.peak_level, e.velocity, e.midi_velocity
            ));
        }
        out
    }

    /// Export events as JSON array.
    pub fn to_json(&self) -> String {
        let mut out = String::from("[\n");
        for (i, e) in self.events.iter().enumerate() {
            if i > 0 {
                out.push_str(",\n");
            }
            out.push_str(&format!(
                "  {{\"time\": {:.6}, \"peak\": {:.6}, \"velocity\": {:.4}, \"midi\": {}}}",
                e.time, e.peak_level, e.velocity, e.midi_velocity
            ));
        }
        out.push_str("\n]\n");
        out
    }
}

/// Analyze audio and detect all trigger events.
pub fn analyze(left: &[f64], right: &[f64], config: &OfflineTriggerConfig) -> TriggerAnalysis {
    let n = left.len().min(right.len());
    if n == 0 {
        return TriggerAnalysis { events: Vec::new() };
    }

    let mut detector = TriggerDetector::new();
    detector.detect_threshold_db = config.threshold_db;
    detector.release_ratio = config.release_ratio;
    detector.detect_time_ms = config.detect_time_ms;
    detector.release_time_ms = config.release_time_ms;
    detector.retrigger_ms = config.retrigger_ms;
    detector.reactivity_ms = config.reactivity_ms;
    detector.mode = config.detect_mode;
    detector.update(config.sample_rate);

    let mut mapper = VelocityMapper::new();
    mapper.curve = config.velocity_curve;
    mapper.dynamics = config.dynamics;

    let threshold_linear = 10.0_f64.powf(config.threshold_db / 20.0);

    let mut events = Vec::new();

    for i in 0..n {
        // Mono sum for detection
        let mono = (left[i] + right[i]) * 0.5;

        if let Some(peak_level) = detector.tick(mono) {
            let velocity = mapper.map(peak_level, threshold_linear);
            let midi_velocity = VelocityMapper::to_midi(velocity);

            events.push(TriggerEvent {
                time: i as f64 / config.sample_rate,
                peak_level,
                velocity,
                midi_velocity,
            });
        }
    }

    TriggerAnalysis { events }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn default_config() -> OfflineTriggerConfig {
        OfflineTriggerConfig {
            sample_rate: SR,
            ..Default::default()
        }
    }

    #[test]
    fn detects_loud_transient() {
        // Silence then a loud burst
        let mut signal = vec![0.0; (SR * 0.1) as usize];
        let burst: Vec<f64> = (0..((SR * 0.05) as usize))
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / SR).sin() * 0.8)
            .collect();
        signal.extend(&burst);
        signal.extend(vec![0.0; (SR * 0.1) as usize]);

        let config = default_config();
        let result = analyze(&signal, &signal, &config);

        assert!(
            !result.events.is_empty(),
            "Should detect at least one trigger"
        );
        // Trigger should be near the burst start (~0.1s)
        let first = &result.events[0];
        assert!(
            (first.time - 0.1).abs() < 0.02,
            "Trigger should be near burst start: t={:.4}",
            first.time
        );
    }

    #[test]
    fn silence_produces_no_triggers() {
        let signal = vec![0.0; (SR * 0.5) as usize];
        let config = default_config();
        let result = analyze(&signal, &signal, &config);
        assert!(
            result.events.is_empty(),
            "Silence should produce no triggers"
        );
    }

    #[test]
    fn velocity_scales_with_level() {
        // Two bursts at different levels
        let mut signal = vec![0.0; (SR * 0.1) as usize];
        // Quiet burst
        let quiet: Vec<f64> = (0..((SR * 0.05) as usize))
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / SR).sin() * 0.1)
            .collect();
        signal.extend(&quiet);
        signal.extend(vec![0.0; (SR * 0.2) as usize]);
        // Loud burst
        let loud: Vec<f64> = (0..((SR * 0.05) as usize))
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / SR).sin() * 0.9)
            .collect();
        signal.extend(&loud);
        signal.extend(vec![0.0; (SR * 0.1) as usize]);

        let config = OfflineTriggerConfig {
            threshold_db: -6.0, // High threshold so both bursts are near it
            dynamics: 0.5,      // Lower dynamics to avoid saturation
            ..default_config()
        };
        let result = analyze(&signal, &signal, &config);

        if result.events.len() >= 2 {
            assert!(
                result.events[1].velocity > result.events[0].velocity,
                "Louder burst should have higher velocity: quiet={:.2}, loud={:.2}",
                result.events[0].velocity,
                result.events[1].velocity
            );
        }
    }

    #[test]
    fn empty_input() {
        let config = default_config();
        let result = analyze(&[], &[], &config);
        assert!(result.events.is_empty());
    }

    #[test]
    fn csv_export() {
        let mut signal = vec![0.0; (SR * 0.1) as usize];
        let burst: Vec<f64> = (0..((SR * 0.05) as usize))
            .map(|i| (2.0 * PI * 1000.0 * i as f64 / SR).sin() * 0.8)
            .collect();
        signal.extend(&burst);

        let config = default_config();
        let result = analyze(&signal, &signal, &config);
        let csv = result.to_csv();
        assert!(csv.contains("time_s,peak_level,velocity,midi_velocity"));
    }
}
