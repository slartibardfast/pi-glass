use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use chrono::Local;
use rusqlite::{params, Connection, OpenFlags};
use surge_ping::{Client, Config as PingConfig, PingIdentifier, PingSequence};

use tower_http::compression::CompressionLayer;

use pi_glass::*;

// Minimal DNS A-query for google.com
const DNS_QUERY: [u8; 28] = [
    0xAB, 0xCD, // ID
    0x01, 0x00, // Flags: standard query, RD=1
    0x00, 0x01, // QDCOUNT: 1
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // AN/NS/AR counts
    0x06, b'g', b'o', b'o', b'g', b'l', b'e',
    0x03, b'c', b'o', b'm',
    0x00,       // end of name
    0x00, 0x01, // type A
    0x00, 0x01, // class IN
];

struct PageCache {
    generation: usize,
    entries: HashMap<u64, Bytes>,
}

fn content_hash(data: &str) -> String {
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    format!("{:016x}", h.finish())
}

struct AppState {
    db: Mutex<Connection>,
    read_db: Mutex<Connection>,
    config: Config,
    config_toml: Option<String>,
    resolved_ips: Mutex<HashMap<String, Option<String>>>,
    poll_generation: AtomicUsize,
    page_cache: RwLock<PageCache>,
    effective_refresh_secs: AtomicUsize,
    css_hash: String,
    js_hash: String,
}

async fn cors_headers(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> impl axum::response::IntoResponse {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
             axum::http::HeaderValue::from_static("*"));
    h.insert(axum::http::HeaderName::from_static("access-control-allow-private-network"),
             axum::http::HeaderValue::from_static("true"));
    resp
}

async fn serve_font() -> impl axum::response::IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "font/woff2"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        SPARKS_WOFF2,
    )
}

async fn serve_css() -> impl axum::response::IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        format!("{TOKENS_CSS}\n{APP_CSS}"),
    )
}

async fn serve_js() -> impl axum::response::IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        INLINE_JS,
    )
}

async fn serve_favicon_ico() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/x-icon"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")], FAVICON_ICO)
}

async fn serve_favicon_svg() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")], FAVICON_SVG)
}

async fn serve_apple_touch() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/png"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")], APPLE_TOUCH_ICON)
}

async fn serve_favicon_192() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/png"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")], FAVICON_192)
}

async fn serve_favicon_512() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "image/png"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")], FAVICON_512)
}

async fn serve_manifest() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/manifest+json"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")], WEB_MANIFEST)
}

