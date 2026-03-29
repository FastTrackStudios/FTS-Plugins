# Justfile for FTS Plugins
# Usage: just <command> [args...]
#
# Build any plugin:  just bundle comp-plugin
# Install any plugin: just install comp-plugin

# ── Configuration ─────────────────────────────────────────────────────

# Default plugin to build (override with: just build comp-plugin)
PLUGIN_NAME := "fts-macros"

# Platform-specific paths
FX_DIR_LINUX := env_var("HOME") / ".clap"
FX_DIR_MAC := env_var("HOME") / "Music/FastTrackStudio/Reaper/FTS-TRACKS/UserPlugins/FX"
LOG_FILE_LINUX := env_var("HOME") / ".config/REAPER/Logs/nih.log"
LOG_FILE_MAC := env_var("HOME") / "Library/Logs/REAPER/nih.log"

# Use FTS_REAPER_CONFIG from nix shell if available, else default
REAPER_CONFIG := env_var_or_default("FTS_REAPER_CONFIG", env_var("HOME") / ".config/REAPER")

# Default recipe - show help
default: help

# ── Build Commands ────────────────────────────────────────────────────

# Bundle a plugin (CLAP + VST3). Usage: just bundle comp-plugin
bundle plugin=PLUGIN_NAME:
    cargo run --package xtask -- bundle {{plugin}} --release

# Bundle in debug mode
bundle-debug plugin=PLUGIN_NAME:
    cargo run --package xtask -- bundle {{plugin}}

# Build alias (same as bundle)
build plugin=PLUGIN_NAME: (bundle plugin)

# Build debug alias
build-debug plugin=PLUGIN_NAME: (bundle-debug plugin)

# ── Install Commands ──────────────────────────────────────────────────

# Install a bundled CLAP plugin to REAPER's FX directory
install plugin=PLUGIN_NAME: (bundle plugin)
    #!/usr/bin/env bash
    set -euo pipefail

    # Determine FX directory (Linux vs macOS)
    if [ "$(uname)" = "Darwin" ]; then
        FX_DIR="{{FX_DIR_MAC}}"
    else
        FX_DIR="{{FX_DIR_LINUX}}"
    fi

    CLAP_FILE="{{plugin}}.clap"

    mkdir -p "$FX_DIR"

    # Remove old plugin
    if [ -e "$FX_DIR/$CLAP_FILE" ] || [ -d "$FX_DIR/$CLAP_FILE" ]; then
        rm -rf "$FX_DIR/$CLAP_FILE"
        echo "Removed old $CLAP_FILE"
    fi

    # Copy new plugin
    BUNDLED="./target/bundled/$CLAP_FILE"
    if [ ! -e "$BUNDLED" ]; then
        echo "ERROR: $BUNDLED not found. Did the build succeed?"
        exit 1
    fi

    cp -r "$BUNDLED" "$FX_DIR/"
    echo "Installed: $FX_DIR/$CLAP_FILE"

# Install all plugins
install-all:
    just install comp-plugin
    just install eq-plugin
    just install gate-plugin
    just install limiter-plugin
    just install tape-plugin
    just install delay-plugin
    just install reverb-plugin
    just install trigger-plugin
    just install rider-plugin
    just install chorus-plugin
    just install pitch-plugin
    just install saturate-plugin
    just install trem-plugin
    just install nam-plugin
    just install midi-guitar-plugin

# Install and show REAPER reload instructions
install-reload plugin=PLUGIN_NAME: (install plugin)
    #!/usr/bin/env bash
    echo ""
    echo "Restart REAPER or rescan plugins to pick up the new build."
    echo "  REAPER → Options → Preferences → Plug-ins → Re-scan"

# Uninstall a plugin from FX directory
uninstall plugin=PLUGIN_NAME:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname)" = "Darwin" ]; then
        FX_DIR="{{FX_DIR_MAC}}"
    else
        FX_DIR="{{FX_DIR_LINUX}}"
    fi
    CLAP_FILE="{{plugin}}.clap"
    if [ -e "$FX_DIR/$CLAP_FILE" ] || [ -d "$FX_DIR/$CLAP_FILE" ]; then
        rm -rf "$FX_DIR/$CLAP_FILE"
        echo "Removed: $FX_DIR/$CLAP_FILE"
    else
        echo "Not found: $FX_DIR/$CLAP_FILE"
    fi

# ── REAPER Commands ───────────────────────────────────────────────────

