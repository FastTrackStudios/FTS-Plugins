# Trigger Plugin Specification

Requirements for the FTS Trigger (drum sample triggering) plugin.

## Trigger DSP Engine

r[trigger.chain.signal-flow]
Signal flow: Input → Sidechain (eq-dsp bandpass to isolate drum) → Onset Detector → Velocity Extraction → Sample Playback → Mix with Original.

### Onset Detection

r[trigger.detector.transient]
Detect transients using a combination of energy rise detection and spectral flux. Must distinguish between true onsets and sustained energy.

r[trigger.detector.retrigger-prevent]
Enforce a configurable minimum interval between triggers (1ms-200ms) to prevent double-triggering on flammy hits.

r[trigger.detector.sensitivity]
Provide a sensitivity parameter that controls the onset detection threshold relative to the local noise floor.

### Velocity

r[trigger.velocity.energy]
Extract trigger velocity from the transient energy envelope. Map the detected energy to MIDI velocity (1-127) using a configurable curve.

r[trigger.velocity.curve]
Support linear, logarithmic, exponential, and custom velocity curves.

### Sample Playback

r[trigger.sampler.round-robin]
Support round-robin sample selection from a loaded sample set to avoid machine-gun effect.

r[trigger.sampler.velocity-layers]
Support velocity-layered samples (different samples triggered at different velocity ranges).

r[trigger.sampler.mix]
Support blend between original audio and triggered sample (replace, layer, or blend modes).

## Trigger Profiles

r[trigger.profile.control]
Full access: sidechain filter, sensitivity, retrigger interval, velocity curve, sample selection, mix mode, output.

r[trigger.profile.drum-replacer]
Simplified drum replacer mode: threshold, velocity curve, sample selector, replace/layer/blend switch.

## Trigger Plugin

r[trigger.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.trigger`.

r[trigger.plugin.vst3]
Export as VST3 with subcategories Dynamics and Instrument.

r[trigger.plugin.midi-output]
Support MIDI note output so triggers can drive external instruments or be recorded as MIDI.

## Trigger Offline Analysis

r[trigger.offline.full-track]
Read an entire track via AudioAccessor and detect all transients with perfect lookahead.

r[trigger.offline.write-midi]
Write detected triggers as MIDI notes to a new take/track in the DAW, with correct timing and velocity.

r[trigger.offline.write-automation]
Alternatively, write trigger velocity as automation on a parameter envelope.
