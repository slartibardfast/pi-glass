use std::collections::{HashMap, HashSet};
use chrono::Local;
use rusqlite::{params, Connection};
use serde::Deserialize;

// --- UI Cookie ---

pub struct UiCookie {
    pub open_hosts: Option<HashSet<String>>,
    pub open_svc_cards: Option<HashSet<String>>,
    pub open_svc_items: Option<HashSet<String>>,
}

pub fn parse_ui_cookie(cookie_str: &str) -> UiCookie {
    let pg = cookie_str
        .split(';')
        .find_map(|p| p.trim().strip_prefix("pg="))
        .unwrap_or("");

    if pg.is_empty() {
        return UiCookie { open_hosts: None, open_svc_cards: None, open_svc_items: None };
    }

    let mut open_hosts = None;
    let mut open_svc_cards = None;
    let mut open_svc_items = None;

    for field in pg.split('&') {
        if let Some(v) = field.strip_prefix("ho=") {
            open_hosts = Some(v.split('|').filter(|s| !s.is_empty()).map(String::from).collect());
        } else if let Some(v) = field.strip_prefix("sc=") {
            open_svc_cards = Some(v.split('|').filter(|s| !s.is_empty()).map(String::from).collect());
        } else if let Some(v) = field.strip_prefix("si=") {
            open_svc_items = Some(v.split('|').filter(|s| !s.is_empty()).map(String::from).collect());
        }
    }

    UiCookie { open_hosts, open_svc_cards, open_svc_items }
}

// --- Constants ---

pub const DEFAULT_LISTEN: &str = "0.0.0.0:8080";
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
pub const DEFAULT_PING_TIMEOUT_SECS: u64 = 2;
pub const DEFAULT_RETENTION_DAYS: i64 = 7;

pub const TOKENS_CSS: &str = include_str!("../web/dist/tokens.css");
pub const APP_CSS: &str = include_str!("app.css");
pub const INLINE_JS: &str = include_str!("app.js");
pub const SPARKS_WOFF2: &[u8] = include_bytes!("fonts/Sparks-Bar-Medium.woff2");

pub const FAVICON_ICO: &[u8] = include_bytes!("favicon/favicon.ico");
pub const FAVICON_SVG: &str = include_str!("favicon/favicon.svg");
pub const APPLE_TOUCH_ICON: &[u8] = include_bytes!("favicon/apple-touch-icon.png");
pub const FAVICON_192: &[u8] = include_bytes!("favicon/favicon-192.png");
pub const FAVICON_512: &[u8] = include_bytes!("favicon/favicon-512.png");
pub const WEB_MANIFEST: &str = include_str!("favicon/site.webmanifest");

// --- Config types ---

#[derive(Deserialize, Clone)]
pub struct Host {
    pub addr: String,
    pub label: String,
}

#[derive(Deserialize, Clone)]
pub struct Service {
    pub label: String,
    #[serde(default)]
    pub icon: String,
    pub check: String,
    pub target: String,
    #[serde(default)]
    pub icon_data: Option<String>,
}

#[derive(Deserialize)]
pub struct MailerConfig {
    pub mailgun_domain: String,
    pub mailgun_api_key: String,
    pub from: String,
    pub to: Vec<String>,
    #[serde(default = "default_mail_subject")]
    pub subject: String,
    #[serde(default = "default_send_at")]
    pub send_at: String,
}

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_db_path")]
    pub db_path: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_ping_timeout")]
    pub ping_timeout_secs: u64,
    #[serde(default = "default_retention_days")]
    pub retention_days: i64,
    #[serde(default)]
    pub wal_mode: bool,
    #[serde(default = "default_hosts")]
    pub hosts: Vec<Host>,
    #[serde(default = "default_services")]
    pub services: Vec<Service>,
    #[serde(default)]
    pub mailer: Option<MailerConfig>,
}

