use std::sync::Arc;

use finance_query_core::{FetchClient, YahooAuthManager, YahooError, YahooFinanceClient};
use serde_json::Value;

use crate::models::{
    EarningsEvent, FinancialSummary, Frequency, HolderType, HoldersOverview, NewsItem, PriceQuote,
    StatementType,
};

pub mod earnings;
pub mod fundamentals;
pub mod holders;
pub mod news;
pub mod options;

#[derive(Debug, thiserror::Error)]
pub enum FinanceServiceError {
    #[error(transparent)]
    Yahoo(#[from] YahooError),
    #[error("No quote data for symbol {0}")]
    NotFound(String),
    #[error("Earnings API error: {0}")]
    Http(String),
}

pub struct FinanceService {
    client: Arc<YahooFinanceClient>,
    #[allow(dead_code)]
    auth: Arc<YahooAuthManager>,
    #[allow(dead_code)]
    fetch: Arc<FetchClient>,
}

impl FinanceService {
    /// Build a finance service with optional proxy support.
    pub fn new(proxy: Option<String>) -> Result<Self, FinanceServiceError> {
        let fetch = Arc::new(FetchClient::new(proxy.clone())?);
        let auth = Arc::new(YahooAuthManager::new(proxy, fetch.cookie_jar().clone()));
        let client = Arc::new(YahooFinanceClient::new(auth.clone(), fetch.clone()));

        Ok(Self {
            client,
            auth,
            fetch,
        })
    }

    /// Access the underlying YahooFinanceClient.
    pub fn client(&self) -> &YahooFinanceClient {
        self.client.as_ref()
    }

    /// Fetch a simple price quote for a single symbol.
    pub async fn get_price(&self, symbol: &str) -> Result<PriceQuote, FinanceServiceError> {
        let data = self.client.get_simple_quotes(&[symbol]).await?;
        let quote = extract_simple_quote(&data)
            .ok_or_else(|| FinanceServiceError::NotFound(symbol.to_string()))?;

        Ok(PriceQuote {
            symbol: quote.symbol,
            name: quote.name,
            price: quote.price,
            currency: quote.currency,
            change: quote.change,
            percent_change: quote.percent_change,
            pre_market_price: quote.pre_market_price,
            after_hours_price: quote.after_hours_price,
        })
    }

    /// Fetch key financial metrics for a symbol.
    pub async fn get_financials(
        &self,
        symbol: &str,
    ) -> Result<FinancialSummary, FinanceServiceError> {
        let fundamentals = fundamentals::fetch_fundamentals_timeseries(
            self.client.as_ref(),
            symbol,
            StatementType::IncomeStatement,
            Frequency::Annual,
            5,
        )
        .await
        .ok();

        let summary = self
            .client
            .get_quote_summary(
                symbol,
                &[
                    "price",
                    "defaultKeyStatistics",
                    "summaryDetail",
                    "financialData",
                ],
            )
            .await?;

        let result = summary
            .get("quoteSummary")
            .and_then(|q| q.get("result"))
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| FinanceServiceError::NotFound(symbol.to_string()))?;

        let revenue = fundamentals
            .as_ref()
            .and_then(|v| extract_timeseries_latest(v, "annualTotalRevenue"))
            .or_else(|| extract_financial_data_f64(result, "financialData", "totalRevenue"));

        let eps = extract_f64_raw(result, &["defaultKeyStatistics", "trailingEps"])
            .or_else(|| extract_f64_raw(result, &["defaultKeyStatistics", "forwardEps"]));

        let pe_ratio = extract_f64_raw(result, &["summaryDetail", "trailingPE"])
            .or_else(|| extract_f64_raw(result, &["defaultKeyStatistics", "forwardPE"]));

        let market_cap = extract_f64_raw(result, &["price", "marketCap"])
            .or_else(|| extract_f64_raw(result, &["summaryDetail", "marketCap"]));

