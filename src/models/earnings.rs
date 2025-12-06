use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Earnings event used by the bot for calendar displays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarningsEvent {
    pub symbol: String,
    pub date: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_end: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<String>, // e.g., BMO/AMC if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eps_estimate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eps_actual: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revenue_estimate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revenue_actual: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub importance: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,  // Base64 encoded logo data from API
}