//! Linear resampler for matching NAM model sample rates to the host.
//!
//! Uses simple linear interpolation — sufficient for the typical 48kHz↔44.1kHz
//! conversions needed by NAM models. More sophisticated resampling (e.g.
//! windowed-sinc) can replace this later if quality demands it.

/// Converts audio between two sample rates using linear interpolation.
pub struct LinearResampler {
    ratio: f64, // source_rate / target_rate
    phase: f64,
    last_sample: f64,
}

impl LinearResampler {
    pub fn new() -> Self {
        Self {
            ratio: 1.0,
            phase: 0.0,
            last_sample: 0.0,
        }
    }

    /// Set the resampling ratio: `from_rate / to_rate`.
    pub fn set_rates(&mut self, from_rate: f64, to_rate: f64) {
        self.ratio = from_rate / to_rate;
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.last_sample = 0.0;
    }

    /// Resample `input` (at source rate) into `output` (at target rate).
    /// Returns the number of output samples written.
    pub fn process(&mut self, input: &[f64], output: &mut [f64]) -> usize {
        if input.is_empty() {
            return 0;
        }

        let mut out_idx = 0;
        while out_idx < output.len() {
            let int_part = self.phase as usize;
            if int_part >= input.len() {
                break;
            }
            let frac = self.phase - int_part as f64;
            let curr = input[int_part];
            let next = if int_part + 1 < input.len() {
                input[int_part + 1]
            } else {
                curr
            };
            output[out_idx] = curr + (next - curr) * frac;
            out_idx += 1;
            self.phase += self.ratio;
        }

        // Track consumed input
        let consumed = self.phase as usize;
        self.phase -= consumed as f64;
        if !input.is_empty() {
            self.last_sample = *input.last().unwrap();
        }

        out_idx
    }

    /// How many output samples will be produced for `input_len` input samples.
    pub fn output_count(&self, input_len: usize) -> usize {
        if self.ratio <= 0.0 {
            return input_len;
        }
        ((input_len as f64) / self.ratio).ceil() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_ratio_passes_through() {
        let mut r = LinearResampler::new();
        r.set_rates(48000.0, 48000.0);

        let input = [1.0, 2.0, 3.0, 4.0];
        let mut output = [0.0; 4];
        let n = r.process(&input, &mut output);
        assert_eq!(n, 4);
        for (i, &v) in output.iter().enumerate() {
            assert!((v - input[i]).abs() < 1e-10, "Mismatch at {i}: {v}");
        }
    }

    #[test]
    fn downsample_2x_halves() {
        let mut r = LinearResampler::new();
        r.set_rates(48000.0, 24000.0); // 2:1

        let input: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let mut output = vec![0.0; 200];
        let n = r.process(&input, &mut output);
        // Should produce ~50 samples
        assert!(n >= 49 && n <= 51, "Expected ~50, got {n}");
    }

    #[test]
    fn upsample_2x_doubles() {
        let mut r = LinearResampler::new();
        r.set_rates(24000.0, 48000.0); // 0.5:1

        let input: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let mut output = vec![0.0; 200];
        let n = r.process(&input, &mut output);
        // Should produce ~100 samples
        assert!(n >= 99 && n <= 101, "Expected ~100, got {n}");
    }
}
