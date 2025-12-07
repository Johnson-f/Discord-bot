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

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct IvSnapshot {
    spot: f64,
    atm_strike: f64,
    call_iv: f64,
    put_iv: f64,
    implied_move_pct: f64,
    days_to_expiry: i64,
}

static LAST_DAILY_POST_DATE: Lazy<Mutex<Option<chrono::NaiveDate>>> =
    Lazy::new(|| Mutex::new(None));

/// Spawn a daily earnings poster (Mon–Fri at 6:00 PM ET).
pub fn spawn_daily_report_poster(
    http: Arc<Http>,
    finance: Arc<FinanceService>,
) -> Option<JoinHandle<()>> {
    if env::var("ENABLE_EARNINGS_PINGER")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        info!("Daily earnings poster disabled via ENABLE_EARNINGS_PINGER=0");
        return None;
    }

    let channel_id = match env::var("EARNINGS_CHANNEL_ID")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        Some(id) => ChannelId::new(id),
        None => {
            info!("EARNINGS_CHANNEL_ID not set; daily earnings poster not started");
            return None;
        }
    };

    info!("Starting daily earnings poster to channel {}", channel_id);

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if should_post_now().await {
                if let Err(e) = send_daily_report(&http, &finance, channel_id).await {
                    warn!("daily earnings poster iteration failed: {e}");
                }
            }
        }
    }))
}

async fn should_post_now() -> bool {
    let now_utc = Utc::now();
    let now_et = now_utc.with_timezone(&America::New_York);

    // Only Mon–Fri at 6:00 PM ET (allow a small window to avoid missing the minute)
    match now_et.weekday() {
        Weekday::Sat | Weekday::Sun => return false,
        _ => {}
    }
    if !(now_et.hour() == 18 && now_et.minute() < 5) {
        return false;
    }

    let today = now_et.date_naive();
    let mut last = LAST_DAILY_POST_DATE.lock().await;
    if let Some(prev) = *last {
        if prev == today {
            return false;
        }
    }
    *last = Some(today);
    true
}

/// Send a daily earnings report for the current day (Mon–Fri).
/// If weekend, posts a no-data message.
pub async fn send_daily_report(
    http: &Http,
    finance: &FinanceService,
    channel_id: ChannelId,
) -> Result<(), String> {
    let now_et = Utc::now().with_timezone(&New_York);
    let weekday = now_et.weekday();

    // Weekend handling: show Friday data on Saturday, Monday data on Sunday
    let target_date = match weekday {
        Weekday::Sat => now_et.date_naive() - Duration::days(1),
        Weekday::Sun => now_et.date_naive() + Duration::days(1),
        _ => now_et.date_naive(),
    };

    let date_label = match weekday {
        Weekday::Sat => format!(
            "{} (weekend request — showing Friday {})",
            target_date.format("%A, %b %e"),
            target_date.format("%Y-%m-%d")
        ),
        Weekday::Sun => format!(
            "{} (weekend request — showing Monday {})",
            target_date.format("%A, %b %e"),
            target_date.format("%Y-%m-%d")
        ),
        _ => target_date.format("%A, %b %e").to_string(),
    };

    let start = target_date;
    let end = start; // same day

    let events = finance
        .get_earnings_range(start, end)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    if events.is_empty() {
        let msg = format!("No companies reporting earnings for ({})", date_label);
        channel_id
            .say(http, msg)
            .await
            .map_err(|e| format!("failed to post empty daily earnings: {e}"))?;
        return Ok(());
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "📅 Earnings ({}) — BMO & AMC with implied move from nearest expiry",
        date_label
    ));
    lines.push(String::new());

    for ev in events {
        let session = classify_session(ev.time_of_day.as_deref());
        let iv_snapshot =
            fetch_iv_snapshot(finance, &ev.symbol, ev.date.date_naive(), session).await;

        match iv_snapshot {
            Some(iv) => lines.push(format!(
                "{} [{}] — IV C {:.1}% | IM ±{:.1}%",
                ev.symbol,
                session,
                iv.call_iv * 100.0,
                iv.implied_move_pct,
            )),
            None => lines.push(format!(
                "{} [{}] — IV/IM unavailable (no options expiring after earnings)",
                ev.symbol, session
            )),
        }
    }

    let content = lines.join("\n");
    info!("Posting daily earnings report with {} lines", lines.len());

    channel_id
        .send_message(http, CreateMessage::new().content(content))
        .await
        .map_err(|e| format!("failed to post daily earnings report: {e}"))?;

    Ok(())
}

