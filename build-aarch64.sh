#!/usr/bin/env bash
# Build for Pi 3/4/5 / Pi Zero 2 W (AArch64 musl). Requires aarch64-linux-musl-cross in ~/.local/
set -e
. ./build-aarch64.env
cargo build --release
$STRIP $EXEC
$STRIP $MAILER_EXEC
echo "Built: $EXEC ($(du -sh $EXEC | cut -f1))"