#[tokio::main]
async fn main() {
    #[cfg(target_os = "windows")]
    bootstrap_config_from_exe();

    let (config, config_toml) = load_config();

    if let Some(parent) = std::path::Path::new(&config.db_path).parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("Failed to create data directory {}: {e}", parent.display()));
    }

    let conn = Connection::open(&config.db_path)
        .unwrap_or_else(|e| panic!("Failed to open database at {}: {e}", config.db_path));

    if config.wal_mode {
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .expect("Failed to enable WAL mode");
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ping_results (
            id         INTEGER PRIMARY KEY,
            host       TEXT NOT NULL,
            timestamp  TEXT NOT NULL,
            status     TEXT NOT NULL,
            latency_ms REAL
        );
        CREATE INDEX IF NOT EXISTS idx_ping_host_ts ON ping_results(host, timestamp);
        CREATE INDEX IF NOT EXISTS idx_ping_host_id ON ping_results(host, id DESC);",
    )
    .expect("Failed to create table");

    let read_conn = Connection::open_with_flags(
        &config.db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap_or_else(|e| panic!("Failed to open read-only database at {}: {e}", config.db_path));
    read_conn.busy_timeout(Duration::from_secs(5)).expect("Failed to set busy timeout");

    let css_hash = content_hash(&format!("{TOKENS_CSS}\n{APP_CSS}"));
    let js_hash = content_hash(INLINE_JS);

    let effective_refresh = config.poll_interval_secs as usize;
    let state = Arc::new(AppState {
        db: Mutex::new(conn),
        read_db: Mutex::new(read_conn),
        config,
        config_toml,
        resolved_ips: Mutex::new(HashMap::new()),
        poll_generation: AtomicUsize::new(0),
        page_cache: RwLock::new(PageCache { generation: 0, entries: HashMap::new() }),
        effective_refresh_secs: AtomicUsize::new(effective_refresh),
        css_hash,
        js_hash,
    });

    pre_render_startup(&state);
    tokio::spawn(poll_loop(state.clone()));

    let css_route = format!("/static/{}.css", state.css_hash);
    let js_route = format!("/static/{}.js", state.js_hash);

    let compression = match state.config.compression.as_str() {
        "gzip" => CompressionLayer::new().br(false).gzip(true),
        "none" => CompressionLayer::new().br(false).gzip(false),
        _ => CompressionLayer::new().br(true).gzip(false),
    };

    let app = axum::Router::new()
        .route("/", axum::routing::get(handler))
        .route(&css_route, axum::routing::get(serve_css))
        .route(&js_route, axum::routing::get(serve_js))
        .route("/font/sparks.woff2", axum::routing::get(serve_font))
        .route("/favicon.ico", axum::routing::get(serve_favicon_ico))
        .route("/favicon.svg", axum::routing::get(serve_favicon_svg))
        .route("/apple-touch-icon.png", axum::routing::get(serve_apple_touch))
        .route("/favicon-192.png", axum::routing::get(serve_favicon_192))
        .route("/favicon-512.png", axum::routing::get(serve_favicon_512))
        .route("/site.webmanifest", axum::routing::get(serve_manifest))
        .layer(compression)
        .layer(axum::middleware::from_fn(cors_headers))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.listen)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {}: {e}", state.config.listen));

    eprintln!("Listening on {} (compression: {})", state.config.listen, state.config.compression);
    axum::serve(listener, app).await.unwrap();
}

// --- Service check functions ---

async fn check_ping(client: &Client, target: &str, seq: u16, timeout_secs: u64) -> (bool, Option<f64>, Option<String>) {
    let addr: IpAddr = match tokio::net::lookup_host(format!("{target}:0")).await {
        Ok(mut addrs) => match addrs.next() {
            Some(sa) => sa.ip(),
            None => return (false, None, None),
        },
        Err(_) => return (false, None, None),
    };

    let mut pinger = client.pinger(addr, PingIdentifier(0xAB)).await;
    pinger.timeout(Duration::from_secs(timeout_secs));

    let payload = [0u8; 56];
    match pinger.ping(PingSequence(seq), &payload).await {
        Ok((_packet, duration)) => (true, Some(duration.as_secs_f64() * 1000.0), Some(addr.to_string())),
        Err(_) => (false, None, Some(addr.to_string())),
    }
}

async fn check_dns(nameserver: &str, timeout_secs: u64) -> (bool, Option<f64>, Option<String>) {
    let addr = format!("{nameserver}:53");
    let bind_addr = if nameserver.contains(':') { "[::]:0" } else { "0.0.0.0:0" };
    let sock = match tokio::net::UdpSocket::bind(bind_addr).await {
        Ok(s) => s,
        Err(_) => return (false, None, None),
    };

    if sock.connect(&addr).await.is_err() {
        return (false, None, None);
    }

    let start = Instant::now();
    if sock.send(&DNS_QUERY).await.is_err() {
        return (false, None, None);
    }

    let mut buf = [0u8; 512];
    // nameserver IS the IP — no resolution to show
    match tokio::time::timeout(Duration::from_secs(timeout_secs), sock.recv(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => (true, Some(start.elapsed().as_secs_f64() * 1000.0), None),
        _ => (false, None, None),
    }
}

async fn check_tcp(target: &str, timeout_secs: u64) -> (bool, Option<f64>, Option<String>) {
    let start = Instant::now();
    match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::net::TcpStream::connect(target),
    )
    .await
    {
        Ok(Ok(stream)) => {
            let peer_ip = stream.peer_addr().ok().map(|a| a.ip().to_string());
            (true, Some(start.elapsed().as_secs_f64() * 1000.0), peer_ip)
        }
        _ => (false, None, None),
    }
}

