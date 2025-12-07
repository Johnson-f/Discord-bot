use chrono::{DateTime, Duration, Utc};
use finance_query_core::{
    utils::{
        financials_constants::{BALANCE_SHEET_FIELDS, CASH_FLOW_FIELDS, INCOME_STATEMENT_FIELDS},
        get_statement_fields,
    },
    YahooError, YahooFinanceClient,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::models::FinancialStatement;
use crate::models::{Frequency, StatementType};

/// Default lookback for fundamentals queries (in years).
pub const FETCH_YEARS_DEFAULT: i64 = 5;

/// Fetch fundamentals timeseries data for a symbol using finance-query-core.
///
/// This builds the correct `type` list from StatementType/Frequency and queries
/// Yahoo Finance fundamentals-timeseries over a configurable lookback.
pub async fn fetch_fundamentals_timeseries(
    client: &YahooFinanceClient,
    symbol: &str,
    statement_type: StatementType,
    frequency: Frequency,
    years_back: i64,
) -> Result<Value, YahooError> {
    let now = Utc::now().timestamp();
    let start = now - Duration::days(365 * years_back).num_seconds();

    let fields = get_statement_fields(statement_type.as_str(), frequency.as_str());
    let refs: Vec<&str> = fields.iter().map(String::as_str).collect();

    client
        .get_fundamentals_timeseries(symbol, start, now, &refs)
        .await
}

/// Reshape the raw finance-query-core fundamentals timeseries payload into our
/// `FinancialStatement` model. Groups metrics by statement type and frequency,
/// and indexes each metric's values by `asOfDate`.
pub fn reshape_timeseries_to_financial_statements(data: &Value) -> Vec<FinancialStatement> {
    let income: HashSet<&'static str> = INCOME_STATEMENT_FIELDS.iter().copied().collect();
    let balance: HashSet<&'static str> = BALANCE_SHEET_FIELDS.iter().copied().collect();
    let cashflow: HashSet<&'static str> = CASH_FLOW_FIELDS.iter().copied().collect();

    let mut grouped: HashMap<(String, String, String), FinancialStatement> = HashMap::new();

    let empty = Vec::new();
    let results = data
        .get("timeseries")
        .and_then(|t| t.get("result"))
        .and_then(|r| r.as_array())
        .unwrap_or(&empty);

    for entry in results {
        let meta = entry.get("meta").and_then(|m| m.as_object());
        let symbol = meta
            .and_then(|m| m.get("symbol"))
            .and_then(|s| s.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let object = match entry.as_object() {
            Some(o) => o,
            None => continue,
        };

        let timestamps = entry
            .get("timestamp")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().map(|v| v.as_i64()).collect::<Vec<_>>())
            .unwrap_or_default();

        for (field, value) in object {
            if field == "meta" || field == "timestamp" {
                continue;
            }

            let (frequency, base_field) = if let Some(rest) = field.strip_prefix("annual") {
                ("annual", rest)
            } else if let Some(rest) = field.strip_prefix("quarterly") {
                ("quarterly", rest)
            } else {
                continue;
            };

            let statement_type = if income.contains(base_field) {
                "income"
            } else if balance.contains(base_field) {
                "balance"
            } else if cashflow.contains(base_field) {
                "cashflow"
            } else {
                continue;
            };

            let key = (
                symbol.clone(),
                statement_type.to_string(),
                frequency.to_string(),
            );

            let fs = grouped
                .entry(key.clone())
                .or_insert_with(|| FinancialStatement {
                    symbol: symbol.clone(),
                    statement_type: statement_type.to_string(),
                    frequency: frequency.to_string(),
                    statement: HashMap::new(),
                });

            let metric_map = fs
                .statement
                .entry(base_field.to_string())
                .or_default();

            if let Some(items) = value.as_array() {
                for (idx, item) in items.iter().enumerate() {
                    if let Some(date) = item.get("asOfDate").and_then(|d| d.as_str()) {
                        let mut item_clone = item.clone();

                        if let Some(ts_secs_opt) = timestamps.get(idx).and_then(|v| *v) {
                            if let Some(dt) = DateTime::<Utc>::from_timestamp(ts_secs_opt, 0) {
                                if let Some(obj) = item_clone.as_object_mut() {
                                    obj.insert(
                                        "timestamp_rfc3339".to_string(),
                                        Value::String(dt.to_rfc3339()),
                                    );
                                }
                            }
                        }

                        metric_map.insert(date.to_string(), item_clone);
                    }
                }
            }
        }
    }

    grouped.into_values().collect()
}
