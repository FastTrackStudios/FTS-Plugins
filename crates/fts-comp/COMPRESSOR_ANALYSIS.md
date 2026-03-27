# FTS Compressor — Analysis & Comparison with FabFilter Pro-C 3

## Goal

Match FabFilter Pro-C 3's compression behavior across all 14 styles, starting with Clean (style 0). Reference data has been captured — only `compare-compressor` needs to run during iteration.

## Reference Data

Location:
```
/home/cody/Development/FastTrackStudio/fts-analyzer/reference/pro-c3-clean/
```

### What was captured

- **34 musically-spaced frequencies**: 20, 30, 40, 50, 60, 80, 100, 120, 140, 160, 180, 200, 240, 280, 320, 400, 480, 560, 640, 800, 1000, 1250, 1500, 2000, 2500, 3000, 4000, 5000, 6000, 8000, 10000, 12000, 16000, 20000 Hz
- **196 attack/release scenarios**: 14 attack × 14 release test points at every character boundary (see `comp-dsp/src/character.rs`)
- **Clean style** (style=0), Auto Gain off (param 19=0)
- **Signal**: sine pulse, -6 dB high / -20 dB low, 240ms each phase, 3s duration
- **Resolution**: 1ms time steps, gain reduction quantized to u8 (~0.21 dB precision)

### File format

- **Binary** (`.bin`): one file per scenario, 102 KB each. Header: `[num_freqs: u32 LE][samples_per_freq: u32 LE]`, then `u8 × num_freqs × samples_per_freq` (frequency-major). Values map linearly from u8 0–255 to -48.0–6.0 dB.
- **CSV** (`capture.csv`): single file with all data. Columns: `scenario,time_ms,20hz,30hz,...,20000hz`. Values at 0.1 dB precision. Not in git (regenerated from binary).
- **Metadata** (`metadata.json`): signal parameters, frequency list, scenario definitions with Pro-C 3 normalized param values.

Total size: ~20 MB (binary) + ~113 MB (CSV).

### Attack/release scenario naming

Scenarios are named `atk-{ms}_rel-{ms}` using the display-value milliseconds, e.g. `atk-0.01ms_rel-0ms`, `atk-24ms_rel-119ms`.

The 14 attack test points (ms): 0.01, 2, 3, 5, 6, 15, 16, 24, 25, 39, 40, 59, 60, 240
The 14 release test points (ms): 0, 9, 10, 19, 20, 24, 35, 49, 50, 79, 80, 119, 120, 199, 200, 400

