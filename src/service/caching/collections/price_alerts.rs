use std::collections::HashMap;

use chrono::{DateTime, Utc};
use redis::{AsyncCommands, RedisError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::service::caching::{CacheError, RedisCache};

const SYMBOL_SET_KEY: &str = "price_alerts:symbols";

fn alerts_key(symbol: &str) -> String {
    format!("price_alerts:{symbol}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceDirection {
    AtOrAbove,
    AtOrBelow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAlertLevel {
    pub label: String,
    pub target: f64,
    pub direction: PriceDirection,
    pub fired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAlert {
    pub id: String,
    pub symbol: String,
    pub created_at: DateTime<Utc>,
    pub created_price: f64,
    pub target_guild_id: u64,
    pub target_channel_id: u64,
    pub levels: Vec<PriceAlertLevel>,
}

#[derive(Debug, Error)]
pub enum PriceAlertStoreError {
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Redis(#[from] RedisError),
}

pub async fn load_all(
    cache: &RedisCache,
) -> Result<HashMap<String, Vec<PriceAlert>>, PriceAlertStoreError> {
    let mut conn = cache.connection();
    let symbols: Vec<String> = conn.smembers(SYMBOL_SET_KEY).await.unwrap_or_default();
    let mut out = HashMap::new();

    for symbol in symbols {
        let key = alerts_key(&symbol);
        let stored: Option<String> = conn.get(&key).await?;
        match stored {
            Some(json) => {
                let alerts: Vec<PriceAlert> = serde_json::from_str(&json)?;
                if !alerts.is_empty() {
                    out.insert(symbol.clone(), alerts);
                } else {
                    // Clean up empty entries.
                    let _: () = redis::pipe()
                        .del(&key)
                        .srem(SYMBOL_SET_KEY, &symbol)
                        .query_async(&mut conn)
                        .await?;
                }
            }
            None => {
                // No data left for this symbol; drop from the set.
                let _: () = redis::pipe()
                    .srem(SYMBOL_SET_KEY, &symbol)
                    .query_async(&mut conn)
                    .await?;
            }
        }
    }

    Ok(out)
}

pub async fn save_symbol_alerts(
    cache: &RedisCache,
    symbol: &str,
    alerts: &[PriceAlert],
) -> Result<(), PriceAlertStoreError> {
    let mut conn = cache.connection();
    if alerts.is_empty() {
        let _: () = redis::pipe()
            .del(alerts_key(symbol))
            .srem(SYMBOL_SET_KEY, symbol)
            .query_async(&mut conn)
            .await?;
        return Ok(());
    }

    let payload = serde_json::to_string(alerts)?;
    let _: () = redis::pipe()
        .set(alerts_key(symbol), payload)
        .sadd(SYMBOL_SET_KEY, symbol)
        .query_async(&mut conn)
        .await?;
    Ok(())
}

pub async fn append_alert(
    cache: &RedisCache,
    alert: &PriceAlert,
) -> Result<(), PriceAlertStoreError> {
    let mut all = load_all(cache).await?;
    let entry = all.entry(alert.symbol.clone()).or_default();
    entry.push(alert.clone());
    save_symbol_alerts(cache, &alert.symbol, entry).await
}

