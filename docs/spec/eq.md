# EQ Plugin Specification

Requirements for the FTS EQ plugin (eq-dsp, eq-profiles, eq-ui, eq-plugin).

## EQ DSP Engine

r[eq.chain.signal-flow]
Signal flow must be: HPF → Low Shelf → Parametric Bands → High Shelf → Air Band → LPF.

r[eq.chain.max-bands]
Support at least 24 simultaneous parametric bands.

r[eq.chain.dynamic-bands]
Bands must be dynamically addable and removable at runtime without audio glitches.

### Parametric Band (BiquadStack)

r[eq.band.standard]
Each parametric band must support frequency (20Hz-20kHz), gain (+/-24dB), and Q (0.1-30) parameters.

r[eq.band.nonlinear]
In "analog" mode, the Q coefficient must be modulated by signal level, replicating BiquadStack's signal-dependent resonance behavior.

r[eq.band.filter-types]
Each band must support selectable filter types: peak, low shelf, high shelf, highpass, lowpass, bandpass, notch.

### Shelving (Baxandall2)

r[eq.shelf.baxandall]
Low and high shelf filters must use the Baxandall2 algorithm where the corner frequency tracks the gain amount.

r[eq.shelf.range]
Shelf gain range must be at least +/-24dB.

### Capacitor Filter (Capacitor2)

r[eq.capacitor.nonlinear]
HPF and LPF must use the Capacitor2 algorithm with voltage-dependent capacitance modeling (signal-level-dependent filter coefficients).

r[eq.capacitor.rotation]
The Capacitor2 implementation must use the 6-pole rotation ("gearbox") pattern that cycles through different IIR pole combinations each sample to prevent resonance artifacts.

### PearEQ

r[eq.pear.slew-topology]
Implement Chris Johnson's PearEQ slew-based filter topology as an alternative "analog character" mode for the EQ. This uses slew-rate feedback rather than standard z-transform coefficients.

### Air Band (Air3/Air4)

r[eq.air.kalman]
Implement the Air3/Air4 Kalman-filter-based high-frequency enhancement as an "air" band at the end of the EQ chain.

## EQ Profiles

r[eq.profile.control]
The Control profile provides full parametric access with no constraints (Pro-Q style). All parameters are exposed, bands are dynamically addable.

r[eq.profile.pultec]
The Pultec EQP-1A profile maps to 2 bands: low boost/atten at stepped frequencies (20/30/60/100 Hz) and high boost at stepped frequencies (3k-16k Hz) with bandwidth control.

r[eq.profile.neve-1073]
The Neve 1073 profile maps to 3 bands + HPF with fixed frequency selections per band (stepped switches).

r[eq.profile.ssl-e]
The SSL E-Series profile maps to 4 bands (LF shelf, LMF peak, HMF peak, HF shelf) + HPF/LPF.

r[eq.profile.api-550a]
The API 550A profile maps to 3 bands with proportional Q (Q narrows as gain increases).

## EQ Plugin

r[eq.plugin.clap]
Export as CLAP plugin with ID `com.fasttrackstudio.eq` and feature tags `audio-effect`, `equalizer`, `stereo`.

r[eq.plugin.vst3]
Export as VST3 plugin with subcategories Fx and Eq.

r[eq.plugin.profile-switch]
Switching profiles at runtime must smoothly interpolate core parameters to the new profile's constraints without audio discontinuity.

r[eq.plugin.advanced-view]
When a hardware profile is active, an expandable "Advanced" panel must show the full frequency response curve and the underlying core parameter values.
