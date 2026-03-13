#!/usr/bin/env bash
set -e

echo ""
echo "  Building QR Frame (Rust)..."
echo ""

if ! command -v cargo &>/dev/null; then
    echo "  [!] Rust not found. Install from https://rustup.rs"
    exit 1
fi

# Linux: install required system libs for OpenGL + window system
if [ "$(uname)" = "Linux" ]; then
    echo "  [*] Installing Linux system dependencies..."
    if command -v apt-get &>/dev/null; then
        sudo apt-get install -y \
            libxcb1-dev libxcb-render0-dev libxcb-shape0-dev \
            libxcb-xfixes0-dev libxkbcommon-dev \
            libgl1-mesa-dev pkg-config 2>/dev/null || true
    elif command -v dnf &>/dev/null; then
        sudo dnf install -y libxcb-devel libxkbcommon-devel mesa-libGL-devel 2>/dev/null || true
    fi
fi

cargo build --release

echo ""
if [ "$(uname)" = "Darwin" ]; then
    echo "  [+] Done! Binary: target/release/qr-frame"
    echo "      To make a .app bundle, run: ./make_app.sh"
else
    echo "  [+] Done! Binary: target/release/qr-frame"
fi
echo ""
