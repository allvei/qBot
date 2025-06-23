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
        event::VoiceStateUpdateEvent,
    },
    prelude::*,
};
use std::{env, sync::Arc};
use tracing::{error, info};

use database::Database;
use handlers::{queue, session_handler, admin};


struct Handler {
    database: Arc<Database>,
}

/// Handler for Discord events with database access
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, pl: Ready) {
        info!("{} is connected!", pl.user.name);
        
        // Register slash commands globally or for specific guild
        let commands = vec![
            CreateCommand::new("queue")
                .description("Join or leave the queue")
                .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "join", "Join the queue"))
                .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "leave", "Leave the queue"))
                .add_option(CreateCommandOption::new(CommandOptionType::SubCommand, "status", "Check queue status")),
        
            CreateCommand::new("shuffle")
                .description("Generate teams from queue"),
        
            CreateCommand::new("accept")
                .description("Accept/confirm generated teams")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "session_id", "Session ID to accept (optional)")
                        .required(false)
                ),
        
            CreateCommand::new("end")
                .description("End a session")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "session_id", "Session ID to end (optional)")
                        .required(false)
                ),
        
            CreateCommand::new("buffer")
                .description("Buffer a player")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "user", "User to buffer")
                        .required(true)
                ),
        
            CreateCommand::new("config")
                .description("View or set bot configuration")
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
            if config.guild_id != 0 {
                let guild_id = GuildId::new(config.guild_id);
                if let Err(why) = guild_id.set_commands(&ctx.http, commands).await {
                    error!("Cannot register slash commands for guild: {}", why);
                } else {
                    info!("Registered slash commands for guild {}", guild_id);
                }
                return;
            }
        }

        // Fallback to global registration
        if let Err(why) = serenity::model::application::Command::set_global_commands(&ctx.http, commands).await {
            error!("Cannot register global slash commands: {}", why);
        } else {
            info!("Registered global slash commands");
        }
    }

    /// Handles interaction create events
    async fn interaction_create(&self, ctx: Context, pl: Interaction) {
        if let Interaction::Command(command) = pl {
            let result = match command.data.name.as_str() {
                "queue" => {
                    if let Some(subcommand) = command.data.options.first() {
                        match subcommand.name.as_str() {
                            "join" | "leave" => queue::handle_queue_command(&ctx, &command, self.database.clone()).await,
                            "status" => queue::handle_queue_status_command(&ctx, &command, self.database.clone()).await,
                            _ => Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                "shuffle" => {
                    session_handler::handle_shuffle_command(&ctx, &command, self.database.clone()).await
                }
                "accept" => {
                    let session_id = command.data.options.iter()
                        .find(|opt| opt.name == "session_id")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    session_handler::handle_accept_command(&ctx, &command, self.database.clone(), session_id).await
                }
                "end" => {
                    let session_id = command.data.options.iter()
                        .find(|opt| opt.name == "session_id")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    session_handler::handle_end_command(&ctx, &command, self.database.clone(), session_id).await
                }
                "buffer" => {
                    if let Some(user_option) = command.data.options.first() {
                        if let Some(user_id) = user_option.value.as_str() {
                            admin::handle_buffer_command(&ctx, &command, self.database.clone(), user_id.to_string()).await
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                "config" => {
                    let key = command.data.options.iter()
                        .find(|opt| opt.name == "key")
                        .and_then(|opt| opt.value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let value = command.data.options.iter()
                        .find(|opt| opt.name == "value")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    
                    admin::handle_config_command(&ctx, &command, self.database.clone(), key, value).await
                }
                _ => {
                    let response = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("❌ Unknown command")
                            .ephemeral(true)
                    );
                    command.create_response(&ctx.http, response).await.map_err(|e| e.into())
                }
            };

            if let Err(e) = result {
                error!("Error handling command '{}': {}", command.data.name, e);
                
                // Try to respond with an error message if we haven't responded yet
                let error_response = CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("❌ An error occurred while processing your command")
                        .ephemeral(true)
                );
                
                if let Err(response_err) = command.create_response(&ctx.http, error_response).await {
                    error!("Failed to send error response: {}", response_err);
                }
            }
        }
    }
    
    async fn voice_state_update(&self, ctx: Context, pl: VoiceStateUpdateEvent) {
        let user_id = pl.user_id;
        let channel_id = pl.channel_id;

        
    }
}

/// Main entry point for the PUG bot application.
/// Sets up tracing, loads environment variables, initializes the database connection,
/// configures the Discord client with necessary intents, and starts the bot.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenvy::dotenv().ok();

    let token = env::var("DISCORD_TOKEN")
        .expect("Expected a Discord token in the environment");
    
    let db_file = env::var("DATABASE_URL").unwrap_or_else(|_| "./pfpug.db".to_string());
    let database_url = format!("sqlite:{}", db_file);

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
