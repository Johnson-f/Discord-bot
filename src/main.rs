use std::{env, sync::Arc};

use anyhow::Result;
use dotenv::dotenv;
use serenity::all::{
    ApplicationId, Command, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage, GatewayIntents, Interaction, GuildId,
};
use serenity::{async_trait, model::gateway::Ready, prelude::*, Client};
use tracing::info;

use Discord_bot::service::automation::options_data;
use Discord_bot::service::command::fundamentals as fundamentals_cmd;
use Discord_bot::service::command::holders as holders_cmd;
use Discord_bot::service::command::quotes as quotes_cmd;
use Discord_bot::service::command::news as news_cmd;
use Discord_bot::service::finance::FinanceService;
use Discord_bot::models::StatementType;

struct Handler {
    app_id: ApplicationId,
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
            let _ = guild_id.create_command(&ctx.http, fundamentals_cmd::register_command(StatementType::IncomeStatement)).await;
            let _ = guild_id.create_command(&ctx.http, fundamentals_cmd::register_command(StatementType::BalanceSheet)).await;
            let _ = guild_id.create_command(&ctx.http, fundamentals_cmd::register_command(StatementType::CashFlow)).await;
            let _ = guild_id.create_command(&ctx.http, quotes_cmd::register_command()).await;
            let _ = guild_id.create_command(&ctx.http, holders_cmd::register_command()).await;
            let _ = guild_id.create_command(&ctx.http, news_cmd::register_command()).await;
            info!("{} is connected. Guild commands registered instantly for testing.", ready.user.name);
        } else {
            // Fallback to global commands (takes up to 1 hour)
        let _ = Command::create_global_command(&ctx.http, ping_command()).await;
            let _ = Command::create_global_command(&ctx.http, fundamentals_cmd::register_command(StatementType::IncomeStatement)).await;
            let _ = Command::create_global_command(&ctx.http, fundamentals_cmd::register_command(StatementType::BalanceSheet)).await;
            let _ = Command::create_global_command(&ctx.http, fundamentals_cmd::register_command(StatementType::CashFlow)).await;
            let _ = Command::create_global_command(&ctx.http, quotes_cmd::register_command()).await;
            let _ = Command::create_global_command(&ctx.http, holders_cmd::register_command()).await;
            let _ = Command::create_global_command(&ctx.http, news_cmd::register_command()).await;
            info!("{} is connected. Global commands registered (may take up to 1 hour).", ready.user.name);
        }

        // Start SPY options pinger (every 15 minutes) if configured
        options_data::spawn_options_pinger(ctx.http.clone(), self.finance.clone());
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
                        .edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().content(content))
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

    let finance = Arc::new(FinanceService::new(None)?);

    let mut client = Client::builder(token, intents)
        .application_id(app_id)
        .event_handler(Handler { app_id, finance })
        .await?;

    if let Err(why) = client.start().await {
        eprintln!("Client error: {why}");
    }

    Ok(())
}

fn ping_command() -> CreateCommand {
    CreateCommand::new("ping").description("Simple ping command")
}