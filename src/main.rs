#![allow(non_snake_case)]

use std::{env, sync::Arc};

use anyhow::Result;
use dotenv::dotenv;
use serenity::all::{
    ApplicationId, Command, CreateAttachment, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage, EditAttachments, EditMessage, GatewayIntents,
    GuildId, Interaction, Message,
};
use serenity::{async_trait, model::gateway::Ready, prelude::*, Client};
use tracing::info;

use stacks_bot::models::StatementType;
use stacks_bot::service::automation::{earnings, options_data};
use stacks_bot::service::caching::RedisCache;
use stacks_bot::service::command::earnings as earnings_cmd;
use stacks_bot::service::command::fundamentals as fundamentals_cmd;
use stacks_bot::service::command::holders as holders_cmd;
use stacks_bot::service::command::mention as mention_cmd;
use stacks_bot::service::command::news as news_cmd;
use stacks_bot::service::command::quotes as quotes_cmd;
use stacks_bot::service::finance::FinanceService;

struct Handler {
    finance: Arc<FinanceService>,
    cache: Option<Arc<RedisCache>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        // Determine if we should use guild commands (dev) or global commands (prod)
        #[cfg(debug_assertions)]
        let use_guild_commands = true;
        #[cfg(not(debug_assertions))]
        let use_guild_commands = false;

        if use_guild_commands {
            // Development: Register guild commands (instant!) to multiple servers
            let guild_ids_str = env::var("GUILD_IDS")
                .unwrap_or_else(|_| env::var("GUILD_ID").unwrap_or_default());
            
            let guild_ids: Vec<GuildId> = guild_ids_str
                .split(',')
                .filter_map(|id| id.trim().parse::<u64>().ok())
                .map(GuildId::new)
                .collect();

            if !guild_ids.is_empty() {
                for guild_id in guild_ids.iter() {
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
                    info!("Guild commands registered for guild ID: {}", guild_id);
                }
                info!(
                    "{} is connected. [DEV MODE] Guild commands registered instantly for {} server(s).",
                    ready.user.name,
                    guild_ids.len()
                );
            } else {
                info!(
                    "{} is connected. [DEV MODE] No GUILD_IDS found, falling back to global commands.",
                    ready.user.name
                );
                register_global_commands(&ctx).await;
            }
        } else {
            // Production: Register global commands (takes up to 1 hour)
            register_global_commands(&ctx).await;
            info!(
                "{} is connected. [PRODUCTION MODE] Global commands registered (may take up to 1 hour).",
                ready.user.name
            );
        }

        // Start SPY options pinger (every 15 minutes) if configured
        options_data::spawn_options_pinger(ctx.http.clone(), self.finance.clone(), self.cache.clone());
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

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let bot_id = ctx.cache.current_user().id;
        let prefixes = [format!("<@{}>", bot_id), format!("<@!{}>", bot_id)];

        let content = msg.content.trim();
        let rest = match prefixes.iter().find_map(|p| content.strip_prefix(p)) {
            Some(r) => r.trim(),
            None => return, // ignore messages that don't start with a mention of the bot
        };

        if rest.is_empty() {
            let _ = msg.reply(&ctx.http, mention_cmd::help_text()).await;
            return;
        }

        match mention_cmd::handle(rest, &ctx.http, msg.channel_id, &self.finance).await {
            Ok(resp) => {
                // Send a placeholder message immediately, then edit with the real response.
                let mut placeholder = match msg
                    .channel_id
                    .send_message(&ctx.http, CreateMessage::new().content("Stacks-bot thinking…"))
                    .await
                {
                    Ok(m) => m,
                    Err(err) => {
                        let _ = msg.reply(&ctx.http, format!("❌ {}", err)).await;
                        return;
                    }
                };

                let mut edit = EditMessage::new().content(resp.content);
                if let Some(attachment) = resp.attachment {
                    let attachments = EditAttachments::new().add(attachment);
                    edit = edit.attachments(attachments);
                }

                if let Err(err) = placeholder.edit(&ctx.http, edit).await {
                    let _ = msg
                        .reply(&ctx.http, format!("❌ failed to edit message: {}", err))
                        .await;
                }
            }
            Err(err) => {
                let _ = msg.reply(&ctx.http, format!("❌ {}", err)).await;
            }
        }
    }
}

// Helper function to register all global commands
async fn register_global_commands(ctx: &Context) {
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
    let _ = Command::create_global_command(&ctx.http, holders_cmd::register_command()).await;
    let _ = Command::create_global_command(&ctx.http, news_cmd::register_command()).await;
    let _ = Command::create_global_command(&ctx.http, earnings_cmd::register_weekly_command())
        .await;
    let _ = Command::create_global_command(&ctx.http, earnings_cmd::register_daily_command())
        .await;
    let _ = Command::create_global_command(
        &ctx.http,
        earnings_cmd::register_after_daily_command(),
    )
    .await;
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let token = env::var("DISCORD_TOKEN")?;
    let app_id_raw: u64 = env::var("APPLICATION_ID")?.parse()?;
    let app_id: ApplicationId = app_id_raw.into();

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    info!("Initializing FinanceService...");
    let finance = Arc::new(FinanceService::new(None)?);

    info!("Initializing Redis cache (optional)...");
    let cache = match RedisCache::from_env().await {
        Ok(c) => {
            info!("Connected to Redis cache");
            Some(Arc::new(c))
        }
        Err(err) => {
            info!("Redis cache disabled: {err}");
            None
        }
    };

    info!("Starting Discord client...");
    let mut client = Client::builder(token, intents)
        .application_id(app_id)
        .event_handler(Handler {
            finance,
            cache,
        })
        .await?;

    if let Err(why) = client.start().await {
        eprintln!("Client error: {why}");
    }

    Ok(())
}

fn ping_command() -> CreateCommand {
    CreateCommand::new("ping").description("Simple ping command")
}
