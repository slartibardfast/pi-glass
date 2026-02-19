use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::response::Html;
use chrono::Local;
use rusqlite::{params, Connection};
use serde::Deserialize;
use surge_ping::{Client, Config as PingConfig, PingIdentifier, PingSequence};

struct UiCookie {
    open_hosts: Option<HashSet<String>>,
    open_svc_cards: Option<HashSet<String>>,
}

fn parse_ui_cookie(headers: &axum::http::HeaderMap) -> UiCookie {
    let cookie_str = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let pg = cookie_str
        .split(';')
        .find_map(|p| p.trim().strip_prefix("pg="))
        .unwrap_or("");

    if pg.is_empty() {
        return UiCookie { open_hosts: None, open_svc_cards: None };
    }

    let mut open_hosts = None;
    let mut open_svc_cards = None;

    for field in pg.split('&') {
        if let Some(v) = field.strip_prefix("ho=") {
            open_hosts = Some(v.split('|').filter(|s| !s.is_empty()).map(String::from).collect());
        } else if let Some(v) = field.strip_prefix("sc=") {
            open_svc_cards = Some(v.split('|').filter(|s| !s.is_empty()).map(String::from).collect());
        }
    }

    UiCookie { open_hosts, open_svc_cards }
}

const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_PING_TIMEOUT_SECS: u64 = 2;
const DEFAULT_RETENTION_DAYS: i64 = 7;

fn data_dir() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .map(|p| format!("{p}\\pi-glass"))
            .unwrap_or_else(|_| ".\\pi-glass".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        "/opt/pi-glass".to_string()
    }
}

/// On Windows: if config.toml sits beside the .exe and no config exists in data_dir yet,
/// validate and copy it into place so the user can bootstrap by dropping a file next to the exe.
#[cfg(target_os = "windows")]
fn bootstrap_config_from_exe() {
    let exe_dir = match std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        Some(d) => d,
        None => return,
    };

    let src = exe_dir.join("config.toml");
    if !src.exists() { return; }

    let dest = std::path::Path::new(&data_dir()).join("config.toml");
    if dest.exists() { return; }

    let contents = match std::fs::read_to_string(&src) {
        Ok(s) => s,
        Err(e) => { eprintln!("Warning: could not read {}: {e}", src.display()); return; }
    };
    if let Err(e) = toml::from_str::<toml::Value>(&contents) {
        eprintln!("Warning: config.toml beside exe is not valid TOML, skipping bootstrap: {e}");
        return;
    }

    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Warning: could not create {}: {e}", parent.display());
            return;
        }
    }

    match std::fs::copy(&src, &dest) {
        Ok(_) => {
            eprintln!("Bootstrapped config: {} -> {}", src.display(), dest.display());
            use std::io::Write;
            let note = format!("\n# see {}\n", dest.display());
            let _ = std::fs::OpenOptions::new()
                .append(true)
                .open(&src)
                .and_then(|mut f| f.write_all(note.as_bytes()));
        }
        Err(e) => eprintln!("Warning: could not bootstrap config: {e}"),
    }
}

const TOKENS_CSS: &str = include_str!("../web/dist/tokens.css");

const APP_CSS: &str = include_str!("app.css");

const INLINE_JS: &str = include_str!("app.js");

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

#[derive(Deserialize, Clone)]
struct Host {
    addr: String,
    label: String,
}

#[derive(Deserialize, Clone)]
struct Service {
    label: String,
    #[serde(default)]
    icon: String,
    check: String,
    target: String,
    /// Optional base64-encoded data URI for custom icon (e.g. "data:image/png;base64,...")
    #[serde(default)]
    icon_data: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_name")]
    name: String,
    #[serde(default = "default_listen")]
    listen: String,
    #[serde(default = "default_db_path")]
    db_path: String,
    #[serde(default = "default_poll_interval")]
    poll_interval_secs: u64,
    #[serde(default = "default_ping_timeout")]
    ping_timeout_secs: u64,
    #[serde(default = "default_retention_days")]
    retention_days: i64,
    #[serde(default = "default_hosts")]
    hosts: Vec<Host>,
    #[serde(default = "default_services")]
    services: Vec<Service>,
}