fn classify_session(time: Option<&str>) -> &'static str {
    let Some(raw) = time else {
        return "TBA";
    };
    let t = raw.trim().to_ascii_lowercase();

    // Explicit keywords first
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

    // Handle clock-style times like "09:00", "9:00 am", "16:00", etc.
    if let Some(hour) = parse_hour_prefix(&t) {
        if hour >= 15 {
            return "AMC";
        }
        if hour <= 11 {
            return "BMO";
        }
    }

    // Fallback to AM/PM suffix hints
    if t.ends_with("am") {
        return "BMO";
    }
    if t.ends_with("pm") {
        return "AMC";
    }

    "TBA"
}

fn parse_hour_prefix(s: &str) -> Option<u32> {
    // Accept formats like "9", "09", "9:00", "09:30", "9 am", "16:00"
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            if digits.len() >= 2 {
                // Enough to form an hour like "09" or "16"
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

async fn fetch_iv_snapshot(
    finance: &FinanceService,
    symbol: &str,
    earnings_date: chrono::NaiveDate,
    earnings_time: &str,
) -> Option<IvSnapshot> {
    // Determine when earnings actually happen
    let earnings_datetime = match earnings_time {
        "BMO" => earnings_date.and_hms_opt(9, 30, 0)?, // Before market open (9:30 AM ET)
        "AMC" => earnings_date.and_hms_opt(16, 0, 0)?, // After market close (4:00 PM ET)
        _ => earnings_date.and_hms_opt(16, 0, 0)?,     // Default to AMC if TBA
    };

    // Get all available option expirations for this symbol
    let expirations = match finance.get_option_expirations(symbol).await {
        Ok(exps) => exps,
        Err(e) => {
            warn!("Failed to get option expirations for {}: {}", symbol, e);
            return None;
        }
    };

    // Find the first expiration AFTER earnings (assume options expire 4:00 PM ET)
    let target_expiry = expirations
        .into_iter()
        .filter_map(|exp| exp.and_hms_opt(16, 0, 0).map(|dt| (exp, dt)))
        .filter(|(_, dt)| *dt > earnings_datetime)
        .min_by(|(_, a), (_, b)| a.cmp(b))
        .map(|(exp, _)| exp)?;

    // Fetch options for that specific expiration
    let slice = match finance.get_option_slice(symbol, target_expiry, 5).await {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "IV fetch failed for {} at expiry {:?}: {}",
                symbol, target_expiry, e
            );
            return None;
        }
    };

    let spot = slice.spot;

    // Find ATM call (strike closest to spot price)
    let atm_call = slice
        .calls
        .iter()
        .min_by(|a, b| float_abs_cmp(a.strike, b.strike, spot))?;

    // Find ATM put (strike closest to spot price)
    let atm_put = slice
        .puts
        .iter()
        .min_by(|a, b| float_abs_cmp(a.strike, b.strike, spot))?;

    let call_price = atm_call.last_price;
    let put_price = atm_put.last_price;

    // Calculate implied move: (ATM call + ATM put) / spot price
    let implied_move_pct = if spot > 0.0 {
        (call_price + put_price) / spot * 100.0
    } else {
        0.0
    };

    // Calculate days until expiration
    let expiry_dt = target_expiry.and_hms_opt(16, 0, 0)?;
    let days_to_expiry =
        (expiry_dt.and_utc().timestamp() - earnings_datetime.and_utc().timestamp()) / 86400; // seconds in a day

    Some(IvSnapshot {
        spot,
        atm_strike: (atm_call.strike + atm_put.strike) / 2.0,
        call_iv: atm_call.implied_volatility,
        put_iv: atm_put.implied_volatility,
        implied_move_pct,
        days_to_expiry,
    })
}

/// Helper function to compare two strikes by their distance from spot price
fn float_abs_cmp(a: f64, b: f64, spot: f64) -> std::cmp::Ordering {
    let da = (a - spot).abs();
    let db = (b - spot).abs();
    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
}