# Launch REAPER (uses fts-gui from nix shell, or system REAPER on mac)
reaper:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname)" = "Darwin" ]; then
        REAPER_EXECUTABLE="/Users/codywright/Music/FTS-REAPER/FTS-LIVE.app/Contents/MacOS/REAPER"
        REAPER_RESOURCES="/Users/codywright/Music/FTS-REAPER/FTS-LIVE.app/Contents/Resources"
        if [ "$(uname)" = "Darwin" ]; then
            LOG_FILE="{{LOG_FILE_MAC}}"
        else
            LOG_FILE="{{LOG_FILE_LINUX}}"
        fi
        mkdir -p "$(dirname "$LOG_FILE")"
        cd "$REAPER_RESOURCES"
        NIH_LOG="$LOG_FILE" "$REAPER_EXECUTABLE"
    else
        # On Linux, use fts-gui from nix shell
        if command -v fts-gui &>/dev/null; then
            fts-gui
        else
            echo "Run 'nix develop --impure' first, or use fts-gui from the nix shell"
            exit 1
        fi
    fi

# ── Development Workflows ─────────────────────────────────────────────

# Build, install, and launch REAPER
run plugin=PLUGIN_NAME: (install plugin) reaper

# Watch logs
logs:
    #!/usr/bin/env bash
    if [ "$(uname)" = "Darwin" ]; then
        LOG_FILE="{{LOG_FILE_MAC}}"
    else
        LOG_FILE="{{LOG_FILE_LINUX}}"
    fi
    if [ -f "$LOG_FILE" ]; then
        tail -f "$LOG_FILE"
    else
        echo "Waiting for log file: $LOG_FILE"
        while [ ! -f "$LOG_FILE" ]; do sleep 1; done
        tail -f "$LOG_FILE"
    fi

# ── Utility Commands ──────────────────────────────────────────────────

# Type-check a plugin
check plugin=PLUGIN_NAME:
    cargo check -p {{plugin}}

# Run clippy on a plugin
lint plugin=PLUGIN_NAME:
    cargo clippy -p {{plugin}}

# Run tests for a plugin's DSP crate
test-dsp dsp_crate:
    cargo test -p {{dsp_crate}}

# Run REAPER integration tests (spawns REAPER, runs #[reaper_test] tests)
# Pass --no-headless to run with a real display (enables plugin GUI rendering)
# Example: just test-reaper comp-plugin comp_gui --no-headless
test-reaper plugin=PLUGIN_NAME *args="":
    cargo run --package xtask -- reaper-test {{plugin}} {{args}}

# Format code
fmt:
    cargo fmt

# Clean build artifacts
clean:
    cargo clean

# Show plugin info and paths
info plugin=PLUGIN_NAME:
    #!/usr/bin/env bash
    if [ "$(uname)" = "Darwin" ]; then
        FX_DIR="{{FX_DIR_MAC}}"
    else
        FX_DIR="{{FX_DIR_LINUX}}"
    fi
    CLAP_FILE="{{plugin}}.clap"
    echo "Plugin:     {{plugin}}"
    echo "FX Dir:     $FX_DIR"
    echo "Bundled:    ./target/bundled/$CLAP_FILE"
    echo ""
    if [ -e "$FX_DIR/$CLAP_FILE" ]; then
        echo "Installed:  YES"
        ls -lhd "$FX_DIR/$CLAP_FILE"
    else
        echo "Installed:  NO"
    fi

# Show help
help:
    #!/usr/bin/env bash
    echo "FTS Plugins - Development Commands"
    echo ""
    echo "Build (any plugin):        just bundle comp-plugin"
    echo "Install (any plugin):      just install comp-plugin"
    echo "Build + Install + REAPER:  just run comp-plugin"
    echo ""
    echo "Build:"
    echo "  just build [plugin]      Bundle plugin (release)"
    echo "  just build-debug [plugin] Bundle plugin (debug)"
    echo ""
    echo "Install:"
    echo "  just install [plugin]    Build and install to REAPER FX dir"
    echo "  just uninstall [plugin]  Remove plugin from FX dir"
    echo ""
    echo "Dev:"
    echo "  just run [plugin]        Build, install, launch REAPER"
    echo "  just check [plugin]      Type-check"
    echo "  just lint [plugin]       Clippy"
    echo "  just test-dsp <crate>    Run DSP tests (e.g. just test-dsp comp-dsp)"
    echo "  just test-reaper [plugin] [test] Run REAPER integration tests"
    echo "  just logs                Tail NIH log"
    echo "  just info [plugin]       Show paths and install status"
    echo ""
    echo "Plugins: comp-plugin, eq-plugin, gate-plugin, limiter-plugin,"
    echo "         tape-plugin, delay-plugin, reverb-plugin, trigger-plugin,"
    echo "         rider-plugin, fts-macros"
