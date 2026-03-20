//! Compressor hardware profiles — parameter mappings and constraints.
//!
//! A profile defines:
//! - Which controls appear (knobs, switches, stepped selectors)
//! - How each control maps to [`comp_dsp::CompChain`] parameters
//! - Constraints (locked ratios, attack/release curves, linked params)
//!
//! Profiles are pure data + mapping functions. No GUI, no framework deps.

pub mod control;
pub mod core;
pub mod la2a;
pub mod ssl_bus;
pub mod urei_1176;

pub use self::core::{Constraint, ParamMapping, Profile, ProfileControl};
