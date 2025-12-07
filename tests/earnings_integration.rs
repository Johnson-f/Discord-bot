use chrono::NaiveDate;
use serde_json::to_string_pretty;
use std::path::Path;

use discord_bot::service::finance::earnings::fetch_earnings_range;

/// Integration test that calls the external earnings calendar API.
///
/// Ignored by default to avoid CI failures. Run manually with:
/// `cargo test -- --ignored fetches_external_earnings_range`.
#[tokio::test]
#[ignore = "requires external network access"]
async fn fetches_external_earnings_range() -> Result<(), Box<dyn std::error::Error>> {
    let from = NaiveDate::from_ymd_opt(2025, 12, 10).unwrap();
    let to = NaiveDate::from_ymd_opt(2025, 12, 20).unwrap();

    let events = fetch_earnings_range(from, to).await?;

    let pretty = to_string_pretty(&events)?;
    let out_path = Path::new("build-docs/stacks-bot-docs/json_output/earnings_output.json");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, &pretty)?;
    println!(
        "earnings events saved to {} ({} events)\n{}",
        out_path.display(),
        events.len(),
        pretty
    );

    assert!(
        !events.is_empty(),
        "expected at least one earnings event in the range"
    );

    Ok(())
}
