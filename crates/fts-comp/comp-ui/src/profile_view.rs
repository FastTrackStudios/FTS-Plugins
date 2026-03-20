//! Profile view — renders a hardware profile's controls as a themed GUI.
//!
//! This is a generic renderer that takes a `Profile` definition and creates
//! appropriate UI controls for each `ProfileControl` entry.

// TODO: Generic profile renderer that iterates over Profile::controls()
// and creates CompSlider / SegmentButton / Toggle for each, respecting
// ParamMapping (Direct, Stepped, Compound) and Constraints.
//
// This will be implemented when the profile system is fleshed out
// (1176, LA-2A, SSL bus profiles).