        let currency = result
            .get("price")
            .and_then(|p| p.get("currency"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());

        Ok(FinancialSummary {
            symbol: symbol.to_uppercase(),
            revenue,
            eps,
            pe_ratio,
            market_cap,
            currency,
        })
    }

    /// Fetch raw fundamentals timeseries for a symbol and frequency.
    pub async fn get_fundamentals_raw(
        &self,
        symbol: &str,
        statement_type: StatementType,
        frequency: Frequency,
        years_back: i64,
    ) -> Result<Value, FinanceServiceError> {
        let data = fundamentals::fetch_fundamentals_timeseries(
            self.client.as_ref(),
            symbol,
            statement_type,
            frequency,
            years_back,
        )
        .await?;

        Ok(data)
    }

    /// Fetch holders data for a symbol for a specific holder type.
    pub async fn get_holders(
        &self,
        symbol: &str,
        holder_type: HolderType,
    ) -> Result<HoldersOverview, FinanceServiceError> {
        let data = holders::fetch_holders(self.client.as_ref(), symbol, holder_type).await?;
        Ok(data)
    }

    /// Fetch news for a symbol (limited number of items).
    pub async fn get_news(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<Vec<NewsItem>, FinanceServiceError> {
        let limit = limit.clamp(1, 20);
        let items = news::fetch_news(self.client.as_ref(), symbol, limit).await?;
        if items.is_empty() {
            return Err(FinanceServiceError::NotFound(format!(
                "no news found for symbol {symbol}"
            )));
        }
        Ok(items)
    }

    /// Fetch earnings events for a date range (external API).
    pub async fn get_earnings_range(
        &self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Result<Vec<EarningsEvent>, FinanceServiceError> {
        earnings::fetch_earnings_range(from, to).await
    }
}

/// Extract the first simple quote from the Yahoo response into our bot-facing struct.
fn extract_simple_quote(data: &Value) -> Option<PriceQuote> {
    let result = data
        .get("quoteResponse")
        .and_then(|q| q.get("result"))
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())?;

    Some(PriceQuote {
        symbol: result.get("symbol")?.as_str()?.to_string(),
        name: result
            .get("longName")
            .or_else(|| result.get("shortName"))
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string(),
        price: result.get("regularMarketPrice").and_then(|v| v.as_f64()),
        currency: result
            .get("currency")
            .or_else(|| result.get("financialCurrency"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string()),
        change: result.get("regularMarketChange").and_then(|v| v.as_f64()),
        percent_change: result
            .get("regularMarketChangePercent")
            .and_then(|v| v.as_f64()),
        pre_market_price: result.get("preMarketPrice").and_then(|v| v.as_f64()),
        after_hours_price: result.get("postMarketPrice").and_then(|v| v.as_f64()),
    })
}

fn extract_timeseries_latest(data: &Value, field: &str) -> Option<f64> {
    let results = data
        .get("timeseries")
        .and_then(|t| t.get("result"))
        .and_then(|r| r.as_array())?;

    for entry in results {
        if let Some(values) = entry.get(field).and_then(|v| v.as_array()) {
            for item in values {
                if let Some(raw) = item
                    .get("reportedValue")
                    .and_then(|rv| rv.get("raw"))
                    .and_then(|r| r.as_f64())
                {
                    return Some(raw);
                }
                if let Some(raw) = item.get("raw").and_then(|r| r.as_f64()) {
                    return Some(raw);
                }
            }
        }
    }
    None
}

fn extract_f64_raw(root: &Value, path: &[&str]) -> Option<f64> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?;
    }

    current
        .get("raw")
        .and_then(|v| v.as_f64())
        .or_else(|| current.as_f64())
}

fn extract_financial_data_f64(root: &Value, module: &str, field: &str) -> Option<f64> {
    root.get(module)
        .and_then(|m| m.get(field))
        .and_then(|v| v.get("raw").and_then(|r| r.as_f64()).or_else(|| v.as_f64()))
}

pub use FinanceServiceError as Error;
