#![allow(non_snake_case)]

use std::{env, sync::Arc};

use anyhow::Result;
use dotenv::dotenv;
use serenity::all::{
    ApplicationId, Command, CreateAttachment, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditAttachments, GatewayIntents, GuildId, Interaction,
};
use serenity::{async_trait, model::gateway::Ready, prelude::*, Client};
use tracing::info;

use discord_bot::models::StatementType;
use discord_bot::service::automation::{earnings, options_data};
use discord_bot::service::command::earnings as earnings_cmd;
use discord_bot::service::command::fundamentals as fundamentals_cmd;
use discord_bot::service::command::holders as holders_cmd;
use discord_bot::service::command::news as news_cmd;
use discord_bot::service::command::quotes as quotes_cmd;
use discord_bot::service::finance::FinanceService;

struct Handler {
    finance: Arc<FinanceService>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        let guild_id = env::var("GUILD_ID")
            .ok()
            .and_then(|id| id.parse::<u64>().ok())
            .map(GuildId::new);

        if let Some(guild_id) = guild_id {
            // Register guild commands (instant!)
            let _ = guild_id.create_command(&ctx.http, ping_command()).await;
            let _ = guild_id
                .create_command(
                    &ctx.http,
                    fundamentals_cmd::register_command(StatementType::IncomeStatement),
                )
                .await;
            let _ = guild_id
                .create_command(
                    &ctx.http,
                    fundamentals_cmd::register_command(StatementType::BalanceSheet),
                )
                .await;
            let _ = guild_id
                .create_command(
                    &ctx.http,
                    fundamentals_cmd::register_command(StatementType::CashFlow),
                )
                .await;
            let _ = guild_id
                .create_command(&ctx.http, quotes_cmd::register_command())
                .await;
            let _ = guild_id
                .create_command(&ctx.http, holders_cmd::register_command())
                .await;
            let _ = guild_id
                .create_command(&ctx.http, news_cmd::register_command())
                .await;
            let _ = guild_id
                .create_command(&ctx.http, earnings_cmd::register_weekly_command())
                .await;
            let _ = guild_id
                .create_command(&ctx.http, earnings_cmd::register_daily_command())
                .await;
            let _ = guild_id
                .create_command(&ctx.http, earnings_cmd::register_after_daily_command())
                .await;
            info!(
                "{} is connected. Guild commands registered instantly for testing.",
                ready.user.name
            );
        } else {
            // Fallback to global commands (takes up to 1 hour)
            let _ = Command::create_global_command(&ctx.http, ping_command()).await;
            let _ = Command::create_global_command(
                &ctx.http,
                fundamentals_cmd::register_command(StatementType::IncomeStatement),
            )
            .await;
            let _ = Command::create_global_command(
                &ctx.http,
                fundamentals_cmd::register_command(StatementType::BalanceSheet),
            )
            .await;
            let _ = Command::create_global_command(
                &ctx.http,
                fundamentals_cmd::register_command(StatementType::CashFlow),
            )
            .await;
            let _ = Command::create_global_command(&ctx.http, quotes_cmd::register_command()).await;
            let _ =
                Command::create_global_command(&ctx.http, holders_cmd::register_command()).await;
            let _ = Command::create_global_command(&ctx.http, news_cmd::register_command()).await;
            let _ =
                Command::create_global_command(&ctx.http, earnings_cmd::register_weekly_command())
                    .await;
            let _ =
                Command::create_global_command(&ctx.http, earnings_cmd::register_daily_command())
                    .await;
            let _ = Command::create_global_command(
                &ctx.http,
                earnings_cmd::register_after_daily_command(),
            )
                    .await;
            info!(
                "{} is connected. Global commands registered (may take up to 1 hour).",
                ready.user.name
            );
        }

