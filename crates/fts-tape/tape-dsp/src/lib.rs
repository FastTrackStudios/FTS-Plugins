//! Tape machine DSP engine — analog tape emulation for the plugin suite.
//!
//! # Algorithms (from Airwindows)
//!
//! - [`dubly::Dubly`] — Encode/decode noise reduction emulation with
//!   frequency-dependent compression/expansion (Dolby-inspired)
//! - [`flutter::Flutter`] — Wow and flutter simulation with multiple
//!   LFO rates and random drift components
//! - [`bias::Bias`] — Golden-ratio slew chain that emulates tape bias
//!   and its effect on high-frequency saturation characteristics
//! - [`saturation::Saturation`] — Mid/side tape saturation with
//!   asymmetric waveshaping and head magnetization modeling
//! - [`head_bump::HeadBump`] — Biquad resonance modeling the low-frequency
//!   head bump characteristic of different tape machine geometries
//! - [`chain::TapeChain`] — Composable chain of the above processors

pub mod bias;
pub mod chain;
pub mod dubly;
pub mod flutter;
pub mod head_bump;
pub mod saturation;

pub use chain::TapeChain;
