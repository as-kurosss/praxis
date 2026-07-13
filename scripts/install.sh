#!/usr/bin/env bash
set -euo pipefail

# ─── Praxis Installer (Unix) ─────────────────────────────────────────────────
# This script installs the Praxis CLI tool and optionally the API server.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/.../scripts/install.sh | bash
#   # or
#   ./scripts/install.sh
# ──────────────────────────────────────────────────────────────────────────────

INSTALL_DIR="${INSTALL_DIR:-$HOME/.praxis/bin}"
REPO_DIR="${REPO_DIR:-}"
BUILD_TYPE="${BUILD_TYPE:-release}"

# ── 1. Check for Rust ─────────────────────────────────────────────────────────
install_rust() {
    echo "→ Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    export PATH="$HOME/.cargo/bin:$PATH"
}

if ! command -v cargo &>/dev/null; then
    install_rust
else
    echo "✓ Rust detected: $(cargo --version)"
fi

# ── 2. Locate or clone the repository ─────────────────────────────────────────
if [ -z "$REPO_DIR" ]; then
    if [ -d "$(dirname "$0")/.." ] && [ -f "$(dirname "$0")/../Cargo.toml" ]; then
        # Running from within the repo
        REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
        echo "✓ Using local repository: $REPO_DIR"
    else
        REPO_DIR="/tmp/praxis-build-$$"
        echo "→ Cloning repository..."
        git clone --depth 1 https://github.com/kurosss/praxis.git "$REPO_DIR"
    fi
fi

cd "$REPO_DIR"

# ── 3. Build ──────────────────────────────────────────────────────────────────
echo "→ Building Praxis ($BUILD_TYPE)..."
if [ "$BUILD_TYPE" = "release" ]; then
    cargo build --release --workspace
    BINARY_PATH="target/release/praxis"
else
    cargo build --workspace
    BINARY_PATH="target/debug/praxis"
fi

# ── 4. Install ────────────────────────────────────────────────────────────────
echo "→ Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cp "$BINARY_PATH" "$INSTALL_DIR/praxis"

# Also copy API server binary if it was built
API_SERVER_BINARY="${BINARY_PATH%/*}/praxis-api-server"
if [ -f "$API_SERVER_BINARY" ]; then
    cp "$API_SERVER_BINARY" "$INSTALL_DIR/praxis-api-server"
    echo "✓ API server also installed"
fi

chmod +x "$INSTALL_DIR/praxis" 2>/dev/null || true

# ── 5. PATH setup ────────────────────────────────────────────────────────────
case ":${PATH}:" in
    *:${INSTALL_DIR}:*)
        echo "✓ $INSTALL_DIR already in PATH"
        ;;
    *)
        echo ""
        echo "⚠  $INSTALL_DIR is not in your PATH."
        echo "   Add the following line to your ~/.bashrc, ~/.zshrc, or ~/.profile:"
        echo ""
        echo "    export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
        ;;
esac

# ── 6. Verify ─────────────────────────────────────────────────────────────────
echo ""
if command -v praxis &>/dev/null; then
    echo "✓ Praxis installed successfully!"
    praxis --help
else
    echo "✓ Praxis installed to $INSTALL_DIR/praxis"
    echo "  Restart your shell or run: export PATH=\"\$PATH:$INSTALL_DIR\""
fi
