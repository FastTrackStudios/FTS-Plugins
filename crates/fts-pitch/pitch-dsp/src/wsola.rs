//! WSOLA — Waveform Similarity Overlap-Add pitch shifter.
//!
//! A time-domain algorithm that produces higher quality than basic granular
//! shifting by finding optimal grain alignment points using cross-correlation.
//! Works on any signal (monophonic or polyphonic) without pitch detection.
//!
//! For each output grain the algorithm searches a tolerance window around the
//! nominal read position to find the offset that maximises waveform similarity
//! with the tail of the previous grain. This eliminates the phase-jump
//! artefacts that plague naive granular approaches.
//!
//! Latency: `grain_size` samples (default 1024).
//! Character: Smooth, low-artefact, works well on complex material.

use fts_dsp::delay_line::DelayLine;

/// WSOLA pitch shifter with cross-correlation grain alignment.
pub struct WsolaShifter {
    /// Pitch ratio: 0.5 = octave down, 2.0 = octave up.
    pub speed: f64,
    /// Mix: 0.0 = dry only, 1.0 = wet only.
    pub mix: f64,

    // Circular analysis buffer (input history).
    analysis_buf: DelayLine,

    // Output accumulator (overlap-add target).
    output_buf: Vec<f64>,
    output_pos: usize,

    // Grain parameters.
    grain_size: usize,
    hop_size: usize,
    tolerance: usize,

    // Fractional read pointer: distance behind the write head (in samples).
    read_offset: f64,

    // Samples until next grain is placed.
    hop_countdown: usize,

    // Previous grain tail for cross-correlation matching.
    prev_tail: Vec<f64>,

    // Total samples written (for startup gating).
    write_count: usize,

    sample_rate: f64,
}

impl WsolaShifter {
    /// Default grain size at 48 kHz.
    const DEFAULT_GRAIN: usize = 1024;
    /// Tolerance (half-width) for the similarity search.
    const DEFAULT_TOLERANCE: usize = 64;
    /// Analysis buffer length in seconds.
    const BUF_SECONDS: f64 = 2.0;

    pub fn new() -> Self {
        let grain_size = Self::DEFAULT_GRAIN;
        let hop_size = grain_size / 2;
        let tolerance = Self::DEFAULT_TOLERANCE;
        let buf_len = 48000 * 2 + grain_size + tolerance;

        Self {
            speed: 0.5,
            mix: 1.0,
            analysis_buf: DelayLine::new(buf_len),
            output_buf: vec![0.0; buf_len],
            output_pos: 0,
            grain_size,
            hop_size,
            tolerance,
            read_offset: grain_size as f64 + tolerance as f64,
            hop_countdown: 0,
            prev_tail: vec![0.0; hop_size],
            write_count: 0,
            sample_rate: 48000.0,
        }
    }

    pub fn update(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;

        // Scale grain size proportionally to sample rate.
        self.grain_size = ((Self::DEFAULT_GRAIN as f64 * sample_rate) / 48000.0) as usize;
        // Keep grain size even for symmetric windowing.
        if self.grain_size % 2 != 0 {
            self.grain_size += 1;
        }
        self.hop_size = self.grain_size / 2;
        self.tolerance = Self::DEFAULT_TOLERANCE;

        let buf_len =
            (sample_rate * Self::BUF_SECONDS) as usize + self.grain_size + self.tolerance + 64;
        if self.analysis_buf.len() < buf_len {
            self.analysis_buf = DelayLine::new(buf_len);
            self.output_buf = vec![0.0; buf_len];
        }

        self.prev_tail.resize(self.hop_size, 0.0);
        self.read_offset = self.grain_size as f64 + self.tolerance as f64;
    }

    pub fn reset(&mut self) {
        self.analysis_buf.clear();
        self.output_buf.fill(0.0);
        self.output_pos = 0;
        self.read_offset = self.grain_size as f64 + self.tolerance as f64;
        self.hop_countdown = 0;
        self.prev_tail.fill(0.0);
        self.write_count = 0;
    }

