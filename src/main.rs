use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::response::Html;
use chrono::Local;
use rusqlite::{params, Connection};
use serde::Deserialize;
use surge_ping::{Client, Config as PingConfig, PingIdentifier, PingSequence};

const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
const DEFAULT_DB_PATH: &str = "/opt/pi-glass/pi-glass.db";
const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_PING_TIMEOUT_SECS: u64 = 2;
const DEFAULT_RETENTION_DAYS: i64 = 7;
const CONFIG_PATH: &str = "/opt/pi-glass/config.toml";

const TOKENS_CSS: &str = include_str!("../web/dist/tokens.css");

const APP_CSS: &str = r#"
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
    background: var(--colorNeutralBackground1);
    color: var(--colorNeutralForeground1);
    font-family: var(--fontFamilyBase);
    font-size: var(--fontSizeBase300);
    line-height: var(--lineHeightBase300);
    padding: var(--spacingVerticalXXL) var(--spacingHorizontalXXL);
    max-width: 960px;
    margin: 0 auto;
}
.title-bar {
    display: flex;
    align-items: center;
    gap: var(--spacingHorizontalXXL);
    margin-bottom: var(--spacingVerticalXXL);
    flex-wrap: wrap;
}
h1 {
    font-size: var(--fontSizeHero700);
    font-weight: var(--fontWeightSemibold);
}
.host-card {
    background: var(--colorNeutralCardBackground);
    border: 1px solid var(--colorNeutralStroke2);
    border-radius: var(--borderRadiusLarge);
    box-shadow: var(--shadow4);
    margin-bottom: var(--spacingVerticalXXL);
    overflow: hidden;
}
.host-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacingVerticalM) var(--spacingHorizontalL);
    border-bottom: 1px solid var(--colorNeutralStroke2);
    background: var(--colorNeutralBackground3);
}
.host-header h2 {
    font-size: var(--fontSizeBase500);
    font-weight: var(--fontWeightSemibold);
}
.host-header .ip {
    color: var(--colorNeutralForeground2);
    font-weight: var(--fontWeightRegular);
}
.streak {
    font-size: var(--fontSizeBase200);
    font-weight: var(--fontWeightSemibold);
    padding: var(--spacingVerticalXS) var(--spacingHorizontalM);
    border-radius: var(--borderRadiusMedium);
}
.streak.up {
    background: var(--colorStatusSuccessBackground1);
    color: var(--colorStatusSuccessForeground1);
}
.streak.down {
    background: var(--colorStatusDangerBackground1);
    color: var(--colorStatusDangerForeground1);
}
table {
    width: 100%;
    border-collapse: collapse;
}
th, td {
    padding: var(--spacingVerticalS) var(--spacingHorizontalM);
    text-align: left;
    border-bottom: 1px solid var(--colorNeutralStroke2);
}
th {
    background: var(--colorNeutralBackground3);
    font-weight: var(--fontWeightSemibold);
    font-size: var(--fontSizeBase200);
    color: var(--colorNeutralForeground2);
}
.stats-section { padding: 0; }
.stats-section th:first-child,
.stats-section td:first-child {
    font-weight: var(--fontWeightSemibold);
}
.pings-header {
    padding: var(--spacingVerticalS) var(--spacingHorizontalL);
    border-bottom: 1px solid var(--colorNeutralStroke2);
    border-top: 1px solid var(--colorNeutralStroke2);
    font-size: var(--fontSizeBase200);
    font-weight: var(--fontWeightSemibold);
    color: var(--colorNeutralForeground2);
    background: var(--colorNeutralBackground3);
}
.status-up { color: var(--colorStatusSuccessForeground1); font-weight: var(--fontWeightSemibold); }
.status-down { color: var(--colorStatusDangerForeground1); font-weight: var(--fontWeightSemibold); }
tr:last-child td { border-bottom: none; }

/* Services bar */
.services-grid {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--spacingVerticalS) var(--spacingHorizontalL);
}
.svc-item {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--spacingHorizontalS);
    cursor: pointer;
}
.svc-icon svg { width: 20px; height: 20px; display: block; }
.svc-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
}
.svc-dot.up { background: var(--colorStatusSuccessForeground1); }
.svc-dot.down { background: var(--colorStatusDangerForeground1); }
.svc-dot.unknown { background: var(--colorNeutralForeground3); }
.svc-label {
    font-size: var(--fontSizeBase200);
    font-weight: var(--fontWeightSemibold);
}
.svc-latency {
    font-size: var(--fontSizeBase100);
    color: var(--colorNeutralForeground2);
}