Note: Pro-C 3 release values below ~19ms all map to normalized 0.0 (the plugin's floor). Some scenarios may be effectively duplicated.

### Key observations from reference data

**Frequency-dependent behavior:**
- 20–100 Hz: noticeably less compression (e.g. -2.3 dB vs -5.3 dB at 1 kHz for fast attack/fast release)
- 100–200 Hz: transition zone
- 200+ Hz: consistent, frequency-independent behavior
- This suggests Clean style has a gentle sidechain HPF or frequency-weighted detector

## How to Iterate

### Quick start: build + compare

All commands from the fts-analyzer directory:

```bash
cd /home/cody/Development/FastTrackStudio/fts-analyzer
```

**One-liner: build FTS-Comp, build analyzer CLI, run comparison:**

```bash
# Build the plugin
cd /home/cody/Development/FastTrackStudio/FTS-Plugins && \
  cargo build --release --package comp-plugin && \
  cp target/release/libcomp_plugin.so target/bundled/comp-plugin.clap

# Run comparison (use pro-c3-clean-nosc for no-sidechain-HPF reference)
cd /home/cody/Development/FastTrackStudio/fts-analyzer && \
  cargo run --release --package fts-analyzer-cli -- compare-compressor \
    /home/cody/Development/FastTrackStudio/FTS-Plugins/target/bundled/comp-plugin.clap \
    --reference ./reference/pro-c3-clean-nosc \
    --out ./results/fts-comp-clean-nosc \
    --param-remap 7=Attack --param-remap 8=Release \
    --param 602006635=-18.0 --param 108285963=4.0 --param 3296579=18.0 \
    --param 660387005=4.0 --param 1955982213=0.0 --param 1908880135=0.0 \
    --tolerance-db 1.0
```

**Current result**: 6661/6664 (99.95%) across 196 scenarios × 34 frequencies at 1.0 dB tolerance.

**3 remaining failures**: `atk-0.01ms_rel-0ms` at 20/30/40 Hz (RMS 1.06–1.23 dB).
These represent the absolute extreme — instantaneous attack + minimum release at very low frequencies —
where the power-domain GR smoothing produces a slightly different steady-state from Pro-C 3.
Varying `SMOOTH_POWER` (0.70–0.90) does not improve these 3 cases.

### Reading comparison output

```
--- Scenario: atk-0.01ms_rel-0ms ---
  [   1/34]     20.0 Hz  PASS  rms=0.123 dB  max=0.456 dB
  [  20/34]   1000.0 Hz  FAIL  rms=2.345 dB  max=4.567 dB
  [  34/34]  20000.0 Hz  PASS  rms=0.089 dB  max=0.234 dB
  -> 30/34 passed (worst RMS diff: 2.345 dB)
```

- **rms**: RMS of gain reduction difference across the full 3s time series
- **max**: worst single-sample gain reduction difference
- Failures always print; passes print every 20th + first + last

### Parameter mapping

When running `compare-compressor`, you pass FTS-Comp parameters via `--param id=value`. FTS-Comp is a CLAP plugin, so parameter IDs come from `comp-plugin/src/lib.rs`.

**Critical difference**: Pro-C 3 uses normalized 0–1 values for attack/release/ratio. FTS-Comp uses plain values (ms, ratio). The reference data scenarios encode Pro-C 3's normalized values in `metadata.json`. The compare tool regenerates the same test signals and runs them through FTS-Comp — it does NOT try to map Pro-C 3 params to FTS-Comp params. Instead, you set FTS-Comp params to produce equivalent behavior.

## Pro-C 3 Parameter Reference

| ID | Name | Pro-C 3 Range | Default | FTS-Comp Equivalent |
|----|------|---------------|---------|---------------------|
| 0 | Style | 0–13 | 1 | **NOT IMPLEMENTED** |
| 1 | Threshold | -60 to 0 dB | -18 | `threshold_db` (-60 to 0) |
| 2 | Auto Threshold | 0–1 | 0 | Not implemented |
| 4 | Ratio | 0–1 (normalized) | 0.60 | `ratio` (1–20, plain) |
| 5 | Knee | 0–72 dB | 18 | `knee_db` (0–30, needs extension) |
| 6 | Range | 0–60 dB | 60 | Not implemented |
| 7 | Attack | 0–1 (normalized) | 0.10 | `attack_ms` (0.01–300 ms, plain) |
| 8 | Release | 0–1 (normalized) | 0.40 | `release_ms` (1–3000 ms, plain) |
| 9 | Auto Release | 0–1 | 0 | Not implemented |
| 10 | Lookahead | 0–20 ms | 0 | Not implemented |
| 11 | Hold | 0–1 (normalized) | 0 | Not implemented |
| 12 | Character | 0–3 | 0 | Not implemented |
| 19 | Auto Gain | 0–1 | 1 | `auto_makeup` (0–1) |
| 88 | Mix | 0–2 | 1 | `fold` (0–1) |

Use `resolve-params` to convert Pro-C 3 display text to normalized values:
```bash
./target/release/fts-analyzer-cli resolve-params \
  '/home/cody/.clap/yabridge/FabFilter Pro-C 3.clap' \
  --param-id 7 --text "10 ms" "50 ms" "200 ms"
```

## FTS-Comp DSP Architecture

All DSP code:
```
FTS-Plugins/crates/fts-comp/comp-dsp/src/
├── compressor.rs   # Core: detector + gain computer + saturation + mix
├── detector.rs     # Envelope follower with exponential attack/release
├── gain.rs         # Gain reduction curve (threshold, ratio, soft knee, inertia)
├── character.rs    # Attack/release character ranges and test points
└── lib.rs          # Module exports
```

Plugin wrapper: `comp-plugin/src/lib.rs`

### Current signal flow (per sample)

1. Input gain
2. Level detection (feedforward + optional feedback blend)
3. Gain reduction computation (threshold / ratio / soft knee / inertia)
4. Channel linking (blend individual GR with max GR)
5. Apply gain reduction
6. Output saturation (tanh soft clip with ceiling)
7. Parallel mix (fold: dry/wet blend)
8. Output gain

### What's implemented
- Feedforward + feedback detection
- Exponential attack/release envelope
- Soft knee with quadratic interpolation
- Inertia (momentum-based GR smoothing)
- Stereo channel linking
- tanh soft clip saturation
- Parallel mix (dry/wet)
- Sidechain HPF
- Auto makeup gain

### Missing for Pro-C 3 parity
1. **Style parameter** (0–13) that reconfigures compressor topology per style
2. **Range** (max GR limit) — Pro-C 3 defaults to 60 dB
3. **Hold** time
4. **Lookahead**
5. **Auto Release**
6. **Auto Threshold**
7. **Character** (harmonic coloring / drive)
8. **Knee range extension** — Pro-C 3 goes to 72 dB, FTS-Comp only 30 dB

## Implementation Strategy

### Phase 1: Match Clean Style

Focus on getting the basic compression curves to match Pro-C 3 Clean with default settings (threshold=-18 dB, ratio≈4:1, knee=18 dB):

1. **Detector envelope shape** — get attack/release curves right. The comparison across 196 attack/release combos will show exactly where the envelope follower diverges.
2. **Frequency-dependent behavior** — Clean style shows reduced compression below 200 Hz. Likely needs a gentle sidechain HPF (~80-100 Hz, shallow slope).
3. **Gain curve** — match the ratio/knee/threshold interaction. Pro-C 3's knee goes to 72 dB; extend FTS-Comp's knee range.
4. **Start with mid-range scenarios** (e.g. `atk-16ms_rel-50ms`) where behavior is most predictable, then work outward to extremes.

### Phase 2: Add Style System

1. Add `Style` IntParam (0–13) to FTS-Comp
2. Each style changes:
   - Detector topology (feedforward vs feedback blend)
   - Envelope curve shape (exponential, optical, program-dependent)
   - Gain curve shape
   - Saturation character
3. Capture reference data for each style:
   ```bash
   cd /home/cody/Development/FastTrackStudio/fts-analyzer
   ./target/release/fts-analyzer-cli capture-compressor \
     '/home/cody/.clap/yabridge/FabFilter Pro-C 3.clap' \
     --out ./reference/pro-c3-classic \
     --scenarios ./reference/pro-c3-full-scenarios.json \
     --param 0=1 --param 19=0 \
     --gain-high -6 --gain-low -20 --duration 3
   ```

### Phase 3: Remaining Features

- Range, Hold, Lookahead, Auto Release
- Auto Gain comparison (capture with auto gain ON)
- Character/Drive

## Re-capturing Reference Data

If you need to re-capture (shouldn't be needed — data is committed):

```bash
cd /home/cody/Development/FastTrackStudio/fts-analyzer
cargo build --release --package fts-analyzer-cli

./target/release/fts-analyzer-cli capture-compressor \
  '/home/cody/.clap/yabridge/FabFilter Pro-C 3.clap' \
  --out ./reference/pro-c3-clean \
  --scenarios ./reference/pro-c3-full-scenarios.json \
  --param 0=0 --param 19=0 \
  --gain-high -6 --gain-low -20 --duration 3
```

This runs 8 parallel Wine instances via yabridge (serialized loading, parallel processing). Takes ~1 minute for all 196 scenarios. Outputs both `.bin` files and `capture.csv`.

To regenerate just the CSV from existing binary files, the capture command will overwrite both.
