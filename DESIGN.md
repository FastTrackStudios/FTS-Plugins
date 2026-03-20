# FTS Plugins — Design Requirements

## Plugin Suite

| Plugin | Type | DSP Source | Cross-Plugin Deps |
|--------|------|------------|-------------------|
| **FTS EQ** | Parametric EQ | Airwindows (Capacitor2, BiquadStack, PearEQ, Baxandall2, Air3) | — |
| **FTS Comp** | Compressor | Airwindows (ButterComp2, Pressure6, Thunder, Logical4) | eq-dsp (sidechain EQ) |
| **FTS Limiter** | Limiter/Clipper | Airwindows (ADClip8, ClipOnly2, ClipSoftly, Loud, BlockParty) | — |
| **FTS Tape** | Tape Machine | Airwindows (ToTape8, IronOxide5, Flutter, Dubly) | — |
| **FTS Delay** | Delay/Echo | Airwindows (TapeDelay2, PitchDelay) | eq-dsp (feedback filters) |
| **FTS Reverb** | Reverb | Airwindows (kPlateA, Galactic, Verbity2) | eq-dsp (input/output EQ) |
| **FTS Gate** | Noise Gate | Airwindows (Dynamics zero-crossing gate) | eq-dsp (sidechain filter) |
| **FTS Trigger** | Drum Trigger | Custom (transient detection + sample playback) | eq-dsp (sidechain filter) |
| **FTS Rider** | Vocal Rider | Custom (RMS/LUFS level tracking) | eq-dsp (sidechain), comp-dsp (detection) |

