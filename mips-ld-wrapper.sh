#!/bin/bash
# Linker wrapper for mipsel-unknown-linux-musl cross-compilation.
# Rust passes CRT files as bare names (crt1.o etc); ld can't find them
# without full paths. This wrapper substitutes them before invoking gcc.
MUSL_LIB=/home/david/.local/mipsel-linux-muslsf-cross/mipsel-linux-muslsf/lib
GCC_LIB=/home/david/.local/mipsel-linux-muslsf-cross/lib/gcc/mipsel-linux-muslsf/11.2.1

args=("-L$GCC_LIB" "-msoft-float")
for arg in "$@"; do
  case "$arg" in
    crt1.o|crti.o|crtn.o) args+=("$MUSL_LIB/$arg") ;;
    crtbegin.o|crtend.o)   args+=("$GCC_LIB/$arg") ;;
    -lunwind)              args+=("-lgcc_eh") ;;
    *)                     args+=("$arg") ;;
  esac
done

exec /home/david/.local/mipsel-linux-muslsf-cross/bin/mipsel-linux-muslsf-gcc "${args[@]}"
