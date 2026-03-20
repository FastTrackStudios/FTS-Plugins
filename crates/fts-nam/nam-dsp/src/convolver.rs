//! Partitioned overlap-add FFT convolver for cabinet impulse responses.
//!
//! The IR is split into blocks of `BLOCK_SIZE` samples, each FFT'd once at
//! load time. During processing, each input block is FFT'd and multiplied
//! with all IR partitions via a frequency-domain delay line, then IFFT'd
//! and overlap-added.
//!
//! Latency = BLOCK_SIZE samples. Complexity: O(P * N log N) per block,
//! where P = number of IR partitions, N = FFT size.

use realfft::{num_complex::Complex, RealFftPlanner};

/// Block size for the convolver. Determines latency (in samples).
const BLOCK_SIZE: usize = 1024;

/// FFT size = 2 * block_size (for linear convolution via overlap-add).
const FFT_SIZE: usize = BLOCK_SIZE * 2;

/// Number of complex bins in the FFT output.
const COMPLEX_BINS: usize = FFT_SIZE / 2 + 1;

/// Partitioned overlap-add FFT convolver.
pub struct Convolver {
    /// Pre-FFT'd IR partitions (frequency domain).
    ir_partitions: Vec<Vec<Complex<f64>>>,
    /// Frequency-domain input delay line (ring buffer of past input FFTs).
    input_fdl: Vec<Vec<Complex<f64>>>,
    /// Current write position in the FDL ring buffer.
    fdl_pos: usize,
    /// Accumulator for frequency-domain multiply-accumulate.
    accum: Vec<Complex<f64>>,
    /// Time-domain input buffer (collects samples until BLOCK_SIZE).
    input_buf: Vec<f64>,
    /// Position in input_buf.
    input_pos: usize,
    /// Time-domain output buffer (current block being read out).
    output_buf: Vec<f64>,
    /// Overlap tail from previous block (added to next block's output).
    overlap: Vec<f64>,
    /// Position in output_buf (how many samples have been read out).
    output_pos: usize,
    /// Scratch for FFT input.
    fft_scratch: Vec<f64>,
    /// Scratch for IFFT output.
    ifft_scratch: Vec<f64>,
    /// Whether an IR is loaded.
    loaded: bool,
    /// IR normalization gain.
    norm_gain: f64,
}

impl Convolver {
    pub fn new() -> Self {
        Self {
            ir_partitions: Vec::new(),
            input_fdl: Vec::new(),
            fdl_pos: 0,
            accum: vec![Complex::new(0.0, 0.0); COMPLEX_BINS],
            input_buf: vec![0.0; BLOCK_SIZE],
            input_pos: 0,
            output_buf: vec![0.0; BLOCK_SIZE],
            overlap: vec![0.0; BLOCK_SIZE],
            output_pos: BLOCK_SIZE, // Start exhausted so first block triggers processing
            fft_scratch: vec![0.0; FFT_SIZE],
            ifft_scratch: vec![0.0; FFT_SIZE],
            loaded: false,
            norm_gain: 1.0,
        }
    }

    /// Load an impulse response. Normalizes to peak 0.8, then partitions and FFTs.
    pub fn load_ir(&mut self, ir: &[f64], _sample_rate: f64) {
        if ir.is_empty() {
            self.unload();
            return;
        }

        // Peak-normalize to 0.8 (matching Ratatouille)
        let peak = ir.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
        self.norm_gain = if peak > 1e-10 { 0.8 / peak } else { 1.0 };

        let num_partitions = (ir.len() + BLOCK_SIZE - 1) / BLOCK_SIZE;

        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);

        self.ir_partitions.clear();
        self.ir_partitions.reserve(num_partitions);

        let mut fft_in = vec![0.0f64; FFT_SIZE];
        let mut fft_out = vec![Complex::new(0.0, 0.0); COMPLEX_BINS];

