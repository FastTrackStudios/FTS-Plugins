# Platform & Format Specification

Requirements for platform targets and plugin format support.

## Desktop Plugin Formats

r[platform.clap]
All plugins must export as CLAP using nih-plug's CLAP backend. CLAP is the primary format.

r[platform.vst3]
All plugins must export as VST3 using nih-plug's VST3 backend. VST3 is the secondary format.

r[platform.clap.detachable-gui]
CLAP plugins must support the `clap_plugin_gui` extension with detachable GUI capability, enabling headless DSP with remote GUI control.

## Desktop OS Targets

r[platform.macos]
Support macOS on both x86_64 (Intel) and aarch64 (Apple Silicon).

r[platform.linux]
Support Linux on x86_64.

r[platform.windows]
Support Windows on x86_64.

## WASM Target

r[platform.wasm.dsp-compile]
All `*-dsp` crates must compile to `wasm32-unknown-unknown` without modification.

r[platform.wasm.no-std-deps]
DSP crates must not use `std::time`, `std::fs`, `std::net`, or `std::thread` to maintain WASM compatibility.

r[platform.wasm.libm]
Use `libm` crate for transcendental math functions on WASM targets.

r[platform.wasm.audioworklet]
WASM DSP must be usable from a Web Audio API AudioWorklet for real-time browser processing.

r[platform.wasm.ui]
WASM UI must use Dioxus web target for browser-based plugin interfaces.

## Embedded / Remote GUI

r[platform.embedded.headless]
Plugins must support headless operation (DSP only, no GUI) for embedded hardware deployment.

r[platform.embedded.remote-gui]
When running headless, a remote GUI (running on a separate machine) must be able to connect and control all parameters.

r[platform.embedded.clap-preferred]
CLAP is the preferred format for embedded targets. If LV2 is needed, write a thin `fts-lv2` wrapper using raw C ABI around the DSP crates.

## Feature Gating

r[platform.feature.gui]
Plugin crates must use `#[cfg(feature = "gui")]` to gate all GUI code. Building without the `gui` feature must produce a headless plugin.

r[platform.feature.analysis]
Analysis crates (gate-analysis, trigger-analysis, rider-analysis) must only be compiled when the `analysis` feature is enabled, since they depend on the daw crate.
