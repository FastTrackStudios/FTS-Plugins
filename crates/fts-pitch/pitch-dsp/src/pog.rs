//! Polyphonic Octave Generator — ERB filter bank phase-scaling pitch shifter.
//!
//! Inspired by the EHX POG pedal, this uses a bank of 80 complex bandpass
//! filters at ERB-spaced frequencies. Each filter produces an analytic signal
//! whose phase is doubled (octave up) or halved (octave down) independently.
//! This is inherently polyphonic — no pitch detection needed, full chords shift
//! cleanly.
//!
//! Based on the ERB-PS2 method from Thuillier (2016) "Real-Time Polyphonic
//! Octave Doubling for the Guitar" and the open-source terrarium-poly-octave
//! implementation by schult.
//!
//! Latency: 0 samples (filter group delay only, no buffering).
//! Character: Clean, polyphonic octave. EHX POG / Micro POG style.

use std::f64::consts::{PI, SQRT_2, TAU};

/// Number of complex bandpass filters in the ERB filter bank.
const NUM_FILTERS: usize = 80;

/// Minimum frequency covered by the filter bank (Hz).
const FREQ_MIN: f64 = 40.0;

/// Maximum frequency as a fraction of sample rate (stay well below Nyquist).
const FREQ_MAX_RATIO: f64 = 0.40;

/// Which octave shift to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OctaveShift {
    /// Two octaves down (−24 semitones).
    Sub2,
    /// One octave down (−12 semitones).
    Sub1,
    /// One octave up (+12 semitones).
    Up1,
    /// Two octaves up (+24 semitones).
    Up2,
}

impl OctaveShift {
    /// Map a semitone value to the nearest octave shift.
    /// Negative → down, positive → up. ±18 threshold for double octave.
    pub fn from_semitones(st: f64) -> Self {
        if st <= -18.0 {
            Self::Sub2
        } else if st < 0.0 {
            Self::Sub1
        } else if st < 18.0 {
            Self::Up1
        } else {
            Self::Up2
        }
    }
}

// ── Complex bandpass filter ─────────────────────────────────────────

/// A single 2nd-order complex bandpass filter.
///
/// Derived from a Butterworth lowpass prototype by substituting
/// z⁻¹ → z⁻¹·e^(−jω₀) to shift the passband to center frequency ω₀.
/// Processes real input, produces complex (analytic) output.
struct ComplexBpf {
    // Feedforward: b0 is real; b1, b2 are complex (frequency-shifted).
    b0: f64,
    b1_re: f64,
    b1_im: f64,
    b2_re: f64,
    b2_im: f64,
    // Feedback: a1, a2 are complex.
    a1_re: f64,
    a1_im: f64,
    a2_re: f64,
    a2_im: f64,
    // Delayed real inputs (Direct Form I).
    x1: f64,
    x2: f64,
    // Delayed complex outputs.
    y1_re: f64,
    y1_im: f64,
    y2_re: f64,
    y2_im: f64,
}

impl ComplexBpf {
    /// Design a complex BPF centered at `center_freq` with given `bandwidth`.
    fn design(center_freq: f64, bandwidth: f64, sample_rate: f64) -> Self {
        // 2nd-order Butterworth LPF prototype, cutoff = bandwidth/2.
        let omega_c = (PI * bandwidth / sample_rate).tan();
        let omega_c2 = omega_c * omega_c;
        let norm = 1.0 / (1.0 + SQRT_2 * omega_c + omega_c2);

        let b0 = omega_c2 * norm;
        let b1 = 2.0 * b0;
        let b2 = b0;
        let a1 = 2.0 * (omega_c2 - 1.0) * norm;
        let a2 = (1.0 - SQRT_2 * omega_c + omega_c2) * norm;

        // Frequency-shift rotation factors: e^(−jω₀), e^(−j2ω₀).
        let w0 = TAU * center_freq / sample_rate;
        let (s1, c1) = w0.sin_cos();
        let (s2, c2) = (2.0 * w0).sin_cos();

        Self {
            b0,
            b1_re: b1 * c1,
            b1_im: -b1 * s1,
            b2_re: b2 * c2,
            b2_im: -b2 * s2,
            a1_re: a1 * c1,
            a1_im: -a1 * s1,
            a2_re: a2 * c2,
            a2_im: -a2 * s2,
            x1: 0.0,
            x2: 0.0,
            y1_re: 0.0,
            y1_im: 0.0,
            y2_re: 0.0,
            y2_im: 0.0,
        }
    }

