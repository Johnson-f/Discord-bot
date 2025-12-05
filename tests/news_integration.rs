use finance_query_core::{FetchClient, YahooAuthManager, YahooFinanceClient};
use serde_json::to_string_pretty;
use std::path::Path;
use std::sync::Arc;

use Discord_bot::service::finance::news::fetch_news;

/// Integration test that hits the live Yahoo Finance API via finance-query-core.
///
/// This requires outbound network access. It is marked ignored by default to
/// avoid failures in offline or CI environments. Run manually with:
/// `cargo test -- --ignored fetches_live_news`.
#[tokio::test]
#[ignore = "requires network access to Yahoo Finance"]
async fn fetches_live_news() -> Result<(), Box<dyn std::error::Error>> {
    let fetch = Arc::new(FetchClient::new(None)?);
    let auth = Arc::new(YahooAuthManager::new(None, fetch.cookie_jar().clone()));
    let client = YahooFinanceClient::new(auth, fetch);

    let raw = client.search("AAPL", 5).await?;

    // Save raw news JSON for documentation/output inspection.
    let pretty = to_string_pretty(&raw)?;
    let out_path = Path::new("build-docs/stacks-bot-docs/json_output/news_output.json");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, &pretty)?;
    println!(
        "full news response saved to {}:\n{}",
        out_path.display(),
        pretty
    );

    let items = fetch_news(&client, "AAPL", 5).await?;
    assert!(!items.is_empty(), "expected at least one news item");
    let first = &items[0];
    println!(
        "first news: {} — {} ({})",
        first.source.as_deref().unwrap_or("Unknown"),
        first.title,
        first.link
    );

    Ok(())
}