fn default_name() -> String { "pi-glass".to_string() }
fn default_listen() -> String { DEFAULT_LISTEN.to_string() }
fn default_db_path() -> String { format!("{}/pi-glass.db", data_dir()) }
fn default_poll_interval() -> u64 { DEFAULT_POLL_INTERVAL_SECS }
fn default_ping_timeout() -> u64 { DEFAULT_PING_TIMEOUT_SECS }
fn default_retention_days() -> i64 { DEFAULT_RETENTION_DAYS }
fn default_hosts() -> Vec<Host> {
    vec![
        Host { addr: "192.168.1.1".into(), label: "Gateway".into() },
    ]
}

fn default_services() -> Vec<Service> {
    vec![
        Service { label: "Google".into(),         icon: "google".into(),     check: "ping".into(), target: "google.com".into(),           icon_data: None },
        Service { label: "Cloudflare".into(),     icon: "cloudflare".into(), check: "tcp".into(),  target: "cloudflare.com:443".into(),   icon_data: None },
        Service { label: "YouTube".into(),        icon: "youtube".into(),    check: "tcp".into(),  target: "youtube.com:443".into(),      icon_data: None },
        Service { label: "Outlook".into(),        icon: "outlook".into(),    check: "tcp".into(),  target: "outlook.com:443".into(),      icon_data: None },
        Service { label: "WhatsApp".into(),       icon: "whatsapp".into(),   check: "tcp".into(),  target: "web.whatsapp.com:443".into(), icon_data: None },
        Service { label: "Cloudflare DNS".into(), icon: "cloudflare".into(), check: "dns".into(),  target: "1.1.1.1".into(),             icon_data: None },
        Service { label: "Google DNS".into(),     icon: "google".into(),     check: "dns".into(),  target: "8.8.8.8".into(),             icon_data: None },
        Service { label: "Quad9 DNS".into(),      icon: "dns".into(),        check: "dns".into(),  target: "9.9.9.9".into(),             icon_data: None },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: default_name(),
            listen: default_listen(),
            db_path: default_db_path(),
            poll_interval_secs: default_poll_interval(),
            ping_timeout_secs: default_ping_timeout(),
            retention_days: default_retention_days(),
            hosts: default_hosts(),
            services: default_services(),
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn default_config_toml() -> String {
    r#"# pi-glass configuration — no config file found, showing defaults
# ─────────────────────────────────────────────────────────────────
# Place this file at:
#   Linux:   /opt/pi-glass/config.toml   (or pass --config <path>)
#   Windows: %LOCALAPPDATA%\pi-glass\config.toml
#            (drop beside pi-glass.exe for automatic first-run copy)

# Dashboard name shown in the browser tab and page heading
name = "pi-glass"

# Address and port to listen on
listen = "0.0.0.0:8080"

# SQLite database path (directory is created automatically on first run)
# db_path = "/opt/pi-glass/pi-glass.db"              # Linux default
# db_path = "%LOCALAPPDATA%\\pi-glass\\pi-glass.db"  # Windows default

# Seconds between each round of checks
poll_interval_secs = 30

# Per-check timeout for ping / TCP connect / DNS query (seconds)
ping_timeout_secs = 2

# Days of history to retain in the database
retention_days = 7

# ── LAN Hosts ────────────────────────────────────────────────────
# Monitored by ICMP ping. Each host gets a collapsible stats card.
# Requires CAP_NET_RAW on Linux (see deploy/pi-glass.service).

[[hosts]]
addr  = "192.168.1.1"
label = "Gateway"

# ── External Services ─────────────────────────────────────────────
# check    : "ping"  — ICMP echo to hostname or IP
#          : "tcp"   — TCP connect to "host:port"
#          : "dns"   — UDP DNS A-query to a nameserver IP
# icon     : built-in key — google, bing, cloudflare, dns,
#                           youtube, outlook, whatsapp
# icon_data: base64 data URI override, e.g. "data:image/png;base64,…"
# target   : hostname (ping), "host:port" (tcp), IP address (dns)

[[services]]
label  = "Google"
icon   = "google"
check  = "ping"
target = "google.com"

[[services]]
label  = "Cloudflare"
icon   = "cloudflare"
check  = "tcp"
target = "cloudflare.com:443"

[[services]]
label  = "YouTube"
icon   = "youtube"
check  = "tcp"
target = "youtube.com:443"

[[services]]
label  = "Outlook"
icon   = "outlook"
check  = "tcp"
target = "outlook.com:443"

[[services]]
label  = "WhatsApp"
icon   = "whatsapp"
check  = "tcp"
target = "web.whatsapp.com:443"

[[services]]
label  = "Cloudflare DNS"
icon   = "cloudflare"
check  = "dns"
target = "1.1.1.1"

[[services]]
label  = "Google DNS"
icon   = "google"
check  = "dns"
target = "8.8.8.8"

[[services]]
label  = "Quad9 DNS"
icon   = "dns"
check  = "dns"
target = "9.9.9.9"
"#.to_string()
}

fn load_config() -> (Config, Option<String>) {
    let path = std::env::args()
        .nth(1)
        .filter(|a| a == "--config")
        .and_then(|_| std::env::args().nth(2))
        .unwrap_or_else(|| format!("{}/config.toml", data_dir()));

    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(cfg) => {
                eprintln!("Loaded config from {path}");
                (cfg, None)
            }
            Err(e) => {
                eprintln!("Failed to parse {path}: {e}, using defaults");
                (Config::default(), Some(default_config_toml()))
            }
        },
        Err(_) => {
            eprintln!("No config at {path}, using defaults");
            (Config::default(), Some(default_config_toml()))
        }
    }
}