All DSP ported from Airwindows (MIT, Chris Johnson) via [airwin2rack](https://github.com/baconpaul/airwin2rack).

---

## Target Platforms

| Target | Format | GUI | Status |
|--------|--------|-----|--------|
| **macOS** (x86_64 + aarch64) | CLAP, VST3 | Dioxus/Blitz (native) | Primary |
| **Linux** (x86_64) | CLAP, VST3 | Dioxus/Blitz (native) | Primary |
| **Windows** (x86_64) | CLAP, VST3 | Dioxus/Blitz (native) | Primary |
| **Web/WASM** | wasm32 module | Dioxus (web) | Secondary |
| **Embedded Linux** (aarch64/armv7) | CLAP or raw LV2 | Remote GUI (detached) | Future |

### WASM Requirements

- All `*-dsp` crates MUST compile to `wasm32-unknown-unknown`
- No `std::time`, no `std::fs`, no `std::net` in DSP crates
- No `f64::sin()` etc. on WASM — use `libm` crate or inline approximations
- Profile crates are pure data — WASM-safe by design
- UI crates use Dioxus which has native WASM support via `dioxus-web`
- WASM target produces a self-contained module that can run in browser AudioWorklet

### Embedded / Remote GUI Requirements

- DSP runs on embedded hardware (headless, no display)
- GUI runs on a separate machine (laptop, tablet, phone)
- Communication via network (WebSocket, OSC, or custom protocol)
- CLAP's `clap_plugin_gui` supports detached GUI natively
- If LV2: write thin `fts-lv2` wrapper around DSP crates directly (skip rust-lv2, use raw C ABI)
- The `-plugin` crate should be feature-gated: `#[cfg(feature = "gui")]` for native GUI, headless without it

---

## Architecture: Three-Layer Separation

```
┌─────────────────────────────────────────────────────────┐
│                     GUI Layer                            │
│  Themes (visual presentation) + Profile Views            │
│  Framework: Dioxus/Blitz (native), Dioxus-web (WASM)    │
│  Can be DETACHED and run on a different machine          │
├─────────────────────────────────────────────────────────┤
│                   Profile Layer                          │
│  Hardware emulation mappings (Pultec, 1176, etc.)        │
│  Pure data + mapping functions — no DSP, no UI           │
│  WASM-safe, embedded-safe                                │
├─────────────────────────────────────────────────────────┤
│                     DSP Layer                            │
│  Airwindows algorithms + custom analysis                 │
│  Zero framework deps — portable to any target            │
│  WASM-safe, embedded-safe, LV2-safe                      │
└─────────────────────────────────────────────────────────┘
```

### Constraint: DSP crates must NEVER depend on

- `nih_plug` or any plugin framework
- `dioxus` or any GUI framework
- `std::thread`, `std::sync::Mutex` (use `atomic` if needed)
- `tokio` or any async runtime
- Platform-specific APIs

### Constraint: Profile crates must NEVER depend on

- GUI frameworks
- Plugin frameworks
- Their sibling UI or plugin crates

### Constraint: Analysis crates depend on

- Their sibling DSP crate (for the detection/processing algorithms)
- `daw` + `daw-control-sync` (for AudioAccessor and AutomationService)
- NOT on nih-plug or GUI frameworks

---

## Plugin Formats

### CLAP (Primary)

- Built via nih-plug's CLAP backend
- CLAP ID format: `com.fasttrackstudio.<plugin>`
- Support CLAP extensions:
  - `clap_plugin_gui` — for native + detachable GUI
  - `clap_plugin_params` — automation
  - `clap_plugin_state` — preset save/load
  - `clap_plugin_note_ports` — for Trigger (MIDI output)

### VST3 (Secondary)

- Built via nih-plug's VST3 backend
- VST3 class IDs: 16-byte identifiers per plugin
- Subcategories mapped per plugin type

### WASM

- Separate build target, not via nih-plug
- `fts-wasm` crate wraps DSP + profiles + Dioxus-web UI
- Exposes `process()`, `set_param()`, `get_state()` via wasm-bindgen
- Runs in AudioWorklet (DSP) + main thread (UI)
- No plugin framework — direct integration with Web Audio API

### LV2 (Future, Embedded)

- Thin `fts-lv2` crate using raw C ABI (`lv2_descriptor`, `lv2_handle`)
- Skip rust-lv2 (abandoned) — write ~200 lines of FFI directly
- Generate `.ttl` metadata at build time via xtask
- GUI-less on embedded; remote GUI connects over network

---

## Offline Analysis (DAW Integration)

Plugins that support offline analysis: **Gate, Trigger, Rider** (and potentially all plugins for A/B preview).

### Workflow

```
1. User selects track(s) in REAPER
2. Plugin reads audio via AudioAccessor API (daw crate)
   - AudioAccessorService.create_track_accessor()
   - AudioAccessorService.get_samples() — full track in one pass
3. DSP analysis runs on the full audio (with perfect lookahead)
   - Gate: detect open/close points
   - Trigger: detect transients + extract velocity
   - Rider: compute ideal gain curve
4. Results written as automation via AutomationService
   - Gate: mute automation (square envelope)
   - Trigger: MIDI notes or velocity automation
   - Rider: volume automation (smooth bezier curves)
5. User can preview, tweak thresholds, re-analyze
```

### Implementation Status

| Component | Status |
|-----------|--------|
| AudioAccessor (read samples) | Implemented in daw-reaper |
| AutomationService (write envelopes) | Proto complete, REAPER impl is STUB |
| Gate offline analyzer | Not started |
| Trigger offline analyzer | Not started |
| Rider offline analyzer | Not started |

**Blocker:** AutomationService REAPER implementation needs `InsertEnvelopePoint`, `SetEnvelopePoint`, `DeleteEnvelopePointRange` bindings before offline analysis can write results.

---

## Profile + Theme System

### Profiles (behavior)

A profile defines HOW a hardware unit behaves:
- Which controls exist (knobs, switches, stepped selectors)
- How each control maps to DSP parameters (direct, stepped, compound)
- What constraints are active (locked frequencies, clamped ranges)

Every plugin has a **Control** profile (full access, Pro-Q/APComp style) plus hardware emulation profiles.

| Plugin | Profiles |
|--------|----------|
| EQ | Control, Pultec EQP-1A, Neve 1073, SSL E-Series, API 550A |
| Comp | Control, UREI 1176, LA-2A, SSL Bus Comp |
| Limiter | Control, L2-style, Mastering |
| Tape | Control, Studer A800, Ampex ATR-102 |
| Delay | Control, Space Echo RE-201, Echoplex |
| Reverb | Control, EMT 140, Lexicon 480 |
| Gate | Control, NS10 Strip |
| Trigger | Control, Drum Replacer |
| Rider | Control, Vocal Rider |

### Themes (visuals)

A theme defines HOW a profile LOOKS:
- Layout, colors, fonts, knob style, background
- Multiple themes can render the same profile
- Themes are independent of DSP and profiles

Default themes:
- **FastTrack** — unified brand identity across all profiles
- **Skeuomorphic** — photorealistic hardware appearance (per profile)
- **Minimal** — clean flat design (per profile)

---

## Crate Dependency Map

```
fts-dsp (zero deps, WASM-safe)
 │
 ├── eq-dsp
 │   ├── eq-profiles
 │   ├── eq-ui ──→ fts-plugin-core, fts-themes
 │   ├── eq-plugin ──→ nih_plug
 │   └── (no analysis crate — EQ is not an analysis plugin)
 │
 ├── comp-dsp ──→ eq-dsp (sidechain)
 │   ├── comp-profiles
 │   ├── comp-ui ──→ fts-plugin-core, fts-themes
 │   └── comp-plugin ──→ nih_plug
 │
 ├── gate-dsp ──→ eq-dsp (sidechain)
 │   ├── gate-profiles
 │   ├── gate-ui ──→ fts-plugin-core, fts-themes
 │   ├── gate-plugin ──→ nih_plug
 │   └── gate-analysis ──→ daw, daw-control-sync
 │
 ├── trigger-dsp ──→ eq-dsp (sidechain)
 │   ├── trigger-profiles
 │   ├── trigger-ui ──→ fts-plugin-core, fts-themes
 │   ├── trigger-plugin ──→ nih_plug
 │   └── trigger-analysis ──→ daw, daw-control-sync
 │
 ├── rider-dsp ──→ eq-dsp, comp-dsp (detection)
 │   ├── rider-profiles
 │   ├── rider-ui ──→ fts-plugin-core, fts-themes
 │   ├── rider-plugin ──→ nih_plug
 │   └── rider-analysis ──→ daw, daw-control-sync
 │
 ├── limiter-dsp
 │   └── ...
 ├── tape-dsp
 │   └── ...
 ├── delay-dsp ──→ eq-dsp (feedback filters)
 │   └── ...
 └── reverb-dsp ──→ eq-dsp (I/O EQ)
     └── ...

fts-plugin-core (nih_plug + nih_plug_dioxus re-exports)
fts-themes (Dioxus theme engine)
```

---

## Build Targets (xtask)

```
just build eq          # Build EQ plugin (CLAP + VST3)
just build comp        # Build Compressor
just build all         # Build all 9 plugins
just build-wasm eq     # Build EQ for WASM
just test dsp          # Test all DSP crates (unit tests)
just analyze gate      # Run gate offline analysis (requires REAPER)
```
