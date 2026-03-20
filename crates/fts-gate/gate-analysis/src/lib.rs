//! Offline gate analysis — reads audio via AudioAccessor, runs gate detection,
//! writes open/close automation to DAW envelopes.
//!
//! Can generate gate automation for an entire track in one pass, enabling
//! perfect lookahead-aware gating that would be impossible in real-time.

pub mod offline;
