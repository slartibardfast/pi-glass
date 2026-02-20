#!/usr/bin/env bash
# Build for Pi Zero (ARMv6 musl). Requires arm-linux-musleabihf-cross in ~/.local/
set -e
. ./build-pi.env
cargo build --release
$STRIP $EXEC
$STRIP $MAILER_EXEC
echo "Built: $EXEC ($(du -sh $EXEC | cut -f1))"
