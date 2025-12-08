use std::env;
use std::sync::Arc;

use chrono::{Datelike, Duration, Timelike, Utc, Weekday};
use chrono_tz::America;
use chrono_tz::America::New_York;
use once_cell::sync::Lazy;
use serenity::all::{CreateMessage, Http};
use serenity::model::prelude::ChannelId;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::service::finance::FinanceService;

#[derive(Debug, Clone)]
struct EarningsActuals {
    eps_actual: Option<f64>,
    revenue_actual: Option<f64>,
}

enum SessionTarget {
    Bmo,
    Amc,
    Waiting,
}

static LAST_AFTER_BMO_POST_DATE: Lazy<Mutex<Option<chrono::NaiveDate>>> =
    Lazy::new(|| Mutex::new(None));
static LAST_AFTER_AMC_POST_DATE: Lazy<Mutex<Option<chrono::NaiveDate>>> =
    Lazy::new(|| Mutex::new(None));

fn resolve_channel_id(var_names: &[&str], feature_label: &str) -> Option<ChannelId> {
    for name in var_names {
        if let Ok(value) = env::var(name) {
            match value.parse::<u64>() {
                Ok(id) => return Some(ChannelId::new(id)),
                Err(_) => warn!("{feature_label}: {name} is set but not a valid u64 channel id"),
            }
        }
    }

    info!(
        "{feature_label} not started; set one of these env vars: {:?}",
        var_names
    );
    None
}

/// Spawn post-earnings snapshots twice daily:
/// - BMO: 8:45 AM ET
/// - AMC: 5:50 PM ET
pub fn spawn_after_daily_poster(
    http: Arc<Http>,
    finance: Arc<FinanceService>,
) -> Option<JoinHandle<()>> {
    if env::var("ENABLE_EARNINGS_PINGER")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        info!("After-daily poster disabled via ENABLE_EARNINGS_PINGER=0");
        return None;
    }

    let channel_id = resolve_channel_id(
        &["EARNINGS_AFTER_CHANNEL_ID", "EARNINGS_CHANNEL_ID"],
        "after-daily earnings poster",
    )?;

    info!(
        "Starting after-daily earnings poster to channel {}",
        channel_id
    );

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let now_et = Utc::now().with_timezone(&America::New_York);
            let weekday = now_et.weekday();

            // Skip weekends for scheduled posts (manual command still works)
            if matches!(weekday, Weekday::Sat | Weekday::Sun) {
                continue;
            }

            if should_post_bmo(&now_et).await {
                if let Err(e) = send_after_daily_report(&http, &finance, channel_id).await {
                    warn!("after-daily BMO iteration failed: {e}");
                }
            }

            if should_post_amc(&now_et).await {
                if let Err(e) = send_after_daily_report(&http, &finance, channel_id).await {
                    warn!("after-daily AMC iteration failed: {e}");
                }
            }
        }
    }))
}

async fn should_post_bmo(now_et: &chrono::DateTime<chrono_tz::Tz>) -> bool {
    if !(now_et.hour() == 8 && now_et.minute() >= 45 && now_et.minute() < 50) {
        return false;
    }

    let today = now_et.date_naive();
    let mut last = LAST_AFTER_BMO_POST_DATE.lock().await;
    if let Some(prev) = *last {
        if prev == today {
            return false;
        }
    }
    *last = Some(today);
    true
}

async fn should_post_amc(now_et: &chrono::DateTime<chrono_tz::Tz>) -> bool {
    if !(now_et.hour() == 17 && now_et.minute() >= 50 && now_et.minute() < 55) {
        return false;
    }

    let today = now_et.date_naive();
    let mut last = LAST_AFTER_AMC_POST_DATE.lock().await;
    if let Some(prev) = *last {
        if prev == today {
            return false;
        }
    }
    *last = Some(today);
    true
}

