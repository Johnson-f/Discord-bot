use finance_query_core::{FetchClient, YahooAuthManager, YahooFinanceClient};
use serde_json::to_string_pretty;
use std::sync::Arc;
use std::{fs, path::Path};
use discord_bot::models::{Frequency, StatementType};
use discord_bot::service::finance::fundamentals::fetch_fundamentals_timeseries;

/// Integration test that hits the live Yahoo Finance API via finance-query-core.
///
/// This requires outbound network access. It is marked ignored by default to
/// avoid failures in offline or CI environments. Run manually with:
/// `cargo test -- --ignored fetches_live_fundamentals_timeseries`.
#[tokio::test]
#[ignore = "requires network access to Yahoo Finance"]
async fn fetches_live_fundamentals_timeseries() -> Result<(), Box<dyn std::error::Error>> {
    let fetch = Arc::new(FetchClient::new(None)?);
    let auth = Arc::new(YahooAuthManager::new(None, fetch.cookie_jar().clone()));
    let client = YahooFinanceClient::new(auth, fetch);

    let data = fetch_fundamentals_timeseries(
        &client,
        "AAPL",
        StatementType::IncomeStatement,
        Frequency::Annual,
        2,
    )
    .await?;

    let pretty = to_string_pretty(&data)?;

    // Save raw fundamentals JSON for documentation/output inspection.
    let out_path = Path::new("build-docs/stacks-bot-docs/json_output/fundamentals_output.json");
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, &pretty)?;
    println!(
        "full fundamentals response saved to {}:\n{}",
        out_path.display(),
        pretty
    );

    let empty = Vec::new();
    let results = data
        .get("timeseries")
        .and_then(|t| t.get("result"))
        .and_then(|r| r.as_array())
        .unwrap_or(&empty);

    if let Some(first) = results.first() {
        let keys: Vec<&str> = first
            .as_object()
            .map(|o| o.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();

        let revenue_sample = first
            .get("annualTotalRevenue")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| {
                item.get("reportedValue")
                    .and_then(|rv| rv.get("raw"))
                    .or_else(|| item.get("raw"))
            });

        println!(
            "fetched {} timeseries entries; first entry keys: {:?}; sample annualTotalRevenue raw: {:?}",
            results.len(),
            keys,
            revenue_sample
        );
    } else {
        println!("no timeseries entries returned");
    }

    assert!(
        !results.is_empty(),
        "expected non-empty fundamentals timeseries response"
    );

    Ok(())
}
