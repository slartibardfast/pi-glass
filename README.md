# pi-glass

Lightweight network monitor for Raspberry Pi Zero. Single Rust binary, zero-JS dashboard with Fluent 2 styling.

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
├── build.env                   # musl cross-compiler env vars — source before building
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

Two targets are supported. Both use [musl.cc](https://musl.cc/) cross-compilers for fully static binaries with no runtime dependencies.

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

## Build & deploy

### Pi Zero

```bash
# Generate CSS tokens (one-time)
cd web && npm install && npm run build && cd ..

# Build
. ./build.env
cargo build --release

# Strip
~/.local/arm-linux-musleabihf-cross/bin/arm-linux-musleabihf-strip \
  target/arm-unknown-linux-musleabihf/release/pi-glass

# Deploy
scp target/arm-unknown-linux-musleabihf/release/pi-glass pi@<ip>:/opt/pi-glass/
scp deploy/config.toml pi@<ip>:/opt/pi-glass/
scp deploy/pi-glass.service pi@<ip>:/etc/systemd/system/
ssh pi@<ip> "sudo systemctl daemon-reload && sudo systemctl enable --now pi-glass"
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

# Strip
~/.local/mipsel-linux-muslsf-cross/bin/mipsel-linux-muslsf-strip \
  target/mipsel-unknown-linux-musl/release/pi-glass

# Deploy
scp target/mipsel-unknown-linux-musl/release/pi-glass root@<ip>:/usr/bin/pi-glass
scp pi-glass.init root@<ip>:/etc/init.d/pi-glass
ssh root@<ip> "chmod +x /etc/init.d/pi-glass /usr/bin/pi-glass \
  && service pi-glass enable && service pi-glass start"
```
