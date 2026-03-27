//! Polyphonic Octave Generator — STFT phase-vocoder octave shifter.
//!
//! Uses an overlap-add STFT to perform exact octave shifts by scaling
//! spectral bin phases. This approach gives near-perfect spectral
//! reconstruction compared to filter-bank methods, at the cost of latency.
//!
//! For octave up (×2): bin k's content is placed at bin 2k with phase doubled.
//! For octave down (÷2): bin k's content is placed at bin k/2 with phase halved.
//! Two-octave shifts apply the operation twice.
//!
//! Latency: FFT_SIZE samples (2048 @ 44.1 kHz ≈ 46 ms).
//! Character: Clean, transparent polyphonic octave shift.

use std::f64::consts::{PI, TAU};

use fts_dsp::biquad::{Biquad, FilterType};
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

/// FFT size for STFT processing.
const FFT_SIZE: usize = 2048;

/// Overlap factor (4 = 75% overlap, hop = FFT_SIZE/4).
const OVERLAP: usize = 4;

/// Hop size in samples.
const HOP_SIZE: usize = FFT_SIZE / OVERLAP;

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

/// Polyphonic Octave Generator using STFT phase vocoder.
pub struct PolyOctave {
    /// Which octave shift to apply.
    pub shift: OctaveShift,
    /// Dry/wet mix: 0.0 = dry, 1.0 = wet.
    pub mix: f64,

    // FFT plan and scratch buffers.
    fft: Arc<dyn RealToComplex<f64>>,
    ifft: Arc<dyn ComplexToReal<f64>>,

    /// Analysis window (Hann).
    window: Vec<f64>,

    /// Circular input buffer. We write incoming samples here and read
    /// FFT_SIZE frames from it every HOP_SIZE samples.
    input_buf: Vec<f64>,
    /// Write position in input_buf.
    in_pos: usize,

    /// Overlap-add output buffer (2× FFT_SIZE for safe wraparound).
    output_buf: Vec<f64>,
    /// Read position in output_buf (tracks output sample extraction).
    out_read: usize,
    /// Write position in output_buf (tracks where next IFFT frame goes).
    out_write: usize,

    /// Dry delay line to align dry signal with wet latency.
    dry_buf: Vec<f64>,
    dry_pos: usize,

    /// Samples accumulated since last hop.
    hop_counter: usize,

    /// Previous frame phases for phase accumulation (input side).
    prev_phase_in: Vec<f64>,
    /// Phase accumulator for output bins.
    phase_acc: Vec<f64>,

    /// FFT scratch buffers.
    fft_in: Vec<f64>,
    fft_out: Vec<realfft::num_complex::Complex<f64>>,
    ifft_in: Vec<realfft::num_complex::Complex<f64>>,
    ifft_out: Vec<f64>,

    /// Total samples processed (for initial latency fill).
    total_samples: usize,

    /// Post-processing LPF for down-shifts (cascaded biquads for 4th-order).
    post_lpf: [Biquad; 2],
    /// Current sample rate for filter design.
    sample_rate: f64,
    /// Last shift mode — track changes to update LPF.
    last_shift: OctaveShift,
}

impl PolyOctave {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let ifft = planner.plan_fft_inverse(FFT_SIZE);

        let num_bins = FFT_SIZE / 2 + 1;

