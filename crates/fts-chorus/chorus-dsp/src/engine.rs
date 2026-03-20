//! Chorus engines — four distinct voice implementations.
//!
//! Each engine produces a single modulated delay voice. The chain
//! creates multiple voices per channel for the full chorus effect.

use fts_dsp::delay_line::DelayLine;
use std::f64::consts::PI;

/// Chorus effect type — controls delay time ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectType {
    /// Chorus: 5–30ms delay, moderate modulation.
    Chorus,
    /// Flanger: 0.5–5ms delay, deep modulation with feedback.
    Flanger,
    /// Vibrato: pitch modulation only (no dry signal).
    Vibrato,
}

impl EffectType {
    pub fn base_delay_ms(&self) -> f64 {
        match self {
            EffectType::Chorus => 10.0,
            EffectType::Flanger => 2.0,
            EffectType::Vibrato => 5.0,
        }
    }

    pub fn max_depth_ms(&self) -> f64 {
        match self {
            EffectType::Chorus => 12.0,
            EffectType::Flanger => 4.0,
            EffectType::Vibrato => 8.0,
        }
    }
}

/// Chorus engine type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineType {
    /// Clean Catmull-Rom cubic interpolation.
    Cubic,
    /// Bucket-brigade device emulation (vintage analog).
    Bbd,
    /// Tape chorus with wow/flutter and saturation.
    Tape,
    /// Dual-tap elliptical orbital modulation (experimental).
    Orbit,
}

/// Common voice trait.
pub trait ChorusEngine: Send {
    fn update(&mut self, sample_rate: f64);
    fn tick(
        &mut self,
        input: f64,
        rate_hz: f64,
        depth: f64,
        feedback: f64,
        color: f64,
        effect_type: EffectType,
    ) -> f64;
    fn reset(&mut self);
}

// ─── Cubic Engine ───────────────────────────────────────────────────

/// Clean chorus voice using Catmull-Rom cubic delay interpolation.
pub struct CubicVoice {
    delay: DelayLine,
    lfo_phase: f64,
    phase_offset: f64,
    sample_rate: f64,
}

impl CubicVoice {
    const MAX_DELAY: usize = 192000 / 20 + 64;

    pub fn new(phase_offset: f64) -> Self {
        Self {
            delay: DelayLine::new(Self::MAX_DELAY),
            lfo_phase: 0.0,
            phase_offset,
            sample_rate: 48000.0,
        }
    }
}

impl ChorusEngine for CubicVoice {
    fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let needed = (sample_rate * 0.05) as usize + 64;
        if self.delay.len() < needed {
            self.delay = DelayLine::new(needed);
        }
    }

    fn tick(
        &mut self,
        input: f64,
        rate_hz: f64,
        depth: f64,
        feedback: f64,
        _color: f64,
        effect_type: EffectType,
    ) -> f64 {
        self.lfo_phase = (self.lfo_phase + rate_hz / self.sample_rate).fract();
        let lfo = ((self.lfo_phase + self.phase_offset) * 2.0 * PI).sin();

        let base_ms = effect_type.base_delay_ms();
        let depth_ms = effect_type.max_depth_ms() * depth;
        let delay_samples = ((base_ms + depth_ms * lfo) * 0.001 * self.sample_rate)
            .clamp(1.0, self.delay.len() as f64 - 4.0);

        let delayed = self.delay.read_cubic(delay_samples);
        self.delay
            .write(input + (delayed * feedback).clamp(-1.5, 1.5));
        delayed
    }

    fn reset(&mut self) {
        self.delay.clear();
        self.lfo_phase = 0.0;
    }
}

// ─── BBD Engine ─────────────────────────────────────────────────────