    /// Process one real sample → complex (re, im) analytic output.
    #[inline]
    fn tick(&mut self, input: f64) -> (f64, f64) {
        // Direct Form I: y = b0·x + B1·x₁ + B2·x₂ − A1·y₁ − A2·y₂
        // where B1, B2, A1, A2 are complex and x is real.
        let y_re = self.b0 * input + self.b1_re * self.x1 + self.b2_re * self.x2
            - (self.a1_re * self.y1_re - self.a1_im * self.y1_im)
            - (self.a2_re * self.y2_re - self.a2_im * self.y2_im);

        let y_im = self.b1_im * self.x1 + self.b2_im * self.x2
            - (self.a1_re * self.y1_im + self.a1_im * self.y1_re)
            - (self.a2_re * self.y2_im + self.a2_im * self.y2_re);

        self.x2 = self.x1;
        self.x1 = input;
        self.y2_re = self.y1_re;
        self.y2_im = self.y1_im;
        self.y1_re = y_re;
        self.y1_im = y_im;

        (y_re, y_im)
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1_re = 0.0;
        self.y1_im = 0.0;
        self.y2_re = 0.0;
        self.y2_im = 0.0;
    }
}

// ── Octave-down sign tracking ───────────────────────────────────────

/// Per-filter state for half-angle sign continuity tracking.
struct SignTracker {
    prev_cos: f64,
    prev_sin: f64,
    sign: f64,
}

impl SignTracker {
    fn new() -> Self {
        Self {
            prev_cos: 1.0,
            prev_sin: 0.0,
            sign: 1.0,
        }
    }

    fn reset(&mut self) {
        self.prev_cos = 1.0;
        self.prev_sin = 0.0;
        self.sign = 1.0;
    }
}

// ── Phase-scaling functions ─────────────────────────────────────────

/// Octave up (phase doubling): Re(z²/|z|) = (a² − b²) / √(a² + b²).
/// Preserves amplitude: output magnitude = input magnitude.
#[inline]
fn octave_up_real(re: f64, im: f64) -> f64 {
    let mag_sq = re * re + im * im;
    if mag_sq < 1e-30 {
        return 0.0;
    }
    (re * re - im * im) / mag_sq.sqrt()
}

/// Octave up returning complex z²/|z| for cascading to +2 octaves.
#[inline]
fn octave_up_complex(re: f64, im: f64) -> (f64, f64) {
    let mag_sq = re * re + im * im;
    if mag_sq < 1e-30 {
        return (0.0, 0.0);
    }
    let inv_mag = 1.0 / mag_sq.sqrt();
    ((re * re - im * im) * inv_mag, 2.0 * re * im * inv_mag)
}

/// Octave down (phase halving) using half-angle formulas.
/// Returns the real part, amplitude-preserving (output mag ≈ input mag).
#[inline]
fn octave_down_real(re: f64, im: f64, st: &mut SignTracker) -> f64 {
    let mag_sq = re * re + im * im;
    if mag_sq < 1e-30 {
        return 0.0;
    }
    let mag = mag_sq.sqrt();

    // Half-angle: cos(θ/2) = √((1+cosθ)/2), sin(θ/2) = √((1−cosθ)/2)
    let cos_half = ((mag + re) * 0.5).max(0.0).sqrt();
    let sin_half_abs = ((mag - re) * 0.5).max(0.0).sqrt();
    let sin_half = if im >= 0.0 {
        sin_half_abs
    } else {
        -sin_half_abs
    };

    // Sign tracking: flip sign when phase wraps past ±π.
    let dot = cos_half * st.prev_cos + sin_half * st.prev_sin;
    if dot < 0.0 {
        st.sign = -st.sign;
    }
    st.prev_cos = cos_half;
    st.prev_sin = sin_half;

    // Multiply by √mag to preserve amplitude: √mag · cos(θ/2) → mag·cos(θ/2)
    // when combined with the fact that cos_half already contains √((mag+re)/2).
    // Full derivation: √mag · √((mag+re)/2) = √(mag·(mag+re)/2) = mag·cos(θ/2).
    st.sign * cos_half * mag.sqrt()
}

