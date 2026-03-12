# Justfile for FTS Plugins
# Usage: just <command> [args...]

# Configuration
PLUGIN_NAME := "fts-macros"
PLUGIN_CLAP := PLUGIN_NAME + ".clap"
FX_DIR := env_var("HOME") / "Music/FastTrackStudio/Reaper/FTS-TRACKS/UserPlugins/FX"
LOG_FILE := env_var("HOME") / "Library/Logs/REAPER/nih.log"
REAPER_EXECUTABLE := "/Users/codywright/Music/FTS-REAPER/FTS-LIVE.app/Contents/MacOS/REAPER"
REAPER_RESOURCES := "/Users/codywright/Music/FTS-REAPER/FTS-LIVE.app/Contents/Resources"

# Default recipe - show help
default: help

# ============================================================================
# Build Commands
# ============================================================================

# Build the plugin in release mode
build:
    cargo run --package xtask -- bundle {{PLUGIN_NAME}} --release

# Build the plugin in debug mode
build-debug:
    cargo run --package xtask -- bundle {{PLUGIN_NAME}}

# ============================================================================
# Install Commands
# ============================================================================

# Install plugin to FTS user FX directory
install: build
    #!/usr/bin/env bash
    set -euo pipefail

    # Create FX directory if it doesn't exist
    mkdir -p "{{FX_DIR}}"

    echo "Removing old plugin if it exists..."
    if [ -d "{{FX_DIR}}/{{PLUGIN_CLAP}}" ]; then
        rm -rf "{{FX_DIR}}/{{PLUGIN_CLAP}}"
        echo "✓ Old plugin removed"
    else
        echo "✓ No existing plugin found"
    fi

    echo "Installing new plugin..."
    cp -r "./target/bundled/{{PLUGIN_CLAP}}" "{{FX_DIR}}/"
    echo "✓ Plugin installed to: {{FX_DIR}}/{{PLUGIN_CLAP}}"

# Install and reload REAPER plugin cache
install-reload: install
    #!/usr/bin/env bash
    set -euo pipefail
    echo ""
    echo "📢 Restart REAPER and rebuild the plugin cache:"
    echo "   1. Close REAPER if running"
    echo "   2. Delete: ~/Library/Caches/reaper-*.cache"
    echo "   3. Launch REAPER - it will rebuild the plugin cache"
    echo "   4. The fts-macros plugin should appear in FX list"

# Uninstall plugin from FX directory
uninstall:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -d "{{FX_DIR}}/{{PLUGIN_CLAP}}" ]; then
        rm -rf "{{FX_DIR}}/{{PLUGIN_CLAP}}"
        echo "✓ Plugin removed from: {{FX_DIR}}/{{PLUGIN_CLAP}}"
    else
        echo "✗ No plugin found at: {{FX_DIR}}/{{PLUGIN_CLAP}}"
    fi

# ============================================================================
# REAPER Commands
# ============================================================================

# Launch FTS REAPER with NIH logging enabled
reaper:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$(dirname "{{LOG_FILE}}")"
    echo "Launching FTS REAPER..."
    echo "Logs: {{LOG_FILE}}"
    cd "{{REAPER_RESOURCES}}"
    NIH_LOG="{{LOG_FILE}}" "{{REAPER_EXECUTABLE}}"

# Launch REAPER with debug logging
reaper-debug:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$(dirname "{{LOG_FILE}}")"
    echo "Launching FTS REAPER (debug mode)..."
    echo "Logs: {{LOG_FILE}}"
    cd "{{REAPER_RESOURCES}}"
    RUST_LOG=debug NIH_LOG="{{LOG_FILE}}" "{{REAPER_EXECUTABLE}}"

# ============================================================================
# Development Workflows
# ============================================================================

# Build, install, and launch REAPER (full dev cycle)
run: install reaper

# Build, install, and launch REAPER with debug logging
run-debug: install reaper-debug

# Build, install, and open tmux session with REAPER + log monitoring
dev: install
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "$(dirname "{{LOG_FILE}}")"

    echo "Starting development panes..."

    # Split current pane horizontally, run REAPER in new pane on right
    tmux split-window -h "cd \"{{REAPER_RESOURCES}}\" && NIH_LOG=\"{{LOG_FILE}}\" \"{{REAPER_EXECUTABLE}}\""

    # Current pane (left) will show logs
    echo "REAPER launched in right pane"
    echo "Use Ctrl+B then arrow keys to switch panes"
    echo "Showing logs below..."
    echo ""

    # Tail logs in current pane
    while [ ! -f "{{LOG_FILE}}" ]; do sleep 1; done
    tail -f "{{LOG_FILE}}"

# Watch logs (tail the NIH log file)
logs:
    #!/usr/bin/env bash
    if [ -f "{{LOG_FILE}}" ]; then
        tail -f "{{LOG_FILE}}"
    else
        echo "Waiting for log file: {{LOG_FILE}}"
        while [ ! -f "{{LOG_FILE}}" ]; do sleep 1; done
        tail -f "{{LOG_FILE}}"
    fi

# Clear log file
clear-logs:
    #!/usr/bin/env bash
    if [ -f "{{LOG_FILE}}" ]; then
        rm "{{LOG_FILE}}"
        echo "✓ Cleared: {{LOG_FILE}}"
    else
        echo "No log file to clear"
    fi

# ============================================================================
# Utility Commands
# ============================================================================

# Check code without building
check:
    cargo check -p {{PLUGIN_NAME}}

# Run clippy lints
lint:
    cargo clippy -p {{PLUGIN_NAME}}

# Format code
fmt:
    cargo fmt

# Clean build artifacts
clean:
    cargo clean

# Show plugin info and paths
info:
    #!/usr/bin/env bash
    echo "Plugin Information"
    echo "=================="
    echo "Name:           {{PLUGIN_NAME}}"
    echo "FX Directory:   {{FX_DIR}}"
    echo "Log File:       {{LOG_FILE}}"
    echo "REAPER:         {{REAPER_EXECUTABLE}}"
    echo ""
    echo "Installed Plugin:"
    if [ -d "{{FX_DIR}}/{{PLUGIN_CLAP}}" ]; then
        ls -lhd "{{FX_DIR}}/{{PLUGIN_CLAP}}"
    else
        echo "  (not installed)"
    fi

# Show help
help:
    #!/usr/bin/env bash
    echo "FTS Plugins - Development Commands"
    echo ""
    echo "Build:"
    echo "  just build           Build plugin (release)"
    echo "  just build-debug     Build plugin (debug)"
    echo ""
    echo "Install:"
    echo "  just install         Build and install to FX directory"
    echo "  just install-reload  Install and show REAPER cache reload instructions"
    echo "  just uninstall       Remove plugin from FX directory"
    echo ""
    echo "Development:"
    echo "  just run             Build, install, and launch REAPER"
    echo "  just run-debug       Same as run, with debug logging"
    echo "  just dev             Build, install, launch REAPER in tmux with log monitoring"
    echo ""
    echo "REAPER:"
    echo "  just reaper          Launch FTS REAPER with NIH logging"
    echo "  just reaper-debug    Launch with debug logging"
    echo ""
    echo "Logs:"
    echo "  just logs            Tail the NIH log file"
    echo "  just clear-logs      Clear the log file"
    echo ""
    echo "Utility:"
    echo "  just check           Check code without building"
    echo "  just lint            Run clippy"
    echo "  just fmt             Format code"
    echo "  just clean           Clean build artifacts"
    echo "  just info            Show plugin info and paths"
