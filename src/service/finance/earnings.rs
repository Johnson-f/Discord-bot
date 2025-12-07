use std::collections::HashMap;
use std::time::Duration as StdDuration;

use chrono::{NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use tracing::{info, warn};

use crate::models::EarningsEvent;
use crate::service::finance::FinanceServiceError;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiDateGroup {
    stocks: Vec<ApiEarning>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiResponse {
    success: bool,
    date_from: Option<String>,
    date_to: Option<String>,
    total_dates: Option<usize>,
    total_earnings: Option<usize>,
    earnings: HashMap<String, ApiDateGroup>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiEarning {
    importance: Option<i64>,
    symbol: String,
    date: String,
    time: Option<String>,
    title: Option<String>,
    emoji: Option<String>,
    logo: Option<String>, // Added logo field
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
            ("includeLogos", "true"), // Request logos from API
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

    let body: ApiResponse = resp.json().await.map_err(|e| {
        warn!("Failed to parse earnings API response: {}", e);
        FinanceServiceError::Http(format!("earnings parse failed: {e}"))
    })?;

    if !body.success {
        warn!("Earnings API returned success=false");
        return Err(FinanceServiceError::Http(
            "earnings api returned success=false".into(),
        ));
    }

    info!(
        "Successfully fetched {} earnings across {} dates",
        body.total_earnings.unwrap_or(0),
        body.total_dates.unwrap_or(0)
    );

    let mut events = Vec::new();
    for (date_str, group) in body.earnings {
        let parsed_date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap_or_else(|_| {
            warn!("Failed to parse date: {}, using fallback", date_str);
            from
        });

        for s in group.stocks {
            events.push(EarningsEvent {
                symbol: s.symbol,
                date: Utc.from_utc_datetime(&parsed_date.and_hms_opt(0, 0, 0).unwrap_or_else(
                    || {
                        parsed_date
                            .and_hms_opt(0, 0, 0)
                            .expect("failed to build datetime")
                    },
                )),
                date_end: None,
                time_of_day: s.time,
                eps_estimate: None,
                eps_actual: None,
                revenue_estimate: None,
                revenue_actual: None,
                importance: s.importance,
                title: s.title,
                emoji: s.emoji,
                logo: s.logo, // Include logo data from API
            });
        }
    }

    events.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.symbol.cmp(&b.symbol)));

    info!("Returning {} earnings events", events.len());
    Ok(events)
}