struct AppState {
    db: Mutex<Connection>,
    config: Config,
    config_toml: Option<String>,
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
    });

    tokio::spawn(poll_loop(state.clone()));

    let app = axum::Router::new()
        .route("/", axum::routing::get(handler))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.listen)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {}: {e}", state.config.listen));

    eprintln!("Listening on {}", state.config.listen);
    axum::serve(listener, app).await.unwrap();
}

// --- Service check functions ---

async fn check_ping(client: &Client, target: &str, seq: u16, timeout_secs: u64) -> (bool, Option<f64>) {
    let addr: IpAddr = match tokio::net::lookup_host(format!("{target}:0")).await {
        Ok(mut addrs) => match addrs.next() {
            Some(sa) => sa.ip(),
            None => return (false, None),
        },
        Err(_) => return (false, None),
    };

    let mut pinger = client.pinger(addr, PingIdentifier(0xAB)).await;
    pinger.timeout(Duration::from_secs(timeout_secs));

    let payload = [0u8; 56];
    match pinger.ping(PingSequence(seq), &payload).await {
        Ok((_packet, duration)) => (true, Some(duration.as_secs_f64() * 1000.0)),
        Err(_) => (false, None),
    }
}

async fn check_dns(nameserver: &str, timeout_secs: u64) -> (bool, Option<f64>) {
    let addr = format!("{nameserver}:53");
    let bind_addr = if nameserver.contains(':') { "[::]:0" } else { "0.0.0.0:0" };
    let sock = match tokio::net::UdpSocket::bind(bind_addr).await {
        Ok(s) => s,
        Err(_) => return (false, None),
    };

    if sock.connect(&addr).await.is_err() {
        return (false, None);
    }

    let start = Instant::now();
    if sock.send(&DNS_QUERY).await.is_err() {
        return (false, None);
    }

    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_secs(timeout_secs), sock.recv(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => (true, Some(start.elapsed().as_secs_f64() * 1000.0)),
        _ => (false, None),
    }
}

