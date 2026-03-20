# DSP Core Specification

Requirements for shared DSP primitives in `fts-dsp`.

## Portability

r[dsp.portable.no-std-time]
DSP crates must not use `std::time`, `std::fs`, `std::net`, or `std::thread`. All DSP code must compile to `wasm32-unknown-unknown`.

r[dsp.portable.no-framework]
`fts-dsp` and all `*-dsp` crates must have zero dependency on nih-plug, dioxus, or any plugin/GUI framework.

r[dsp.portable.f64-math]
Use `libm` crate for transcendental math functions (`sin`, `cos`, `pow`, `log`, etc.) to ensure WASM compatibility. Do not rely on `std` float methods that may not be available on all targets.

## Biquad Filter

r[dsp.biquad.types]
Support all standard biquad filter types: lowpass, highpass, bandpass, notch, low shelf, high shelf, and parametric peak.

r[dsp.biquad.coefficients]
Calculate coefficients from frequency (Hz), Q, gain (dB), and sample rate using the Audio EQ Cookbook formulas. Coefficients must be normalized by a0.

r[dsp.biquad.tdf2]
Implement Transposed Direct Form II for numerical stability at low frequencies.

r[dsp.biquad.stereo]
Maintain independent filter state per channel (minimum 2 channels for stereo).

r[dsp.biquad.reset]
Provide a reset method that zeroes all filter state without changing coefficients.

## Delay Line

r[dsp.delay.circular-buffer]
Implement a circular buffer delay line with heap-allocated storage (`Box<[f64]>`).

r[dsp.delay.integer-read]
Support reading at integer delay lengths (in samples behind the write head).

r[dsp.delay.fractional-read]
Support fractional-sample delay with linear interpolation between adjacent samples.

r[dsp.delay.clear]
Provide a clear method that zeroes the buffer and resets the write position.

## PRNG

r[dsp.prng.xorshift32]
Implement the Airwindows XorShift32 PRNG: `state ^= state << 13; state ^= state >> 17; state ^= state << 5`.

r[dsp.prng.nonzero-seed]
Ensure the PRNG is never seeded with zero (zero is a fixed point of xorshift).

r[dsp.prng.bipolar]
Provide a method that returns bipolar noise samples in the range [-1.0, 1.0].

## Dither

r[dsp.dither.airwindows]
Implement Airwindows-style dither for 32-bit float output using the XorShift32 PRNG.

## Soft Clipping

r[dsp.clip.sin]
Implement sin-based soft clipping that maps the input through `sin()` in the range `[-pi/2, pi/2]`.

r[dsp.clip.golden]
Implement golden-ratio interpolated hard clipping from ClipOnly2. When transitioning out of clip state, blend using phi (0.618) between current and previous sample to control intersample peaks.

r[dsp.clip.golden.state]
The golden clipper must track per-channel clip state (was the previous sample clipped?) for correct transition blending.

## Slew Rate Limiting

r[dsp.slew.single]
Implement a single-stage slew rate limiter that constrains the sample-to-sample difference to a configurable threshold.

r[dsp.slew.golden-chain]
Implement a multi-stage slew limiter chain where each stage's threshold is spaced by the golden ratio (phi = 1.618...), matching ToTape8's bias algorithm.

r[dsp.slew.stereo]
All slew limiters must maintain independent state per channel.

## Processor Trait

r[dsp.processor.trait]
Define a `Processor` trait with `reset()`, `update(AudioConfig)`, and `process(&mut [f64], &mut [f64])` methods.

r[dsp.processor.send]
All types implementing `Processor` must be `Send` to allow moving between threads (e.g., from UI thread to audio thread).