/// Octave down returning complex result for cascading to −2 octaves.
#[inline]
fn octave_down_complex(re: f64, im: f64, st: &mut SignTracker) -> (f64, f64) {
    let mag_sq = re * re + im * im;
    if mag_sq < 1e-30 {
        return (0.0, 0.0);
    }
    let mag = mag_sq.sqrt();

    let cos_half = ((mag + re) * 0.5).max(0.0).sqrt();
    let sin_half_abs = ((mag - re) * 0.5).max(0.0).sqrt();
    let sin_half = if im >= 0.0 {
        sin_half_abs
    } else {
        -sin_half_abs
    };

    let dot = cos_half * st.prev_cos + sin_half * st.prev_sin;
    if dot < 0.0 {
        st.sign = -st.sign;
    }
    st.prev_cos = cos_half;
    st.prev_sin = sin_half;

    let amp = mag.sqrt();
    (st.sign * cos_half * amp, st.sign * sin_half * amp)
}

// ── ERB scale ───────────────────────────────────────────────────────

/// Convert frequency (Hz) to ERB-rate scale (Glasberg & Moore 1990).
fn erb_rate(f: f64) -> f64 {
    21.4 * (0.00437 * f + 1.0).log10()
}

/// Convert ERB-rate back to frequency (Hz).
fn erb_rate_to_freq(e: f64) -> f64 {
    (10.0f64.powf(e / 21.4) - 1.0) / 0.00437
}

// ── PolyOctave ──────────────────────────────────────────────────────

/// Simple 1st-order filter for post-processing shifted output.
struct Filter1 {
    coeff: f64,
    y1: f64,
    is_hpf: bool,
}

impl Filter1 {
    fn new_lpf() -> Self {
        Self {
            coeff: 0.0,
            y1: 0.0,
            is_hpf: false,
        }
    }

    fn new_hpf() -> Self {
        Self {
            coeff: 0.0,
            y1: 0.0,
            is_hpf: true,
        }
    }

    fn set_cutoff(&mut self, freq: f64, sample_rate: f64) {
        let w = (PI * freq / sample_rate).tan();
        self.coeff = w / (1.0 + w);
    }

    #[inline]
    fn tick(&mut self, input: f64) -> f64 {
        self.y1 += self.coeff * (input - self.y1);
        if self.is_hpf {
            input - self.y1
        } else {
            self.y1
        }
    }

    fn reset(&mut self) {
        self.y1 = 0.0;
    }
}

/// Polyphonic Octave Generator using ERB filter bank phase scaling.
pub struct PolyOctave {
    /// Which octave shift to apply.
    pub shift: OctaveShift,
    /// Dry/wet mix: 0.0 = dry, 1.0 = wet.
    pub mix: f64,

    filters: Vec<ComplexBpf>,
    down_state: Vec<SignTracker>,
    /// Second sign-tracking layer for sub2 (cascaded half-angle).
    down2_state: Vec<SignTracker>,

    sample_rate: f64,
    /// Per-shift gain normalization (empirically calibrated).
    gain_sub2: f64,
    gain_sub1: f64,
    gain_up1: f64,
    gain_up2: f64,
    /// Post-LPF for Sub2 down-shift artifact removal.
    post_lpf_sub2: Filter1,
    /// Post-HPF for up-shift sub/low leakage removal.
    post_hpf_up1: Filter1,
    post_hpf_up2: Filter1,
    /// DC blocker state.
    dc_x1: f64,
    dc_y1: f64,
    dc_coeff: f64,
}