/// Post a post-earnings snapshot for the current day.
/// - Before 4:00 PM ET: show BMO results.
/// - 6:00 PM ET or later: show AMC results.
/// - Between 4:00–6:00 PM ET: post a waiting message.
pub async fn send_after_daily_report(
    http: &Http,
    finance: &FinanceService,
    channel_id: ChannelId,
) -> Result<(), String> {
    let now_et = Utc::now().with_timezone(&New_York);
    let today = now_et.date_naive();
    let weekday = now_et.weekday();

    // Weekend handling:
    // - Saturday: show Friday (yesterday) AND Sunday (tomorrow)
    // - Sunday: show Friday (two days ago)
    // - Weekdays: show today
    let target_dates: Vec<_> = match weekday {
        Weekday::Sat => vec![today - Duration::days(1), today + Duration::days(1)],
        Weekday::Sun => vec![today - Duration::days(2)],
        _ => vec![today],
    };
    let date_labels = target_dates
        .iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let session_target = if now_et.hour() < 16 {
        SessionTarget::Bmo
    } else if now_et.hour() > 17 || (now_et.hour() == 17 && now_et.minute() >= 50) {
        SessionTarget::Amc
    } else {
        SessionTarget::Waiting
    };

    if let SessionTarget::Waiting = session_target {
        let msg = format!(
            "⏱️ It's {} ET. BMO results are done; AMC results will be posted after 6:00 PM ET.",
            now_et.format("%-I:%M %p")
        );
        channel_id
            .send_message(http, CreateMessage::new().content(msg))
            .await
            .map_err(|e| format!("failed to post waiting message: {e}"))?;
        return Ok(());
    }

    let start = *target_dates.iter().min().unwrap();
    let end = *target_dates.iter().max().unwrap();

    let events = finance
        .get_earnings_range(start, end)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    if events.is_empty() {
        let msg = format!(
            "No earnings events scheduled for target dates ({})",
            date_labels
        );
        channel_id
            .send_message(http, CreateMessage::new().content(msg))
            .await
            .map_err(|e| format!("failed to post empty after-daily earnings: {e}"))?;
        return Ok(());
    }

    let session_label = match session_target {
        SessionTarget::Bmo => "BMO",
        SessionTarget::Amc => "AMC",
        SessionTarget::Waiting => unreachable!(),
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "📈 Post-earnings results ({}) — {} as of {} ET",
        date_labels,
        session_label,
        now_et.format("%-I:%M %p")
    ));
    lines.push(String::new());

    let mut shown = 0usize;

    for ev in events {
        let ev_date = ev.date.date_naive();
        if !target_dates.contains(&ev_date) {
            continue;
        }

        let session = classify_session(ev.time_of_day.as_deref());

        let matches_target = match session_target {
            SessionTarget::Bmo => session == "BMO",
            SessionTarget::Amc => session == "AMC",
            SessionTarget::Waiting => false,
        };

        if !matches_target {
            continue;
        }

        let actuals = fetch_latest_actuals(finance, &ev.symbol).await;
        let eps_text = format_eps(actuals.as_ref().and_then(|a| a.eps_actual));
        let rev_text = format_revenue(actuals.as_ref().and_then(|a| a.revenue_actual));

        let date_str = ev_date.format("%Y-%m-%d").to_string();

        lines.push(format!(
            "{} [{} {}] — EPS {} | Revenue {}",
            ev.symbol, session, date_str, eps_text, rev_text
        ));
        shown += 1;
    }

    if shown == 0 {
        lines.push(format!(
            "No {} results detected yet for target dates ({}). If they just reported, retry in a few minutes.",
            session_label, date_labels
        ));
    }

    let content = lines.join("\n");
    info!(
        "Posting after-daily earnings report ({} lines, {})",
        lines.len(),
        session_label
    );

    channel_id
        .send_message(http, CreateMessage::new().content(content))
        .await
        .map_err(|e| format!("failed to post after-daily earnings report: {e}"))?;

    Ok(())
}

async fn fetch_latest_actuals(finance: &FinanceService, symbol: &str) -> Option<EarningsActuals> {
    let resp = match finance
        .client()
        .get_quote_summary(symbol, &["earnings"])
        .await
    {
        Ok(v) => v,
        Err(e) => {
            warn!("earnings fetch failed for {}: {}", symbol, e);
            return None;
        }
    };

    let root = resp
        .get("quoteSummary")
        .and_then(|q| q.get("result"))
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())?;

    let earnings = root.get("earnings")?;

    let eps_actual = earnings
        .get("earningsChart")
        .and_then(|c| c.get("quarterly"))
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.last())
        .and_then(|entry| entry.get("actual"))
        .and_then(|v| v.get("raw").and_then(|r| r.as_f64()).or_else(|| v.as_f64()));

    let revenue_actual = earnings
        .get("financialsChart")
        .and_then(|c| c.get("quarterly"))
        .and_then(|arr| arr.as_array())
        .and_then(|arr| arr.last())
        .and_then(|entry| entry.get("revenue"))
        .and_then(|v| v.get("raw").and_then(|r| r.as_f64()).or_else(|| v.as_f64()));

    if eps_actual.is_none() && revenue_actual.is_none() {
        warn!("no actuals available yet for {}", symbol);
        return None;
    }

    Some(EarningsActuals {
        eps_actual,
        revenue_actual,
    })
}

fn format_eps(eps: Option<f64>) -> String {
    eps.map(|v| format!("{:.2}", v))
        .unwrap_or_else(|| "N/A".to_string())
}

fn format_revenue(revenue: Option<f64>) -> String {
    match revenue {
        Some(v) => {
            let abs = v.abs();
            if abs >= 1_000_000_000.0 {
                format!("{:.1}B", v / 1_000_000_000.0)
            } else if abs >= 1_000_000.0 {
                format!("{:.1}M", v / 1_000_000.0)
            } else {
                format!("{:.0}", v)
            }
        }
        None => "N/A".to_string(),
    }
}

fn classify_session(time: Option<&str>) -> &'static str {
    let Some(raw) = time else {
        return "TBA";
    };
    let t = raw.trim().to_ascii_lowercase();

    if t.contains("bmo") || t.contains("before market") || t.contains("pre") {
        return "BMO";
    }
    if t.contains("amc")
        || t.contains("after close")
        || t.contains("after market")
        || t.contains("post")
    {
        return "AMC";
    }

    if let Some(hour) = parse_hour_prefix(&t) {
        if hour >= 15 {
            return "AMC";
        }
        if hour <= 11 {
            return "BMO";
        }
    }

    if t.ends_with("am") {
        return "BMO";
    }
    if t.ends_with("pm") {
        return "AMC";
    }

    "TBA"
}

fn parse_hour_prefix(s: &str) -> Option<u32> {
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            if digits.len() >= 2 {
                break;
            }
        } else if ch == ':' || !digits.is_empty() {
            break;
        }
    }

    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}