        for p in 0..num_partitions {
            let start = p * BLOCK_SIZE;
            let end = (start + BLOCK_SIZE).min(ir.len());

            fft_in.fill(0.0);
            for (i, &s) in ir[start..end].iter().enumerate() {
                fft_in[i] = s * self.norm_gain;
            }

            fft.process(&mut fft_in, &mut fft_out).unwrap();
            self.ir_partitions.push(fft_out.clone());
        }

        // Initialize FDL
        self.input_fdl = vec![vec![Complex::new(0.0, 0.0); COMPLEX_BINS]; num_partitions];
        self.fdl_pos = 0;
        self.input_pos = 0;
        self.output_pos = BLOCK_SIZE; // Exhausted → triggers on first input
        self.output_buf.fill(0.0);
        self.overlap.fill(0.0);
        self.loaded = true;
    }

    /// Unload the current IR.
    pub fn unload(&mut self) {
        self.ir_partitions.clear();
        self.input_fdl.clear();
        self.loaded = false;
    }

    /// Whether an IR is loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Reset internal state (keeps IR loaded).
    pub fn reset(&mut self) {
        for fdl in &mut self.input_fdl {
            for c in fdl.iter_mut() {
                *c = Complex::new(0.0, 0.0);
            }
        }
        self.fdl_pos = 0;
        self.input_pos = 0;
        self.output_pos = BLOCK_SIZE;
        self.output_buf.fill(0.0);
        self.overlap.fill(0.0);
    }

    /// Process one sample. Returns the convolved output.
    #[inline]
    pub fn tick(&mut self, input: f64) -> f64 {
        if !self.loaded {
            return input;
        }

        // Accumulate input sample
        self.input_buf[self.input_pos] = input;
        self.input_pos += 1;

        // Read from output buffer
        let out = if self.output_pos < BLOCK_SIZE {
            let v = self.output_buf[self.output_pos];
            self.output_pos += 1;
            v
        } else {
            0.0
        };

        // When input buffer is full, process a block
        if self.input_pos >= BLOCK_SIZE {
            self.process_block();
            self.input_pos = 0;
            self.output_pos = 0;
        }

        out
    }

    fn process_block(&mut self) {
        let num_partitions = self.ir_partitions.len();
        if num_partitions == 0 {
            return;
        }

        let mut planner = RealFftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let ifft = planner.plan_fft_inverse(FFT_SIZE);

        // Zero-pad input to FFT_SIZE: [input_buf | zeros]
        self.fft_scratch[..BLOCK_SIZE].copy_from_slice(&self.input_buf);
        self.fft_scratch[BLOCK_SIZE..].fill(0.0);

        // Forward FFT of input block
        let mut input_spectrum = vec![Complex::new(0.0, 0.0); COMPLEX_BINS];
        fft.process(&mut self.fft_scratch, &mut input_spectrum)
            .unwrap();

        // Store in FDL at current position
        self.input_fdl[self.fdl_pos].copy_from_slice(&input_spectrum);

        // Multiply-accumulate across all partitions
        for c in self.accum.iter_mut() {
            *c = Complex::new(0.0, 0.0);
        }

        for p in 0..num_partitions {
            // FDL index: current input is partition 0, previous is partition 1, etc.
            let fdl_idx = (self.fdl_pos + num_partitions - p) % num_partitions;
            let ir_part = &self.ir_partitions[p];
            let input_part = &self.input_fdl[fdl_idx];

            for (acc, (ir, inp)) in self
                .accum
                .iter_mut()
                .zip(ir_part.iter().zip(input_part.iter()))
            {
                *acc += ir * inp;
            }
        }

        // Inverse FFT
        let mut freq_buf = self.accum.clone();
        ifft.process(&mut freq_buf, &mut self.ifft_scratch).unwrap();

        // Normalize IFFT output
        let norm = 1.0 / FFT_SIZE as f64;

        // Overlap-add:
        // First half = new output + overlap from previous block
        for i in 0..BLOCK_SIZE {
            self.output_buf[i] = self.ifft_scratch[i] * norm + self.overlap[i];
        }
        // Second half = save as overlap for next block
        for i in 0..BLOCK_SIZE {
            self.overlap[i] = self.ifft_scratch[BLOCK_SIZE + i] * norm;
        }

        // Advance FDL position
        self.fdl_pos = (self.fdl_pos + 1) % num_partitions;
    }

    /// Latency in samples introduced by the convolver.
    pub fn latency(&self) -> usize {
        if self.loaded {
            BLOCK_SIZE
        } else {
            0
        }
    }
}