async fn check_tcp(target: &str, timeout_secs: u64) -> (bool, Option<f64>) {
    let start = Instant::now();
    match tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::net::TcpStream::connect(target),
    )
    .await
    {
        Ok(Ok(_)) => (true, Some(start.elapsed().as_secs_f64() * 1000.0)),
        _ => (false, None),
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
            let (up, latency_ms) = match svc.check.as_str() {
                "ping" => check_ping(&client, &svc.target, seq, state.config.ping_timeout_secs).await,
                "dns" => check_dns(&svc.target, state.config.ping_timeout_secs).await,
                "tcp" => check_tcp(&svc.target, state.config.ping_timeout_secs).await,
                other => {
                    eprintln!("Unknown check type '{}' for service '{}'", other, svc.label);
                    (false, None)
                }
            };

            let status = if up { "UP" } else { "DOWN" };
            let key = format!("svc:{}", svc.label);
            let now = Local::now().to_rfc3339();
            let db = state.db.lock().unwrap();
            db.execute(
                "INSERT INTO ping_results (host, timestamp, status, latency_ms) VALUES (?1, ?2, ?3, ?4)",
                params![key, now, status, latency_ms],
            )
            .unwrap();
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

// --- Stats queries ---

struct WindowStats {
    uptime_pct: Option<f64>,
    avg_ms: Option<f64>,
    min_ms: Option<f64>,
    max_ms: Option<f64>,
}

fn query_window_stats(db: &Connection, host: &str, minutes: i64) -> WindowStats {
    let cutoff = (Local::now() - chrono::Duration::minutes(minutes)).to_rfc3339();
    let mut stmt = db
        .prepare(
            "SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'UP' THEN 1 ELSE 0 END) as up_count,
                AVG(CASE WHEN status = 'UP' THEN latency_ms END) as avg_ms,
                MIN(CASE WHEN status = 'UP' THEN latency_ms END) as min_ms,
                MAX(CASE WHEN status = 'UP' THEN latency_ms END) as max_ms
            FROM ping_results
            WHERE host = ?1 AND timestamp > ?2",
        )
        .unwrap();

    stmt.query_row(params![host, cutoff], |row| {
        let total: i64 = row.get(0)?;
        let up_count: Option<i64> = row.get(1)?;
        Ok(WindowStats {
            uptime_pct: match (total, up_count) {
                (t, Some(u)) if t > 0 => Some(u as f64 * 100.0 / t as f64),
                _ => None,
            },
            avg_ms: row.get(2)?,
            min_ms: row.get(3)?,
            max_ms: row.get(4)?,
        })
    })
    .unwrap()
}


fn query_latest_status(db: &Connection, host: &str) -> (String, Option<f64>) {
    db.query_row(
        "SELECT status, latency_ms FROM ping_results WHERE host = ?1 ORDER BY id DESC LIMIT 1",
        params![host],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?)),
    )
    .unwrap_or(("--".to_string(), None))
}

fn query_recent_checks(db: &Connection, host: &str, limit: i64) -> Vec<(String, String, Option<f64>)> {
    let mut stmt = db
        .prepare(
            "SELECT timestamp, status, latency_ms FROM ping_results WHERE host = ?1 ORDER BY id DESC LIMIT ?2",
        )
        .unwrap();

    stmt.query_map(params![host, limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<f64>>(2)?,
        ))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn fmt_pct(v: Option<f64>) -> String {
    v.map_or("--".into(), |v| format!("{v:.1}%"))
}

fn fmt_ms(v: Option<f64>) -> String {
    v.map_or("--".into(), |v| format!("{v:.1}"))
}

fn tier_class(uptime_pct: Option<f64>) -> &'static str {
    match uptime_pct {
        Some(p) if p >= 100.0 => "tier-perfect",
        Some(p) if p >= 99.0  => "tier-good",
        Some(p) if p >= 95.0  => "tier-degraded",
        Some(p) if p > 0.0    => "tier-critical",
        _                     => "tier-down",
    }
}

fn query_avg_stddev(db: &Connection, host: &str, minutes: i64) -> (Option<f64>, Option<f64>) {
    let cutoff = (Local::now() - chrono::Duration::minutes(minutes)).to_rfc3339();
    db.query_row(
        "SELECT AVG(latency_ms), AVG(latency_ms * latency_ms)
         FROM ping_results WHERE host = ?1 AND timestamp > ?2 AND status = 'UP'",
        params![host, cutoff],
        |row| {
            let avg: Option<f64> = row.get(0)?;
            let avg_sq: Option<f64> = row.get(1)?;
            let stddev = match (avg, avg_sq) {
                (Some(a), Some(sq)) => Some((sq - a * a).max(0.0).sqrt()),
                _ => None,
            };
            Ok((avg, stddev))
        },
    ).unwrap_or((None, None))
}

fn query_card_uptime(db: &Connection, keys: &[String], minutes: i64) -> Option<f64> {
    if keys.is_empty() { return None; }
    let cutoff = (Local::now() - chrono::Duration::minutes(minutes)).to_rfc3339();
    let placeholders = std::iter::repeat("?").take(keys.len()).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COUNT(*), SUM(CASE WHEN status='UP' THEN 1 ELSE 0 END)
         FROM ping_results WHERE host IN ({placeholders}) AND timestamp > ?"
    );
    let mut stmt = db.prepare(&sql).unwrap();
    stmt.query_row(
        rusqlite::params_from_iter(
            keys.iter().map(|s| s.as_str()).chain(std::iter::once(cutoff.as_str()))
        ),
        |row| {
            let total: i64 = row.get(0)?;
            let up: Option<i64> = row.get(1)?;
            Ok(match (total, up) {
                (t, Some(u)) if t > 0 => Some(u as f64 * 100.0 / t as f64),
                _ => None,
            })
        },
    ).unwrap_or(None)
}

