#!/usr/bin/env bash
# Build for MT76x8 / OpenWRT (MIPS32r2 LE musl). Requires mipsel-linux-muslsf-cross in ~/.local/
set -e
. ./build-mt76x8.env
cargo +nightly build -Z build-std=std,panic_abort --release
$STRIP $EXEC
echo "Built: $EXEC ($(du -sh $EXEC | cut -f1))"
