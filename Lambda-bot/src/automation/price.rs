use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::Utc;
use futures_util::StreamExt;
use serenity::all::{ChannelId, GuildId, Http};
use serenity::async_trait;
use stacks_bot::service::caching::collections::price_alerts::{
    load_all, save_symbol_alerts, PriceAlert, PriceAlertLevel, PriceAlertStoreError, PriceDirection,
};
use stacks_bot::service::caching::RedisCache;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::finance::price::PriceService;

#[async_trait]
pub trait AlertNotifier: Send + Sync {
    async fn send(&self, channel_id: u64, content: String);
}

pub struct HttpNotifier {
    http: Arc<Http>,
}

impl HttpNotifier {
    pub fn new(http: Arc<Http>) -> Self {
        Self { http }
    }
}

#[async_trait]
impl AlertNotifier for HttpNotifier {
    async fn send(&self, channel_id: u64, content: String) {
        if let Err(err) = ChannelId::new(channel_id).say(&self.http, content).await {
            error!(?err, "failed to send price alert message");
        }
    }
}

#[derive(Debug, Error)]
pub enum PriceAlertError {
    #[error("missing field {0}")]
    MissingField(&'static str),
    #[error("parse error for {0}: {1}")]
    ParseError(&'static str, String),
    #[error(transparent)]
    Store(#[from] PriceAlertStoreError),
}

fn parse_field(lines: &[&str], label: &'static str) -> Result<String, PriceAlertError> {
    lines
        .windows(2)
        .find(|w| w[0].eq_ignore_ascii_case(label))
        .and_then(|w| {
            let val = w[1].trim();
            if val.is_empty() {
                None
            } else {
                Some(val.to_string())
            }
        })
        .ok_or(PriceAlertError::MissingField(label))
}

fn parse_f64_field(lines: &[&str], label: &'static str) -> Result<f64, PriceAlertError> {
    let raw = parse_field(lines, label)?;
    raw.parse::<f64>()
        .map_err(move |e| PriceAlertError::ParseError(label, e.to_string()))
}

fn choose_direction(target: f64, current: f64) -> PriceDirection {
    if target >= current {
        PriceDirection::AtOrAbove
    } else {
        PriceDirection::AtOrBelow
    }
}

fn make_level(label: &str, target: f64, current: f64) -> PriceAlertLevel {
    PriceAlertLevel {
        label: label.to_string(),
        target,
        direction: choose_direction(target, current),
        fired: false,
    }
}

pub fn parse_alert_message(
    raw: &str,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<PriceAlert, PriceAlertError> {
    let lines: Vec<&str> = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let symbol = parse_field(&lines, "Ticker")?;
    let current_price = parse_f64_field(&lines, "Current Price")?;
    let lambda = parse_f64_field(&lines, "Lambda Level")?;
    let fail_safe = parse_f64_field(&lines, "Fail-Safe")?;
    let up1 = parse_f64_field(&lines, "Upside PT1")?;
    let up2 = parse_f64_field(&lines, "Upside PT2")?;
    let up3 = parse_f64_field(&lines, "Upside PT3")?;
    let dn1 = parse_f64_field(&lines, "Downside PT1")?;
    let dn2 = parse_f64_field(&lines, "Downside PT2")?;
    let dn3 = parse_f64_field(&lines, "Downside PT3")?;

    let id = format!(
        "{}-{}",
        symbol,
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
    );

    let levels = vec![
        make_level("Lambda", lambda, current_price),
        make_level("FAIL SAFE", fail_safe, current_price),
        make_level("PT1 Upside", up1, current_price),
        make_level("PT2 Upside", up2, current_price),
        make_level("PT3 Upside", up3, current_price),
        make_level("PT1 Downside", dn1, current_price),
        make_level("PT2 Downside", dn2, current_price),
        make_level("PT3 Downside", dn3, current_price),
    ];

    Ok(PriceAlert {
        id,
        symbol,
        created_at: Utc::now(),
        created_price: current_price,
        target_guild_id: guild_id.get(),
        target_channel_id: channel_id.get(),
        levels,
    })
}

pub struct PriceAlertManager {
    notifier: Arc<dyn AlertNotifier>,
    price_service: Arc<PriceService>,
    cache: Option<Arc<RedisCache>>,
    state: Arc<Mutex<HashMap<String, Vec<PriceAlert>>>>,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    interval: Duration,
}

impl PriceAlertManager {
    pub fn new(
        http: Arc<Http>,
        price_service: Arc<PriceService>,
        cache: Option<Arc<RedisCache>>,
    ) -> Self {
        Self {
            notifier: Arc::new(HttpNotifier::new(http)),
            price_service,
            cache,
            state: Arc::new(Mutex::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            interval: Duration::from_secs(2),
        }
    }

    pub fn new_with_notifier(
        notifier: Arc<dyn AlertNotifier>,
        price_service: Arc<PriceService>,
        cache: Option<Arc<RedisCache>>,
    ) -> Self {
        Self {
            notifier,
            price_service,
            cache,
            state: Arc::new(Mutex::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            interval: Duration::from_secs(2),
        }
    }

    pub async fn hydrate(&self) -> Result<(), PriceAlertStoreError> {
        if let Some(cache) = &self.cache {
            let all = load_all(cache).await?;
            let mut state = self.state.lock().await;
            for (symbol, alerts) in all {
                state.insert(symbol.clone(), alerts);
                self.ensure_stream(&symbol).await;
            }
        }
        Ok(())
    }

    pub async fn register_from_message(
        &self,
        raw: &str,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<PriceAlert, PriceAlertError> {
        let alert = parse_alert_message(raw, guild_id, channel_id)?;
        self.insert_alert(alert.clone()).await?;
        Ok(alert)
    }

    async fn insert_alert(&self, alert: PriceAlert) -> Result<(), PriceAlertError> {
        let symbol = alert.symbol.clone();
        {
            let mut state = self.state.lock().await;
            state.entry(symbol.clone()).or_default().push(alert.clone());
        }

        if let Some(cache) = &self.cache {
            save_symbol_alerts(cache, &symbol, &self.snapshot_symbol(&symbol).await?).await?;
        }

        self.ensure_stream(&symbol).await;
        Ok(())
    }

    async fn snapshot_symbol(
        &self,
        symbol: &str,
    ) -> Result<Vec<PriceAlert>, PriceAlertStoreError> {
        let state = self.state.lock().await;
        Ok(state.get(symbol).cloned().unwrap_or_default())
    }

    async fn ensure_stream(&self, symbol: &str) {
        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(symbol) {
            return;
        }

        let symbol_owned = symbol.to_string();
        let notifier = Arc::clone(&self.notifier);
        let price_service = Arc::clone(&self.price_service);
        let cache = self.cache.clone();
        let state = Arc::clone(&self.state);
        let tasks_map = Arc::clone(&self.tasks);
        let interval = self.interval;

        let handle = tokio::spawn(async move {
            run_symbol_loop(
                symbol_owned.clone(),
                notifier,
                price_service,
                cache,
                state,
                interval,
            )
            .await;
            tasks_map.lock().await.remove(&symbol_owned);
        });

        tasks.insert(symbol.to_string(), handle);
    }
}

async fn run_symbol_loop(
    symbol: String,
    notifier: Arc<dyn AlertNotifier>,
    price_service: Arc<PriceService>,
    cache: Option<Arc<RedisCache>>,
    state: Arc<Mutex<HashMap<String, Vec<PriceAlert>>>>,
    interval: Duration,
) {
    info!("starting price alert stream for {symbol}");
    let mut stream = price_service
        .stream_prices(vec![symbol.clone()], interval)
        .boxed();

    while let Some(next) = stream.next().await {
        match next {
            Ok(update) => {
                let price = update
                    .prices
                    .iter()
                    .find(|p| p.symbol.eq_ignore_ascii_case(&symbol))
                    .or_else(|| update.prices.first())
                    .map(|p| p.price);

                let Some(price) = price else {
                    warn!("price update missing price for {}", symbol);
                    continue;
                };

                match handle_price(
                    &symbol,
                    price,
                    &notifier,
                    cache.as_ref(),
                    &state,
                )
                .await
                {
                    Ok(stop) => {
                        if stop {
                            info!("no remaining alerts for {symbol}; stopping stream");
                            break;
                        }
                    }
                    Err(err) => {
                        warn!(?err, "failed to handle price update for {symbol}");
                    }
                }
            }
            Err(err) => {
                warn!(?err, "price stream error for {symbol}");
                // brief backoff before retry loop continues
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn handle_price(
    symbol: &str,
    price: f64,
    notifier: &Arc<dyn AlertNotifier>,
    cache: Option<&Arc<RedisCache>>,
    state: &Arc<Mutex<HashMap<String, Vec<PriceAlert>>>>,
) -> Result<bool, PriceAlertStoreError> {
    let mut to_send = Vec::new();
    let mut persist: Option<Vec<PriceAlert>> = None;
    let mut stop = false;

    {
        let mut guard = state.lock().await;
        if let Some(alerts) = guard.get_mut(symbol) {
            for alert in alerts.iter_mut() {
                for level in alert.levels.iter_mut() {
                    if level.fired {
                        continue;
                    }
                    let hit = match level.direction {
                        PriceDirection::AtOrAbove => price >= level.target,
                        PriceDirection::AtOrBelow => price <= level.target,
                    };
                    if hit {
                        level.fired = true;
                        to_send.push((alert.target_channel_id, format!("{} {:.2} HIT", level.label, level.target)));
                    }
                }
            }

            alerts.retain(|a| a.levels.iter().any(|lvl| !lvl.fired));
            stop = alerts.is_empty();
            if stop {
                guard.remove(symbol);
                persist = Some(Vec::new());
            } else {
                persist = Some(alerts.clone());
            }
        } else {
            stop = true;
            persist = Some(Vec::new());
        }
    }

    for (channel_id, content) in to_send {
        notifier.send(channel_id, content).await;
    }

    if let Some(cache) = cache {
        if let Some(alerts) = persist {
            save_symbol_alerts(cache, symbol, &alerts).await?;
        }
    }

    Ok(stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::price::{Price, PriceUpdate};
    use serenity::all::{ChannelId, GuildId, Http};
    use std::env;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    #[derive(Default)]
    struct MockNotifier {
        sent: TokioMutex<Vec<(u64, String)>>,
    }

    #[async_trait]
    impl AlertNotifier for MockNotifier {
        async fn send(&self, channel_id: u64, content: String) {
            self.sent.lock().await.push((channel_id, content));
        }
    }

    fn sample_price(symbol: &str, price: f64) -> PriceUpdate {
        PriceUpdate {
            prices: vec![Price {
                symbol: symbol.to_string(),
                name: symbol.to_string(),
                price,
                change: 0.0,
                percent_change: 0.0,
                pre_market_price: None,
                after_hours_price: None,
            }],
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn triggers_expected_alerts() {
        let notifier = Arc::new(MockNotifier::default());
        let notifier_dyn: Arc<dyn AlertNotifier> = notifier.clone();
        let cache: Option<Arc<RedisCache>> = None;
        let state: Arc<Mutex<HashMap<String, Vec<PriceAlert>>>> = Arc::new(Mutex::new(HashMap::new()));

        let raw = r#"Ticker

SPY
Current Price
683.63
Lambda Level
684.5
Fail-Safe
681
Upside PT1
690
Upside PT2
687
Upside PT3
693
Downside PT1
680
Downside PT2
677
Downside PT3
674"#;

        let alert = parse_alert_message(raw, GuildId::new(1), ChannelId::new(2)).unwrap();
        {
            let mut guard = state.lock().await;
            guard.insert(alert.symbol.clone(), vec![alert]);
        }

        let symbol = "SPY";
        let prices = vec![683.0, 681.0, 680.0, 684.5, 687.0];
        for p in prices {
            let _ = handle_price(symbol, p, &notifier_dyn, cache.as_ref(), &state)
                .await
                .unwrap();
        }

        let sent = notifier.sent.lock().await.clone();
        let texts: Vec<_> = sent.iter().map(|(_, msg)| msg.as_str()).collect();

        assert!(texts.iter().any(|m| *m == "FAIL SAFE 681.00 HIT"));
        assert!(texts.iter().any(|m| *m == "PT1 Downside 680.00 HIT"));
        assert!(texts.iter().any(|m| *m == "Lambda 684.50 HIT"));
        assert!(texts.iter().any(|m| *m == "PT2 Upside 687.00 HIT"));
        // PT1 Upside (690) not reached in this sequence; ensure not sent
        assert!(!texts.iter().any(|m| m.contains("PT1 Upside 690.00")));
    }

    #[tokio::test]
    async fn sends_real_alert_to_discord() -> Result<(), Box<dyn std::error::Error>> {
        // Auto-load .env so RUN_REAL_DISCORD_TEST and tokens set there are visible.
        let _ = dotenvy::dotenv();
        if env::var("RUN_REAL_DISCORD_TEST").ok().as_deref() != Some("1") {
            eprintln!("set RUN_REAL_DISCORD_TEST=1 to run this live Discord test");
            return Ok(());
        }

        let token = env::var("DISCORD_TOKEN")?;
        let channel_id = env::var("TARGET_CHANNEL_ID")?.parse::<u64>()?;
        let guild_id = env::var("REGISTER_GUILD_ID")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let notifier: Arc<dyn AlertNotifier> = Arc::new(HttpNotifier::new(Arc::new(Http::new(&token))));
        let cache: Option<Arc<RedisCache>> = None;
        let state: Arc<Mutex<HashMap<String, Vec<PriceAlert>>>> = Arc::new(Mutex::new(HashMap::new()));

        let raw = r#"Ticker

SPY
Current Price
683.63
Lambda Level
684.5
Fail-Safe
681
Upside PT1
690
Upside PT2
687
Upside PT3
693
Downside PT1
680
Downside PT2
677
Downside PT3
674"#;

        let alert = parse_alert_message(raw, GuildId::new(guild_id), ChannelId::new(channel_id)).unwrap();
        {
            let mut guard = state.lock().await;
            guard.insert(alert.symbol.clone(), vec![alert]);
        }

        let prices = [684.5, 687.0];
        for p in prices {
            handle_price("SPY", p, &notifier, cache.as_ref(), &state).await?;
        }

        Ok(())
    }
}