// --- SVG Icons ---

fn get_icon_svg(key: &str) -> &'static str {
    match key {
        "google" => include_str!("icons/google.svg"),
        "bing" => include_str!("icons/bing.svg"),
        "heanet" => include_str!("icons/heanet.svg"),
        "digiweb" => include_str!("icons/digiweb.svg"),
        "digiweb-dns" => include_str!("icons/digiweb-dns.svg"),
        "dkit" => include_str!("icons/dkit.svg"),
        "youtube" => include_str!("icons/youtube.html"),
        "outlook" => include_str!("icons/outlook.html"),
        "whatsapp" => include_str!("icons/whatsapp.svg"),
        "cloudflare" => include_str!("icons/cloudflare.svg"),
        "dns" => include_str!("icons/dns.svg"),
        _ => include_str!("icons/fallback.svg"),
    }
}

// --- HTML rendering ---

fn render_host(db: &Connection, host: &Host, user_open: Option<bool>) -> String {
    let w1h = query_window_stats(db, &host.addr, 60);
    let w24h = query_window_stats(db, &host.addr, 1440);
    let w7d = query_window_stats(db, &host.addr, 10080);
    let tier = tier_class(w1h.uptime_pct);
    let (cur_status, _) = query_latest_status(db, &host.addr);
    let streak_display = match w1h.uptime_pct {
        Some(p) => format!(
            r#"<span class="streak {tier}" title="1h uptime">{cur_status} {p:.1}%</span>"#
        ),
        None => r#"<span class="streak tier-down" title="No data yet">--</span>"#.to_string(),
    };

    let loss_1h = w1h.uptime_pct.map(|u| 100.0 - u);
    let loss_24h = w24h.uptime_pct.map(|u| 100.0 - u);
    let loss_7d = w7d.uptime_pct.map(|u| 100.0 - u);

    let all_up_1h = w1h.uptime_pct.map_or(true, |p| p >= 100.0);
    let open_attr = match user_open {
        Some(true)  => " open",
        Some(false) => "",
        None        => if all_up_1h { "" } else { " open" },
    };

    let mut html = format!(
        include_str!("templates/host.html"),
        open_attr = open_attr,
        label = host.label,
        addr = host.addr,
        streak_display = streak_display,
        uptime_1h = fmt_pct(w1h.uptime_pct),
        uptime_24h = fmt_pct(w24h.uptime_pct),
        uptime_7d = fmt_pct(w7d.uptime_pct),
        avg_1h = fmt_ms(w1h.avg_ms),
        avg_24h = fmt_ms(w24h.avg_ms),
        avg_7d = fmt_ms(w7d.avg_ms),
        min_1h = fmt_ms(w1h.min_ms),
        min_24h = fmt_ms(w24h.min_ms),
        min_7d = fmt_ms(w7d.min_ms),
        max_1h = fmt_ms(w1h.max_ms),
        max_24h = fmt_ms(w24h.max_ms),
        max_7d = fmt_ms(w7d.max_ms),
        loss_1h = fmt_pct(loss_1h),
        loss_24h = fmt_pct(loss_24h),
        loss_7d = fmt_pct(loss_7d),
    );

    let rows = query_recent_checks(db, &host.addr, 20);
    for (ts, status, latency) in rows {
        let latency_str = latency.map_or("--".to_string(), |v| format!("{v:.1}"));
        let class = if status == "UP" { "status-up" } else { "status-down" };
        html.push_str(&format!(
            r#"<tr><td>{ts}</td><td class="{class}">{status}</td><td>{latency_str}</td></tr>"#
        ));
    }

    html.push_str("</table></details>");
    html
}

