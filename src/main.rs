use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::response::Html;
use chrono::Local;
use rusqlite::{params, Connection};
use surge_ping::{Client, Config as PingConfig, PingIdentifier, PingSequence};

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

struct AppState {
    db: Mutex<Connection>,
    config: Config,
    config_toml: Option<String>,
    resolved_ips: Mutex<HashMap<String, Option<String>>>,
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
        )",
    )
    .expect("Failed to create table");

    let state = Arc::new(AppState {
        db: Mutex::new(conn),
        config,
        config_toml,
        resolved_ips: Mutex::new(HashMap::new()),
    });

    tokio::spawn(poll_loop(state.clone()));

    let app = axum::Router::new()
        .route("/", axum::routing::get(handler))
        .route("/font/sparks.woff2", axum::routing::get(serve_font))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.listen)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {}: {e}", state.config.listen));

    eprintln!("Listening on {}", state.config.listen);
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

            let now = Local::now().to_rfc3339();
            let db = state.db.lock().unwrap();
            db.execute(
                "INSERT INTO ping_results (host, timestamp, status, latency_ms) VALUES (?1, ?2, ?3, ?4)",
                params![host.addr, now, status, latency_ms],
            )
            .unwrap();
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

            let status = if up { "UP" } else { "DOWN" };
            let key = format!("svc:{}", svc.label);
            let now = Local::now().to_rfc3339();
            {
                let db = state.db.lock().unwrap();
                db.execute(
                    "INSERT INTO ping_results (host, timestamp, status, latency_ms) VALUES (?1, ?2, ?3, ?4)",
                    params![key, now, status, latency_ms],
                )
                .unwrap();
            }
            state.resolved_ips.lock().unwrap().insert(svc.label.clone(), resolved_ip);
        }

        // Purge old records
        let cutoff = (Local::now() - chrono::Duration::days(state.config.retention_days)).to_rfc3339();
        let db = state.db.lock().unwrap();
        db.execute(
            "DELETE FROM ping_results WHERE timestamp < ?1",
            params![cutoff],
        )
        .unwrap();

        seq = seq.wrapping_add(1);
    }
}

// --- HTTP handler ---

async fn handler(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap) -> Html<String> {
    let cookie_str = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ui = parse_ui_cookie(cookie_str);
    let db = state.db.lock().unwrap();
    let resolved_ips = state.resolved_ips.lock().unwrap().clone();

    let services_html = render_services(&db, &state.config.services, &ui, &resolved_ips);
    let name = &state.config.name;

    let mut html = format!(
        include_str!("templates/page.html"),
        name = name,
        tokens_css = TOKENS_CSS,
        app_css = APP_CSS,
        services_html = services_html,
    );

    // LAN host cards
    for host in &state.config.hosts {
        let user_open = ui.open_hosts.as_ref().map(|set| set.contains(&host.addr));
        html.push_str(&render_host(&db, host, user_open));
    }

    // Config block — only shown when no config file was found
    if let Some(ref toml) = state.config_toml {
        html.push_str(r#"<details class="config-card" open><summary class="config-summary">config.toml — save this file to get started</summary><pre class="config-block">"#);
        html.push_str(&html_escape(toml));
        html.push_str("</pre></details>");
    }

    // Footer
    html.push_str(r##"<footer>Made with &#10084;&#65039; by <a href="mailto:david@connol.ly">David Connolly</a> &amp; <a href="https://claude.ai">Claude</a> &middot; <a href="https://github.com/slartibardfast/pi-glass">pi-glass</a></footer>"##);

    html.push_str(&format!("<script>{INLINE_JS}</script>"));
    html.push_str("</body></html>");
    Html(html)
}
