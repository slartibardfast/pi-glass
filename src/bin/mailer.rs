use std::time::Duration;

use chrono::NaiveDateTime;
use rusqlite::{Connection, OpenFlags};

use pi_glass::*;

/// Returns seconds until the next occurrence of "HH:MM" in local time.
fn secs_until(hh_mm: &str) -> u64 {
    let now: NaiveDateTime = chrono::Local::now().naive_local();
    let mut it = hh_mm.splitn(2, ':');
    let h: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(8);
    let m: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    let today_at = now
        .date()
        .and_hms_opt(h, m, 0)
        .unwrap_or_else(|| now.date().and_hms_opt(8, 0, 0).unwrap());

    let target = if now < today_at {
        today_at
    } else {
        today_at + chrono::Duration::days(1)
    };

    (target - now).num_seconds().max(0) as u64
}

async fn send_mailgun(cfg: &MailerConfig, html: &str) -> Result<(), reqwest::Error> {
    let url = format!("https://api.mailgun.net/v3/{}/messages", cfg.mailgun_domain);
    let client = reqwest::Client::new();

    let mut form = reqwest::multipart::Form::new()
        .text("from",    cfg.from.clone())
        .text("subject", cfg.subject.clone())
        .text("html",    html.to_string());

    for recipient in &cfg.to {
        form = form.text("to", recipient.clone());
    }

    let resp = client
        .post(&url)
        .basic_auth("api", Some(&cfg.mailgun_api_key))
        .multipart(form)
        .send()
        .await?;

    if resp.status().is_success() {
        eprintln!("pi-glass-mailer: sent to {}", cfg.to.join(", "));
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("pi-glass-mailer: mailgun error {status}: {body}");
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    let (config, _) = load_config();
    let mcfg = config
        .mailer
        .as_ref()
        .expect("pi-glass-mailer requires a [mailer] section in config.toml");

    eprintln!("pi-glass-mailer: will send daily at {} to {}", mcfg.send_at, mcfg.to.join(", "));

    loop {
        let secs = secs_until(&mcfg.send_at);
        eprintln!("pi-glass-mailer: next send in {}m", secs / 60);
        tokio::time::sleep(Duration::from_secs(secs)).await;

        let db = match Connection::open_with_flags(
            &config.db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(db) => db,
            Err(e) => { eprintln!("pi-glass-mailer: db error: {e}"); continue; }
        };

        let html = render_full_page(&db, &config);
        let html = inline_css_vars(html);

        if let Err(e) = send_mailgun(mcfg, &html).await {
            eprintln!("pi-glass-mailer: send error: {e}");
        }
    }
}
