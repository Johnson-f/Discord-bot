use serenity::all::{
    CommandDataOptionValue, CommandInteraction, CommandOptionType, CreateCommand,
    CreateCommandOption,
};

use crate::service::finance::FinanceService;

pub fn register_command() -> CreateCommand {
    CreateCommand::new("quote")
        .description("Get a simple quote for a ticker")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "ticker",
                "Ticker symbol, e.g., AAPL",
            )
            .required(true),
        )
}

pub async fn handle(
    command: &CommandInteraction,
    finance: &FinanceService,
) -> Result<String, String> {
    let ticker = get_str_opt(command, "ticker").ok_or("ticker is required")?;
    build_response(finance, ticker).await
}

pub async fn handle_text(finance: &FinanceService, ticker: &str) -> Result<String, String> {
    build_response(finance, ticker).await
}

async fn build_response(finance: &FinanceService, ticker: &str) -> Result<String, String> {
    let quote = finance
        .get_price(ticker)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    let mut parts = Vec::new();
    parts.push(format!("{} ({})", quote.name, quote.symbol));
    if let Some(price) = quote.price {
        let currency = quote.currency.as_deref().unwrap_or("");
        parts.push(format!("Price: {:.2} {}", price, currency));
    }
    if let Some(ch) = quote.change {
        let pct = quote
            .percent_change
            .map(|p| format!("{:+.2}%", p))
            .unwrap_or_default();
        parts.push(format!("Change: {:+.2} {}", ch, pct));
    }
    if let Some(pm) = quote.pre_market_price {
        parts.push(format!("Pre-market: {:.2}", pm));
    }
    if let Some(ah) = quote.after_hours_price {
        parts.push(format!("After-hours: {:.2}", ah));
    }

    Ok(parts.join(" | "))
}

fn get_str_opt<'a>(command: &'a CommandInteraction, name: &str) -> Option<&'a str> {
    command
        .data
        .options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| match o.value {
            CommandDataOptionValue::String(ref s) => Some(s.as_str()),
            _ => None,
        })
}