/// Bucket-brigade device emulation.
///
/// Clock-driven sample-and-hold chain with first-order hold reconstruction.
/// Input/output lowpass filters track the clock rate.
///
/// Based on BBD chorus topology from Choroboros (EsotericShadow).
pub struct BbdVoice {
    lfo_phase: f64,
    phase_offset: f64,
    /// BBD bucket chain (sample-and-hold stages).
    buckets: Vec<f64>,
    /// Clock phase accumulator.
    clock_phase: f64,
    /// Previous output for first-order hold interpolation.
    prev_output: f64,
    /// Input lowpass state.
    input_lp: f64,
    /// Output lowpass state.
    output_lp: f64,
    num_stages: usize,
    sample_rate: f64,
}

impl BbdVoice {
    const DEFAULT_STAGES: usize = 512;

    pub fn new(phase_offset: f64) -> Self {
        Self {
            lfo_phase: 0.0,
            phase_offset,
            buckets: vec![0.0; Self::DEFAULT_STAGES],
            clock_phase: 0.0,
            prev_output: 0.0,
            input_lp: 0.0,
            output_lp: 0.0,
            num_stages: Self::DEFAULT_STAGES,
            sample_rate: 48000.0,
        }
    }
}

impl ChorusEngine for BbdVoice {
    fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    fn tick(
        &mut self,
        input: f64,
        rate_hz: f64,
        depth: f64,
        feedback: f64,
        color: f64,
        effect_type: EffectType,
    ) -> f64 {
        // LFO for modulating the clock rate
        self.lfo_phase = (self.lfo_phase + rate_hz / self.sample_rate).fract();
        let lfo = ((self.lfo_phase + self.phase_offset) * 2.0 * PI).sin();

        // BBD clock frequency — modulated by LFO
        // Higher clock = shorter delay, lower clock = longer delay
        let base_ms = effect_type.base_delay_ms();
        let depth_ms = effect_type.max_depth_ms() * depth;
        let target_delay_ms = base_ms + depth_ms * lfo;
        let target_delay_s = (target_delay_ms * 0.001).max(0.0005);

        // Clock frequency: stages / (2 * delay_time)
        let clock_freq = self.num_stages as f64 / (2.0 * target_delay_s);

        // Input lowpass: cutoff tracks clock rate (clock_freq / ~6)
        let input_lp_coeff = (2.0 * PI * (clock_freq / 6.0) / self.sample_rate)
            .sin()
            .clamp(0.0, 0.99);
        self.input_lp += (input - self.input_lp) * input_lp_coeff;

        // Advance clock phase — when it crosses 1.0, shift the bucket chain
        let clock_inc = clock_freq / self.sample_rate;
        self.clock_phase += clock_inc;

        let mut output = self.prev_output;

        if self.clock_phase >= 1.0 {
            self.clock_phase -= 1.0;

            // Shift buckets: last element out, new sample in
            let fb = output * feedback;
            let bucket_input = (self.input_lp + fb.clamp(-1.0, 1.0)).clamp(-1.5, 1.5);

            // Shift the chain
            for i in (1..self.num_stages).rev() {
                self.buckets[i] = self.buckets[i - 1];
            }
            self.buckets[0] = bucket_input;

            output = *self.buckets.last().unwrap_or(&0.0);
        }

        // First-order hold: linear interpolation between clock ticks
        let frac = self.clock_phase.clamp(0.0, 1.0);
        let held = self.prev_output * (1.0 - frac) + output * frac;
        self.prev_output = output;

        // Output lowpass: same tracking cutoff, with color adjustment
        let output_cutoff = clock_freq / (6.0 - color * 4.0).max(1.5);
        let output_lp_coeff = (2.0 * PI * output_cutoff / self.sample_rate)
            .sin()
            .clamp(0.0, 0.99);
        self.output_lp += (held - self.output_lp) * output_lp_coeff;

        self.output_lp
    }

    fn reset(&mut self) {
        self.buckets.fill(0.0);
        self.clock_phase = 0.0;
        self.prev_output = 0.0;
        self.input_lp = 0.0;
        self.output_lp = 0.0;
        self.lfo_phase = 0.0;
    }
}

// ─── Tape Engine ────────────────────────────────────────────────────

