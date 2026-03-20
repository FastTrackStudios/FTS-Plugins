//! Noise gate profiles — parameter mappings and constraints.
//!
//! A profile defines which controls appear, how they map to
//! [`gate_dsp::GateChain`] parameters, and any constraints.
//!
//! Profiles are pure data + mapping functions. No GUI, no framework deps.

pub mod control;
pub mod core;
pub mod ns10_strip;
