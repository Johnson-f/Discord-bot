use std::collections::HashMap;
use std::time::Duration as StdDuration;

use chrono::{NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use serde_json;
use tracing::{info, warn};

use crate::models::EarningsEvent;
use crate::service::finance::FinanceServiceError;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiDateGroup {
    #[serde(default, alias = "stocks", alias = "earnings", alias = "items")]
    stocks: Vec<ApiEarning>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
#[serde(untagged)]
enum ApiEarningsPayload {
    ByDate(HashMap<String, ApiDateGroup>),
    Flat(Vec<ApiEarning>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiTopLevel {
    Response(ApiResponse),
    ByDate(HashMap<String, ApiDateGroup>),
    Flat(Vec<ApiEarning>),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiResponse {
    success: Option<bool>,
    date_from: Option<String>,
    date_to: Option<String>,
    total_dates: Option<usize>,
    total_earnings: Option<usize>,
    earnings: Option<ApiEarningsPayload>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct ApiEarning {
    importance: Option<i64>,
    symbol: String,
    date: String,
    time: Option<String>,
    title: Option<String>,
    emoji: Option<String>,
    logo: Option<String>, // This is now a URL string, not base64 data
}

const EARNINGS_API_URL: &str =
    "https://bnavjgbowcekeppwgnxc.supabase.co/functions/v1/earnings-calendar";
const EARNINGS_API_BEARER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImJuYXZqZ2Jvd2Nla2VwcHdnbnhjIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NTkwODk1OTAsImV4cCI6MjA3NDY2NTU5MH0.AK7v9ofCWWgjsj4fUfr4nsRRcVwFQMeaNt1zNs6bjN0";

/// Fetch earnings for a date range via external API (hardcoded endpoint).
pub async fn fetch_earnings_range(
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<EarningsEvent>, FinanceServiceError> {
    info!("Fetching earnings from {} to {}", from, to);

    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(15)) // 15 second timeout
        .build()
        .map_err(|e| FinanceServiceError::Http(format!("failed to build client: {e}")))?;

    let from_str = from.format("%Y-%m-%d").to_string();
    let to_str = to.format("%Y-%m-%d").to_string();

    info!("Making request to earnings API with logo support...");

    let resp = client
        .get(EARNINGS_API_URL)
        .query(&[
            ("fromDate", from_str.as_str()),
            ("toDate", to_str.as_str()),
            ("includeLogos", "true"), // Request logo URLs from API
        ])
        .header("Authorization", format!("Bearer {}", EARNINGS_API_BEARER))
        .send()
        .await
        .map_err(|e| {
            warn!("Earnings API request failed: {}", e);
            FinanceServiceError::Http(format!("earnings request failed: {e}"))
        })?;

    info!("Received response with status: {}", resp.status());

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "unable to read body".to_string());
        warn!("Earnings API returned error status {}: {}", status, body);
        return Err(FinanceServiceError::Http(format!(
            "earnings api status {}: {}",
            status, body
        )));
    }

    let raw_bytes = resp.bytes().await.map_err(|e| {
        warn!("Failed to read earnings API body: {}", e);
        FinanceServiceError::Http(format!("earnings body read failed: {e}"))
    })?;

    let parsed: ApiTopLevel = serde_json::from_slice(&raw_bytes).map_err(|e| {
        let preview = String::from_utf8_lossy(&raw_bytes[..raw_bytes.len().min(500)]);
        warn!(
            "Failed to parse earnings API response: {}; body preview: {}",
            e, preview
        );
        FinanceServiceError::Http(format!("earnings parse failed: {e}"))
    })?;

    let (payload, totals) = match parsed {
        ApiTopLevel::Response(body) => {
            if body.success == Some(false) {
                warn!("Earnings API returned success=false");
                return Err(FinanceServiceError::Http(
                    "earnings api returned success=false".into(),
                ));
            }
            let payload = body
                .earnings
                .ok_or_else(|| FinanceServiceError::Http("earnings payload missing".into()))?;
            (payload, (body.total_earnings, body.total_dates))
        }
        ApiTopLevel::ByDate(map) => (ApiEarningsPayload::ByDate(map), (None, None)),
        ApiTopLevel::Flat(list) => (ApiEarningsPayload::Flat(list), (None, None)),
    };

    let mut events = Vec::new();

    match payload {
        ApiEarningsPayload::ByDate(map) => {
            for (date_str, group) in map {
                push_events(&mut events, &group.stocks, &date_str, from);
            }
        }
        ApiEarningsPayload::Flat(list) => {
            for s in &list {
                push_events(&mut events, std::slice::from_ref(s), &s.date, from);
            }
        }
    }

    events.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.symbol.cmp(&b.symbol)));

    info!(
        "Successfully parsed earnings payload; api totals earnings={:?} dates={:?}; built {} events",
        totals.0,
        totals.1,
        events.len()
    );

    // Count how many logos were received as URLs
    let logo_count = events.iter().filter(|e| e.logo.is_some()).count();
    info!("Received {} logo URLs from API", logo_count);

    Ok(events)
}

fn push_events(
    events: &mut Vec<EarningsEvent>,
    stocks: &[ApiEarning],
    date_str: &str,
    fallback_date: NaiveDate,
) {
    let parsed_date =
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d").unwrap_or_else(|_| fallback_date);

    for s in stocks {
        events.push(EarningsEvent {
            symbol: s.symbol.clone(),
            date: Utc
                .from_utc_datetime(&parsed_date.and_hms_opt(0, 0, 0).unwrap_or_else(|| {
                    parsed_date
                        .and_hms_opt(0, 0, 0)
                        .expect("failed to build datetime")
                })),
            date_end: None,
            time_of_day: s.time.clone(),
            eps_estimate: None,
            eps_actual: None,
            revenue_estimate: None,
            revenue_actual: None,
            importance: s.importance,
            title: s.title.clone(),
            emoji: s.emoji.clone(),
            logo: s.logo.clone(), // Now stores logo URL as a string
        });
    }
}