    /// Hann window value for index `i` within a window of length `len`.
    #[inline]
    fn hann(i: usize, len: usize) -> f64 {
        0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / len as f64).cos())
    }

    /// Find the best offset within `[-tolerance, +tolerance]` around `nominal`
    /// that maximises normalised cross-correlation with `prev_tail`.
    fn best_offset(&self, nominal: f64) -> f64 {
        let overlap_len = self.prev_tail.len();
        let buf_len = self.analysis_buf.len();
        let tol = self.tolerance as isize;

        let mut best_corr: f64 = f64::NEG_INFINITY;
        let mut best_delta: isize = 0;

        // Precompute energy of prev_tail.
        let mut energy_prev = 0.0f64;
        for s in &self.prev_tail {
            energy_prev += s * s;
        }

        for delta in -tol..=tol {
            let candidate = nominal + delta as f64;
            // Ensure the candidate read region is inside the buffer.
            if candidate < 1.0 || (candidate as usize + overlap_len) >= buf_len {
                continue;
            }

            let mut correlation = 0.0f64;
            let mut energy_cand = 0.0f64;

            for i in 0..overlap_len {
                let cand_sample = self.analysis_buf.read(candidate as usize + i);
                correlation += self.prev_tail[i] * cand_sample;
                energy_cand += cand_sample * cand_sample;
            }

            let denom = (energy_prev * energy_cand).sqrt();
            let norm_corr = if denom > 1e-12 { correlation / denom } else { 0.0 };

            if norm_corr > best_corr {
                best_corr = norm_corr;
                best_delta = delta;
            }
        }

        nominal + best_delta as f64
    }

    /// Place a Hann-windowed grain starting at `read_start` samples behind the
    /// write head into the output accumulator.
    fn place_grain(&mut self, read_start: f64) {
        let grain_len = self.grain_size;
        let buf_len = self.output_buf.len();
        let analysis_len = self.analysis_buf.len();

        for i in 0..grain_len {
            let read_pos = read_start as usize + i;
            if read_pos == 0 || read_pos >= analysis_len {
                continue;
            }
            let sample = self.analysis_buf.read(read_pos);
            let win = Self::hann(i, grain_len);

            let write_idx = (self.output_pos + i) % buf_len;
            self.output_buf[write_idx] += sample * win;
        }

        // Store the tail of this grain (last hop_size samples, windowed) for
        // cross-correlation in the next grain placement.
        let tail_start = grain_len - self.hop_size;
        for i in 0..self.hop_size {
            let read_pos = read_start as usize + tail_start + i;
            if read_pos == 0 || read_pos >= analysis_len {
                self.prev_tail[i] = 0.0;
            } else {
                let sample = self.analysis_buf.read(read_pos);
                let win = Self::hann(tail_start + i, grain_len);
                self.prev_tail[i] = sample * win;
            }
        }
    }

    /// Process one sample. Returns the pitch-shifted output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        // Write input into analysis buffer.
        self.analysis_buf.write(input);
        self.write_count += 1;

        // Place grains at hop intervals.
        if self.hop_countdown == 0 {
            // Need enough data so that reading read_offset + grain_size behind
            // the write head stays within what has actually been written.
            let min_data = self.grain_size * 2 + self.tolerance;
            if self.write_count >= min_data {
                // Clamp read_offset so we never read beyond what has been
                // written (the delay line stores write_count samples at most).
                let max_readable = self.write_count as f64
                    - self.grain_size as f64
                    - self.tolerance as f64;
                if self.read_offset > max_readable {
                    self.read_offset = max_readable.max(1.0);
                }

                let nominal = self.read_offset;

                // Search for best alignment via cross-correlation.
                let best_start = self.best_offset(nominal);

                self.place_grain(best_start);

                // Advance the read offset for the next grain.
                // Between grain placements the write head advances by
                // hop_size samples, so the read offset naturally decreases
                // by hop_size. We want it to decrease by hop_size / speed
                // instead (slower read = pitch down, faster read = pitch up).
                let advance = self.hop_size as f64;
                self.read_offset = self.read_offset - advance + advance / self.speed;

                // Clamp read offset to stay within valid buffer range.
                let max_offset = self.analysis_buf.len() as f64
                    - self.grain_size as f64
                    - self.tolerance as f64
                    - 2.0;
                let min_offset = 1.0_f64;
                if self.read_offset < min_offset || self.read_offset > max_offset {
                    self.read_offset =
                        (self.grain_size as f64 + self.tolerance as f64 + 1.0).min(max_offset);
                }
            }

            self.hop_countdown = self.hop_size;
        }
        self.hop_countdown -= 1;

        // Read from output accumulator and clear.
        let wet = self.output_buf[self.output_pos];
        self.output_buf[self.output_pos] = 0.0;
        self.output_pos = (self.output_pos + 1) % self.output_buf.len();

        // Normalize for 50% overlap (two grains overlap at any point).
        // The sum of two half-overlapped Hann windows averages ~1.0, but the
        // actual peak is ~1.0 and the trough is ~0.5, so we don't need extra
        // scaling for Hann OLA at 50% overlap (the Hann windows sum to 1).
        input * (1.0 - self.mix) + wet * self.mix
    }

    pub fn latency(&self) -> usize {
        self.grain_size
    }
}

