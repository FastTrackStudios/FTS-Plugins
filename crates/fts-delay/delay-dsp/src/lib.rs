//! Delay DSP engine — time-based effects for the plugin suite.
//!
//! Based on techniques from qdelay (tiagolr) and ChowDSP's AnalogTapeModel.
//!
//! - [`tape_delay::TapeDelay`] — Tape echo with wow/flutter, feedback filtering,
//!   saturation, ducking, and diffusion
//! - [`pitch_delay::PitchDelay`] — Per-repeat pitch shifting with granular crossfade
//! - [`chain::DelayChain`] — Full stereo delay with ping-pong, swing, and mix

pub mod chain;
pub mod modulation;
pub mod pitch_delay;
pub mod tape_delay;

pub use chain::DelayChain;
