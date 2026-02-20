#!/usr/bin/env bash
# One-time setup for pi-glass development on a fresh WSL2 Ubuntu system.
set -e

MUSL_DIR="$HOME/.local"

# --- Apt packages ---
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    musl-tools \
    gcc-mingw-w64-x86-64 \
    binutils-mingw-w64-x86-64 \
    curl \
    xz-utils

# --- Rust ---
if ! command -v rustup &>/dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    source "$HOME/.cargo/env"
fi

rustup toolchain install stable nightly
rustup component add rust-src --toolchain nightly
rustup target add \
    arm-unknown-linux-musleabihf \
    aarch64-unknown-linux-musl \
    x86_64-unknown-linux-musl \
    x86_64-pc-windows-gnu

# --- musl.cc toolchains ---
mkdir -p "$MUSL_DIR"

ARM_TC="arm-linux-musleabihf-cross"
if [ ! -d "$MUSL_DIR/$ARM_TC" ]; then
    echo "Downloading $ARM_TC..."
    curl -L "https://musl.cc/$ARM_TC.tgz" | tar -xz -C "$MUSL_DIR"
fi

AARCH64_TC="aarch64-linux-musl-cross"
if [ ! -d "$MUSL_DIR/$AARCH64_TC" ]; then
    echo "Downloading $AARCH64_TC..."
    curl -L "https://musl.cc/$AARCH64_TC.tgz" | tar -xz -C "$MUSL_DIR"
fi

MIPS_TC="mipsel-linux-muslsf-cross"
if [ ! -d "$MUSL_DIR/$MIPS_TC" ]; then
    echo "Downloading $MIPS_TC..."
    curl -L "https://musl.cc/$MIPS_TC.tgz" | tar -xz -C "$MUSL_DIR"
fi

chmod +x mips-ld-wrapper.sh

# --- Node.js (for CSS token generation) ---
if ! command -v node &>/dev/null; then
    curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash -
    sudo apt-get install -y nodejs
fi

cd web && npm install && npm run build && cd ..

echo ""
echo "Setup complete. Add ~/.cargo/bin to PATH if not already present."
echo "Use build-pi.sh / build-mips.sh / build-x86_64.sh / build-win64.sh to build."