impl Default for WsolaShifter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;

    fn make_wsola() -> WsolaShifter {
        let mut w = WsolaShifter::new();
        w.speed = 0.5;
        w.mix = 1.0;
        w.update(SR);
        w
    }

    #[test]
    fn silence_in_silence_out() {
        let mut w = make_wsola();
        for _ in 0..4800 {
            let out = w.tick(0.0);
            assert!(out.abs() < 1e-6, "Should be silent: {out}");
        }
    }

    #[test]
    fn produces_output_on_sine() {
        let mut w = make_wsola();
        let freq = 220.0;

        let mut energy = 0.0;
        let n = 48000;
        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = w.tick(input);
            if i > 4096 {
                energy += out * out;
            }
        }
        assert!(energy > 0.1, "Should produce output: energy={energy}");
    }

    #[test]
    fn no_nan() {
        let mut w = make_wsola();
        for i in 0..48000 {
            let input = (2.0 * PI * 82.0 * i as f64 / SR).sin() * 0.9;
            let out = w.tick(input);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn different_speeds_differ() {
        let freq = 220.0;
        let n = 9600;

        let collect = |speed: f64| -> Vec<f64> {
            let mut w = make_wsola();
            w.speed = speed;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let s = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
                out.push(w.tick(s));
            }
            out
        };

        let down = collect(0.5);
        let up = collect(2.0);

        let diff: f64 = down
            .iter()
            .zip(up.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / n as f64;

        assert!(
            diff > 0.001,
            "Different speeds should produce different output: {diff}"
        );
    }

    #[test]
    fn dry_wet_mix() {
        let mut w = make_wsola();
        w.mix = 0.0;

        for i in 0..4800 {
            let input = (2.0 * PI * 440.0 * i as f64 / SR).sin() * 0.5;
            let out = w.tick(input);
            assert!((out - input).abs() < 1e-10, "Mix=0 should pass dry");
        }
    }

    #[test]
    fn high_quality_sine() {
        let mut w = make_wsola();
        w.speed = 2.0; // Octave up.
        w.update(SR);

        let freq = 220.0;
        let n = 48000;
        let mut energy = 0.0;

        for i in 0..n {
            let input = (2.0 * PI * freq * i as f64 / SR).sin() * 0.5;
            let out = w.tick(input);
            // Skip startup latency.
            if i > 4096 {
                energy += out * out;
            }
        }

        assert!(
            energy > 0.1,
            "Octave-up sine should produce measurable energy: {energy}"
        );
    }
}