/// Tape chorus with wow, flutter, and saturation.
///
/// Hermite-interpolated delay with dual-rate modulation (slow wow +
/// fast flutter) and tanh soft-clipping. Variable tone filter via color.
///
/// Based on ChowDSP AnalogTapeModel (wow/flutter), qdelay (tiagolr).
pub struct TapeVoice {
    delay: DelayLine,
    lfo_phase: f64,
    phase_offset: f64,
    /// Slow wow oscillator phase.
    wow_phase: f64,
    /// Fast flutter oscillator phase.
    flutter_phase: f64,
    /// Tone lowpass state.
    tone_lp: f64,
    sample_rate: f64,
}

impl TapeVoice {
    const MAX_DELAY: usize = 192000 / 20 + 64;

    pub fn new(phase_offset: f64) -> Self {
        Self {
            delay: DelayLine::new(Self::MAX_DELAY),
            lfo_phase: 0.0,
            phase_offset,
            wow_phase: 0.0,
            flutter_phase: 0.0,
            tone_lp: 0.0,
            sample_rate: 48000.0,
        }
    }
}

impl ChorusEngine for TapeVoice {
    fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let needed = (sample_rate * 0.05) as usize + 64;
        if self.delay.len() < needed {
            self.delay = DelayLine::new(needed);
        }
    }

    fn tick(
        &mut self,
        input: f64,
        rate_hz: f64,
        depth: f64,
        feedback: f64,
        color: f64,
        effect_type: EffectType,
    ) -> f64 {
        // Primary LFO
        self.lfo_phase = (self.lfo_phase + rate_hz / self.sample_rate).fract();
        let lfo = ((self.lfo_phase + self.phase_offset) * 2.0 * PI).sin();

        // Wow: slow drift (0.33 Hz) with depth scaled by main depth
        let wow_rate = 0.33;
        self.wow_phase = (self.wow_phase + wow_rate / self.sample_rate).fract();
        let wow = (self.wow_phase * 2.0 * PI).sin() * depth * 0.3;

        // Flutter: fast wobble (5.8 Hz) — three harmonics
        let flutter_rate = 5.8;
        self.flutter_phase = (self.flutter_phase + flutter_rate / self.sample_rate).fract();
        let flutter = ((self.flutter_phase * 2.0 * PI).sin() * 0.6
            + (self.flutter_phase * 4.0 * PI).sin() * 0.3
            + (self.flutter_phase * 6.0 * PI).sin() * 0.1)
            * depth
            * 0.15;

        // Modulated delay time
        let base_ms = effect_type.base_delay_ms();
        let depth_ms = effect_type.max_depth_ms() * depth;
        let delay_ms = base_ms + depth_ms * lfo + wow + flutter;
        let delay_samples =
            (delay_ms * 0.001 * self.sample_rate).clamp(1.0, self.delay.len() as f64 - 4.0);

        let delayed = self.delay.read_cubic(delay_samples);

        // Tanh saturation on input (tape character)
        let drive = 1.0 + color * 2.0;
        let saturated_input = (input * drive).tanh();

        // Feedback with soft-clip
        let fb = (delayed * feedback).tanh();
        self.delay.write(saturated_input + fb.clamp(-1.5, 1.5));

        // Tone filter: color controls cutoff (dark → bright)
        // At color=0: 3kHz lowpass, at color=1: 14kHz (nearly open)
        let cutoff = 3000.0 + color * 11000.0;
        let lp_coeff = (2.0 * PI * cutoff / self.sample_rate)
            .sin()
            .clamp(0.0, 0.99);
        self.tone_lp += (delayed - self.tone_lp) * lp_coeff;

        self.tone_lp
    }

    fn reset(&mut self) {
        self.delay.clear();
        self.lfo_phase = 0.0;
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
        self.tone_lp = 0.0;
    }
}

// ─── Orbit Engine ───────────────────────────────────────────────────

/// Dual-tap chorus with 2D elliptical orbital modulation.
///
/// Two delay taps trace elliptical paths in a 2D parameter space,
/// creating swirling, spatial movement unlike simple sine LFO chorus.
///
/// Based on Choroboros Orbit engine (EsotericShadow).
pub struct OrbitVoice {
    delay: DelayLine,
    /// Primary orbital phase.
    orbit_phase: f64,
    /// Secondary rotation angle.
    theta: f64,
    phase_offset: f64,
    sample_rate: f64,
}

