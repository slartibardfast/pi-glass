#!/usr/bin/env bash
# Build for Windows x64 (MinGW). Requires: apt install gcc-mingw-w64-x86-64
# Note: surge-ping raw ICMP sockets require Administrator privileges on Windows.
set -e
. ./build-win64.env
cargo build --release
$STRIP $EXEC
echo "Built: $EXEC ($(du -sh $EXEC | cut -f1))"
