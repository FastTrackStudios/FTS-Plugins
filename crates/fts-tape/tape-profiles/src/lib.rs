//! Tape machine hardware profiles — parameter mappings and constraints.
//!
//! A profile defines:
//! - Which controls appear (knobs, switches, stepped selectors)
//! - How each control maps to [`tape_dsp::TapeChain`] parameters
//! - Constraints (tape speed, head type, bias settings)
//!
//! Profiles are pure data + mapping functions. No GUI, no framework deps.

pub mod ampex_atr102;
pub mod control;
pub mod core;
pub mod studer_a800;

pub use self::core::{Constraint, ParamMapping, Profile, ProfileControl};