        // Hann window.
        let window: Vec<f64> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (TAU * i as f64 / FFT_SIZE as f64).cos()))
            .collect();

        let out_buf_size = FFT_SIZE * 2;

        Self {
            shift: OctaveShift::Sub1,
            mix: 1.0,
            fft,
            ifft,
            window,
            input_buf: vec![0.0; FFT_SIZE],
            in_pos: 0,
            output_buf: vec![0.0; out_buf_size],
            out_read: 0,
            out_write: 0,
            dry_buf: vec![0.0; FFT_SIZE],
            dry_pos: 0,
            hop_counter: 0,
            prev_phase_in: vec![0.0; num_bins],
            phase_acc: vec![0.0; num_bins],
            fft_in: vec![0.0; FFT_SIZE],
            fft_out: vec![realfft::num_complex::Complex::new(0.0, 0.0); num_bins],
            ifft_in: vec![realfft::num_complex::Complex::new(0.0, 0.0); num_bins],
            ifft_out: vec![0.0; FFT_SIZE],
            total_samples: 0,
            post_lpf: [Biquad::new(), Biquad::new()],
            sample_rate: 48000.0,
            last_shift: OctaveShift::Sub1,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.update_lpf();
        self.reset();
    }

    /// Configure the post-LPF based on current shift direction.
    fn update_lpf(&mut self) {
        // Gentle post-LPF for down-shifts to tame excess HF content.
        // Single 2nd-order stage (-12 dB/oct) to avoid over-cutting.
        let cutoff = match self.shift {
            OctaveShift::Sub1 => self.sample_rate * 0.22,
            OctaveShift::Sub2 => self.sample_rate * 0.11,
            _ => self.sample_rate * 0.499,
        };
        self.post_lpf[0].set(FilterType::Lowpass, cutoff, 0.707, self.sample_rate);
        self.post_lpf[1] = Biquad::new();
        self.last_shift = self.shift;
    }

    pub fn reset(&mut self) {
        self.input_buf.fill(0.0);
        self.in_pos = 0;
        self.output_buf.fill(0.0);
        self.out_read = 0;
        self.out_write = 0;
        self.dry_buf.fill(0.0);
        self.dry_pos = 0;
        self.hop_counter = 0;
        self.prev_phase_in.fill(0.0);
        self.phase_acc.fill(0.0);
        self.total_samples = 0;
        for lpf in &mut self.post_lpf {
            lpf.reset();
        }
    }

    /// Process one sample. Returns the mixed output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        let out_buf_size = self.output_buf.len();

        // Store input in circular buffer.
        self.input_buf[self.in_pos] = input;
        self.in_pos = (self.in_pos + 1) % FFT_SIZE;

        // Store dry signal in delay line for latency compensation.
        let dry_delayed = self.dry_buf[self.dry_pos];
        self.dry_buf[self.dry_pos] = input;
        self.dry_pos = (self.dry_pos + 1) % FFT_SIZE;

        self.hop_counter += 1;
        self.total_samples += 1;

        // Every HOP_SIZE samples, process an STFT frame.
        if self.hop_counter >= HOP_SIZE {
            self.hop_counter = 0;
            self.process_frame();
        }

        // Update LPF if shift changed.
        if self.shift != self.last_shift {
            self.update_lpf();
        }

        // Read from output buffer.
        let mut wet = self.output_buf[self.out_read];
        self.output_buf[self.out_read] = 0.0; // Clear for next overlap-add.
        self.out_read = (self.out_read + 1) % out_buf_size;

        // Apply post-LPF (cascaded biquads for 4th-order rolloff).
        for lpf in &mut self.post_lpf {
            wet = lpf.tick(wet, 0);
        }

        dry_delayed * (1.0 - self.mix) + wet * self.mix
    }

    /// Process one STFT frame: window → FFT → shift bins → IFFT → overlap-add.
    fn process_frame(&mut self) {
        let num_bins = FFT_SIZE / 2 + 1;

        // Copy input with window, reading backwards from current position.
        for i in 0..FFT_SIZE {
            let idx = (self.in_pos + i) % FFT_SIZE;
            self.fft_in[i] = self.input_buf[idx] * self.window[i];
        }

        // Forward FFT.
        self.fft
            .process(&mut self.fft_in, &mut self.fft_out)
            .expect("FFT failed");

        // Compute input magnitudes and phases.
        // Then shift bins according to the octave shift mode.
        self.ifft_in
            .iter_mut()
            .for_each(|c| *c = realfft::num_complex::Complex::new(0.0, 0.0));

        match self.shift {
            OctaveShift::Up1 => self.shift_up(1),
            OctaveShift::Up2 => self.shift_up(2),
            OctaveShift::Sub1 => self.shift_down(1),
            OctaveShift::Sub2 => self.shift_down(2),
        }

        // Update previous input phases for next frame.
        for k in 0..num_bins {
            self.prev_phase_in[k] = self.fft_out[k].arg();
        }

        // realfft requires DC and Nyquist bins to be real-valued.
        self.ifft_in[0] = realfft::num_complex::Complex::new(self.ifft_in[0].re, 0.0);
        self.ifft_in[num_bins - 1] =
            realfft::num_complex::Complex::new(self.ifft_in[num_bins - 1].re, 0.0);

        // Inverse FFT.
        self.ifft
            .process(&mut self.ifft_in, &mut self.ifft_out)
            .expect("IFFT failed");

        // Normalize IFFT output (realfft doesn't normalize).
        let norm = 1.0 / FFT_SIZE as f64;

        // Window the output and overlap-add.
        // With OVERLAP=4 and Hann window, the synthesis window gain is:
        // sum of w²(n) for 4 overlapping frames = 1.5, so divide by 1.5.
        let ola_norm = norm / 1.5;
        let out_buf_size = self.output_buf.len();
        for i in 0..FFT_SIZE {
            let pos = (self.out_write + i) % out_buf_size;
            self.output_buf[pos] += self.ifft_out[i] * self.window[i] * ola_norm;
        }

        // Advance output write position by hop.
        self.out_write = (self.out_write + HOP_SIZE) % out_buf_size;
    }

    /// Shift bins up by `octaves` octaves (1 or 2).
    ///
    /// Uses phase accumulation with instantaneous-frequency estimation for
    /// coherent resynthesis.
    fn shift_up(&mut self, octaves: u32) {
        let num_bins = FFT_SIZE / 2 + 1;
        let ratio = (1u32 << octaves) as usize; // 2 or 4
        let expected_hop_phase = TAU * HOP_SIZE as f64 / FFT_SIZE as f64;

        // Gain compensation for sidelobe energy lost in gaps between shifted bins.
        // Tuned for broadband guitar signals (slightly less than the pure-sine
        // calibration values of 1.64/2.45 since broadband content has less
        // concentrated sidelobe loss).
        let gain = match ratio {
            2 => 1.50, // ~+3.5 dB
            4 => 2.45, // ~+7.8 dB
            _ => 1.0,
        };

        for k in 0..num_bins {
            let dest = k * ratio;
            if dest >= num_bins {
                break;
            }

            let mag = self.fft_out[k].norm() * gain;
            let phase_in = self.fft_out[k].arg();

            // Estimate instantaneous frequency via phase difference.
            let expected = expected_hop_phase * k as f64;
            let phase_diff = phase_in - self.prev_phase_in[k];
            let deviation = wrap_phase(phase_diff - expected);
            let true_freq = expected + deviation;

            // At the destination bin, phase advances at scaled rate.
            let dest_advance = true_freq * ratio as f64;
            self.phase_acc[dest] = wrap_phase(self.phase_acc[dest] + dest_advance);

            self.ifft_in[dest] =
                realfft::num_complex::Complex::from_polar(mag, self.phase_acc[dest]);
        }

        // Zero DC bin — gain compensation amplifies DC content beyond what
        // analog octave generators produce.
        self.ifft_in[0] = realfft::num_complex::Complex::new(0.0, 0.0);

        // Phase-lock non-peak bins to the nearest peak for coherent sidelobes.
        self.phase_lock(num_bins);
    }

    /// Shift bins down by `octaves` octaves (1 or 2).
    fn shift_down(&mut self, octaves: u32) {
        let num_bins = FFT_SIZE / 2 + 1;
        let ratio = (1u32 << octaves) as usize; // 2 or 4
        let expected_hop_phase = TAU * HOP_SIZE as f64 / FFT_SIZE as f64;

        // Iterate over destination bins. For each dest, find the dominant source
        // bin among the `ratio` source bins that map to it.
        let num_dest = num_bins / ratio;
        for d in 0..num_dest {
            let src_start = d * ratio;
            let src_end = ((d + 1) * ratio).min(num_bins);

            // Find the source bin with largest magnitude and compute energy sum.
            let mut best_k = src_start;
            let mut best_mag = 0.0f64;
            let mut energy = 0.0f64;

            for k in src_start..src_end {
                let mag = self.fft_out[k].norm();
                energy += mag * mag;
                if mag > best_mag {
                    best_mag = mag;
                    best_k = k;
                }
            }

            if energy < 1e-40 {
                continue;
            }

            // Energy-preserving magnitude: sqrt(sum of squared mags).
            // Gain compensation for phase incoherence in reconstruction.
            let gain = match ratio {
                2 => 1.50, // +3.5 dB
                4 => 1.50, // +3.4 dB
                _ => 1.0,
            };
            let out_mag = energy.sqrt() * gain;

            // Use the dominant bin's phase for coherent output.
            let phase_in = self.fft_out[best_k].arg();
            let expected = expected_hop_phase * best_k as f64;
            let phase_diff = phase_in - self.prev_phase_in[best_k];
            let deviation = wrap_phase(phase_diff - expected);
            let true_freq = expected + deviation;

            // Phase advances at reduced rate for the destination bin.
            let dest_advance = true_freq / ratio as f64;
            self.phase_acc[d] = wrap_phase(self.phase_acc[d] + dest_advance);

            self.ifft_in[d] = realfft::num_complex::Complex::from_polar(out_mag, self.phase_acc[d]);
        }

        // Apply a spectral rolloff to the upper destination bins.
        // Combined with the time-domain post-LPF for smooth overall rolloff.
        let taper_start = (num_dest as f64 * 0.6) as usize;
        let taper_len = num_dest - taper_start;
        for d in taper_start..num_dest {
            let t = (d - taper_start) as f64 / taper_len as f64;
            // Cosine taper: 1 at start → 0 at end.
            let gain = 0.5 * (1.0 + (PI * t).cos());
            let mag = self.ifft_in[d].norm() * gain;
            let phase = self.ifft_in[d].arg();
            self.ifft_in[d] = realfft::num_complex::Complex::from_polar(mag, phase);
        }

        // Phase-lock for coherent reconstruction.
        self.phase_lock(num_dest);
    }

    /// Rigid phase locking: propagate each spectral peak's phase to neighboring
    /// bins so the Hann window sidelobes reconstruct coherently.
    fn phase_lock(&mut self, num_bins: usize) {
        let expected_hop_phase = TAU * HOP_SIZE as f64 / FFT_SIZE as f64;

        // Find peaks: bins where magnitude > both neighbors.
        // For each non-peak bin, inherit phase from the nearest peak.
        let mut peak_bin = vec![0usize; num_bins];
        let mut is_peak = vec![false; num_bins];

        // Identify peaks.
        for k in 1..num_bins.saturating_sub(1) {
            let m = self.ifft_in[k].norm();
            let m_prev = self.ifft_in[k - 1].norm();
            let m_next = self.ifft_in[k + 1].norm();
            if m >= m_prev && m >= m_next && m > 1e-20 {
                is_peak[k] = true;
            }
        }
        // Bin 0 is always a "peak" for purposes of phase locking.
        if self.ifft_in[0].norm() > 1e-20 {
            is_peak[0] = true;
        }

        // Assign each bin to its nearest peak.
        let mut last_peak = 0;
        for k in 0..num_bins {
            if is_peak[k] {
                last_peak = k;
            }
            peak_bin[k] = last_peak;
        }
        // Backward pass: check if a later peak is closer.
        let mut next_peak = num_bins.saturating_sub(1);
        for k in (0..num_bins).rev() {
            if is_peak[k] {
                next_peak = k;
            }
            if next_peak.abs_diff(k) < peak_bin[k].abs_diff(k) {
                peak_bin[k] = next_peak;
            }
        }

        // Apply phase locking: set non-peak bins' phases relative to their peak.
        for k in 0..num_bins {
            if is_peak[k] || self.ifft_in[k].norm() < 1e-20 {
                continue;
            }
            let p = peak_bin[k];
            let peak_phase = self.phase_acc[p];
            // Phase at bin k should be peak's phase + frequency-difference term.
            let locked_phase = peak_phase + (k as f64 - p as f64) * expected_hop_phase;
            let mag = self.ifft_in[k].norm();
            self.ifft_in[k] = realfft::num_complex::Complex::from_polar(mag, locked_phase);
        }
    }

    pub fn latency(&self) -> usize {
        FFT_SIZE
    }
}