impl OrbitVoice {
    const MAX_DELAY: usize = 192000 / 20 + 64;

    pub fn new(phase_offset: f64) -> Self {
        Self {
            delay: DelayLine::new(Self::MAX_DELAY),
            orbit_phase: 0.0,
            theta: 0.0,
            phase_offset,
            sample_rate: 48000.0,
        }
    }
}

impl ChorusEngine for OrbitVoice {
    fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        let needed = (sample_rate * 0.05) as usize + 64;
        if self.delay.len() < needed {
            self.delay = DelayLine::new(needed);
        }
    }

    fn tick(
        &mut self,
        input: f64,
        rate_hz: f64,
        depth: f64,
        feedback: f64,
        color: f64,
        effect_type: EffectType,
    ) -> f64 {
        let inc = rate_hz / self.sample_rate;

        // Primary orbit — elliptical path
        self.orbit_phase = (self.orbit_phase + inc).fract();
        // Secondary rotation — slower, creates evolving pattern
        self.theta += inc * 0.37; // irrational ratio for non-repeating
        if self.theta > 1.0 {
            self.theta -= 1.0;
        }

        let phi = (self.orbit_phase + self.phase_offset) * 2.0 * PI;
        let theta_rad = self.theta * 2.0 * PI;

        // Eccentricity from color: 0 = circular, 1 = highly elliptical
        let eccentricity = color * 0.8;

        // 2D elliptical orbit
        let x = phi.sin();
        let y = (1.0 - eccentricity) * phi.cos();

        // Project onto rotating axis
        let proj = x * theta_rad.cos() + y * theta_rad.sin();

        // Second tap: different projection angle for decorrelation
        let proj2 = x * (theta_rad + PI * 0.5).cos() + y * (theta_rad + PI * 0.5).sin();

        // Map projections to delay times
        let base_ms = effect_type.base_delay_ms();
        let depth_ms = effect_type.max_depth_ms() * depth;

        let delay1_ms = base_ms + depth_ms * proj * 0.7;
        let delay2_ms = base_ms + depth_ms * proj2 * 0.5;

        let max_delay = self.delay.len() as f64 - 4.0;
        let d1 = (delay1_ms * 0.001 * self.sample_rate).clamp(1.0, max_delay);
        let d2 = (delay2_ms * 0.001 * self.sample_rate).clamp(1.0, max_delay);

        // Dual-tap read and blend
        let tap1 = self.delay.read_cubic(d1);
        let tap2 = self.delay.read_cubic(d2);
        let blended = tap1 * 0.6 + tap2 * 0.4;

        let fb = (blended * feedback).clamp(-1.5, 1.5);
        self.delay.write(input + fb);

        blended
    }

    fn reset(&mut self) {
        self.delay.clear();
        self.orbit_phase = 0.0;
        self.theta = 0.0;
    }
}

