use chrono::{DateTime, Utc};
use finance_query_core::{YahooFinanceClient, YahooError};
use serde_json::Value;

use std::cmp::Ordering;

use crate::models::NewsItem;

/// Fetch news items for a symbol via Yahoo search.
pub async fn fetch_news(
    client: &YahooFinanceClient,
    symbol: &str,
    limit: usize,
) -> Result<Vec<NewsItem>, YahooError> {
    let data = client.search(symbol, limit).await?;
    Ok(parse_news(&data, limit))
}

fn parse_news(data: &Value, limit: usize) -> Vec<NewsItem> {
    let mut items = Vec::new();
    let empty = Vec::new();
    let news = data.get("news").and_then(|n| n.as_array()).unwrap_or(&empty);

    for item in news {
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let link = item
            .get("link")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if title.is_empty() || link.is_empty() {
            continue;
        }

        let source = item
            .get("publisher")
            .or_else(|| item.get("provider"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let published_at = item
            .get("providerPublishTime")
            .and_then(|v| v.as_i64())
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

        let thumbnail = item
            .get("thumbnail")
            .and_then(|t| t.get("resolutions"))
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.first())
            .and_then(|r| r.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                item.get("main_image")
                    .and_then(|m| m.get("original_url"))
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string())
            });

        items.push(NewsItem {
            title,
            link,
            source,
            published_at,
            thumbnail,
        });
    }

    // Sort newest first
    items.sort_by(|a, b| match (a.published_at, b.published_at) {
        (Some(a), Some(b)) => b.cmp(&a), // descending
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });

    if items.len() > limit {
        items.truncate(limit);
    }

    items
}
