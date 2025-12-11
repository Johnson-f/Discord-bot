use Lambda_bot::finance::price::PriceService;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn stream_prices_returns_data() {
    // This test makes a real network call to Yahoo Finance.
    let service = PriceService::new()
        .await
        .expect("failed to initialize PriceService");

    let mut stream = service.stream_prices(vec!["AAPL".to_string()], Duration::from_secs(2));

    let first = timeout(Duration::from_secs(20), async {
        while let Some(update) = stream.next().await {
            let update = update.expect("stream error");
            if !update.prices.is_empty() {
                return Some(update);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();

    assert!(first.is_some(), "expected at least one price update");
}