impl Default for Convolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unloaded_passes_through() {
        let mut c = Convolver::new();
        for i in 0..100 {
            let inp = (i as f64) * 0.01;
            let out = c.tick(inp);
            assert!((out - inp).abs() < 1e-10);
        }
    }

    #[test]
    fn impulse_response_identity() {
        // Load a unit impulse as IR — output should equal input (delayed by BLOCK_SIZE).
        let mut c = Convolver::new();
        let mut ir = vec![0.0; 1];
        ir[0] = 1.0;
        c.load_ir(&ir, 48000.0);

        // The IR gets peak-normalized to 0.8, so output will be scaled by 0.8
        let norm = 0.8;

        // Feed an impulse and collect output
        let total = BLOCK_SIZE * 4;
        let mut input = vec![0.0; total];
        input[0] = 1.0;

        let mut output = vec![0.0; total];
        for i in 0..total {
            output[i] = c.tick(input[i]);
        }

        // The impulse should appear at sample BLOCK_SIZE (one block of latency)
        let peak_idx = output
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.abs().partial_cmp(&b.abs()).unwrap())
            .unwrap()
            .0;

        assert_eq!(
            peak_idx, BLOCK_SIZE,
            "Impulse should appear at block boundary, got {peak_idx}"
        );
        assert!(
            (output[peak_idx] - norm).abs() < 0.01,
            "Impulse amplitude should be ~{norm}, got {}",
            output[peak_idx]
        );
    }

    #[test]
    fn sine_through_ir_no_nan() {
        let mut c = Convolver::new();
        // Simple decaying IR
        let ir: Vec<f64> = (0..256).map(|i| (-i as f64 / 50.0).exp()).collect();
        c.load_ir(&ir, 48000.0);

        for i in 0..48000 {
            let inp = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48000.0).sin() * 0.5;
            let out = c.tick(inp);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn large_ir_works() {
        let mut c = Convolver::new();
        // 2-second IR at 48kHz
        let ir: Vec<f64> = (0..96000)
            .map(|i| (-i as f64 / 10000.0).exp() * 0.5)
            .collect();
        c.load_ir(&ir, 48000.0);

        for i in 0..4800 {
            let inp = if i < 480 { 0.5 } else { 0.0 };
            let out = c.tick(inp);
            assert!(out.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn energy_conservation() {
        // A unit impulse IR should preserve energy (minus normalization)
        let mut c = Convolver::new();
        let ir = vec![1.0];
        c.load_ir(&ir, 48000.0);

        let n = BLOCK_SIZE * 4;
        let input: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48000.0).sin() * 0.5)
            .collect();

        let mut output = vec![0.0; n];
        for i in 0..n {
            output[i] = c.tick(input[i]);
        }

        // Compare energy after latency settles (skip first 2 blocks)
        let skip = BLOCK_SIZE * 2;
        let in_energy: f64 = input[skip..].iter().map(|s| s * s).sum::<f64>();
        let out_energy: f64 = output[skip..].iter().map(|s| s * s).sum::<f64>();

        // Output should be ~0.8^2 = 0.64 of input energy (due to normalization)
        let ratio = out_energy / in_energy;
        assert!(
            (ratio - 0.64).abs() < 0.1,
            "Energy ratio should be ~0.64 (0.8^2), got {ratio:.4}"
        );
    }
}
