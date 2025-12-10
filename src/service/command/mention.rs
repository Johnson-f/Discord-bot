use serenity::all::{ChannelId, CreateAttachment, Http};

use crate::models::StatementType;
use crate::service::finance::FinanceService;

use super::{
    earnings,
    fundamentals,
    holders,
    news,
    quotes,
};

pub struct MentionResponse {
    pub content: String,
    pub attachment: Option<CreateAttachment>,
}

pub async fn handle(
    text: &str,
    http: &Http,
    channel_id: ChannelId,
    finance: &FinanceService,
) -> Result<MentionResponse, String> {
    let mut parts = text.split_whitespace();
    let cmd = parts
        .next()
        .ok_or_else(|| "No command provided. Try: ".to_string() + help_text())?
        .to_ascii_lowercase();

    match cmd.as_str() {
        "quote" => {
            let ticker = parts.next().ok_or("ticker required, e.g., quote AAPL")?;
            let content = quotes::handle_text(finance, ticker).await?;
            Ok(MentionResponse {
                content,
                attachment: None,
            })
        }
        "holders" => {
            let ticker = parts.next().ok_or("ticker required, e.g., holders AAPL major")?;
            let holder_type = parts
                .next()
                .ok_or("type required: major|institutional|mutualfund|insider_transactions|insider_purchases|insider_roster")?
                .to_ascii_lowercase();
            let limit = parts
                .next()
                .map(parse_usize)
                .transpose()
                .map_err(|e| format!("invalid limit: {e}"))?;
            let content = holders::handle_text(finance, ticker, &holder_type, limit).await?;
            Ok(MentionResponse {
                content,
                attachment: None,
            })
        }
        "news" => {
            let ticker = parts.next().ok_or("ticker required, e.g., news AAPL 3")?;
            let limit = parts
                .next()
                .map(parse_usize)
                .transpose()
                .map_err(|e| format!("invalid limit: {e}"))?
                .unwrap_or(1)
                .clamp(1, 10);
            let content = news::handle_text(finance, ticker, limit).await?;
            Ok(MentionResponse {
                content,
                attachment: None,
            })
        }
        "income" | "balance" | "cashflow" => {
            let ticker = parts.next().ok_or("ticker required, e.g., income AAPL revenue annual")?;
            let metric = parts.next().ok_or("metric required (see slash choices)")?;
            let freq = parts.next().ok_or("freq required: annual|quarterly")?;
            let year = parts
                .next()
                .map(parse_i32)
                .transpose()
                .map_err(|e| format!("invalid year: {e}"))?;
            let quarter = parts.next();

            let statement_type = match cmd.as_str() {
                "income" => StatementType::IncomeStatement,
                "balance" => StatementType::BalanceSheet,
                "cashflow" => StatementType::CashFlow,
                _ => unreachable!(),
            };

            let content = fundamentals::handle_text(
                finance,
                statement_type,
                ticker,
                &metric.to_ascii_lowercase(),
                &freq.to_ascii_lowercase(),
                year,
                quarter,
            )
            .await?;

            Ok(MentionResponse {
                content,
                attachment: None,
            })
        }
        "earnings" => {
            let mode = parts
                .next()
                .ok_or("earnings mode required: weekly|daily|reports")?
                .to_ascii_lowercase();
            match mode.as_str() {
                "weekly" => {
                    let resp = earnings::handle_weekly_plain(finance).await?;
                    let attachment = resp
                        .image
                        .map(|bytes| CreateAttachment::bytes(bytes, "earnings-calendar.png"));
                    Ok(MentionResponse {
                        content: resp.content,
                        attachment,
                    })
                }
                "daily" => {
                    let content =
                        earnings::handle_daily_for_channel(finance, http, channel_id).await?;
                    Ok(MentionResponse {
                        content,
                        attachment: None,
                    })
                }
                "reports" => {
                    let content =
                        earnings::handle_after_daily_for_channel(finance, http, channel_id).await?;
                    Ok(MentionResponse {
                        content,
                        attachment: None,
                    })
                }
                _ => Err("earnings mode must be weekly | daily | reports".into()),
            }
        }
        _ => Err(format!("Unknown command: {}. {}", cmd, help_text())),
    }
}

pub fn help_text() -> &'static str {
    "Usage: @Bot quote TICKER | holders TICKER TYPE [LIMIT] | news TICKER [LIMIT] | income|balance|cashflow TICKER METRIC FREQ [YEAR] [QUARTER] | earnings weekly|daily|reports"
}

fn parse_usize(raw: &str) -> Result<usize, std::num::ParseIntError> {
    raw.parse::<usize>()
}

fn parse_i32(raw: &str) -> Result<i32, std::num::ParseIntError> {
    raw.parse::<i32>()
}

