mod automation;
mod finance;
mod models;

use automation::test;
use serenity::all::{
    ChannelId, Client, Context, CreateAllowedMentions, CreateMessage, EventHandler, GatewayIntents,
    GuildId, Interaction, Message, Ready,
};
use serenity::async_trait;
use std::env;

fn read_env_var(key: &str) -> Result<String, Box<dyn std::error::Error>> {
    let raw = env::var(key)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{key} is set but empty").into());
    }
    Ok(trimmed.to_string())
}

fn read_env_u64(key: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let val = read_env_var(key)?;
    val.parse::<u64>()
        .map_err(|e| format!("{key} must be a numeric ID: {e}").into())
}

struct Handler {
    source_channel: ChannelId,
    target_channel: ChannelId,
    register_guild: Option<GuildId>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _data: Ready) {
        if let Some(guild_id) = self.register_guild {
            if let Err(err) = test::register_commands(&ctx.http, guild_id).await {
                tracing::error!(?err, "failed to register slash commands");
            }
        } else {
            tracing::warn!("No REGISTER_GUILD_ID set; slash command not registered");
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if msg.channel_id != self.source_channel {
            return;
        }

        let processed = format!("[{}] {}", msg.author.name, msg.content);

        let allowed = CreateAllowedMentions::new()
            .everyone(false) // block @everyone/@here
            .all_users(false) // block @user
            .all_roles(false) // block @role
            .empty_users()
            .empty_roles();

        if let Err(err) = self
            .target_channel
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(processed)
                    .allowed_mentions(allowed),
            )
            .await
        {
            tracing::error!(?err, "failed to relay message");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(err) =
            test::handle_interaction(&ctx, &interaction, self.source_channel, self.target_channel)
                .await
        {
            tracing::error!(?err, "failed to handle interaction");
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .compact()
        .init();

    let token = read_env_var("DISCORD_TOKEN")?;
    let source_channel = read_env_u64("SOURCE_CHANNEL_ID")?;
    let target_channel = read_env_u64("TARGET_CHANNEL_ID")?;
    let register_guild = env::var("REGISTER_GUILD_ID")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(GuildId::new);

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler {
        source_channel: ChannelId::new(source_channel),
        target_channel: ChannelId::new(target_channel),
        register_guild,
    };

    let mut client = Client::builder(token, intents)
        .event_handler(handler)
        .await?;

    client.start().await?;
    Ok(())
}
