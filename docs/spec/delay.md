# Delay Plugin Specification

Requirements for the FTS Delay plugin.

## Delay DSP Engine

r[delay.chain.signal-flow]
Signal flow: Input (optional saturation) → Delay Line → Feedback Path (filter + saturation + auto-limiting) → Output Filter → Mix.

### TapeDelay2

r[delay.tape.variable-speed]
Implement variable-speed playback with linear interpolation between write and read positions on an 88200-sample (2s at 44.1kHz) buffer.

r[delay.tape.input-vibrato]
Vibrato speed must be modulated by input signal level (`sweep += 0.05 * input^2`), creating dynamics-responsive wow/flutter.

r[delay.tape.dual-filter]
Two biquad bandpass filters: one in the feedback path (regen filter) and one on the output. Output filter Q is scaled by the golden ratio relative to the regen filter.

### PitchDelay

r[delay.pitch.shift]
Support per-repeat pitch shifting (up or down) derived from a bipolar speed control.

r[delay.pitch.feedback-limit]
Feedback must self-limit using `feedback * (3.0 - fabs(regen * 2.0))` to prevent runaway oscillation while allowing musical self-oscillation.

## Delay Profiles

r[delay.profile.control]
Full access: time (ms or tempo-synced), feedback, filter frequency/resonance, modulation depth/rate, pitch shift, saturation, stereo width, mix.

r[delay.profile.space-echo]
Space Echo RE-201 profile: mode selector (head combinations), repeat rate, intensity, tone.

r[delay.profile.echoplex]
Echoplex profile: simplified tape echo with wow/flutter character.

## Delay Plugin

r[delay.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.delay`, feature tag `delay`.

r[delay.plugin.vst3]
Export as VST3 with subcategory Delay.

r[delay.plugin.tempo-sync]
Support tempo-synced delay times (1/4, 1/8, 1/16, dotted, triplet) reading host tempo from CLAP transport.