fn render_service_item(db: &Connection, svc: &Service, id: &str) -> String {
    let key = format!("svc:{}", svc.label);
    let (status, latency) = query_latest_status(db, &key);
    let dot_class = match status.as_str() {
        "UP" => "up",
        "DOWN" => "down",
        _ => "unknown",
    };
    let icon_html = if let Some(data) = &svc.icon_data {
        format!(r#"<img style="width:20px;height:20px" src="{data}">"#)
    } else {
        get_icon_svg(&svc.icon).to_string()
    };
    let latency_str = latency.map_or("--".to_string(), |ms| format!("{ms:.0}ms"));

    let (avg_ms, stddev_ms) = query_avg_stddev(db, &key, 60);
    let avg_stddev_str = match (avg_ms, stddev_ms) {
        (Some(avg), Some(sd)) => format!(
            r#"<span class="svc-avg-latency" title="1h average latency ± standard deviation">[{avg:.0}ms ±{sd:.0}]</span>"#
        ),
        (Some(avg), None) => format!(
            r#"<span class="svc-avg-latency" title="1h average latency">[{avg:.0}ms]</span>"#
        ),
        _ => String::new(),
    };

    // Query detail data
    let w1h = query_window_stats(db, &key, 60);
    let recent = query_recent_checks(db, &key, 10);

    let mut detail_rows = String::new();
    for (ts, s, lat) in &recent {
        let cls = if s == "UP" { "status-up" } else { "status-down" };
        let lat_str = lat.map_or("--".to_string(), |v| format!("{v:.1}"));
        let time = if ts.len() > 11 { &ts[11..19] } else { ts };
        detail_rows.push_str(&format!(
            r#"<tr><td>{time}</td><td class="{cls}">{s}</td><td>{lat_str}</td></tr>"#
        ));
    }

    format!(
        include_str!("templates/service_item.html"),
        id = id,
        icon_html = icon_html,
        dot_class = dot_class,
        label = svc.label,
        latency_str = latency_str,
        avg_stddev_str = avg_stddev_str,
        check = svc.check,
        target = svc.target,
        uptime_1h = fmt_pct(w1h.uptime_pct),
        avg_1h = fmt_ms(w1h.avg_ms),
        detail_rows = detail_rows,
    )
}

fn render_service_card(db: &Connection, title: &str, svcs: &[&Service], start_idx: usize, open: bool) -> String {
    if svcs.is_empty() {
        return String::new();
    }

    let up_count = svcs.iter().filter(|s| {
        let key = format!("svc:{}", s.label);
        let (status, _) = query_latest_status(db, &key);
        status == "UP"
    }).count();
    let total = svcs.len();
    let keys: Vec<String> = svcs.iter().map(|s| format!("svc:{}", s.label)).collect();
    let card_uptime = query_card_uptime(db, &keys, 60);
    let tier = tier_class(card_uptime);
    let title_attr = match card_uptime {
        Some(p) => format!("1h uptime: {p:.1}%"),
        None    => "No data".to_string(),
    };
    let summary_text = format!(r#"<span class="{tier}" title="{title_attr}">{up_count}/{total}</span>"#);

    let open_attr = if open { " open" } else { "" };
    let mut html = format!(
        include_str!("templates/service_card.html"),
        title = title,
        summary_text = summary_text,
        open_attr = open_attr,
    );
    for (i, svc) in svcs.iter().enumerate() {
        let id = format!("svc-{}", start_idx + i);
        html.push_str(&render_service_item(db, svc, &id));
    }
    html.push_str("</div></details>");
    html
}

fn render_services(db: &Connection, services: &[Service], ui: &UiCookie) -> String {
    if services.is_empty() {
        return String::new();
    }

    let mut non_dns: Vec<&Service> = services.iter().filter(|s| s.check != "dns").collect();
    let mut dns: Vec<&Service> = services.iter().filter(|s| s.check == "dns").collect();
    non_dns.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    dns.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));

    let svc_open = |title: &str| -> bool {
        match &ui.open_svc_cards {
            None => true,
            Some(set) => set.contains(title),
        }
    };

    let mut html = render_service_card(db, "Web", &non_dns, 0, svc_open("Web"));
    html.push_str(&render_service_card(db, "DNS", &dns, non_dns.len(), svc_open("DNS")));
    html
}

async fn handler(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap) -> Html<String> {
    let ui = parse_ui_cookie(&headers);
    let db = state.db.lock().unwrap();

    let services_html = render_services(&db, &state.config.services, &ui);
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

    // Mobile backdrop + inline JS
    html.push_str(r#"<div id="svc-backdrop" class="svc-backdrop"></div>"#);
    html.push_str(&format!("<script>{INLINE_JS}</script>"));
    html.push_str("</body></html>");
    Html(html)
}
