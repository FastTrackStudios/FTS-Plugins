# Justfile - FTS-Plugins (Audio Plugins)
# Install just: cargo install just
# Run commands: just <recipe-name>

# Default recipe - show help
_default:
    @just --list

# ============================================================================
# Build
# ============================================================================

# Build all plugins (debug)
build:
    cargo build

# Build all plugins (release)
build-release:
    cargo build --release

# Build and bundle a specific plugin (e.g., just bundle fts-guide)
bundle plugin:
    cargo xtask bundle {{plugin}}

# Build and bundle a specific plugin in release mode
bundle-release plugin:
    cargo xtask bundle {{plugin}} --release

# ============================================================================
# Install / Symlink
# ============================================================================

# Bundle and install FTS Guide plugin to REAPER
install-guide:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -f .env ]; then set -a; source .env; set +a; fi

    REAPER_PATH="${REAPER_PATH:-/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/}"
    FX_DIR="$REAPER_PATH/UserPlugins/FX"

    cargo xtask bundle fts-guide --release

    mkdir -p "$FX_DIR"

    # Copy CLAP plugin
    if [[ -d "target/bundled/FTS Guide.clap" ]]; then
        rm -rf "$FX_DIR/FTS Guide.clap"
        cp -r "target/bundled/FTS Guide.clap" "$FX_DIR/"
        echo "Installed: $FX_DIR/FTS Guide.clap"
    fi

    # Copy VST3 plugin
    if [[ -d "target/bundled/FTS Guide.vst3" ]]; then
        rm -rf "$FX_DIR/FTS Guide.vst3"
        cp -r "target/bundled/FTS Guide.vst3" "$FX_DIR/"
        echo "Installed: $FX_DIR/FTS Guide.vst3"
    fi

# ============================================================================
# Development
# ============================================================================

# Check all crates compile
check:
    cargo check --workspace

# Run clippy on all crates
lint:
    cargo clippy --workspace

# Clean build artifacts
clean:
    cargo clean

# ============================================================================
# Info
# ============================================================================

# Show configured REAPER paths
show-reaper-path:
    #!/usr/bin/env bash
    if [ -f .env ]; then set -a; source .env; set +a; fi

    REAPER_PATH="${REAPER_PATH:-/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/}"

    echo "REAPER Path: $REAPER_PATH"
    echo "FX Dir:      $REAPER_PATH/UserPlugins/FX"
    echo ""

    if [[ -d "$REAPER_PATH/UserPlugins/FX" ]]; then
        echo "Installed plugins:"
        ls -la "$REAPER_PATH/UserPlugins/FX" | grep -E "\.(clap|vst3)$" || echo "  (none)"
    fi

# Aliases
alias b := build
alias br := build-release
alias c := check
alias l := lint