/// Wrap phase to [-π, π].
#[inline]
fn wrap_phase(phase: f64) -> f64 {
    let mut p = phase;
    while p > PI {
        p -= TAU;
    }
    while p < -PI {
        p += TAU;
    }
    p
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
    fn latency_is_fft_size() {
        assert_eq!(make_pog(OctaveShift::Sub1).latency(), FFT_SIZE);
    }

    #[test]
    fn silence_in_silence_out() {
        let mut p = make_pog(OctaveShift::Sub1);
        for _ in 0..48000 {
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
                if i > p.latency() + 4800 {
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

        // Measure spectral energy: expect peak near 880 Hz.
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

        let mag_880 = mag_at(880.0);
        let mag_1320 = mag_at(1320.0);
        let mag_noise = mag_at(1000.0);

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

        // Need to go past latency for dry signal to come through.
        for i in 0..48000 {
            let input = sine(440.0, i);
            let out = p.tick(input);
            if i >= p.latency() {
                let expected = sine(440.0, i - p.latency());
                assert!(
                    (out - expected).abs() < 1e-6,
                    "Mix=0 should pass dry at sample {i}: got {out}, expected {expected}"
                );
            }
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

        let mag_110 = mag_at(110.0);
        let mag_220 = mag_at(220.0);
        let mag_440 = mag_at(440.0);

        eprintln!(
            "Sub2 440Hz input: mag_110={mag_110:.1}, mag_220={mag_220:.1}, mag_440={mag_440:.1}"
        );

        assert!(
            mag_110 > mag_220 * 2.0,
            "110 Hz (two oct down) should dominate over 220 Hz: \
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

        let mag_1760 = mag_at(1760.0);
        let mag_880 = mag_at(880.0);
        let mag_440 = mag_at(440.0);

        eprintln!(
            "Up2 440Hz input: mag_1760={mag_1760:.1}, mag_880={mag_880:.1}, mag_440={mag_440:.1}"
        );

        assert!(
            mag_1760 > mag_880 * 2.0,
            "1760 Hz (two oct up) should dominate over 880 Hz: \
             mag_1760={mag_1760:.1} mag_880={mag_880:.1}"
        );
    }

    #[test]
    fn output_level_near_unity() {
        for shift in [
            OctaveShift::Sub2,
            OctaveShift::Sub1,
            OctaveShift::Up1,
            OctaveShift::Up2,
        ] {
            let mut p = make_pog(shift);
            let n = 96000;
            let warmup = p.latency() + 24000;
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
            assert!(
                ratio_db.abs() < 12.0,
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
                if i > p.latency() + (sr * 0.1) as usize {
                    energy += out * out;
                }
                assert!(out.is_finite(), "NaN at sr={sr}, sample {i}");
            }
            assert!(energy > 0.1, "No output at sr={sr}: energy={energy}");
        }
    }
}
