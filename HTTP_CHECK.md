# HTTP(S) Check — Design Notes

## New check type: `"http"`

Config shape (full URL, unlike host:port used by ping/dns/tcp):

```toml
[[services]]
label = "My API"
icon  = "🌐"
check  = "http"
target = "https://example.com/healthz"
```

## Implementation

### New dep: none

`reqwest` is already in `Cargo.toml`. The main binary doesn't currently link it
(only `mailer` does). Adding `check_http` to `main.rs` will pull it in.

**Verify first**: `cargo check --target mipsel-unknown-linux-musl`
`reqwest` with `rustls-tls` depends on `ring`, which has limited MIPS support.
If it fails, options:
- Plain HTTP only (raw TCP + parse status line — zero extra deps)
- `native-tls` feature (links OpenWrt's system `libssl.so`)
- Feature-gate: HTTP checks compile out when `--features openwrt`

### Persistent `reqwest::Client` in `poll_loop`

Create once alongside the ping `Client`. `reqwest::Client` is `Clone + Send + Sync`
(internally `Arc`-backed), so sharing across futures is simple — no `client_ref`
trick needed; just `.clone()` it into each `async move`.

Set `pool_idle_timeout` to match the poll interval so connections aren't evicted
between polls:

```rust
let http_client = reqwest::ClientBuilder::new()
    .pool_idle_timeout(Duration::from_secs(state.config.poll_interval_secs + 10))
    .danger_accept_invalid_certs(state.config.skip_tls_verify.unwrap_or(false))
    .build()
    .expect("Failed to build HTTP client");
```

### `check_http` function

```rust
async fn check_http(client: &reqwest::Client, url: &str, timeout_secs: u64)
    -> (bool, Option<f64>, Option<String>)
{
    let start = Instant::now();
    let req = client
        .head(url)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await;

    match req {
        Ok(resp) => {
            // Any response (including 4xx/5xx) = server is reachable = UP.
            // 405 Method Not Allowed also = UP (HEAD not supported, but server responded).
            let latency = start.elapsed().as_secs_f64() * 1000.0;
            (true, Some(latency), None)  // resolved IP not available from reqwest
        }
        Err(_) => (false, None, None),
    }
}
```

### UP/DOWN policy

**Any HTTP response = UP.** This is a network monitor, not an application monitor.
A 401, 403, 404, 500 all prove the host is reachable and HTTP/TLS works.
Only a timeout or connection failure = DOWN.

### Resolved IP

`reqwest` doesn't expose the resolved IP from its connection pool. Skip `resolved_ip`
for HTTP checks (leave it `None`). The URL already shows the hostname.

### TLS session resumption

On MIPS, a full TLS handshake costs ~200–500ms (no AES hardware). Subsequent polls
reuse the TLS session ticket (valid ~24h server-side) even when the TCP connection
has been closed — paying only ~1 RTT instead of full handshake. This is handled
automatically by rustls inside reqwest.

### Config addition

```toml
# Optional — skip TLS certificate verification (useful on OpenWrt without CA bundle)
# skip_tls_verify = false
```

Add `skip_tls_verify: Option<bool>` to `Config` in `lib.rs`.