        // Start SPY options pinger (every 15 minutes) if configured
        options_data::spawn_options_pinger(ctx.http.clone(), self.finance.clone());
        // Start daily earnings poster
        earnings::spawn_earnings_poster(ctx.http.clone(), self.finance.clone());
        // Start daily earnings (IV/IM) poster at 6pm ET
        earnings::spawn_daily_report_poster(ctx.http.clone(), self.finance.clone());
        // Start post-earnings (actuals) poster at 8:45am ET (BMO) and 5:50pm ET (AMC)
        earnings::spawn_after_daily_poster(ctx.http.clone(), self.finance.clone());
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            match command.data.name.as_str() {
                "ping" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new().content("Pong!"),
                            ),
                        )
                        .await;
                }
                "income" | "balance" | "cashflow" => {
                    // Defer immediately to avoid 3-second timeout
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    // Now we have up to 15 minutes to send the actual response
                    let content = match fundamentals_cmd::handle(&command, &self.finance).await {
                        Ok(msg) => msg,
                        Err(err) => format!("❌ {}", err),
                    };

                    // Send the follow-up message
                    let _ = command
                        .edit_response(
                            &ctx.http,
                            serenity::all::EditInteractionResponse::new().content(content),
                        )
                        .await;
                }
                "quote" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    let content = match quotes_cmd::handle(&command, &self.finance).await {
                        Ok(msg) => msg,
                        Err(err) => format!("❌ {}", err),
                    };

                    let _ = command
                        .edit_response(
                            &ctx.http,
                            serenity::all::EditInteractionResponse::new().content(content),
                        )
                        .await;
                }
                "holders" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    let content = match holders_cmd::handle(&command, &self.finance).await {
                        Ok(msg) => msg,
                        Err(err) => format!("❌ {}", err),
                    };

                    let _ = command
                        .edit_response(
                            &ctx.http,
                            serenity::all::EditInteractionResponse::new().content(content),
                        )
                        .await;
                }
                "news" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    let content = match news_cmd::handle(&command, &self.finance).await {
                        Ok(msg) => msg,
                        Err(err) => format!("❌ {}", err),
                    };

                    let _ = command
                        .edit_response(
                            &ctx.http,
                            serenity::all::EditInteractionResponse::new().content(content),
                        )
                        .await;
                }
                "weekly-earnings" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    let response = match earnings_cmd::handle_weekly(&command, &self.finance).await
                    {
                        Ok(resp) => resp,
                        Err(err) => {
                            let _ = command
                                .edit_response(
                                    &ctx.http,
                                    serenity::all::EditInteractionResponse::new()
                                        .content(format!("❌ {}", err)),
                                )
                                .await;
                            return;
                        }
                    };

                    let mut edit =
                        serenity::all::EditInteractionResponse::new().content(response.content);

                    if let Some(bytes) = response.image {
                        let attachment = CreateAttachment::bytes(bytes, "earnings-calendar.png");
                        let attachments = EditAttachments::new().add(attachment);
                        edit = edit.attachments(attachments);
                    }

                    let _ = command.edit_response(&ctx.http, edit).await;
                }
                "daily-earnings" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    let content = match earnings_cmd::handle_daily(
                        &command,
                        &self.finance,
                        &ctx.http,
                    )
                    .await
                    {
                        Ok(msg) => msg,
                        Err(err) => format!("❌ {}", err),
                    };

                    let _ = command
                        .edit_response(
                            &ctx.http,
                            serenity::all::EditInteractionResponse::new().content(content),
                        )
                        .await;
                }
                "er-reports" => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(Default::default()),
                        )
                        .await;

                    let content =
                        match earnings_cmd::handle_after_daily(&command, &self.finance, &ctx.http)
                            .await
                        {
                        Ok(msg) => msg,
                        Err(err) => format!("❌ {}", err),
                    };

                    let _ = command
                        .edit_response(
                            &ctx.http,
                            serenity::all::EditInteractionResponse::new().content(content),
                        )
                        .await;
                }
                _ => {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Command not implemented."),
                            ),
                        )
                        .await;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let token = env::var("DISCORD_TOKEN")?;
    let app_id_raw: u64 = env::var("APPLICATION_ID")?.parse()?;
    let app_id: ApplicationId = app_id_raw.into();

    let intents = GatewayIntents::empty();

    info!("Initializing FinanceService...");
    let finance = Arc::new(FinanceService::new(None)?);

    info!("Starting Discord client...");
    let mut client = Client::builder(token, intents)
        .application_id(app_id)
        .event_handler(Handler { finance })
        .await?;

    if let Err(why) = client.start().await {
        eprintln!("Client error: {why}");
    }

    Ok(())
}

fn ping_command() -> CreateCommand {
    CreateCommand::new("ping").description("Simple ping command")
}
