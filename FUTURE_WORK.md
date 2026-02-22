# Future Work

## HTTP(S) check type

See [HTTP_CHECK.md](HTTP_CHECK.md) for the full design.

Short version: add `check = "http"` / `check = "https"` using a persistent
`reqwest::Client` (connection pool + TLS session reuse), HEAD requests, and
"any HTTP response = UP" policy. First poll pays the TLS handshake; subsequent
polls reuse the session ticket (~1 RTT on MIPS).

**Blocker to verify first:** `reqwest` with `rustls-tls` depends on `ring`,
which has limited MIPS support. Run
`cargo check --target mipsel-unknown-linux-musl` before committing to this path.

---

## Down alerting

The daily mailer tells you 23 hours after a host went down. A state-change
push (UP→DOWN, DOWN→UP) would be immediate.

Cheapest implementation: [ntfy.sh](https://ntfy.sh) — one HTTP POST, no new
deps beyond existing `reqwest`. Gate behind an optional `[alert]` config
section:

```toml
[alert]
ntfy_url = "https://ntfy.sh/my-private-topic"
# or any generic webhook:
# webhook_url = "https://..."
```

State-change detection: compare current poll result against last known status
in-memory (a `HashMap<String, &'static str>` in `AppState`). No DB reads needed.

---

## Precompressed static assets ✓ Done in v1.14.0

`build.rs` compresses CSS and JS (brotli + gzip) at compile time.
`tower-http` removed. `encoding_response()` serves the pre-compressed bytes
with `Content-Encoding: br/gzip` based on the client's `Accept-Encoding` header.
Both encodings are pre-built; the client gets the best it supports.

---

## WAL checkpoint control

SQLite WAL auto-checkpoints at 1000 pages by default. On NAND flash (OpenWrt),
each checkpoint is an unnecessary write burst. Disable auto-checkpoint and do
it manually after each poll cycle instead:

```sql
PRAGMA wal_autocheckpoint=0;
```

Then after each `tx.commit()` in `poll_loop`:
```rust
db.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
```

`PASSIVE` doesn't block readers; it checkpoints as many frames as possible
without waiting. This moves the checkpoint cost to a known point (after the
write transaction) rather than triggering it unpredictably mid-read.

---

## Data retention bucketing

Currently every poll result is kept for `retention_days` and then hard-deleted.
On a 30s poll interval, 7 days = ~20,000 rows per host. Queries stay fast due
to the indexes, but the DB file grows indefinitely until the delete runs.

Alternative: downsample old data rather than delete it.

```
age < 2h   → keep all rows (full resolution)
2h–24h     → keep 1 row per 5 minutes (12× compression)
24h–7d     → keep 1 row per 30 minutes (60× compression)
> 7d       → delete
```

Implementation: a separate `compact_loop` that runs once per hour, replacing
runs of rows with a single averaged/representative row. Keeps the DB small
on long-running installs while preserving useful history.

---

## State-change events table

Querying "when did this host go down?" from the current schema requires
scanning `ping_results` for transitions. A separate `events` table makes this
instant:

```sql
CREATE TABLE events (
    id        INTEGER PRIMARY KEY,
    host      TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL
);
```

Populated in `poll_loop` when a status changes. Used by:
- The alert system (trigger on INSERT into events)
- A future "outage history" UI section
- The daily mailer (summarise outages rather than just current state)

---

## `/health` endpoint

Returns last poll timestamp + per-host UP/DOWN counts as JSON. Useful for:
- OpenWrt watchdog (if `/health` 404s, restart the service)
- External uptime monitors watching the monitor itself
- Nagios/Prometheus scraping

```json
{
  "last_poll": "2026-02-22T14:30:00+00:00",
  "up": 7,
  "down": 1,
  "generation": 42
}
```

No DB read needed — `poll_generation` and `resolved_ips` already in AppState.
Keep it read-only; no state mutation.

---

## Config hot-reload

Currently requires a restart to pick up `config.toml` changes (new hosts,
changed intervals, etc.). A `SIGHUP` handler or `inotify` watch could rebuild
the non-DB parts of `AppState` in place.

Complexity: `config` is behind `Arc<AppState>` which is immutable. Hot-reload
would need either an `RwLock<Config>` inside AppState or a full state swap
(new `Arc<AppState>`, drain old one). The latter is cleaner but requires
care around in-flight requests.

Low priority — restarts are cheap on embedded and the DB state is preserved.

---

## Prometheus metrics endpoint

Expose `ping_results` as a Prometheus scrape target:

```
# HELP pi_glass_host_up 1 if host is UP, 0 if DOWN
# TYPE pi_glass_host_up gauge
pi_glass_host_up{host="192.168.1.1",label="Router"} 1
pi_glass_host_latency_ms{host="192.168.1.1"} 2.3
```

No new deps — format is plain text. Grafana + prometheus on a separate host
can then graph long-term trends without pi-glass needing to store them.

---

## Concurrent poll checks — lessons learned (v1.12.0 → v1.13.0)

v1.12.0 introduced `futures::future::join_all` to run all checks concurrently.
This caused dropped pings, false DOWN readings, and inflated latency. Root causes:

1. **PingIdentifier collision** — all concurrent pingers shared `PingIdentifier(0xAB)`.
   Surge_ping demultiplexes ICMP replies by identifier; with N pingers sharing one
   identifier, replies were misassigned between hosts.

2. **ICMP burst** — N pings sent simultaneously overwhelmed the network stack on
   embedded routers with ICMP rate limiting.

3. **Latency inflation** — on a single-core `current_thread` runtime, the event loop
   services N concurrent socket operations at once. Each check's measured latency
   included scheduler overhead from all the others.

v1.13.0 reverted to sequential loops with `pinger.timeout()` already set per-check.
This IS correct cooperative multitasking for single-core: one task runs, yields at
`.await`, the runtime services other work (HTTP) in between. Total worst-case time
is `N × timeout_secs`, which is acceptable for typical poll intervals.

**If true concurrency is needed in future** (e.g., very large host counts):
- Use `FuturesUnordered` with a semaphore to limit concurrent pings to ≤3
- Assign unique `PingIdentifier` per host (index-based, not a shared constant)
- Stagger ping start times by 50–100ms to avoid ICMP burst
- Keep DNS/TCP fully concurrent (no shared socket resource)
- Test on actual embedded hardware before shipping

---

## DB / render optimisations ✓ Done in v2.0.0

Seven items addressed based on deep review:

1. **`prepare_cached` throughout** — `rusqlite::Connection::prepare_cached`
   memoises compiled statements by SQL string. Was ~50 `prepare` calls per render
   (parse + compile on every call). Now: zero recompilation for repeated SQL.

2. **Single-window-stats query** — `query_all_window_stats` replaces four
   separate `query_window_stats` calls (5m/1h/24h/7d) with one query using CASE
   expressions for each time window. 4 DB round-trips → 1 per host/service.

3. **Eliminated double `query_latest_status` per service** — `render_service_card`
   fetched status for the UP/DOWN badge count; `render_service_item` fetched it
   again for the item display. Status is now fetched once in `render_service_card`
   and passed through as parameters. Saves N queries per render.

4. **Dead `WEB_MANIFEST` const removed** — `include_str!` was compiling the
   static manifest file into the binary. Manifest is generated dynamically at
   startup (to embed hashed icon routes); the static version was unused dead bytes.

5. **`compression` config field removed** — Documented in config template as
   `"br"/"gzip"/"none"` but the serving code always negotiated via
   `Accept-Encoding` regardless. Removing avoids user confusion. Compression is
   now automatic: brotli if the client supports it, gzip as fallback, plain last.

6. **Single-pass `render_services` filter** — Was three separate
   `.filter().collect()` passes over the services slice. Now a single loop that
   pushes each service into the correct bucket.

7. **Single-pass `html_escape`** — Was three chained `replace()` calls (three
   heap allocations + three string scans). Now a single char-by-char loop into a
   pre-sized buffer.

---

## Priority order (suggested)

1. **HTTP check** — closes biggest monitoring gap; most users have HTTP services
2. **Down alerting** — changes the tool from passive dashboard to active monitor
3. **WAL checkpoint control** — flash longevity on OpenWrt
4. **Events table + data bucketing** — better history, smaller DB
5. **Health endpoint** — operational nicety
6. **Config hot-reload / Prometheus** — nice-to-have, low urgency
