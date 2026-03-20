# Compressor Plugin Specification

Requirements for the FTS Compressor plugin.

## Compressor DSP Engine

r[comp.chain.signal-flow]
Signal flow: Input Gain → Sidechain HPF (eq-dsp) → Detector → Character Engine → Mix → Output Gain.

r[comp.chain.sidechain-eq]
Sidechain filtering uses eq-dsp's EqChain. At minimum, a variable HPF (off/60/90/150/300 Hz) to prevent bass-driven pumping.

r[comp.chain.parallel-mix]
Support parallel compression via a dry/wet mix control (0-100%).

### ButterComp2 (Opto Character)

r[comp.butter.pos-neg]
Track positive and negative half-cycles independently with separate gain control variables, matching ButterComp2's unique topology.

r[comp.butter.output-recovery]
Recovery speed must be inversely proportional to output level: `divisor = compfactor / (1.0 + fabs(lastOutput))`, replicating opto-compressor photocell behavior.

### Pressure6 (FET/VCA Character)

r[comp.pressure.mu-law]
Implement mu-law compression with adaptive speed control matching Pressure6's algorithm.

r[comp.pressure.sin-clip]
Apply sin() soft clipping on output: `sin(clamp(sample, -pi/2, pi/2))`.

### Thunder (Bass-Aware)

r[comp.thunder.bass-split]
Split the signal into lows and rest using nonlinear IIR. Highpass the compressor sidechain (subtract bass), then add bass back after compression.

r[comp.thunder.no-pump]
Bass reinforcement must be modulated by the mono bass envelope to prevent low-frequency pumping.

### Logical4 (SSL Bus)

r[comp.logical.cascaded]
Implement cascaded ButterComp stages with ratio control that crossfades between 1, 2, and 3 stages.

r[comp.logical.power-sag]
Include power sag modeling (transformer droop simulation) from Desk.

## Compressor Profiles

r[comp.profile.control]
Full parametric access: threshold, ratio, attack, release, knee, character selector, sidechain HPF, parallel mix, output gain.

r[comp.profile.1176]
UREI 1176 profile: Input/Output knobs (input drives threshold), stepped attack/release (1-7), ratio buttons (4/8/12/20 + all-buttons-in). Maps to Pressure6 character.

r[comp.profile.la2a]
LA-2A profile: 2 knobs only — Peak Reduction (compound mapping: one knob drives threshold + ratio + knee on linked curves) and Gain. Compress/Limit switch. Maps to ButterComp2 character.

r[comp.profile.ssl-bus]
SSL Bus Comp profile: Threshold, Makeup, stepped Ratio (2/4/10), stepped Attack (0.1-30ms), stepped Release (0.1-1.2s + Auto). Maps to Logical4 character.

## Compressor Plugin

r[comp.plugin.clap]
Export as CLAP with ID `com.fasttrackstudio.comp`, feature tags `audio-effect`, `compressor`, `stereo`.

r[comp.plugin.vst3]
Export as VST3 with subcategory Dynamics.

r[comp.plugin.gr-meter]
Display real-time gain reduction metering in all views.

r[comp.plugin.transfer-curve]
Control view must show the compression transfer curve (input vs output level).
