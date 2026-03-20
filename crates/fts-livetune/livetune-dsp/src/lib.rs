//! FTS LiveTune — Real-time pitch correction (auto-tune) DSP.
//!
//! Pipeline: Input → Pitch Detect (YIN) → Scale Quantize → Pitch Shift → Output
//!
//! Features:
//! - YIN pitch detection with SVF pre-filter and confidence gating
//! - Scale quantization with key, scale, per-note enable, retune speed
//! - Sine-shaped note transitions (Autotalent-style)
//! - Formant-preserving phase vocoder (cepstral envelope) for large shifts
//! - PSOLA fallback for small shifts (<5 semitones)
//! - Dry/wet mix and correction amount controls

pub mod chain;
pub mod detector;
pub mod quantizer;
pub mod vocoder;
