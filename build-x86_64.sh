#!/usr/bin/env bash
# Build for x86_64 Linux (musl). Requires: apt install musl-tools
set -e
. ./build-x86_64.env
cargo build --release
$STRIP $EXEC
echo "Built: $EXEC ($(du -sh $EXEC | cut -f1))"
