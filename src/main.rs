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
    margin-bottom: var(--spacingVerticalXXL);
}
h1 {
    font-size: var(--fontSizeHero700);
    margin-bottom: var(--spacingVerticalM);
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
    cursor: pointer;
    list-style: none;
}
.host-header::-webkit-details-marker { display: none; }
.host-header h2 {
    font-size: var(--fontSizeBase500);
    font-weight: var(--fontWeightSemibold);
}
.host-header .ip {
    color: var(--colorNeutralForeground2);
    font-weight: var(--fontWeightRegular);
    display: block;
    text-align: center;
    font-size: var(--fontSizeBase300);
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
    display: grid;
    grid-template-columns: 20px 12px 1fr auto;
    gap: var(--spacingVerticalXS) var(--spacingHorizontalXS);
    align-items: center;
}
.svc-item {
    display: grid;
    grid-template-columns: subgrid;
    grid-column: 1 / -1;
    position: relative;
    cursor: pointer;
    align-items: center;
}
.svc-icon svg, .svc-icon img { width: 20px; height: 20px; display: block; }
.svc-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    justify-self: center;
}
.svc-dot.up { background: var(--colorStatusSuccessForeground1); }
.svc-dot.down { background: var(--colorStatusDangerForeground1); }
.svc-dot.unknown { background: var(--colorNeutralForeground3); }
.svc-label {
    font-size: var(--fontSizeBase200);
    font-weight: var(--fontWeightSemibold);
    white-space: nowrap;
}
.svc-latency {
    font-size: var(--fontSizeBase100);
    color: var(--colorNeutralForeground2);
    text-align: right;
}
.svc-card {
    background: var(--colorNeutralCardBackground);
    border: 1px solid var(--colorNeutralStroke2);
    border-radius: var(--borderRadiusLarge);
    box-shadow: var(--shadow4);
    margin-bottom: var(--spacingVerticalL);
}
.svc-card > summary {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacingVerticalS) var(--spacingHorizontalL);
    background: var(--colorNeutralBackground3);
    border-bottom: 1px solid var(--colorNeutralStroke2);
    cursor: pointer;
    list-style: none;
    font-size: var(--fontSizeBase400);
    font-weight: var(--fontWeightSemibold);
}
.svc-card > summary::-webkit-details-marker { display: none; }
.svc-card .services-grid {
    padding: var(--spacingVerticalS) var(--spacingHorizontalL);
}

/* Service detail tooltip */
.svc-detail {
    display: none;
    position: fixed;
    z-index: 10;
    background: var(--colorNeutralCardBackground);
    border: 1px solid var(--colorNeutralStroke2);
    border-radius: var(--borderRadiusMedium);
    box-shadow: var(--shadow16);
    padding: var(--spacingVerticalM) var(--spacingHorizontalM);
    min-width: 300px;
    max-width: 400px;
}
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
.svc-close { background: none; border: none; font-size: 20px; cursor: pointer; color: var(--colorNeutralForeground2); }
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
    .svc-detail.open {
        bottom: 0; left: 0; right: 0;
        top: auto;
        border-radius: var(--borderRadiusLarge) var(--borderRadiusLarge) 0 0;
        box-shadow: var(--shadow28);
        max-height: 60vh;
        max-width: none;
        overflow-y: auto;
        padding: var(--spacingVerticalL);
        z-index: 11;
    }
}
.svc-backdrop {
    display: none;
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.3);
    z-index: 9;
}
.svc-backdrop.open { display: block; }
footer {
    text-align: center;
    padding: var(--spacingVerticalXXL) 0 var(--spacingVerticalM);
    font-size: var(--fontSizeBase200);
    color: var(--colorNeutralForeground3);
}
footer a { color: var(--colorBrandForeground1); text-decoration: none; }
footer a:hover { text-decoration: underline; }
"#;