/* Service detail tooltip (desktop) */
.svc-detail {
    display: none;
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 10;
    background: var(--colorNeutralCardBackground);
    border: 1px solid var(--colorNeutralStroke2);
    border-radius: var(--borderRadiusMedium);
    box-shadow: var(--shadow16);
    padding: var(--spacingVerticalM) var(--spacingHorizontalM);
    min-width: 300px;
}
.svc-item:hover .svc-detail { display: block; }
.svc-detail.open { display: block; }
.svc-detail-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--spacingVerticalS);
    font-size: var(--fontSizeBase200);
}
.svc-detail-header strong { font-size: var(--fontSizeBase300); }
.svc-detail-header .svc-target { color: var(--colorNeutralForeground2); }
.svc-close { display: none; background: none; border: none; font-size: 20px; cursor: pointer; color: var(--colorNeutralForeground2); }
.svc-detail-stats {
    display: flex;
    gap: var(--spacingHorizontalL);
    margin-bottom: var(--spacingVerticalS);
    font-size: var(--fontSizeBase200);
    color: var(--colorNeutralForeground2);
}
.svc-detail table { font-size: var(--fontSizeBase200); }
.svc-detail th, .svc-detail td {
    padding: var(--spacingVerticalXS) var(--spacingHorizontalS);
}

/* Mobile overlay */
@media (max-width: 768px) {
    .svc-item:hover .svc-detail { display: none; }
    .svc-detail.open {
        display: block;
        position: fixed;
        bottom: 0; left: 0; right: 0;
        top: auto;
        border-radius: var(--borderRadiusLarge) var(--borderRadiusLarge) 0 0;
        box-shadow: var(--shadow28);
        max-height: 60vh;
        overflow-y: auto;
        padding: var(--spacingVerticalL);
        z-index: 11;
    }
    .svc-close { display: block; }
}
.svc-backdrop {
    display: none;
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.3);
    z-index: 9;
}
.svc-backdrop.open { display: block; }
"#;

const INLINE_JS: &str = r#"
function openDetail(id){
    closeDetail();
    document.getElementById(id).classList.add('open');
    document.getElementById('svc-backdrop').classList.add('open');
}
function closeDetail(){
    document.querySelectorAll('.svc-detail.open').forEach(function(e){e.classList.remove('open')});
    document.getElementById('svc-backdrop').classList.remove('open');
}
document.querySelectorAll('.svc-item').forEach(function(el){
    el.addEventListener('click',function(e){
        if(window.innerWidth<=768){e.stopPropagation();openDetail(el.dataset.svc)}
    });
});
document.getElementById('svc-backdrop').addEventListener('click',closeDetail);
"#;

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
    #[serde(default)]
    services: Vec<Service>,
}

fn default_name() -> String { "pi-glass".to_string() }
fn default_listen() -> String { DEFAULT_LISTEN.to_string() }
fn default_db_path() -> String { DEFAULT_DB_PATH.to_string() }
fn default_poll_interval() -> u64 { DEFAULT_POLL_INTERVAL_SECS }
fn default_ping_timeout() -> u64 { DEFAULT_PING_TIMEOUT_SECS }
fn default_retention_days() -> i64 { DEFAULT_RETENTION_DAYS }
fn default_hosts() -> Vec<Host> {
    vec![
        Host { addr: "192.168.178.1".into(), label: "Router".into() },
        Host { addr: "192.168.178.6".into(), label: "AP 1".into() },
        Host { addr: "192.168.178.7".into(), label: "AP 2".into() },
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
            services: Vec::new(),
        }
    }
}

fn load_config() -> Config {
    let path = std::env::args()
        .nth(1)
        .filter(|a| a == "--config")
        .and_then(|_| std::env::args().nth(2))
        .unwrap_or_else(|| CONFIG_PATH.to_string());

    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(cfg) => {
                eprintln!("Loaded config from {path}");
                cfg
            }
            Err(e) => {
                eprintln!("Failed to parse {path}: {e}, using defaults");
                Config::default()
            }
        },
        Err(_) => {
            eprintln!("No config at {path}, using defaults");
            Config::default()
        }
    }
}

