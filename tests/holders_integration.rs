use finance_query_core::{FetchClient, HolderType, YahooAuthManager, YahooFinanceClient};
use std::{fs, path::PathBuf, sync::Arc};

use discord_bot::service::finance::holders::fetch_holders;

#[tokio::test]
#[ignore = "requires network access to Yahoo Finance"]
async fn fetches_live_holders() -> Result<(), Box<dyn std::error::Error>> {
    let fetch = Arc::new(FetchClient::new(None)?);
    let auth = Arc::new(YahooAuthManager::new(None, fetch.cookie_jar().clone()));
    let client = YahooFinanceClient::new(auth, fetch);

    let data = fetch_holders(&client, "AAPL", HolderType::Institutional).await?;

    let has_rows = data
        .institutional_holders
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    assert!(has_rows, "no institutional holders returned");

    let path = PathBuf::from("build-docs/stacks-bot-docs/json_output/holders_output.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({
            "institutional": data.institutional_holders,
            "symbol": data.symbol,
        }))?,
    )?;
    println!("holders output written to {}", path.display());

    Ok(())
}
