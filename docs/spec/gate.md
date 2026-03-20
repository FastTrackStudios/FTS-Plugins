# Gate Plugin Specification

Requirements for the FTS Gate plugin.

## Gate DSP Engine

r[gate.chain.signal-flow]
Signal flow: Input → Sidechain (eq-dsp HPF/LPF) → Detector → Gate Envelope (attack/hold/release) → Gain Modulation → Output.

### Detector

r[gate.detector.zero-crossing]
Implement zero-crossing-aware gate detection from Airwindows Dynamics: gate state changes must prefer zero crossings to minimize click artifacts.

r[gate.detector.hysteresis]
Support separate open and close thresholds (hysteresis) to prevent gate chatter near threshold.

r[gate.detector.lookahead]
Support configurable lookahead (0-10ms) via delay buffer so the gate can open before the transient arrives.

### Envelope

r[gate.envelope.attack]
Gate attack (open) time: 0.01ms to 100ms with cos() shaping for smooth onset.

r[gate.envelope.hold]
Gate hold time: 0ms to 2000ms. Gate stays fully open for this duration after signal drops below close threshold.

r[gate.envelope.release]
Gate release (close) time: 1ms to 2000ms with configurable curve shape.

r[gate.envelope.range]
Support configurable gate range (depth): 0dB to -inf. At less than full range, the gate attenuates rather than fully muting.

## Gate Profiles

r[gate.profile.control]
Full access: open/close thresholds, attack, hold, release, range, sidechain HPF/LPF, lookahead, sidechain listen.

## Gate Plugin

r[gate.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.gate`.

r[gate.plugin.vst3]
Export as VST3 with subcategory Dynamics.

r[gate.plugin.sidechain-listen]
Provide a sidechain listen mode that solos the filtered sidechain signal for dialing in the filter.

## Gate Offline Analysis

r[gate.offline.full-track]
Read an entire track via AudioAccessor in one pass and run gate detection with perfect lookahead (the full audio is available).

r[gate.offline.write-automation]
Write gate open/close results as mute automation (square envelope with configurable fade times) to the DAW via AutomationService.

r[gate.offline.preview]
Allow previewing the gated result before committing automation, with adjustable thresholds that re-analyze in real time.
