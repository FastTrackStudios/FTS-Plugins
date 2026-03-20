//! TremChain — complete tremolo processor with modulation engine.
//!
//! Wraps the Tremolo with a Modulator from fts-modulation and
//! implements the Processor trait.

use fts_dsp::{AudioConfig, Processor};
use fts_modulation::curves::CurveType;
use fts_modulation::modulator::Modulator;
use fts_modulation::pattern::Point;
use fts_modulation::tempo::TransportInfo;
use fts_modulation::trigger::TriggerMode;

use crate::tremolo::{TremMode, Tremolo};

/// Complete tremolo processing chain.
///
/// Signal flow: Modulator → Tremolo (L/R with stereo offset) → Mix → Output.
pub struct TremChain {
    pub tremolo_l: Tremolo,
    pub tremolo_r: Tremolo,
    pub modulator: Modulator,

    /// Dry/wet mix (0..1).
    pub mix: f64,
    /// Stereo phase offset in degrees (-180..180).
    pub stereo_phase: f64,

    transport: TransportInfo,
}

impl TremChain {
    pub fn new() -> Self {
        let mut modulator = Modulator::new();
        // Default: sine-like LFO at 1/4 note
        modulator.trigger.mode = TriggerMode::Sync;
        modulator.trigger.sync_index = 7; // 1/4 note
        modulator.smoother.set_params(0.0, 0.0, 48000.0); // No smoothing for clean LFO

        // Default sine pattern
        let pat = modulator.patterns.get_mut(0);
        pat.add_point(Point {
            id: 0,
            x: 0.0,
            y: 0.0,
            tension: 0.0,
            curve_type: CurveType::HalfSine,
        });
        pat.add_point(Point {
            id: 0,
            x: 0.5,
            y: 1.0,
            tension: 0.0,
            curve_type: CurveType::HalfSine,
        });
        pat.add_point(Point {
            id: 0,
            x: 1.0,
            y: 0.0,
            tension: 0.0,
            curve_type: CurveType::HalfSine,
        });
        modulator.patterns.set_active(0);

        Self {
            tremolo_l: Tremolo::new(),
            tremolo_r: Tremolo::new(),
            modulator,
            mix: 1.0,
            stereo_phase: 0.0,
            transport: TransportInfo::default(),
        }
    }

    /// Set transport info (call per-block from the plugin).
    pub fn set_transport(&mut self, transport: TransportInfo) {
        self.transport = transport;
    }

    /// Set tremolo mode on both channels.
    pub fn set_mode(&mut self, mode: TremMode) {
        self.tremolo_l.mode = mode;
        self.tremolo_r.mode = mode;
    }

    /// Set depth on both channels.
    pub fn set_depth(&mut self, depth: f64) {
        self.tremolo_l.depth = depth;
        self.tremolo_r.depth = depth;
    }
}

impl Default for TremChain {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for TremChain {
    fn reset(&mut self) {
        self.tremolo_l.reset();
        self.tremolo_r.reset();
        self.modulator.reset();
    }

    fn update(&mut self, config: AudioConfig) {
        self.tremolo_l.update(config.sample_rate);
        self.tremolo_r.update(config.sample_rate);
        self.modulator.update(config.sample_rate);
        self.modulator.stereo_offset = self.stereo_phase / 360.0;
    }

    fn process(&mut self, left: &mut [f64], right: &mut [f64]) {
        for i in 0..left.len().min(right.len()) {
            // Advance transport position
            let pos =
                self.transport.position_qn + i as f64 * self.transport.beats_per_sample(48000.0);
            let t = TransportInfo {
                position_qn: pos,
                ..self.transport
            };

            let mod_l = self.modulator.tick(&t, 0.0);
            let mod_r = if self.stereo_phase.abs() > 0.01 {
                self.modulator.output_stereo()
            } else {
                mod_l
            };

            let dry_l = left[i];
            let dry_r = right[i];

            let wet_l = self.tremolo_l.tick(left[i], mod_l, 0);
            let wet_r = self.tremolo_r.tick(right[i], mod_r, 1);

            left[i] = dry_l * (1.0 - self.mix) + wet_l * self.mix;
            right[i] = dry_r * (1.0 - self.mix) + wet_r * self.mix;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn config() -> AudioConfig {
        AudioConfig {
            sample_rate: SR,
            max_buffer_size: 512,
        }
    }

    #[test]
    fn silence_in_silence_out() {
        let mut c = TremChain::new();
        c.update(config());
        c.set_transport(TransportInfo {
            position_qn: 0.0,
            tempo_bpm: 120.0,
            playing: true,
        });

        let mut l = vec![0.0; 4800];
        let mut r = vec![0.0; 4800];
        c.process(&mut l, &mut r);

        for (i, &s) in l.iter().enumerate() {
            assert!(s.abs() < 1e-10, "Non-zero at {i}: {s}");
        }
    }

    #[test]
    fn modulates_signal() {
        let mut c = TremChain::new();
        c.set_depth(1.0);
        c.update(config());
        c.set_transport(TransportInfo {
            position_qn: 0.0,
            tempo_bpm: 120.0,
            playing: true,
        });

        // Constant 1.0 input
        let mut l = vec![1.0; 48000];
        let mut r = vec![1.0; 48000];
        c.process(&mut l, &mut r);

        // Should have both loud and quiet parts
        let min: f64 = l.iter().copied().fold(f64::MAX, f64::min);
        let max: f64 = l.iter().copied().fold(f64::MIN, f64::max);
        assert!(max > 0.9, "Should have loud parts: max={max}");
        assert!(min < 0.2, "Should have quiet parts: min={min}");
    }

    #[test]
    fn zero_depth_passes_through() {
        let mut c = TremChain::new();
        c.set_depth(0.0);
        c.update(config());
        c.set_transport(TransportInfo {
            position_qn: 0.0,
            tempo_bpm: 120.0,
            playing: true,
        });

        let mut l = vec![0.5; 4800];
        let mut r = vec![0.5; 4800];
        c.process(&mut l, &mut r);

        for (i, &s) in l.iter().enumerate() {
            assert!((s - 0.5).abs() < 1e-5, "Should pass through at {i}: {s}");
        }
    }

    #[test]
    fn no_nan() {
        let mut c = TremChain::new();
        c.set_depth(1.0);
        c.set_mode(TremMode::Harmonic);
        c.update(config());
        c.set_transport(TransportInfo {
            position_qn: 0.0,
            tempo_bpm: 120.0,
            playing: true,
        });

        use std::f64::consts::PI;
        let mut l: Vec<f64> = (0..48000)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5)
            .collect();
        let mut r = l.clone();
        c.process(&mut l, &mut r);

        for (i, &s) in l.iter().enumerate() {
            assert!(s.is_finite(), "NaN at {i}");
        }
    }
}
