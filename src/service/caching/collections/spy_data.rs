use std::collections::HashMap;

use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::Deserialize;

use crate::service::caching::{CacheError, RedisCache};
use crate::service::finance::options::OptionSlice;

const HISTORY_LIMIT: isize = 200;
const HISTORY_TTL_SECS: i64 = 60 * 60 * 24 * 7; // 7 days

#[derive(Debug, Deserialize)]
struct Point {
    t: String,
    p: f64,
}

fn strikes_key(expiration: &str) -> String {
    format!("spy:history:{expiration}:strikes")
}

fn strike_key(expiration: &str, strike: &str) -> String {
    format!("spy:history:{expiration}:{strike}")
}

/// Append the latest slice prices to Redis and keep the history bounded.
pub async fn append_slice(cache: &RedisCache, slice: &OptionSlice) -> Result<(), CacheError> {
    let mut conn = cache.connection();
    let now = Utc::now().to_rfc3339();
    let strikes_key = strikes_key(&slice.expiration);

    for contract in slice.calls.iter().chain(slice.puts.iter()) {
        let strike = format!("{:.2}", contract.strike);
        let key = strike_key(&slice.expiration, &strike);
        let entry = serde_json::json!({ "t": now, "p": contract.last_price }).to_string();

        redis::pipe()
            .lpush(&key, entry)
            .ltrim(&key, 0, HISTORY_LIMIT - 1)
            .expire(&key, HISTORY_TTL_SECS)
            .sadd(&strikes_key, &strike)
            .expire(&strikes_key, HISTORY_TTL_SECS)
            .query_async::<()>(&mut conn)
            .await?;
    }

    Ok(())
}

/// Load bounded history for the given expiration.
pub async fn load_history(
    cache: &RedisCache,
    expiration: &str,
    max_points: usize,
) -> Result<HashMap<String, Vec<(DateTime<Utc>, f64)>>, CacheError> {
    let mut conn = cache.connection();
    let strikes: Vec<String> = conn.smembers(strikes_key(expiration)).await.unwrap_or_default();
    let mut out = HashMap::new();
    let end = if max_points == 0 {
        -1
    } else {
        max_points.saturating_sub(1) as isize
    };

    for strike in strikes {
        let key = strike_key(expiration, &strike);
        let entries: Vec<String> = conn.lrange(&key, 0, end).await.unwrap_or_default();
        let mut points = Vec::new();
        for entry in entries.into_iter().rev() {
            if let Ok(point) = serde_json::from_str::<Point>(&entry) {
                if let Ok(dt) = DateTime::parse_from_rfc3339(&point.t) {
                    points.push((dt.with_timezone(&Utc), point.p));
                }
            }
        }
        if !points.is_empty() {
            out.insert(strike, points);
        }
    }

    Ok(out)
}

/// Fallback helper to build a minimal history map from the current slice.
pub fn history_from_slice(slice: &OptionSlice) -> HashMap<String, Vec<(DateTime<Utc>, f64)>> {
    let mut out = HashMap::new();
    let now = Utc::now();
    for contract in slice.calls.iter().chain(slice.puts.iter()) {
        let strike = format!("{:.2}", contract.strike);
        out.insert(strike, vec![(now, contract.last_price)]);
    }
    out
}

pub const DEFAULT_HISTORY_POINTS: usize = HISTORY_LIMIT as usize;