const INLINE_JS: &str = r#"
function openDetail(id,anchor){
    closeDetail();
    var d=document.getElementById(id);
    d.classList.add('open');
    document.getElementById('svc-backdrop').classList.add('open');
    if(window.innerWidth>768&&anchor){
        var r=anchor.getBoundingClientRect();
        var top=r.bottom+4;
        if(top+300>window.innerHeight){top=r.top-304}
        d.style.top=top+'px';
        d.style.left=Math.max(8,Math.min(r.left,window.innerWidth-320))+'px';
    }
}
function closeDetail(){
    document.querySelectorAll('.svc-detail.open').forEach(function(e){e.classList.remove('open');e.style.top='';e.style.left=''});
    document.getElementById('svc-backdrop').classList.remove('open');
}
document.querySelectorAll('.svc-item').forEach(function(el){
    el.addEventListener('click',function(){openDetail(el.dataset.svc,el)});
});
document.querySelectorAll('.svc-detail').forEach(function(d){
    d.addEventListener('click',function(e){e.stopPropagation()});
});
document.querySelectorAll('.svc-close').forEach(function(b){
    b.addEventListener('click',function(e){e.stopPropagation();closeDetail()});
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
        "dkit" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#003B5C"/><text x="12" y="10" text-anchor="middle" font-size="6.5" font-weight="bold" fill="white" font-family="sans-serif">DkIT</text><rect x="3" y="13" width="18" height="2" rx="1" fill="#8DC63F"/></svg>"##,
        "youtube" => r##"<img style="width:20px;height:20px" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAABr0lEQVRYhe3Xv48MYQDG8c87uTjubkmEjd+hkEs0crfRiUahYVfhDxCUCpVoVBKiENGKnEKiccUFEY2C2uxFIuQo/CgUGw171p5iRzEzCiGxs5sdxT3J5H0zmed9vsX7zjwTkiRRpqJS01cB/geAkE8StXEcxSx2o4oKJrNxQ/Z8wPq/rPcVCXrZvI1v2djCezTxMIhXfgEkavuxkAWPQh/QCOIXIVFbhzfYMaLwXB8xHeF4CeGwC/UIB0oIz3Uwwr4SAfZG2N6XpbqR0w2ioZzgrRE29WWZWMutizy/w6HZQQGqEaYKWWemeXqTe1fZs60owFRxgFwnDvN6nitnqUz0664M51U8voYLJ3m7wKl6X/uj9G/B2FBWWfnB9btcnqPd6Rtg2SD7YP4J52/w7lMRd7s4wOIS567xrFkkONfyGD5jyz9bOl3OXOL2A3q9QcKhFRK1xzgy6EoF9SjCq5LC4WWEuESAZl5IlrBzxOFpIQni7zgm7WujDK8Hcff3UtrAjLQbbpaWz8nsyktpJC2pf1JeShN8kR7xTna/Je2Ci7gfxF0Iq79mqwBlA/wEihVj07SFCdQAAAAASUVORK5CYII=">"##,
        "outlook" => r##"<img style="width:20px;height:20px" src="data:image/png;base64,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">"##,
        "whatsapp" => r##"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect width="24" height="24" rx="4" fill="#25D366"/><path d="M17.5 14.4c-.3-.15-1.7-.84-2-.94-.3-.1-.5-.15-.7.15-.2.3-.75.94-.9 1.13-.17.2-.33.22-.63.07-.3-.15-1.25-.46-2.38-1.47-.88-.78-1.47-1.75-1.64-2.05-.17-.3-.02-.46.13-.61.13-.13.3-.34.44-.51.15-.17.2-.3.3-.49.1-.2.05-.37-.03-.52-.07-.15-.68-1.64-.93-2.24-.25-.6-.5-.52-.68-.53h-.58c-.2 0-.52.07-.8.37-.27.3-1.04 1.02-1.04 2.49s1.07 2.89 1.22 3.09c.15.2 2.1 3.2 5.08 4.49.71.31 1.27.49 1.7.63.71.23 1.36.2 1.87.12.57-.09 1.7-.7 1.94-1.37.24-.68.24-1.26.17-1.38-.08-.12-.27-.2-.57-.34z" fill="white"/><path d="M12 2C6.48 2 2 6.48 2 12c0 1.77.47 3.44 1.28 4.88L2 22l5.27-1.38C8.69 21.52 10.3 22 12 22c5.52 0 10-4.48 10-10S17.52 2 12 2zm0 18c-1.5 0-2.94-.4-4.2-1.15l-.3-.18-3.12.82.83-3.04-.2-.31A7.94 7.94 0 014 12c0-4.41 3.59-8 8-8s8 3.59 8 8-3.59 8-8 8z" fill="white" opacity="0.3"/></svg>"##,
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

    let all_up_1h = w1h.uptime_pct.map_or(true, |p| p >= 100.0);
    let open_attr = if all_up_1h { "" } else { " open" };

    let mut html = format!(
        r#"<details class="host-card"{open_attr}>
<summary class="host-header">
  <h2>{} <span class="ip">({})</span></h2>
  {streak_display}
</summary>
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
        r#"<div class="svc-item" data-svc="{id}">
<span class="svc-icon">{icon_html}</span>
<span class="svc-dot {dot_class}"></span>
<span class="svc-label">{}</span>
<span class="svc-latency">{latency_str}</span>
<div class="svc-detail" id="{id}">
<div class="svc-detail-header">
<div><strong>{}</strong> <span class="svc-target">{} &rarr; {}</span></div>
<button class="svc-close" >&times;</button>
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
    )
}

fn render_service_card(db: &Connection, title: &str, svcs: &[&Service], start_idx: usize) -> String {
    if svcs.is_empty() {
        return String::new();
    }

    let up_count = svcs.iter().filter(|s| {
        let key = format!("svc:{}", s.label);
        let (status, _) = query_latest_status(db, &key);
        status == "UP"
    }).count();
    let total = svcs.len();
    let summary_class = if up_count == total { "status-up" } else { "status-down" };
    let summary_text = format!(r#"<span class="{summary_class}">{up_count}/{total}</span>"#);

    let mut html = format!(
        r#"<details class="svc-card" open><summary>{title} {summary_text}</summary><div class="services-grid">"#
    );
    for (i, svc) in svcs.iter().enumerate() {
        let id = format!("svc-{}", start_idx + i);
        html.push_str(&render_service_item(db, svc, &id));
    }
    html.push_str("</div></details>");
    html
}

fn render_services(db: &Connection, services: &[Service]) -> String {
    if services.is_empty() {
        return String::new();
    }

    let mut non_dns: Vec<&Service> = services.iter().filter(|s| s.check != "dns").collect();
    let mut dns: Vec<&Service> = services.iter().filter(|s| s.check == "dns").collect();
    non_dns.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    dns.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));

    let mut html = render_service_card(db, "Web", &non_dns, 0);
    html.push_str(&render_service_card(db, "DNS", &dns, non_dns.len()));
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
{services_html}
</div>
"#
    );

    // LAN host cards
    for host in &state.config.hosts {
        html.push_str(&render_host(&db, host));
    }

    // Footer
    html.push_str(r##"<footer>Made with &#10084;&#65039; by <a href="mailto:david@connol.ly">David Connolly</a> &amp; <a href="https://claude.ai">Claude</a> &middot; <a href="https://github.com/slartibardfast/pi-glass">pi-glass</a></footer>"##);

    // Mobile backdrop + inline JS
    html.push_str(r#"<div id="svc-backdrop" class="svc-backdrop"></div>"#);
    html.push_str(&format!("<script>{INLINE_JS}</script>"));
    html.push_str("</body></html>");
    Html(html)
}