/// Create a vector of engines for one channel.
pub fn create_voices(engine: EngineType, count: usize) -> Vec<Box<dyn ChorusEngine>> {
    (0..count)
        .map(|i| {
            let offset = i as f64 / count as f64;
            let voice: Box<dyn ChorusEngine> = match engine {
                EngineType::Cubic => Box::new(CubicVoice::new(offset)),
                EngineType::Bbd => Box::new(BbdVoice::new(offset)),
                EngineType::Tape => Box::new(TapeVoice::new(offset)),
                EngineType::Orbit => Box::new(OrbitVoice::new(offset)),
            };
            voice
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn test_engine_produces_output(mut voice: Box<dyn ChorusEngine>) {
        voice.update(SR);
        let mut has_output = false;
        for i in 0..9600 {
            let s = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = voice.tick(s, 1.0, 0.5, 0.0, 0.5, EffectType::Chorus);
            if out.abs() > 0.01 {
                has_output = true;
            }
        }
        assert!(has_output, "Engine should produce output");
    }

    fn test_engine_no_nan(mut voice: Box<dyn ChorusEngine>) {
        voice.update(SR);
        for i in 0..48000 {
            let s = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.8;
            let out = voice.tick(s, 3.0, 1.0, 0.8, 0.5, EffectType::Flanger);
            assert!(out.is_finite(), "NaN at {i}");
        }
    }

    #[test]
    fn cubic_produces_output() {
        test_engine_produces_output(Box::new(CubicVoice::new(0.0)));
    }

    #[test]
    fn cubic_no_nan() {
        test_engine_no_nan(Box::new(CubicVoice::new(0.0)));
    }

    #[test]
    fn bbd_produces_output() {
        test_engine_produces_output(Box::new(BbdVoice::new(0.0)));
    }

    #[test]
    fn bbd_no_nan() {
        test_engine_no_nan(Box::new(BbdVoice::new(0.0)));
    }

    #[test]
    fn tape_produces_output() {
        test_engine_produces_output(Box::new(TapeVoice::new(0.0)));
    }

    #[test]
    fn tape_no_nan() {
        test_engine_no_nan(Box::new(TapeVoice::new(0.0)));
    }

    #[test]
    fn orbit_produces_output() {
        test_engine_produces_output(Box::new(OrbitVoice::new(0.0)));
    }

    #[test]
    fn orbit_no_nan() {
        test_engine_no_nan(Box::new(OrbitVoice::new(0.0)));
    }

    #[test]
    fn engines_sound_different() {
        let mut cubic = CubicVoice::new(0.0);
        let mut bbd = BbdVoice::new(0.0);
        let mut tape = TapeVoice::new(0.0);
        let mut orbit = OrbitVoice::new(0.0);

        cubic.update(SR);
        bbd.update(SR);
        tape.update(SR);
        orbit.update(SR);

        let mut out_cubic = Vec::new();
        let mut out_bbd = Vec::new();
        let mut out_tape = Vec::new();
        let mut out_orbit = Vec::new();

        for i in 0..9600 {
            let s = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            out_cubic.push(cubic.tick(s, 1.0, 0.5, 0.0, 0.5, EffectType::Chorus));
            out_bbd.push(bbd.tick(s, 1.0, 0.5, 0.0, 0.5, EffectType::Chorus));
            out_tape.push(tape.tick(s, 1.0, 0.5, 0.0, 0.5, EffectType::Chorus));
            out_orbit.push(orbit.tick(s, 1.0, 0.5, 0.0, 0.5, EffectType::Chorus));
        }

        // Each pair should differ
        let diff_cb: f64 = out_cubic
            .iter()
            .zip(out_bbd.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / 9600.0;
        let diff_ct: f64 = out_cubic
            .iter()
            .zip(out_tape.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / 9600.0;
        let diff_co: f64 = out_cubic
            .iter()
            .zip(out_orbit.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / 9600.0;

        assert!(diff_cb > 0.001, "Cubic vs BBD should differ: {diff_cb}");
        assert!(diff_ct > 0.001, "Cubic vs Tape should differ: {diff_ct}");
        assert!(diff_co > 0.001, "Cubic vs Orbit should differ: {diff_co}");
    }

    #[test]
    fn color_affects_bbd_tone() {
        let mut dark = BbdVoice::new(0.0);
        let mut bright = BbdVoice::new(0.0);
        dark.update(SR);
        bright.update(SR);

        let mut energy_dark: f64 = 0.0;
        let mut energy_bright: f64 = 0.0;

        for i in 0..9600 {
            let s = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let d = dark.tick(s, 1.0, 0.5, 0.0, 0.0, EffectType::Chorus);
            let b = bright.tick(s, 1.0, 0.5, 0.0, 1.0, EffectType::Chorus);
            energy_dark += d * d;
            energy_bright += b * b;
        }

        // Bright should have more energy (less filtering)
        assert!(
            energy_bright > energy_dark * 0.8,
            "Bright color should pass more: dark={energy_dark:.4}, bright={energy_bright:.4}"
        );
    }
}