// --- Poll loop ---

async fn poll_loop(state: Arc<AppState>) {
    let client = Client::new(&PingConfig::default())
        .expect("Failed to create ping client (need CAP_NET_RAW)");

    let mut interval = tokio::time::interval(Duration::from_secs(state.config.poll_interval_secs));
    let mut seq = 0u16;

    loop {
        interval.tick().await;

        let mut rows: Vec<(String, String, &'static str, Option<f64>)> = Vec::new();
        let mut new_resolved: Vec<(String, Option<String>)> = Vec::new();

        // LAN hosts
        for host in &state.config.hosts {
            let addr: IpAddr = host.addr.parse().unwrap_or_else(|e| {
                panic!("Invalid host address '{}': {e}", host.addr)
            });

            let mut pinger = client.pinger(addr, PingIdentifier(0xAB)).await;
            pinger.timeout(Duration::from_secs(state.config.ping_timeout_secs));

            let payload = [0u8; 56];
            let (status, latency_ms) = match pinger.ping(PingSequence(seq), &payload).await {
                Ok((_packet, duration)) => ("UP", Some(duration.as_secs_f64() * 1000.0)),
                Err(_) => ("DOWN", None),
            };

            rows.push((host.addr.clone(), Local::now().to_rfc3339(), status, latency_ms));
        }

        // External services
        for svc in &state.config.services {
            let (up, latency_ms, resolved_ip) = match svc.check.as_str() {
                "ping" => check_ping(&client, &svc.target, seq, state.config.ping_timeout_secs).await,
                "dns" => check_dns(&svc.target, state.config.ping_timeout_secs).await,
                "tcp" => check_tcp(&svc.target, state.config.ping_timeout_secs).await,
                other => {
                    eprintln!("Unknown check type '{}' for service '{}'", other, svc.label);
                    (false, None, None)
                }
            };

            rows.push((
                format!("svc:{}", svc.label),
                Local::now().to_rfc3339(),
                if up { "UP" } else { "DOWN" },
                latency_ms,
            ));
            new_resolved.push((svc.label.clone(), resolved_ip));
        }

        // Update resolved IPs
        {
            let mut ips = state.resolved_ips.lock().unwrap();
            for (label, ip) in new_resolved {
                ips.insert(label, ip);
            }
        }

        // Single transaction: all INSERTs + purge (one fsync)
        let cutoff = (Local::now() - chrono::Duration::days(state.config.retention_days)).to_rfc3339();
        {
            let mut db = state.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            for (host, now, status, latency_ms) in &rows {
                tx.execute(
                    "INSERT INTO ping_results (host, timestamp, status, latency_ms) VALUES (?1, ?2, ?3, ?4)",
                    params![host, now, status, latency_ms],
                ).unwrap();
            }
            tx.execute(
                "DELETE FROM ping_results WHERE timestamp < ?1",
                params![cutoff],
            ).unwrap();
            tx.commit().unwrap();
        }

        pre_render_and_advance(&state);
        seq = seq.wrapping_add(1);
    }
}

// --- Page rendering ---

fn render_page(state: &AppState, ui: &UiCookie, refresh_secs: u64) -> String {
    let db = state.read_db.lock().unwrap();
    let resolved_ips = state.resolved_ips.lock().unwrap().clone();

    let services_html = render_services(&db, &state.config.services, ui, &resolved_ips);
    let name = &state.config.name;

    let style_head = format!(
        r#"<link rel="stylesheet" href="/static/{}.css">"#,
        state.css_hash,
    );
    let mut html = format!(
        include_str!("templates/page.html"),
        name = name,
        refresh_secs = refresh_secs,
        style_head = style_head,
        services_html = services_html,
    );

    for host in &state.config.hosts {
        let user_open = ui.open_hosts.as_ref().map(|set| set.contains(&host.addr));
        html.push_str(&render_host(&db, host, user_open));
    }

    if let Some(ref toml) = state.config_toml {
        html.push_str(r#"<details class="config-card" open><summary class="config-summary">config.toml — save this file to get started</summary><pre class="config-block">"#);
        html.push_str(&html_escape(toml));
        html.push_str("</pre></details>");
    }

    html.push_str(r##"<footer>Made with &#10084;&#65039; by <a href="mailto:david@connol.ly">David Connolly</a> &amp; <a href="https://claude.ai">Claude</a> &middot; <a href="https://github.com/slartibardfast/pi-glass">pi-glass</a></footer>"##);
    html.push_str(&format!(r#"<script src="/static/{}.js"></script>"#, state.js_hash));
    html.push_str("</body></html>");

    html
}

fn pre_render_startup(state: &AppState) {
    let html = render_page(state, &parse_ui_cookie(""), state.config.poll_interval_secs);
    let mut hasher = DefaultHasher::new();
    "".hash(&mut hasher);
    let default_hash = hasher.finish();
    let mut cache = state.page_cache.write().unwrap();
    cache.entries.insert(default_hash, Bytes::from(html));
}

fn pre_render_and_advance(state: &AppState) {
    let start = Instant::now();
    let mut html = render_page(state, &parse_ui_cookie(""), state.config.poll_interval_secs);
    let render_secs = start.elapsed().as_secs();

    let effective = if render_secs >= 1 {
        state.config.poll_interval_secs.saturating_sub(render_secs).max(1)
    } else {
        state.config.poll_interval_secs
    };
    state.effective_refresh_secs.store(effective as usize, Ordering::Release);

    if effective != state.config.poll_interval_secs {
        html = html.replacen(
            &format!(r#"content="{}""#, state.config.poll_interval_secs),
            &format!(r#"content="{}""#, effective),
            1,
        );
    }

    let mut hasher = DefaultHasher::new();
    "".hash(&mut hasher);
    let default_hash = hasher.finish();

    let mut cache = state.page_cache.write().unwrap();
    cache.generation += 1;
    cache.entries.clear();
    cache.entries.insert(default_hash, Bytes::from(html));
    let new_gen = cache.generation;
    drop(cache);

    state.poll_generation.store(new_gen, Ordering::Release);
}

// --- HTTP handler ---

async fn handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    let cookie_str = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let generation = state.poll_generation.load(Ordering::Acquire);

    let mut hasher = DefaultHasher::new();
    cookie_str.hash(&mut hasher);
    let cookie_hash = hasher.finish();

    let etag = format!("\"{generation}-{cookie_hash}\"");

    // Fast path: 304 Not Modified
    if let Some(inm) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) {
        if inm == etag {
            return (
                StatusCode::NOT_MODIFIED,
                [
                    (header::CACHE_CONTROL, "no-cache"),
                    (header::ETAG, &etag),
                ],
            ).into_response();
        }
    }

    // Check in-memory cache
    {
        let cache = state.page_cache.read().unwrap();
        if cache.generation == generation {
            if let Some(html) = cache.entries.get(&cookie_hash) {
                return (
                    [
                        (header::CACHE_CONTROL, "no-cache".to_string()),
                        (header::ETAG, etag.clone()),
                        (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
                    ],
                    html.clone(),
                ).into_response();
            }
        }
    }

    // Cache miss — render fresh (cookie-specific view)
    let refresh = state.effective_refresh_secs.load(Ordering::Acquire) as u64;
    let html = render_page(&state, &parse_ui_cookie(cookie_str), refresh);
    let body = Bytes::from(html);

    // Store in cache only if generation still matches
    {
        let mut cache = state.page_cache.write().unwrap();
        if cache.generation == generation {
            cache.entries.insert(cookie_hash, body.clone());
        }
    }

    (
        [
            (header::CACHE_CONTROL, "no-cache".to_string()),
            (header::ETAG, etag),
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
        ],
        body,
    ).into_response()
}