impl PolyOctave {
    pub fn new() -> Self {
        Self {
            shift: OctaveShift::Sub1,
            mix: 1.0,
            filters: Vec::new(),
            down_state: Vec::new(),
            down2_state: Vec::new(),
            sample_rate: 48000.0,
            gain_sub2: 1.0,
            gain_sub1: 1.0,
            gain_up1: 1.0,
            gain_up2: 1.0,
            post_lpf_sub2: Filter1::new_lpf(),
            post_hpf_up1: Filter1::new_hpf(),
            post_hpf_up2: Filter1::new_hpf(),
            dc_x1: 0.0,
            dc_y1: 0.0,
            dc_coeff: 0.9995,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;

        // DC blocker: ~10 Hz cutoff.
        self.dc_coeff = 1.0 - TAU * 10.0 / sample_rate;

        // Compute ERB-spaced center frequencies.
        let freq_max = (sample_rate * FREQ_MAX_RATIO).min(20000.0);
        let erb_lo = erb_rate(FREQ_MIN);
        let erb_hi = erb_rate(freq_max);
        let erb_step = (erb_hi - erb_lo) / (NUM_FILTERS - 1) as f64;

        let mut centers = Vec::with_capacity(NUM_FILTERS);
        for i in 0..NUM_FILTERS {
            centers.push(erb_rate_to_freq(erb_lo + i as f64 * erb_step));
        }

        // Build filters with bandwidths from adjacent spacing.
        self.filters.clear();
        self.down_state.clear();
        self.down2_state.clear();

        for i in 0..NUM_FILTERS {
            let bw = if i == 0 {
                centers[1] - centers[0]
            } else if i == NUM_FILTERS - 1 {
                centers[NUM_FILTERS - 1] - centers[NUM_FILTERS - 2]
            } else {
                let left = centers[i] - centers[i - 1];
                let right = centers[i + 1] - centers[i];
                2.0 * left * right / (left + right)
            };

            self.filters
                .push(ComplexBpf::design(centers[i], bw, sample_rate));
            self.down_state.push(SignTracker::new());
            self.down2_state.push(SignTracker::new());
        }

        // Post-LPF for Sub2 only: 1st-order at 0.18×SR (~8 kHz @ 44.1k).
        // Sub2 (÷4) generates more HF phase-scaling noise. Sub1 has no LPF —
        // any LPF steep enough to cut presence excess also destroys the air band.
        self.post_lpf_sub2
            .set_cutoff(sample_rate * 0.18, sample_rate);

        // Post-HPF for up-shifts: remove sub/low leakage that the filter bank
        // passes through un-shifted. Harmonitron cuts this aggressively.
        // Up1 (+12st): HPF at ~250 Hz — at 1st-order, gives ~-21 dB at 20 Hz.
        // Up2 (+24st): HPF at ~120 Hz — gentler since two_oct_up has less leakage.
        self.post_hpf_up1.set_cutoff(250.0, sample_rate);
        self.post_hpf_up2.set_cutoff(120.0, sample_rate);

        // Empirical per-shift gain calibration: process broadband noise through
        // the actual phase-scaling paths and measure output level.
        // Phase scaling changes inter-filter interference patterns, so the
        // passthrough-calibrated gain doesn't apply to shifted output.
        self.gain_sub2 = self.calibrate_shift_gain(OctaveShift::Sub2);
        self.gain_sub1 = self.calibrate_shift_gain(OctaveShift::Sub1);
        self.gain_up1 = self.calibrate_shift_gain(OctaveShift::Up1);
        self.gain_up2 = self.calibrate_shift_gain(OctaveShift::Up2);
    }

    /// Empirically measure the gain for a given shift by processing broadband noise.
    /// Uses deterministic pseudo-random noise to excite all 80 filters simultaneously,
    /// which captures inter-filter interference patterns that single test tones miss.
    fn calibrate_shift_gain(&mut self, shift: OctaveShift) -> f64 {
        // Reset filter states for clean measurement.
        for f in &mut self.filters {
            f.reset();
        }
        for s in &mut self.down_state {
            s.reset();
        }
        for s in &mut self.down2_state {
            s.reset();
        }

        let sr = self.sample_rate;
        let n = (sr * 0.5) as usize; // 500ms of noise
        let warmup = (sr * 0.1) as usize; // 100ms warmup (filters need to fill)
        let input_amp = 0.5;

        // Simple deterministic PRNG (xorshift64).
        let mut rng_state: u64 = 0xDEADBEEF12345678;
        let mut next_sample = || -> f64 {
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 7;
            rng_state ^= rng_state << 17;
            (rng_state as f64 / u64::MAX as f64 - 0.5) * 2.0 * input_amp
        };

        let mut input_sum_sq = 0.0;
        let mut output_sum_sq = 0.0;
        let mut count = 0;

        for i in 0..n {
            let input = next_sample();
            let out = self.process_shifted(input, shift);
            if i >= warmup {
                input_sum_sq += input * input;
                output_sum_sq += out * out;
                count += 1;
            }
        }

        // Reset everything after calibration.
        for f in &mut self.filters {
            f.reset();
        }
        for s in &mut self.down_state {
            s.reset();
        }
        for s in &mut self.down2_state {
            s.reset();
        }

        if count > 0 && output_sum_sq > 1e-20 {
            let input_rms = (input_sum_sq / count as f64).sqrt();
            let output_rms = (output_sum_sq / count as f64).sqrt();
            input_rms / output_rms
        } else {
            1.0
        }
    }

    /// Run the filter bank + phase scaling for a single sample (no gain/DC/mix).
    #[inline]
    fn process_shifted(&mut self, input: f64, shift: OctaveShift) -> f64 {
        let mut sum = 0.0;
        match shift {
            OctaveShift::Up1 => {
                for filt in &mut self.filters {
                    let (re, im) = filt.tick(input);
                    sum += octave_up_real(re, im);
                }
            }
            OctaveShift::Up2 => {
                for filt in &mut self.filters {
                    let (re, im) = filt.tick(input);
                    let (up_re, up_im) = octave_up_complex(re, im);
                    sum += octave_up_real(up_re, up_im);
                }
            }
            OctaveShift::Sub1 => {
                for (filt, st) in self.filters.iter_mut().zip(self.down_state.iter_mut()) {
                    let (re, im) = filt.tick(input);
                    sum += octave_down_real(re, im, st);
                }
            }
            OctaveShift::Sub2 => {
                for ((filt, st1), st2) in self
                    .filters
                    .iter_mut()
                    .zip(self.down_state.iter_mut())
                    .zip(self.down2_state.iter_mut())
                {
                    let (re, im) = filt.tick(input);
                    let (d_re, d_im) = octave_down_complex(re, im, st1);
                    sum += octave_down_real(d_re, d_im, st2);
                }
            }
        }
        sum
    }

    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f.reset();
        }
        for s in &mut self.down_state {
            s.reset();
        }
        for s in &mut self.down2_state {
            s.reset();
        }
        self.post_lpf_sub2.reset();
        self.post_hpf_up1.reset();
        self.post_hpf_up2.reset();
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
    }

    /// Process one sample. Returns the mixed output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        let mut sum = 0.0;

        match self.shift {
            OctaveShift::Up1 => {
                for filt in &mut self.filters {
                    let (re, im) = filt.tick(input);
                    sum += octave_up_real(re, im);
                }
            }
            OctaveShift::Up2 => {
                for filt in &mut self.filters {
                    let (re, im) = filt.tick(input);
                    let (up_re, up_im) = octave_up_complex(re, im);
                    sum += octave_up_real(up_re, up_im);
                }
            }
            OctaveShift::Sub1 => {
                for (filt, st) in self.filters.iter_mut().zip(self.down_state.iter_mut()) {
                    let (re, im) = filt.tick(input);
                    sum += octave_down_real(re, im, st);
                }
            }
            OctaveShift::Sub2 => {
                for ((filt, st1), st2) in self
                    .filters
                    .iter_mut()
                    .zip(self.down_state.iter_mut())
                    .zip(self.down2_state.iter_mut())
                {
                    let (re, im) = filt.tick(input);
                    let (d_re, d_im) = octave_down_complex(re, im, st1);
                    sum += octave_down_real(d_re, d_im, st2);
                }
            }
        }

        // Apply per-shift gain normalization and post-filtering.
        let wet = match self.shift {
            OctaveShift::Sub2 => self.post_lpf_sub2.tick(sum) * self.gain_sub2,
            OctaveShift::Sub1 => sum * self.gain_sub1,
            OctaveShift::Up1 => self.post_hpf_up1.tick(sum) * self.gain_up1,
            OctaveShift::Up2 => self.post_hpf_up2.tick(sum) * self.gain_up2,
        };

        // DC blocker on wet signal.
        let dc_out = wet - self.dc_x1 + self.dc_coeff * self.dc_y1;
        self.dc_x1 = wet;
        self.dc_y1 = dc_out;

        input * (1.0 - self.mix) + dc_out * self.mix
    }

    pub fn latency(&self) -> usize {
        // No explicit buffering — only inherent IIR filter group delay.
        0
    }
}

