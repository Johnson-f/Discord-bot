use std::{collections::HashMap, env, sync::Arc, time::Duration};

use chrono::{Datelike, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;
use finance_query_core::OptionContract;
use serenity::all::{CreateAttachment, Http};
use serenity::model::prelude::ChannelId;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::service::finance::options::OptionSlice;
use crate::service::finance::FinanceService;

#[allow(clippy::type_complexity)]
static STRIKE_HISTORY: once_cell::sync::Lazy<
    Mutex<HashMap<String, Vec<(chrono::DateTime<Utc>, f64)>>>,
> = once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));
static LAST_RUN: once_cell::sync::Lazy<Mutex<Option<chrono::DateTime<Utc>>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

/// Spawn the 15-minute SPY options pinger.
pub fn spawn_options_pinger(
    http: Arc<Http>,
    finance: Arc<FinanceService>,
) -> Option<JoinHandle<()>> {
    if env::var("ENABLE_OPTIONS_PINGER")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        info!("Options pinger disabled via ENABLE_OPTIONS_PINGER=0");
        return None;
    }

    let channel_id = match env::var("OPTIONS_CHANNEL_ID")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        Some(id) => ChannelId::new(id),
        None => {
            info!("OPTIONS_CHANNEL_ID not set; options pinger not started");
            return None;
        }
    };

    info!("Starting options pinger for SPY to channel {}", channel_id);

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if should_run_now().await {
                if let Err(e) = post_once(&http, &finance, channel_id).await {
                    error!("options pinger iteration failed: {e}");
                }
            }
        }
    }))
}

async fn post_once(
    http: &Http,
    finance: &FinanceService,
    channel_id: ChannelId,
) -> Result<(), String> {
    let slice = finance
        .get_option_slice_today("SPY", 5)
        .await
        .map_err(|e| e.to_string())?;

    let summary = format_slice(&slice);
    match build_chart_bytes(&slice).await {
        Ok(bytes) => {
            let attachment = CreateAttachment::bytes(bytes, "spy_options.png");
            let builder = serenity::builder::CreateMessage::new()
                .content(summary)
                .add_file(attachment);
            channel_id
                .send_message(http, builder)
                .await
                .map_err(|e| format!("failed to post options chart: {e}"))?;
        }
        Err(err) => {
            let msg = format!("{summary}\n\n(chart generation failed: {err})");
            channel_id
                .say(http, msg)
                .await
                .map_err(|e| format!("failed to post options text fallback: {e}"))?;
        }
    }

    Ok(())
}

fn format_slice(slice: &OptionSlice) -> String {
    let mut out = Vec::new();
    out.push(format!(
        "SPY options (exp {}) | spot {:.2} | fetched {}",
        slice.expiration,
        slice.spot,
        Utc::now().format("%H:%M UTC")
    ));
    out.push(format!(
        "Calls (top 5 above spot):\n{}",
        fmt_side(&slice.calls)
    ));
    out.push(format!(
        "Puts (top 5 below spot):\n{}",
        fmt_side(&slice.puts)
    ));
    out.join("\n\n")
}

fn fmt_side(contracts: &[OptionContract]) -> String {
    if contracts.is_empty() {
        return "none".to_string();
    }

    let mut lines = Vec::new();
    for c in contracts {
        lines.push(format!(
            "K {:>7.2} | LTP {:>6.2} | B/A {:>6.2}/{:>6.2} | IV {:>5.1}% | OI {:>7} | Vol {:>7}{}",
            c.strike,
            c.last_price,
            c.bid,
            c.ask,
            c.implied_volatility * 100.0,
            c.open_interest.unwrap_or(0),
            c.volume.unwrap_or(0),
            if c.in_the_money { " | ITM" } else { "" },
        ));
    }
    lines.join("\n")
}

async fn record_slice(slice: &OptionSlice) {
    let mut map = STRIKE_HISTORY.lock().await;
    let now = Utc::now();
    for c in slice.calls.iter().chain(slice.puts.iter()) {
        let price = c.last_price;
        let key = format!("{:.2}", c.strike);
        let hist = map.entry(key).or_default();
        hist.push((now, price));
        if hist.len() > 200 {
            let drop = hist.len() - 200;
            hist.drain(0..drop);
        }
    }
}

async fn build_chart_bytes(slice: &OptionSlice) -> Result<Vec<u8>, String> {
    // Record history first
    record_slice(slice).await;

    let map = STRIKE_HISTORY.lock().await;
    let mut datasets = Vec::new();
    // assign distinct colors per strike
    let palette = [
        "#4caf50", "#f44336", "#2196f3", "#ff9800", "#9c27b0", "#00bcd4", "#8bc34a", "#ff5722",
        "#3f51b5", "#cddc39",
    ];
    let mut idx = 0usize;

    for (strike, points) in map.iter() {
        if points.len() < 2 {
            continue;
        }
        let data: Vec<_> = points
            .iter()
            .map(|(t, p)| serde_json::json!({"x": t.to_rfc3339(), "y": p}))
            .collect();
        let color = palette[idx % palette.len()];
        idx += 1;
        datasets.push(serde_json::json!({
            "label": format!("K {}", strike),
            "data": data,
            "showLine": true,
            "tension": 0.2,
            "borderColor": color,
            "backgroundColor": format!("{}33", color), // semi-transparent
        }));
    }
    drop(map);

    if datasets.is_empty() {
        return Err("no historical data to chart yet".into());
    }

    let chart = serde_json::json!({
        "type": "line",
        "data": { "datasets": datasets },
        "options": {
            "plugins": {
                "legend": { "position": "bottom" },
                "title": { "display": true, "text": format!("SPY {} history", slice.expiration) }
            },
            "scales": {
                "x": {
                    "type": "time",
                    "time": { "unit": "minute" },
                    "title": { "display": true, "text": "Time" }
                },
                "y": { "title": { "display": true, "text": "Price" } }
            }
        }
    });

    let body = serde_json::json!({
        "chart": chart,
        "width": 800,
        "height": 400,
        "backgroundColor": "white",
        "plugins": ["chartjs-adapter-date-fns"],
        "version": "4.4.0"
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://quickchart.io/chart")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("chart request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("chart service status {status}: {text}"));
    }

    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("chart bytes error: {e}"))
}

async fn should_run_now() -> bool {
    let now_utc = Utc::now();
    let now_et = now_utc.with_timezone(&New_York);
    let weekday = now_et.weekday();
    if weekday == Weekday::Sat || weekday == Weekday::Sun {
        return false;
    }
    let hour = now_et.hour();
    let minute = now_et.minute();

    // Market window: 9:30 <= t < 16:00 ET
    let in_window = (hour > 9 || (hour == 9 && minute >= 30)) && hour < 16;
    if !in_window {
        return false;
    }

    // Only on 15-minute marks
    if minute % 15 != 0 {
        return false;
    }

    // Deduplicate same minute
    let mut last = LAST_RUN.lock().await;
    if let Some(prev) = *last {
        let prev_et = prev.with_timezone(&New_York);
        if prev_et.date_naive() == now_et.date_naive()
            && prev_et.hour() == hour
            && prev_et.minute() == minute
        {
            return false;
        }
    }
    *last = Some(now_utc);
    true
}