struct AppState {
    db: Mutex<Connection>,
    config: Config,
}

#[tokio::main]
async fn main() {
    let config = load_config();

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
    let sock = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
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

fn query_streak(db: &Connection, host: &str) -> (String, i64) {
    let mut stmt = db
        .prepare("SELECT status FROM ping_results WHERE host = ?1 ORDER BY id DESC LIMIT 200")
        .unwrap();

    let statuses: Vec<String> = stmt
        .query_map(params![host], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    if statuses.is_empty() {
        return ("--".to_string(), 0);
    }

    let first = &statuses[0];
    let count = statuses.iter().take_while(|s| *s == first).count() as i64;
    (first.clone(), count)
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

// --- SVG Icons ---

fn get_icon_svg(key: &str) -> &'static str {
    match key {
        "google" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z" fill="#4285F4"/><path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853"/><path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" fill="#FBBC05"/><path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" fill="#EA4335"/></svg>"##,
        "bing" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M5 3v16.5l4.5 2.5 7-4v-4l-5-2.5V3z" fill="#00809D"/><path d="M5 19.5L9.5 22l7-4v-4L9.5 11V3L5 5z" fill="#008373" opacity="0.8"/></svg>"##,
        "heanet" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#00594F"/><text x="12" y="16" text-anchor="middle" font-size="11" font-weight="bold" fill="white" font-family="sans-serif">HE</text></svg>"##,
        "digiweb" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#E31937"/><text x="12" y="16" text-anchor="middle" font-size="10" font-weight="bold" fill="white" font-family="sans-serif">DW</text></svg>"##,
        "digiweb-dns" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#E31937" opacity="0.7"/><text x="12" y="12" text-anchor="middle" font-size="7" font-weight="bold" fill="white" font-family="sans-serif">DW</text><text x="12" y="20" text-anchor="middle" font-size="7" font-weight="bold" fill="white" font-family="sans-serif">NS</text></svg>"##,
        "cloudflare" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#F48120"/><text x="12" y="16" text-anchor="middle" font-size="10" font-weight="bold" fill="white" font-family="sans-serif">CF</text></svg>"##,
        "dns" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#5B5FC7"/><text x="12" y="16" text-anchor="middle" font-size="10" font-weight="bold" fill="white" font-family="sans-serif">NS</text></svg>"##,
        _ => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#888"/><text x="12" y="16" text-anchor="middle" font-size="10" font-weight="bold" fill="white" font-family="sans-serif">?</text></svg>"##,
    }
}

// --- HTML rendering ---

fn render_host(db: &Connection, host: &Host) -> String {
    let w1h = query_window_stats(db, &host.addr, 60);
    let w24h = query_window_stats(db, &host.addr, 1440);
    let w7d = query_window_stats(db, &host.addr, 10080);
    let (streak_status, streak_count) = query_streak(db, &host.addr);

    let streak_class = if streak_status == "UP" { "up" } else { "down" };
    let streak_display = if streak_count > 0 {
        format!(
            r#"<span class="streak {streak_class}">{streak_status} &times; {streak_count}</span>"#
        )
    } else {
        r#"<span class="streak">--</span>"#.to_string()
    };

    let loss_1h = w1h.uptime_pct.map(|u| 100.0 - u);
    let loss_24h = w24h.uptime_pct.map(|u| 100.0 - u);
    let loss_7d = w7d.uptime_pct.map(|u| 100.0 - u);

    let mut html = format!(
        r#"<div class="host-card">
<div class="host-header">
  <h2>{} <span class="ip">({})</span></h2>
  {streak_display}
</div>
<div class="stats-section">
<table>
<tr><th></th><th>1 hour</th><th>24 hours</th><th>7 days</th></tr>
<tr><td>Uptime</td><td>{}</td><td>{}</td><td>{}</td></tr>
<tr><td>Avg ms</td><td>{}</td><td>{}</td><td>{}</td></tr>
<tr><td>Min ms</td><td>{}</td><td>{}</td><td>{}</td></tr>
<tr><td>Max ms</td><td>{}</td><td>{}</td><td>{}</td></tr>
<tr><td>Loss</td><td>{}</td><td>{}</td><td>{}</td></tr>
</table>
</div>
<div class="pings-header">Last 20 pings</div>
<table>
<tr><th>Timestamp</th><th>Status</th><th>Latency (ms)</th></tr>"#,
        host.label,
        host.addr,
        fmt_pct(w1h.uptime_pct),
        fmt_pct(w24h.uptime_pct),
        fmt_pct(w7d.uptime_pct),
        fmt_ms(w1h.avg_ms),
        fmt_ms(w24h.avg_ms),
        fmt_ms(w7d.avg_ms),
        fmt_ms(w1h.min_ms),
        fmt_ms(w24h.min_ms),
        fmt_ms(w7d.min_ms),
        fmt_ms(w1h.max_ms),
        fmt_ms(w24h.max_ms),
        fmt_ms(w7d.max_ms),
        fmt_pct(loss_1h),
        fmt_pct(loss_24h),
        fmt_pct(loss_7d),
    );

    let rows = query_recent_checks(db, &host.addr, 20);
    for (ts, status, latency) in rows {
        let latency_str = latency.map_or("--".to_string(), |v| format!("{v:.1}"));
        let class = if status == "UP" { "status-up" } else { "status-down" };
        html.push_str(&format!(
            r#"<tr><td>{ts}</td><td class="{class}">{status}</td><td>{latency_str}</td></tr>"#
        ));
    }

    html.push_str("</table></div>");
    html
}

fn render_services(db: &Connection, services: &[Service]) -> String {
    if services.is_empty() {
        return String::new();
    }

    let mut html = String::new();

    for (i, svc) in services.iter().enumerate() {
        let key = format!("svc:{}", svc.label);
        let id = format!("svc-{i}");
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
        let latency_str = latency.map_or(String::new(), |ms| {
            format!(r#" <span class="svc-latency">{ms:.0}ms</span>"#)
        });

        // Query detail data
        let w1h = query_window_stats(db, &key, 60);
        let recent = query_recent_checks(db, &key, 10);

        // Build detail table rows
        let mut detail_rows = String::new();
        for (ts, s, lat) in &recent {
            let cls = if s == "UP" { "status-up" } else { "status-down" };
            let lat_str = lat.map_or("--".to_string(), |v| format!("{v:.1}"));
            // Show just time portion for compactness
            let time = if ts.len() > 11 { &ts[11..19] } else { ts };
            detail_rows.push_str(&format!(
                r#"<tr><td>{time}</td><td class="{cls}">{s}</td><td>{lat_str}</td></tr>"#
            ));
        }

        html.push_str(&format!(
            r#"<div class="svc-item" data-svc="{id}">
<span class="svc-icon">{icon_html}</span>
<span class="svc-dot {dot_class}"></span>
<span class="svc-label">{}{latency_str}</span>
<div class="svc-detail" id="{id}">
<div class="svc-detail-header">
<div><strong>{}</strong> <span class="svc-target">{} &rarr; {}</span></div>
<button class="svc-close" onclick="closeDetail()">&times;</button>
</div>
<div class="svc-detail-stats">
<span>Uptime 1h: {}</span>
<span>Avg: {}</span>
</div>
<table>
<tr><th>Time</th><th>Status</th><th>ms</th></tr>
{detail_rows}
</table>
</div>
</div>"#,
            svc.label,
            svc.label,
            svc.check,
            svc.target,
            fmt_pct(w1h.uptime_pct),
            fmt_ms(w1h.avg_ms),
        ));
    }

    html
}

async fn handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let db = state.db.lock().unwrap();

    let services_html = render_services(&db, &state.config.services);
    let name = &state.config.name;

    let mut html = format!(
        r#"<!DOCTYPE html>
<html><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="30">
<title>{name}</title>
<style>{TOKENS_CSS}</style>
<style>{APP_CSS}</style>
</head><body>
<div class="title-bar">
<h1>{name}</h1>
<div class="services-grid">{services_html}</div>
</div>
"#
    );

    // LAN host cards
    for host in &state.config.hosts {
        html.push_str(&render_host(&db, host));
    }

    // Mobile backdrop + inline JS
    html.push_str(r#"<div id="svc-backdrop" class="svc-backdrop"></div>"#);
    html.push_str(&format!("<script>{INLINE_JS}</script>"));
    html.push_str("</body></html>");
    Html(html)
}
