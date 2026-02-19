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
const DEFAULT_DB_PATH: &str = "/opt/pi-glass/pi-glass.db";
const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_PING_TIMEOUT_SECS: u64 = 2;
const DEFAULT_RETENTION_DAYS: i64 = 7;
const CONFIG_PATH: &str = "/opt/pi-glass/config.toml";

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
    let summary_class = if up_count == total { "status-up" } else { "status-down" };
    let summary_text = format!(r#"<span class="{summary_class}">{up_count}/{total}</span>"#);

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

    // Footer
    html.push_str(r##"<footer>Made with &#10084;&#65039; by <a href="mailto:david@connol.ly">David Connolly</a> &amp; <a href="https://claude.ai">Claude</a> &middot; <a href="https://github.com/slartibardfast/pi-glass">pi-glass</a></footer>"##);

    // Mobile backdrop + inline JS
    html.push_str(r#"<div id="svc-backdrop" class="svc-backdrop"></div>"#);
    html.push_str(&format!("<script>{INLINE_JS}</script>"));
    html.push_str("</body></html>");
    Html(html)
}
