//! FTS NAM — Neural Amp Modeler plugin DSP.
//!
//! Two modes of operation:
//!
//! 1. **NamChain** (Ratatouille-style) — dual NAM slots with dual IR, blend,
//!    phase correction, delta delay. Simple fixed signal path.
//!
//! 2. **NamGraph** (sandbox/pedalboard) — arbitrary node graph where NAM models,
//!    IRs, gain stages, and mixers can be wired together in any configuration.
//!    Build chains like: drive → preamp → amp → IR, or run 3 amps in parallel
//!    and blend them, etc.

pub mod chain;
pub mod convolver;
pub mod graph;
pub mod resampler;
pub mod slot;
