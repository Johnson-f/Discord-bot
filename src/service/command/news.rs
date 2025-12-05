use chrono::Utc;
use serenity::all::{
    CommandDataOptionValue, CommandInteraction, CommandOptionType, CreateCommand,
    CreateCommandOption,
};

use crate::service::finance::FinanceService;

pub fn register_command() -> CreateCommand {
    CreateCommand::new("news")
        .description("Latest headlines for a ticker")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "ticker",
                "Ticker symbol, e.g., AAPL",
            )
            .required(true),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "limit",
                "How many headlines (1-10, default 1)",
            )
            .min_int_value(1)
            .max_int_value(10),
        )
}

pub async fn handle(
    command: &CommandInteraction,
    finance: &FinanceService,
) -> Result<String, String> {
    let ticker = get_str_opt(command, "ticker").ok_or("ticker is required")?;
    let limit = get_int_opt(command, "limit").unwrap_or(1).clamp(1, 10) as usize;

    let news = finance
        .get_news(ticker, limit)
        .await
        .map_err(|e| format!("fetch error: {e}"))?;

    let mut lines = Vec::new();
    lines.push(format!("Latest news for {}", ticker.to_uppercase()));
    for item in news {
        let source = item.source.clone().unwrap_or_else(|| "Unknown".to_string());
        let time_str = item
            .published_at
            .map(|t| t.with_timezone(&Utc).format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "time n/a".to_string());
        lines.push(format!("• [{}]({}) — {} ({})", item.title, item.link, source, time_str));
    }

    Ok(lines.join("\n"))
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

fn get_int_opt(command: &CommandInteraction, name: &str) -> Option<i64> {
    command
        .data
        .options
        .iter()
        .find(|o| o.name == name)
        .and_then(|o| match o.value {
            CommandDataOptionValue::Integer(i) => Some(i),
            _ => None,
        })
}
