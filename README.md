pi-glass

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

Key features:

- TOML config at /opt/pi-glass/config.toml — LAN hosts, external services, listen addr, db path, intervals, retention
- LAN host monitoring — ICMP ping every 30s with full stats (uptime %, avg/min/max latency, packet loss % across 1h/24h/7d windows, current streak)
- External service checks — ping (ICMP), dns (raw UDP query), tcp (connect latency). Configurable targets with built-in or custom base64 icons
- Fluent 2 styling — tokens extracted at build time via Node.js, embedded via include_str!, zero runtime JS (except ~15 lines for mobile overlay)
- Auto-refresh via <meta http-equiv="refresh" content="30">

Cross-compilation:

Pi Zero is ARMv6. Ubuntu's arm-linux-gnueabihf toolchain ships ARMv7 CRT
files which produce binaries that segfault on Pi Zero. We use musl instead
for a fully static binary:

  Target:   arm-unknown-linux-musleabihf
  Toolchain: musl.cc cross-compiler (https://musl.cc/)
  Install to: ~/.local/arm-linux-musleabihf-cross/

To build & deploy:

# Generate CSS tokens (one-time)
cd web && npm install && npm run build && cd ..

# Build for Pi Zero (in WSL2)
. ./build.env
cargo build --release

# Or explicitly:
# cargo build --release --target arm-unknown-linux-musleabihf

# Strip (optional, saves ~2MB)
~/.local/arm-linux-musleabihf-cross/bin/arm-linux-musleabihf-strip \
  target/arm-unknown-linux-musleabihf/release/pi-glass

# Deploy to Pi
scp target/arm-unknown-linux-musleabihf/release/pi-glass pi@<ip>:/opt/pi-glass/
scp deploy/config.toml pi@<ip>:/opt/pi-glass/
scp deploy/pi-glass.service pi@<ip>:/etc/systemd/system/
# Then: sudo systemctl daemon-reload && sudo systemctl enable --now pi-glass