impl Default for PolyOctave {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn make_pog(shift: OctaveShift) -> PolyOctave {
        let mut p = PolyOctave::new();
        p.shift = shift;
        p.mix = 1.0;
        p.update(SR);
        p
    }

    fn sine(freq: f64, i: usize) -> f64 {
        (TAU * freq * i as f64 / SR).sin() * 0.5
    }

    #[test]
    fn zero_latency() {
        assert_eq!(make_pog(OctaveShift::Sub1).latency(), 0);
    }

    #[test]
    fn silence_in_silence_out() {
        let mut p = make_pog(OctaveShift::Sub1);
        for _ in 0..4800 {
            let out = p.tick(0.0);
            assert!(out.abs() < 1e-10, "Should be silent: {out}");
        }
    }

    #[test]
    fn no_nan_all_shifts() {
        for shift in [
            OctaveShift::Sub2,
            OctaveShift::Sub1,
            OctaveShift::Up1,
            OctaveShift::Up2,
        ] {
            let mut p = make_pog(shift);
            for i in 0..96000 {
                let out = p.tick(sine(220.0, i));
                assert!(out.is_finite(), "{shift:?} NaN at sample {i}");
            }
        }
    }

    #[test]
    fn produces_output_all_shifts() {
        for shift in [
            OctaveShift::Sub2,
            OctaveShift::Sub1,
            OctaveShift::Up1,
            OctaveShift::Up2,
        ] {
            let mut p = make_pog(shift);
            let mut energy = 0.0;
            for i in 0..48000 {
                let out = p.tick(sine(220.0, i));
                if i > 4800 {
                    energy += out * out;
                }
            }
            assert!(
                energy > 0.1,
                "{shift:?} should produce output: energy={energy}"
            );
        }
    }

