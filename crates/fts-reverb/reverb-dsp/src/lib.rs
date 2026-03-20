//! Reverb DSP engine — spatial effects for the plugin suite.
//!
//! Depends on `eq-dsp` for input/output EQ shaping (pre-reverb filtering,
//! post-reverb tone control).
//!
//! # Algorithms (from Airwindows)
//!
//! - [`kplate::KPlate`] — kPlateA plate reverb with dense early reflections,
//!   allpass diffusion network, and modulated delay lines
//! - [`galactic::Galactic`] — Shimmer reverb with pitch-shifted feedback,
//!   freeze mode, and infinite sustain capability
//! - [`verbity::Verbity`] — Hall/room reverb with configurable room size,
//!   pre-delay, and damping characteristics
//! - [`chain::ReverbChain`] — Composable chain of the above reverb processors

pub mod chain;
pub mod galactic;
pub mod kplate;
pub mod verbity;

pub use chain::ReverbChain;
