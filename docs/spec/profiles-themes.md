# Profiles & Themes Specification

Requirements for the profile and theme system shared across all plugins.

## Profile System

r[profile.trait]
All profiles must implement a `Profile` trait with `id()`, `name()`, `controls()`, and `constraints()` methods.

r[profile.control-type.direct]
Support direct parameter mapping: one control → one DSP parameter with a continuous range.

r[profile.control-type.stepped]
Support stepped parameter mapping: one control → one DSP parameter with discrete values and labels.

r[profile.control-type.compound]
Support compound parameter mapping: one control → multiple DSP parameters on linked curves. Each target parameter has its own transfer function from the control value.

r[profile.constraint.fixed]
Support fixed constraints that lock a DSP parameter to a constant value when a profile is active.

r[profile.constraint.clamped]
Support clamped constraints that restrict a DSP parameter to a narrower range than the core supports.

r[profile.constraint.stepped-only]
Support stepped-only constraints that restrict a DSP parameter to discrete values.

r[profile.switch-interpolation]
When switching between profiles at runtime, core parameters must smoothly interpolate to the new profile's constraint values. No audio discontinuity.

r[profile.control-always-available]
Every plugin must have a "Control" profile that provides full parametric access with zero constraints. This is the advanced/Pro view.

r[profile.advanced-overlay]
When a hardware profile is active, an expandable "Advanced" panel must be available showing the full core parameter state and what the profile's constraints are producing.

## Theme System

r[theme.trait]
All themes must implement a `Theme` trait with `id()`, `name()`, and `profile_id()` (which profile this theme renders).

r[theme.decoupled-from-profile]
Multiple themes can reference the same profile. Switching themes changes only visuals, never DSP behavior.

r[theme.fasttrack-default]
A "FastTrack" branded theme must exist for every profile, providing a consistent visual identity across all hardware emulations.

r[theme.no-dsp-dependency]
Theme crates must never depend on DSP crates. They depend only on profile crates (for control definitions) and the GUI framework.
