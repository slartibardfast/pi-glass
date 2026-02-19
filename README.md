# pi-glass

Lightweight network monitor for Raspberry Pi Zero. Single Rust binary, low-JS dashboard with Fluent 2 styling.

## Features

- **LAN host monitoring** — ICMP ping every 30s with full stats (uptime %, avg/min/max latency, packet loss across 1h/24h/7d windows, current streak)
- **External service checks** — ping (ICMP), dns (raw UDP query), tcp (connect latency). Configurable targets with built-in or custom icons
- **Collapsible cards** — hosts auto-collapse when 100% up for the last hour; Web and DNS service cards with up/total summaries
- **TOML config** — LAN hosts, external services, listen addr, db path, poll intervals, retention
- **Fluent 2 styling** — tokens extracted at build time via Node.js, embedded via `include_str!`
- **Auto-refresh** — `<meta http-equiv="refresh" content="30">`

## Project structure

```
pi-glass/
├── .cargo/config.toml          # cross-compile linker config (musl)
├── .gitignore                  # /target, *.db, node_modules, web/dist
├── build-pi.env                # ARMv6 musl cross-compiler env — source before building
├── build-mt76x8.env            # MIPS musl cross-compiler env — source before building
├── build-x86_64.env            # x86_64 musl env — source before building
├── build-win64.env             # Windows x64 MinGW env — source before building
├── build-pi.sh                 # convenience: source env + cargo build + strip (Pi Zero)
├── build-mips.sh               # convenience: source env + cargo +nightly build + strip (MT76x8)
├── build-x86_64.sh             # convenience: source env + cargo build + strip (x86_64)
├── build-win64.sh              # convenience: source env + cargo build + strip (Windows x64)
├── init-wsl.sh                 # one-time WSL2/Ubuntu dev environment setup
├── Cargo.toml                  # 7 deps: tokio, axum, rusqlite, surge-ping, chrono, serde, toml
├── src/main.rs                 # config, poller, stats, services, web handler
├── deploy/
│   ├── config.toml             # LAN hosts + external services config
│   └── pi-glass.service        # systemd unit with CAP_NET_RAW
└── web/
    ├── package.json            # @fluentui/tokens dependency
    ├── build.js                # extracts 459 Fluent 2 tokens → dist/tokens.css
    └── dist/tokens.css         # generated (gitignored)
```

## Cross-compilation

Four targets are supported. All use musl or MinGW for fully static binaries with no runtime dependencies. Each target has a corresponding `.env` file that exports `CC`, `AR`, `CARGO_BUILD_TARGET`, `STRIP`, and `EXEC` — source it before building so `$STRIP $EXEC` works consistently across targets.

### Pi Zero (ARMv6)

Ubuntu's `arm-linux-gnueabihf` toolchain ships ARMv7 CRT files which segfault on Pi Zero. Musl avoids this.

| | |
|---|---|
| **Target** | `arm-unknown-linux-musleabihf` |
| **Toolchain** | `arm-linux-musleabihf-cross` from musl.cc |
| **Install to** | `~/.local/arm-linux-musleabihf-cross/` |

### MT76x8 / OpenWRT (MIPS32r2 little-endian)

Targets MediaTek MT76x8-based routers running OpenWRT (e.g. MT7628, MT7688). The OpenWRT `ramips/mt76x8` target is little-endian (`mipsel`). Tier 3 Rust target — requires nightly and `build-std`.

| | |
|---|---|
| **Target** | `mipsel-unknown-linux-musl` |
| **Toolchain** | `mipsel-linux-muslsf-cross` from musl.cc (`sf` = soft-float) |
| **Install to** | `~/.local/mipsel-linux-muslsf-cross/` |

The linker wrapper `mips-ld-wrapper.sh` is required because Rust passes CRT startup files (`crt1.o` etc.) as bare names to the linker, which can't find them without full paths. The wrapper also remaps `-lunwind` to `-lgcc_eh` since the musl.cc toolchain provides unwind support via GCC's exception-handling library rather than LLVM libunwind. Linker and RUSTFLAGS are configured in `.cargo/config.toml`; only the C compiler env vars live in `build-mt76x8.env`.

### x86_64 Linux

Native musl build for x86_64 Linux hosts. Requires `musl-tools` for the `musl-gcc` wrapper.

| | |
|---|---|
| **Target** | `x86_64-unknown-linux-musl` |
| **Toolchain** | `musl-tools` (Ubuntu: `apt install musl-tools`) |

### Windows x64

Cross-compiled from Linux using MinGW. Statically links the CRT — no MSVCRT or MinGW DLL dependencies. Raw ICMP sockets (used by `surge-ping`) require the binary to be run as Administrator.

| | |
|---|---|
| **Target** | `x86_64-pc-windows-gnu` |
| **Toolchain** | `gcc-mingw-w64-x86-64` (Ubuntu: `apt install gcc-mingw-w64-x86-64`) |

## Build & deploy

### Pi Zero

```bash
# Generate CSS tokens (one-time)
cd web && npm install && npm run build && cd ..

# Build
. ./build-pi.env
cargo build --release

# Strip and deploy
$STRIP $EXEC
scp $EXEC pi@pi-glass:/opt/pi-glass/
scp deploy/config.toml pi@pi-glass:/opt/pi-glass/
scp deploy/pi-glass.service pi@pi-glass:/etc/systemd/system/
ssh pi@pi-glass "sudo systemctl daemon-reload && sudo systemctl enable --now pi-glass"
```

### MT76x8 / OpenWRT

```bash
# Generate CSS tokens (one-time)
cd web && npm install && npm run build && cd ..

# Make the linker wrapper executable (one-time)
chmod +x mips-ld-wrapper.sh

# Build (nightly required; build-std compiles std from source for Tier 3 target)
. ./build-mt76x8.env
cargo +nightly build -Z build-std=std,panic_abort --release

# Strip and deploy
$STRIP $EXEC
scp $EXEC root@pi-glass:/usr/bin/pi-glass
scp pi-glass.init root@pi-glass:/etc/init.d/pi-glass
ssh root@pi-glass "chmod +x /etc/init.d/pi-glass /usr/bin/pi-glass \
  && service pi-glass enable && service pi-glass start"
```

### x86_64 Linux

```bash
# One-time setup
rustup target add x86_64-unknown-linux-musl
sudo apt install musl-tools

# Generate CSS tokens (one-time)
cd web && npm install && npm run build && cd ..

# Build
. ./build-x86_64.env
cargo build --release

# Strip and deploy
$STRIP $EXEC
scp $EXEC user@pi-glass:/opt/pi-glass/
scp deploy/config.toml user@pi-glass:/opt/pi-glass/
scp deploy/pi-glass.service user@pi-glass:/etc/systemd/system/
ssh user@pi-glass "sudo systemctl daemon-reload && sudo systemctl enable --now pi-glass"
```

### Windows x64

```bash
# One-time setup
rustup target add x86_64-pc-windows-gnu
sudo apt install gcc-mingw-w64-x86-64

# Generate CSS tokens (one-time)
cd web && npm install && npm run build && cd ..

# Build
. ./build-win64.env
cargo build --release

# Strip
$STRIP $EXEC
```
