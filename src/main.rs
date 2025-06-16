mod models;
mod database;
mod handlers;

use anyhow::Result;
use serenity::{
    async_trait,
    builder::{CreateCommand, CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage},
    model::{
        application::{CommandOptionType, Interaction},
        gateway::Ready,
        id::GuildId,
    },
    prelude::*,
};
use std::{env, sync::Arc};
use tracing::{error, info};

use database::Database;
use handlers::*;

struct Handler {
    database: Arc<Database>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
        
        // Register slash commands globally or for specific guild
        let commands = vec![
            CreateCommand::new("queue")
                .description("Join or leave the PUG queue")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "action", "Action to perform")
                        .required(true)
                        .add_string_choice("join", "join")
                        .add_string_choice("leave", "leave")
                        .add_string_choice("status", "status")
                ),
            CreateCommand::new("autogen")
                .description("Generate teams automatically (Runner only)"),
            CreateCommand::new("regen")
                .description("Regenerate teams (Runner only)")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "match_id", "Match ID to regenerate")
                        .required(false)
                ),
            CreateCommand::new("confirm")
                .description("Confirm generated teams (Runner only)")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "match_id", "Match ID to confirm")
                        .required(false)
                ),
            CreateCommand::new("end")
                .description("End a match (Runner only)")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "match_id", "Match ID to end")
                        .required(true)
                ),
            CreateCommand::new("bench")
                .description("Bench a player (Admin only)")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::User, "user", "User to bench")
                        .required(true)
                ),
            CreateCommand::new("config")
                .description("View or modify bot configuration (Admin only)")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "key", "Configuration key")
                        .required(false)
                )
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "value", "Configuration value")
                        .required(false)
                ),
        ];

        // Try to get guild ID from config first, otherwise register globally
        let config_result = self.database.get_config().await;
        if let Ok(config) = config_result {
            if !config.guild_id.is_empty() {
                if let Ok(guild_id) = config.guild_id.parse::<u64>() {
                    let guild_id = GuildId::new(guild_id);
                    if let Err(why) = guild_id.set_commands(&ctx.http, commands).await {
                        error!("Cannot register slash commands for guild: {}", why);
                    } else {
                        info!("Registered slash commands for guild {}", guild_id);
                    }
                    return;
                }
            }
        }

        // Fallback to global registration
        if let Err(why) = serenity::model::application::Command::set_global_commands(&ctx.http, commands).await {
            error!("Cannot register global slash commands: {}", why);
        } else {
            info!("Registered global slash commands");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            let result = match command.data.name.as_str() {
                "queue" => {
                    let action = command.data.options.first()
                        .and_then(|option| option.value.as_str())
                        .unwrap_or("status");
                    
                    match action {
                        "join" | "leave" => handle_queue_command(&ctx, &command, self.database.clone()).await,
                        "status" => handle_queue_status_command(&ctx, &command, self.database.clone()).await,
                        _ => {
                            let response = CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("❌ Invalid queue action")
                                    .ephemeral(true)
                            );
                            command.create_response(&ctx.http, response).await.map_err(|e| e.into())
                        }
                    }
                },
                "autogen" => handle_autogen_command(&ctx, &command, self.database.clone()).await,
                "regen" => {
                    if let Some(cmd_option) = command.data.options.first() {
                        let _match_id = cmd_option.value.as_str()
                            .and_then(|s| s.parse::<i64>().ok());
                    }
                    handle_autogen_command(&ctx, &command, self.database.clone()).await // For now, regen = autogen
                },
                "confirm" => {
                    let match_id = command.data.options.first()
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    handle_confirm_command(&ctx, &command, self.database.clone(), match_id).await
                },
                "end" => {
                    let match_id = command.data.options.first()
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    handle_end_command(&ctx, &command, self.database.clone(), match_id).await
                },
                "bench" => {
                    let user_mention = command.data.options.first()
                        .and_then(|option| option.value.as_str())
                        .unwrap_or("")
                        .to_string();
                    handle_bench_command(&ctx, &command, self.database.clone(), user_mention).await
                },
                "config" => {
                    let key = command.data.options.get(0)
                        .and_then(|option| option.value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let value = command.data.options.get(1)
                        .and_then(|option| option.value.as_str())
                        .map(|s| s.to_string());
                    handle_config_command(&ctx, &command, self.database.clone(), key, value).await
                },
                _ => {
                    let response = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("❌ Unknown command")
                            .ephemeral(true)
                    );
                    command.create_response(&ctx.http, response).await.map_err(|e| e.into())
                }
            };

            if let Err(why) = result {
                error!("Cannot respond to slash command: {}", why);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenvy::dotenv().ok();

    let token = env::var("DISCORD_TOKEN")
        .expect("Expected a Discord token in the environment");
    
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:./pfpug.db".to_string());

    // Initialize database
    info!("Connecting to database: {}", database_url);
    let database = Arc::new(Database::new(&database_url).await?);

    // Configure the client with the framework and intents
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILDS;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler { database: database.clone() })
        .await
        .expect("Error creating client");

    info!("Starting bot...");

    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }

    Ok(())
}
