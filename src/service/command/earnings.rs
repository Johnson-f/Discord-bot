use chrono::{Datelike, Duration, Utc, Weekday};
use chrono_tz::America::New_York;
use serenity::all::{CommandInteraction, CreateCommand, Http};
use std::time::Duration as StdDuration;
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::models::EarningsEvent;
use crate::service::automation::earnings;
use crate::service::finance::FinanceService;

fn week_range_mon_fri(
    weekday: Weekday,
    today: chrono::NaiveDate,
) -> (chrono::NaiveDate, chrono::NaiveDate) {
    // Monday is 0, Sunday is 6
    let days_from_mon = weekday.num_days_from_monday() as i64;
    let monday = if weekday == Weekday::Sun {
        today + Duration::days(1)
    } else {
        today - Duration::days(days_from_mon)
    };
    let friday = monday + Duration::days(4);
    (monday, friday)
}

/// Response payload for the /earnings command.
pub struct EarningsResponse {
    pub content: String,
    pub image: Option<Vec<u8>>,
}

pub fn register_weekly_command() -> CreateCommand {
    CreateCommand::new("weekly-earnings").description("Weekly earnings calendar")
}

pub fn register_daily_command() -> CreateCommand {
    CreateCommand::new("daily-earnings").description("Today's earnings calendar with implied move")
}

pub fn register_after_daily_command() -> CreateCommand {
    CreateCommand::new("er-reports")
        .description("Post-earnings reports for companies just announcing their numbers")
}

pub async fn handle_weekly(
    _command: &CommandInteraction,
    finance: &FinanceService,
) -> Result<EarningsResponse, String> {
    info!("Starting earnings command handler");

    // Compute the Monday–Friday range for the relevant week:
    // - Mon–Fri: current week (Mon..Fri)
    // - Sat: still the current week's Mon..Fri
    // - Sun: next week's Mon..Fri
    let now_et = Utc::now().with_timezone(&New_York);
    let (start, end) = week_range_mon_fri(now_et.weekday(), now_et.date_naive());

    info!("Fetching earnings from {} to {}", start, end);

    // Wrap the entire fetch in a timeout
    let events = match timeout(
        StdDuration::from_secs(200), // 20 second total timeout
        finance.get_earnings_range(start, end),
    )
    .await
    {
        Ok(Ok(events)) => {
            info!("Successfully fetched {} earnings events", events.len());
            events
        }
        Ok(Err(e)) => {
            error!("Failed to fetch earnings: {}", e);
            return Err(format!("Failed to fetch earnings: {}", e));
        }
        Err(_) => {
            error!("Earnings fetch timed out after 20 seconds");
            return Err("Request timed out. The earnings API is taking too long to respond. Please try again later.".to_string());
        }
    };

    if events.is_empty() {
        info!("No earnings found in the next 7 days");
        return Ok(EarningsResponse {
            content: "No earnings within the next 7 days.".to_string(),
            image: None,
        });
    }

    info!("Formatting output for {} events", events.len());
    let output = format_output(&events);
    let summary = format!(
        "📊 Earnings Calendar ({} to {}) — {} events",
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d"),
        events.len()
    );

    match earnings::render_calendar_image(&events).await {
        Ok(bytes) => Ok(EarningsResponse {
            content: summary,
            image: Some(bytes),
        }),
        Err(err) => {
            warn!("Falling back to text earnings calendar: {}", err);

            // Discord has a 2000 character limit
            let content = if output.len() > 1900 {
                warn!(
                    "Output is {} characters, truncating to fit Discord limit",
                    output.len()
                );
                format!(
                    "{}\n\n⚠️ *Message truncated - showing first {} of {} events. Use filters to see more.*\n⚠️ Image render unavailable: {}",
                    &output[..1800],
                    output.matches("📈").count().min(30),
                    events.len(),
                    err
                )
            } else {
                info!(
                    "Output is {} characters, within Discord limit",
                    output.len()
                );
                format!("{}\n\n⚠️ Image render unavailable: {}", output, err)
            };

            Ok(EarningsResponse {
                content,
                image: None,
            })
        }
    }
}

/// Post today's earnings summary to the current channel using the daily automation helper.
pub async fn handle_daily(
    command: &CommandInteraction,
    finance: &FinanceService,
    http: &Http,
) -> Result<String, String> {
    earnings::send_daily_report(http, finance, command.channel_id).await?;
    Ok("Posted today's earnings report to this channel.".to_string())
}

/// Manually trigger the post-earnings report for today.
/// - Before 4pm ET: BMO actuals
/// - After 6pm ET: AMC actuals
/// - Between 4–6pm ET: posts a waiting message
pub async fn handle_after_daily(
    command: &CommandInteraction,
    finance: &FinanceService,
    http: &Http,
) -> Result<String, String> {
    earnings::send_after_daily_report(http, finance, command.channel_id).await?;
    Ok("Posted today's post-earnings report to this channel.".to_string())
}

pub fn format_output(events: &[EarningsEvent]) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "📊 **Earnings Calendar (Next 7 Days)**\nFetched: {} | Total: {}",
        Utc::now().format("%Y-%m-%d %H:%M UTC"),
        events.len()
    ));
    lines.push(String::new()); // Empty line for spacing

    // Limit to first 50 events to avoid message being too long
    let display_events = if events.len() > 50 {
        warn!("Limiting display to first 50 of {} events", events.len());
        &events[..50]
    } else {
        events
    };

    for event in display_events {
        let date_str = event.date.format("%m/%d").to_string(); // Shorter date format
        let tod = match event.time_of_day.as_deref() {
            Some("16:00") | Some("amc") => "AMC",
            Some("09:00") | Some("bmo") => "BMO",
            Some(t) => t,
            None => "TBA",
        };

        let emoji = event.emoji.as_deref().unwrap_or("📈");
        let importance_indicator = match event.importance {
            Some(5) => " 🔥",
            Some(4) => " ⭐",
            _ => "",
        };

        // Compact format: emoji symbol date (time) importance
        let line = format!(
            "{} **{}** {} ({}){}",
            emoji, event.symbol, date_str, tod, importance_indicator
        );

        lines.push(line);
    }

    if events.len() > 50 {
        lines.push(String::new());
        lines.push(format!("*...and {} more*", events.len() - 50));
    }

    lines.join("\n")
}