    #[test]
    fn octave_up_shifts_frequency_higher() {
        let mut p = make_pog(OctaveShift::Up1);
        let n = 96000;
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            output.push(p.tick(sine(440.0, i)));
        }

        // Measure spectral energy: expect peak near 880 Hz (octave up from 440).
        let start = n / 2;
        let fft_size = 8192;
        let signal = &output[start..start + fft_size];
        let target_bin = (880.0 * fft_size as f64 / SR) as usize;
        let input_bin = (440.0 * fft_size as f64 / SR) as usize;

        let mag_at = |bin: usize| -> f64 {
            let omega = TAU * bin as f64 / fft_size as f64;
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (i, &s) in signal.iter().enumerate() {
                let w = 0.5 * (1.0 - (TAU * i as f64 / fft_size as f64).cos());
                re += s * w * (omega * i as f64).cos();
                im -= s * w * (omega * i as f64).sin();
            }
            (re * re + im * im).sqrt()
        };

        let mag_target = mag_at(target_bin);
        let mag_input = mag_at(input_bin);

        assert!(
            mag_target > mag_input * 2.0,
            "880 Hz should dominate over 440 Hz: mag_880={mag_target:.1} mag_440={mag_input:.1}"
        );
    }

    #[test]
    fn octave_down_shifts_frequency_lower() {
        let mut p = make_pog(OctaveShift::Sub1);
        let n = 96000;
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            output.push(p.tick(sine(440.0, i)));
        }

        let start = n / 2;
        let fft_size = 8192;
        let signal = &output[start..start + fft_size];
        let target_bin = (220.0 * fft_size as f64 / SR) as usize;
        let input_bin = (440.0 * fft_size as f64 / SR) as usize;

        let mag_at = |bin: usize| -> f64 {
            let omega = TAU * bin as f64 / fft_size as f64;
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (i, &s) in signal.iter().enumerate() {
                let w = 0.5 * (1.0 - (TAU * i as f64 / fft_size as f64).cos());
                re += s * w * (omega * i as f64).cos();
                im -= s * w * (omega * i as f64).sin();
            }
            (re * re + im * im).sqrt()
        };

        let mag_target = mag_at(target_bin);
        let mag_input = mag_at(input_bin);

        assert!(
            mag_target > mag_input * 2.0,
            "220 Hz should dominate over 440 Hz: mag_220={mag_target:.1} mag_440={mag_input:.1}"
        );
    }

    #[test]
    fn polyphonic_preserves_both_notes() {
        // Feed a two-note chord (A4=440 + E5=660) and verify both notes
        // appear shifted in the output.
        let mut p = make_pog(OctaveShift::Up1);
        let n = 96000;
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            let chord = sine(440.0, i) + sine(660.0, i);
            output.push(p.tick(chord));
        }

        let start = n / 2;
        let fft_size = 8192;
        let signal = &output[start..start + fft_size];

        let mag_at = |freq: f64| -> f64 {
            let bin = (freq * fft_size as f64 / SR) as usize;
            let omega = TAU * bin as f64 / fft_size as f64;
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (i, &s) in signal.iter().enumerate() {
                let w = 0.5 * (1.0 - (TAU * i as f64 / fft_size as f64).cos());
                re += s * w * (omega * i as f64).cos();
                im -= s * w * (omega * i as f64).sin();
            }
            (re * re + im * im).sqrt()
        };

        let mag_880 = mag_at(880.0); // A4 → A5
        let mag_1320 = mag_at(1320.0); // E5 → E6
        let mag_noise = mag_at(1000.0); // Should be low

        assert!(
            mag_880 > mag_noise * 3.0,
            "880 Hz (shifted A4) should be present: {mag_880:.1} vs noise {mag_noise:.1}"
        );
        assert!(
            mag_1320 > mag_noise * 3.0,
            "1320 Hz (shifted E5) should be present: {mag_1320:.1} vs noise {mag_noise:.1}"
        );
    }

    #[test]
    fn dry_wet_mix() {
        let mut p = make_pog(OctaveShift::Sub1);
        p.mix = 0.0;

        for i in 0..4800 {
            let input = sine(440.0, i);
            let out = p.tick(input);
            assert!(
                (out - input).abs() < 1e-10,
                "Mix=0 should pass dry at sample {i}"
            );
        }
    }

    #[test]
    fn from_semitones_mapping() {
        assert_eq!(OctaveShift::from_semitones(-24.0), OctaveShift::Sub2);
        assert_eq!(OctaveShift::from_semitones(-18.0), OctaveShift::Sub2);
        assert_eq!(OctaveShift::from_semitones(-12.0), OctaveShift::Sub1);
        assert_eq!(OctaveShift::from_semitones(-1.0), OctaveShift::Sub1);
        assert_eq!(OctaveShift::from_semitones(0.0), OctaveShift::Up1);
        assert_eq!(OctaveShift::from_semitones(12.0), OctaveShift::Up1);
        assert_eq!(OctaveShift::from_semitones(18.0), OctaveShift::Up2);
        assert_eq!(OctaveShift::from_semitones(24.0), OctaveShift::Up2);
    }

    #[test]
    fn two_octave_down_shifts_to_quarter_frequency() {
        let mut p = make_pog(OctaveShift::Sub2);
        let n = 96000;
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            output.push(p.tick(sine(440.0, i)));
        }

        let start = n / 2;
        let fft_size = 8192;
        let signal = &output[start..start + fft_size];

        let mag_at = |freq: f64| -> f64 {
            let bin = (freq * fft_size as f64 / SR) as usize;
            let omega = TAU * bin as f64 / fft_size as f64;
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (i, &s) in signal.iter().enumerate() {
                let w = 0.5 * (1.0 - (TAU * i as f64 / fft_size as f64).cos());
                re += s * w * (omega * i as f64).cos();
                im -= s * w * (omega * i as f64).sin();
            }
            (re * re + im * im).sqrt()
        };

        let mag_110 = mag_at(110.0); // Target: 440/4 = 110 Hz
        let mag_220 = mag_at(220.0); // One octave down (should NOT dominate)
        let mag_440 = mag_at(440.0); // Original (should NOT dominate)

        eprintln!(
            "Sub2 440Hz input: mag_110={mag_110:.1}, mag_220={mag_220:.1}, mag_440={mag_440:.1}"
        );

        assert!(
            mag_110 > mag_220 * 2.0,
            "110 Hz (two oct down) should dominate over 220 Hz (one oct down): \
             mag_110={mag_110:.1} mag_220={mag_220:.1}"
        );
        assert!(
            mag_110 > mag_440 * 2.0,
            "110 Hz should dominate over 440 Hz: mag_110={mag_110:.1} mag_440={mag_440:.1}"
        );
    }

    #[test]
    fn two_octave_up_shifts_to_quadruple_frequency() {
        let mut p = make_pog(OctaveShift::Up2);
        let n = 96000;
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            output.push(p.tick(sine(440.0, i)));
        }

        let start = n / 2;
        let fft_size = 8192;
        let signal = &output[start..start + fft_size];

        let mag_at = |freq: f64| -> f64 {
            let bin = (freq * fft_size as f64 / SR) as usize;
            let omega = TAU * bin as f64 / fft_size as f64;
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (i, &s) in signal.iter().enumerate() {
                let w = 0.5 * (1.0 - (TAU * i as f64 / fft_size as f64).cos());
                re += s * w * (omega * i as f64).cos();
                im -= s * w * (omega * i as f64).sin();
            }
            (re * re + im * im).sqrt()
        };

        let mag_1760 = mag_at(1760.0); // Target: 440*4 = 1760 Hz
        let mag_880 = mag_at(880.0); // One octave up
        let mag_440 = mag_at(440.0); // Original

        eprintln!(
            "Up2 440Hz input: mag_1760={mag_1760:.1}, mag_880={mag_880:.1}, mag_440={mag_440:.1}"
        );

        assert!(
            mag_1760 > mag_880 * 2.0,
            "1760 Hz (two oct up) should dominate over 880 Hz (one oct up): \
             mag_1760={mag_1760:.1} mag_880={mag_880:.1}"
        );
    }

    #[test]
    fn calibration_gain_is_reasonable() {
        let p = make_pog(OctaveShift::Up1);
        for (name, gain) in [
            ("sub2", p.gain_sub2),
            ("sub1", p.gain_sub1),
            ("up1", p.gain_up1),
            ("up2", p.gain_up2),
        ] {
            assert!(
                gain > 0.01 && gain < 100.0,
                "Gain {name} should be reasonable: {gain}",
            );
        }
    }

    #[test]
    fn output_level_near_unity() {
        // Verify that calibrated POG output RMS ≈ input RMS for each shift.
        for shift in [
            OctaveShift::Sub2,
            OctaveShift::Sub1,
            OctaveShift::Up1,
            OctaveShift::Up2,
        ] {
            let mut p = make_pog(shift);
            let n = 96000;
            let warmup = 24000;
            let mut in_sq = 0.0;
            let mut out_sq = 0.0;
            let mut count = 0;

            for i in 0..n {
                let input = sine(440.0, i);
                let out = p.tick(input);
                if i >= warmup {
                    in_sq += input * input;
                    out_sq += out * out;
                    count += 1;
                }
            }

            let in_rms = (in_sq / count as f64).sqrt();
            let out_rms = (out_sq / count as f64).sqrt();
            let ratio_db = 20.0 * (out_rms / in_rms).log10();
            // Post-LPF on down shifts can boost low-frequency test tones
            // since calibration is measured pre-LPF. Allow wider tolerance.
            assert!(
                ratio_db.abs() < 8.0,
                "{shift:?}: output {ratio_db:+.1} dB from unity",
            );
        }
    }

    #[test]
    fn different_sample_rates() {
        for &sr in &[44100.0, 48000.0, 96000.0] {
            let mut p = PolyOctave::new();
            p.shift = OctaveShift::Up1;
            p.mix = 1.0;
            p.update(sr);

            let mut energy = 0.0;
            for i in 0..((sr * 1.0) as usize) {
                let input = (TAU * 440.0 * i as f64 / sr).sin() * 0.5;
                let out = p.tick(input);
                if i > (sr * 0.1) as usize {
                    energy += out * out;
                }
                assert!(out.is_finite(), "NaN at sr={sr}, sample {i}");
            }
            assert!(energy > 0.1, "No output at sr={sr}: energy={energy}");
        }
    }
}
