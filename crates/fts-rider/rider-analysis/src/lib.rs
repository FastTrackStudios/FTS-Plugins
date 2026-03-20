//! Offline vocal rider analysis — reads audio via AudioAccessor, computes ideal
//! gain ride curve, writes volume automation to DAW envelopes.
//!
//! Enables batch vocal leveling on entire tracks with perfect lookahead that
//! would be impossible in real-time processing.

pub mod offline;