fn default_name() -> String { "pi-glass".to_string() }
fn default_listen() -> String { DEFAULT_LISTEN.to_string() }
pub fn default_db_path() -> String { format!("{}/pi-glass.db", data_dir()) }
fn default_poll_interval() -> u64 { DEFAULT_POLL_INTERVAL_SECS }
fn default_ping_timeout() -> u64 { DEFAULT_PING_TIMEOUT_SECS }
fn default_retention_days() -> i64 { DEFAULT_RETENTION_DAYS }
fn default_mail_subject() -> String { "pi-glass status".to_string() }
fn default_send_at() -> String { "08:00".to_string() }

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
        Service { label: "Quad9 DNS".into(),      icon: "quad9".into(),      check: "dns".into(),  target: "9.9.9.9".into(),             icon_data: None },
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
            wal_mode: false,
            hosts: default_hosts(),
            services: default_services(),
            mailer: None,
        }
    }
}

// --- Config loading ---

pub fn data_dir() -> String {
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
pub fn bootstrap_config_from_exe() {
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

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn default_config_toml() -> String {
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

# Enable WAL journal mode for concurrent read/write access (default: false)
# Requires filesystem support for shared memory — not supported on all Pi mounts.
# wal_mode = true

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
icon   = "quad9"
check  = "dns"
target = "9.9.9.9"
"#.to_string()
}

pub fn load_config() -> (Config, Option<String>) {
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

// --- Stats queries ---

pub struct WindowStats {
    pub uptime_pct: Option<f64>,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

pub fn query_window_stats(db: &Connection, host: &str, minutes: i64) -> WindowStats {
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

pub fn query_latest_status(db: &Connection, host: &str) -> (String, Option<f64>) {
    db.query_row(
        "SELECT status, latency_ms FROM ping_results WHERE host = ?1 ORDER BY id DESC LIMIT 1",
        params![host],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<f64>>(1)?)),
    )
    .unwrap_or(("--".to_string(), None))
}

pub fn query_recent_checks(db: &Connection, host: &str, limit: i64) -> Vec<(String, String, Option<f64>)> {
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

pub fn query_card_uptime(db: &Connection, keys: &[String], minutes: i64) -> Option<f64> {
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

// --- Formatting ---

pub fn fmt_pct(v: Option<f64>) -> String {
    v.map_or("--".into(), |v| {
        if v <= 0.0 || v >= 100.0 { format!("{v:.0}%") } else { format!("{v:.1}%") }
    })
}

pub fn fmt_ms(v: Option<f64>) -> String {
    v.map_or("--".into(), |v| format!("{v:.1}"))
}

pub fn fmt_latency(v: Option<f64>) -> String {
    v.map_or_else(String::new, |v| format!("{v:.0}ms"))
}

pub fn fmt_sparkline(checks: &[(String, String, Option<f64>)]) -> String {
    const SPARK_BARS: usize = 40;

    // checks arrive DESC (newest first); reverse for left→right chronological display
    let ordered: Vec<_> = checks.iter().rev().collect();

    // Transparent gap bars fill the left side so every sparkline is SPARK_BARS wide.
    let pad_count = SPARK_BARS.saturating_sub(checks.len());
    let pad_str = if pad_count > 0 {
        let pads = vec!["50"; pad_count].join(",");
        format!(r#"<span class="spark spark-pad">{{{pads}}}</span>"#)
    } else {
        String::new()
    };

    if checks.is_empty() {
        return pad_str;
    }

    let latencies: Vec<f64> = ordered.iter()
        .filter_map(|(_, s, l)| if s == "UP" { *l } else { None })
        .collect();

    let (values, title): (Vec<String>, String) = if latencies.is_empty() {
        // All DOWN — floor bars, no latency stats
        (ordered.iter().map(|_| "0".to_string()).collect(),
         format!("{} checks · all down", checks.len()))
    } else {
        let count = latencies.len() as f64;
        let avg = latencies.iter().sum::<f64>() / count;
        let stddev = if count > 1.0 {
            let var = latencies.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / (count - 1.0);
            var.sqrt()
        } else { 0.0 };
        let min = latencies.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = latencies.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let range = max - min;

        let vals = ordered.iter().map(|(_, status, latency)| {
            if status == "UP" {
                let norm = if range < 0.5 {
                    50u32  // flat mid-line for very consistent latency
                } else {
                    let v = latency.unwrap_or(min);
                    (1.0 + (v - min) / range * 99.0).round() as u32
                };
                norm.to_string()
            } else {
                "0".to_string()  // DOWN → floor bar
            }
        }).collect();
        let t = format!(
            "{} checks · avg {avg:.0}ms ±{stddev:.0} · min {min:.0}ms · max {max:.0}ms",
            checks.len()
        );
        (vals, t)
    };

    format!(r#"{pad_str}<span class="spark" title="{title}">{{{}}}</span>"#, values.join(","))
}

// --- Tier / status helpers ---

pub fn tier_class(uptime_pct: Option<f64>) -> &'static str {
    match uptime_pct {
        Some(p) if p >= 100.0 => "tier-perfect",
        Some(p) if p >= 99.0  => "tier-good",
        Some(p) if p >= 95.0  => "tier-degraded",
        Some(p) if p > 0.0    => "tier-critical",
        _                     => "tier-down",
    }
}

pub fn state_tier(status: &str) -> &'static str {
    match status {
        "UP"   => "tier-good",
        "DOWN" => "tier-down",
        _      => "tier-neutral",
    }
}

// --- SVG Icons ---

pub fn get_icon_svg(key: &str) -> &'static str {
    match key {
        "google"     => include_str!("icons/google.svg"),
        "bing"       => include_str!("icons/bing.svg"),
        "heanet"     => include_str!("icons/heanet.html"),
        "digiweb"    => include_str!("icons/digiweb.html"),
        "dkit"       => include_str!("icons/dkit.svg"),
        "youtube"    => include_str!("icons/youtube.html"),
        "outlook"    => include_str!("icons/outlook.html"),
        "whatsapp"   => include_str!("icons/whatsapp.svg"),
        "cloudflare" => include_str!("icons/cloudflare.svg"),
        "quad9"      => include_str!("icons/quad9.svg"),
        "dns"        => include_str!("icons/dns.svg"),
        _            => include_str!("icons/fallback.svg"),
    }
}

// --- HTML rendering ---

pub fn render_stats_section(
    w5m: &WindowStats, w1h: &WindowStats, w24h: &WindowStats, w7d: &WindowStats,
    pings_label: &str, time_col_label: &str, detail_rows: &str,
) -> String {
    let loss_5m  = w5m.uptime_pct.map(|u| 100.0 - u);
    let loss_1h  = w1h.uptime_pct.map(|u| 100.0 - u);
    let loss_24h = w24h.uptime_pct.map(|u| 100.0 - u);
    let loss_7d  = w7d.uptime_pct.map(|u| 100.0 - u);
    format!(
        include_str!("templates/stats_section.html"),
        uptime_5m  = fmt_pct(w5m.uptime_pct),
        uptime_1h  = fmt_pct(w1h.uptime_pct),
        uptime_24h = fmt_pct(w24h.uptime_pct),
        uptime_7d  = fmt_pct(w7d.uptime_pct),
        avg_5m  = fmt_ms(w5m.avg_ms),
        avg_1h  = fmt_ms(w1h.avg_ms),
        avg_24h = fmt_ms(w24h.avg_ms),
        avg_7d  = fmt_ms(w7d.avg_ms),
        min_5m  = fmt_ms(w5m.min_ms),
        min_1h  = fmt_ms(w1h.min_ms),
        min_24h = fmt_ms(w24h.min_ms),
        min_7d  = fmt_ms(w7d.min_ms),
        max_5m  = fmt_ms(w5m.max_ms),
        max_1h  = fmt_ms(w1h.max_ms),
        max_24h = fmt_ms(w24h.max_ms),
        max_7d  = fmt_ms(w7d.max_ms),
        loss_5m  = fmt_pct(loss_5m),
        loss_1h  = fmt_pct(loss_1h),
        loss_24h = fmt_pct(loss_24h),
        loss_7d  = fmt_pct(loss_7d),
        pings_label = pings_label,
        time_col_label = time_col_label,
        detail_rows = detail_rows,
    )
}

pub fn render_host(db: &Connection, host: &Host, user_open: Option<bool>) -> String {
    let w5m  = query_window_stats(db, &host.addr, 5);
    let w1h  = query_window_stats(db, &host.addr, 60);
    let w24h = query_window_stats(db, &host.addr, 1440);
    let w7d  = query_window_stats(db, &host.addr, 10080);
    let (cur_status, latency) = query_latest_status(db, &host.addr);
    let tier = state_tier(&cur_status);
    let latency_str = latency.map_or_else(String::new, |ms| format!("{ms:.0}ms"));
    let rows = query_recent_checks(db, &host.addr, 40);
    let spark_str = fmt_sparkline(&rows);
    let (dot_class, dot_char) = match cur_status.as_str() {
        "UP"   => ("up",      "✓"),
        "DOWN" => ("down",    "✗"),
        _      => ("unknown", "–"),
    };
    let uptime_pct = fmt_pct(w1h.uptime_pct);
    let streak_display = format!(
        r#"<span class="host-badge-group"><span class="svc-latency">{spark_str}{latency_str}</span><span class="streak {tier}" title="1h uptime: {uptime_pct}">{uptime_pct}</span><span class="svc-status {dot_class}">{dot_char}</span></span>"#,
    );

    let all_up_1h = w1h.uptime_pct.map_or(true, |p| p >= 100.0);
    let open_attr = match user_open {
        Some(true)  => " open",
        Some(false) => "",
        None        => if all_up_1h { "" } else { " open" },
    };

    let mut detail_rows = String::new();
    for (ts, status, latency) in &rows[..rows.len().min(20)] {
        let time = if ts.len() >= 23 { &ts[11..23] } else { ts.as_str() };
        let latency_str = latency.map_or(String::new(), |v| format!("{v:.1}ms"));
        let (dot_class, dot_char) = match status.as_str() {
            "UP"   => ("status-up",   "✓"),
            "DOWN" => ("status-down", "✗"),
            _      => ("",            "–"),
        };
        detail_rows.push_str(&format!(
            r#"<div class="pg-row"><span>{time}</span><span>{latency_str}</span><span class="{dot_class}">{dot_char}</span></div>"#
        ));
    }
    let stats_section = render_stats_section(&w5m, &w1h, &w24h, &w7d, "Last 20 pings", "Time", &detail_rows);

    format!(
        include_str!("templates/host.html"),
        open_attr = open_attr,
        label = host.label,
        addr = host.addr,
        streak_display = streak_display,
        stats_section = stats_section,
    )
}

pub fn render_service_item(db: &Connection, svc: &Service, id: &str, user_open: Option<bool>, resolved_ip: Option<&str>) -> String {
    let key = format!("svc:{}", svc.label);
    let (cur_status, latency) = query_latest_status(db, &key);
    let (dot_class, dot_char) = match cur_status.as_str() {
        "UP"   => ("up",      "✓"),
        "DOWN" => ("down",    "✗"),
        _      => ("unknown", "–"),
    };
    let icon_html = if let Some(data) = &svc.icon_data {
        format!(r#"<img style="width:20px;height:20px" src="{data}">"#)
    } else {
        get_icon_svg(&svc.icon).to_string()
    };
    let latency_str = fmt_latency(latency);

    let w5m  = query_window_stats(db, &key, 5);
    let w1h  = query_window_stats(db, &key, 60);
    let w24h = query_window_stats(db, &key, 1440);
    let w7d  = query_window_stats(db, &key, 10080);
    let tier = state_tier(&cur_status);
    let uptime_badge = fmt_pct(w1h.uptime_pct);
    let streak_title = format!("1h uptime: {uptime_badge}");
    let open_attr = if user_open.unwrap_or(false) { " open" } else { "" };

    let recent = query_recent_checks(db, &key, 40);
    let spark_str = fmt_sparkline(&recent);
    let mut detail_rows = String::new();
    for (ts, s, lat) in &recent[..recent.len().min(10)] {
        let lat_str = lat.map_or(String::new(), |v| format!("{v:.1}ms"));
        let time = if ts.len() >= 23 { &ts[11..23] } else { ts.as_str() };
        let (dot_class, dot_char) = match s.as_str() {
            "UP"   => ("status-up",   "✓"),
            "DOWN" => ("status-down", "✗"),
            _      => ("",            "–"),
        };
        detail_rows.push_str(&format!(
            r#"<div class="pg-row"><span>{time}</span><span>{lat_str}</span><span class="{dot_class}">{dot_char}</span></div>"#
        ));
    }
    let stats_section = render_stats_section(&w5m, &w1h, &w24h, &w7d, "Last 10 checks", "Time", &detail_rows);
    let resolved_ip_html = match resolved_ip {
        Some(ip) => format!(r#" · <span class="ip">{ip}</span>"#),
        None => String::new(),
    };

    format!(
        include_str!("templates/service_item.html"),
        id = id,
        open_attr = open_attr,
        icon_html = icon_html,
        dot_class = dot_class,
        dot_char = dot_char,
        label = svc.label,
        latency_str = latency_str,
        spark_str = spark_str,
        tier = tier,
        uptime_badge = uptime_badge,
        streak_title = streak_title,
        check = svc.check,
        target = svc.target,
        resolved_ip_html = resolved_ip_html,
        stats_section = stats_section,
    )
}

pub fn render_service_card(db: &Connection, title: &str, svcs: &[&Service], start_idx: usize, open: bool, open_svc_items: Option<&HashSet<String>>, resolved_ips: &HashMap<String, Option<String>>) -> String {
    if svcs.is_empty() {
        return String::new();
    }

    let mut up_count = 0usize;
    for svc in svcs {
        let key = format!("svc:{}", svc.label);
        let (status, _) = query_latest_status(db, &key);
        if status == "UP" { up_count += 1; }
    }
    let total = svcs.len();
    let keys: Vec<String> = svcs.iter().map(|s| format!("svc:{}", s.label)).collect();
    let card_uptime = query_card_uptime(db, &keys, 60);
    let tier = tier_class(card_uptime);
    let title_attr = match card_uptime {
        Some(_) => format!("1h uptime: {}", fmt_pct(card_uptime)),
        None    => "No data".to_string(),
    };
    let (card_dot_class, card_dot_char) = if total == 0 {
        ("unknown", "–")
    } else if up_count == total {
        ("up", "✓")
    } else {
        ("down", "✗")
    };
    let center_html = format!(
        r#"<span class="svc-card-center"><span class="streak svc-card-count {tier}" title="{title_attr}">{up_count}/{total}</span></span>"#
    );
    let right_html = format!(
        r#"<span class="svc-card-right svc-status {card_dot_class}">{card_dot_char}</span>"#
    );

    let open_attr = if open { " open" } else { "" };
    let mut html = format!(
        include_str!("templates/service_card.html"),
        title      = title,
        center_html = center_html,
        right_html  = right_html,
        open_attr  = open_attr,
    );
    for (i, svc) in svcs.iter().enumerate() {
        let id = format!("svc-{}", start_idx + i);
        let item_open = open_svc_items.map(|set| set.contains(&id));
        let resolved_ip = resolved_ips.get(&svc.label).and_then(|o| o.as_deref());
        html.push_str(&render_service_item(db, svc, &id, item_open, resolved_ip));
    }
    html.push_str("</div></details>");
    html
}

pub fn render_services(db: &Connection, services: &[Service], ui: &UiCookie, resolved_ips: &HashMap<String, Option<String>>) -> String {
    if services.is_empty() {
        return String::new();
    }

    let mut web: Vec<&Service>  = services.iter().filter(|s| s.check == "tcp").collect();
    let mut icmp: Vec<&Service> = services.iter().filter(|s| s.check == "ping").collect();
    let mut dns: Vec<&Service>  = services.iter().filter(|s| s.check == "dns").collect();
    web.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    icmp.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    dns.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));

    let svc_open = |title: &str| -> bool {
        match &ui.open_svc_cards {
            None => true,
            Some(set) => set.contains(title),
        }
    };

    let open_items = ui.open_svc_items.as_ref();
    let mut html = render_service_card(db, "Web", &web, 0, svc_open("Web"), open_items, resolved_ips);
    html.push_str(&render_service_card(db, "ICMP", &icmp, web.len(), svc_open("ICMP"), open_items, resolved_ips));
    html.push_str(&render_service_card(db, "DNS", &dns, web.len() + icmp.len(), svc_open("DNS"), open_items, resolved_ips));
    html
}

// --- Mailer helpers ---

/// Render the full page with all sections forced open (for email).
pub fn render_full_page(db: &Connection, config: &Config) -> String {
    let n = config.services.len();
    let all_open_ui = UiCookie {
        open_hosts: Some(config.hosts.iter().map(|h| h.addr.clone()).collect()),
        open_svc_cards: None,  // None = all open (no cookie state)
        open_svc_items: Some((0..n).map(|i| format!("svc-{i}")).collect()),
    };
    let empty_ips: HashMap<String, Option<String>> = HashMap::new();
    let services_html = render_services(db, &config.services, &all_open_ui, &empty_ips);

    let mut html = format!(
        include_str!("templates/page.html"),
        name         = config.name,
        tokens_css   = TOKENS_CSS,
        app_css      = APP_CSS,
        services_html = services_html,
    );

    for host in &config.hosts {
        html.push_str(&render_host(db, host, Some(true)));
    }

    html.push_str(r##"<footer>Made with &#10084;&#65039; by <a href="mailto:david@connol.ly">David Connolly</a> &amp; <a href="https://claude.ai">Claude</a> &middot; <a href="https://github.com/slartibardfast/pi-glass">pi-glass</a></footer>"##);
    html.push_str("</body></html>");
    html
}

/// Resolve all CSS custom property `var(--name)` references in the HTML.
/// Parses variable definitions from TOKENS_CSS, resolves cross-references,
/// then substitutes all `var(--x)` occurrences in the HTML with their values.
pub fn inline_css_vars(html: String) -> String {
    // 1. Parse --name: value; pairs from tokens CSS
    let mut vars: HashMap<String, String> = HashMap::new();
    for line in TOKENS_CSS.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("--") {
            if let Some(colon) = rest.find(':') {
                let name = format!("--{}", rest[..colon].trim());
                let value = rest[colon + 1..].trim().trim_end_matches(';').trim().to_string();
                vars.insert(name, value);
            }
        }
    }

    // 2. Multi-pass: resolve values that reference other vars (chain resolution)
    for _ in 0..10 {
        let mut changed = false;
        let keys: Vec<String> = vars.keys().cloned().collect();
        for key in &keys {
            let val = vars[key].clone();
            if val.contains("var(") {
                let resolved = substitute_vars(&val, &vars);
                if resolved != val {
                    vars.insert(key.clone(), resolved);
                    changed = true;
                }
            }
        }
        if !changed { break; }
    }

    // 3. Substitute all var() references in the HTML
    substitute_vars(&html, &vars)
}

/// Replace all `var(--name)` and `var(--name, fallback)` occurrences in `s`
/// with their resolved values from `vars`. Unresolvable vars are left as-is.
fn substitute_vars(s: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find("var(") {
        result.push_str(&rest[..idx]);
        rest = &rest[idx + 4..]; // advance past "var("

        // Find matching closing paren (handles nested parens in fallback values)
        let mut depth = 1usize;
        let mut end = rest.len();
        for (i, ch) in rest.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 { end = i; break; }
                }
                _ => {}
            }
        }

        let inner = &rest[..end];
        let var_name = inner.split(',').next().unwrap_or("").trim();
        if let Some(val) = vars.get(var_name) {
            result.push_str(val);
        } else {
            // Unknown var — keep original expression
            result.push_str("var(");
            result.push_str(inner);
            result.push(')');
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}
