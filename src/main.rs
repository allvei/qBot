// CHECK ME
mod database;
mod handlers;
mod models;

use std::{env, sync::{Arc, Mutex}};

use anyhow::Result;
use serenity::{
    all::GuildId,
    async_trait,
    builder::{
        CreateCommand,
        CreateCommandOption,
        CreateEmbed,
        CreateEmbedFooter,
        CreateInteractionResponse,
        CreateInteractionResponseMessage,
        CreateMessage,
    },
    model::{
        application::{CommandOptionType, Interaction},
        gateway::Ready,
        guild::Guild,
        id::ChannelId,
        voice::VoiceState,
    },
    prelude::*,
};
use tracing::{error, info};

use database::Database;
use handlers::{queue, session, admin};
use models::{session::{PugManager, Group}, command::CommandContext};


struct Handler {
    database:   Arc<Database>,
    pugmanager: Arc<Mutex<PugManager>>,
}

/// Handler for Discord events with database access
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let guilds = ctx.cache.guilds().len();
        println!("Guilds in the Cache: {}", guilds);

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

        if let Err(why) = serenity::model::application::Command::set_global_commands(&ctx.http, commands).await {
            error!("Cannot register global slash commands: {}", why);
        } else {
            info!("Registered global slash commands");
        }
    }

    /// Handles interaction create events
    async fn interaction_create(&self, ctx: Context, pl: Interaction) {
        if let Interaction::Command(command) = pl {
            let cmd_ctx = CommandContext {
                ctx: &ctx,
                intax: &command,
                db: self.database.clone(),
            };

            let result = match command.data.name.as_str() {
                "queue" => {
                    if let Some(subcommand) = command.data.options.first() {
                        match subcommand.name.as_str() {
                            "join" | "leave" => queue::handle_queue_command(&cmd_ctx).await,
                            "status" => queue::handle_queue_status_command(&cmd_ctx).await,
                            _ => Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                "shuffle" => {
                    session::handle_shuffle_command(&cmd_ctx).await
                }
                "accept" => {
                    let session_id = command.data.options.iter()
                        .find(|opt| opt.name == "session_id")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    session::handle_accept_command(&cmd_ctx, session_id).await
                }
                "end" => {
                    let session_id = command.data.options.iter()
                        .find(|opt| opt.name == "session_id")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    session::handle_end_command(&cmd_ctx, session_id).await
                }
                "buffer" => {
                    if let Some(user_option) = command.data.options.first() {
                        if let Some(user_id) = user_option.value.as_str() {
                            admin::handle_buffer_command(&cmd_ctx, user_id.to_string()).await
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
                    
                    admin::handle_config_command(&cmd_ctx, key, value).await
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
    
    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let user_id = new.user_id;
        let old_channel_id = old.and_then(|vs| vs.channel_id);
        let new_channel_id = new.channel_id;
        
        // Handle user leaving a queue channel
        if let Some(old_channel_id) = old_channel_id {
            let mut pugmanager = self.pugmanager.lock().unwrap();
            
            // Check all groups to find if this was a queue channel
            for group in pugmanager.groups_mut() {
                if group.queue == old_channel_id.get() {
                    // User left a queue channel - remove them from the session
                    if group.session.players.iter().any(|sp| sp.player.discord_id == user_id.get()) {
                        group.session.remove_member(user_id.get());
                        info!("User {} left queue channel and was removed from session", user_id);
                        info!("Session now has {} players", group.session.count());
                        
                        // Session notification when the queue reaches capacity
                        if group.session.count() < 8 && matches!(group.session.status, SessionStatus::Hot) {
                            group.session.status = SessionStatus::Idle;
                            info!("Session is now IDLE with {} players", group.session.count());
                        }
                    }
                    break;
                }
            }
            // Drop the lock before any potential async operations
        }
        
        // Handle user joining a queue channel
        if let Some(new_channel_id) = new_channel_id {
            // First, get the player data without holding the lock
            if let Ok(player) = self.database.get_or_create_player(user_id.get()).await {
                // Variables that will be used AFTER the mutex is released
                let mut notify_dashboard: Option<(u64, Vec<u16>, usize)> = None;

                {
                    // Limit the scope of the lock so that it is released **before** any `.await` points
                    let mut pugmanager = self.pugmanager.lock().unwrap();

                    // Check if the new channel is a queue channel in any group
                    for group in pugmanager.groups_mut() {
                        if group.queue == new_channel_id.get() {
                            // User joined queue channel
                            info!("User {} joined queue channel {}", user_id, new_channel_id);

                            // Check if player is already in session
                            let is_in_session = group.session.players.iter()
                                .any(|sp| sp.player.discord_id == user_id.get());

                            if !is_in_session {
                                // Add player to session
                                let session_player = SessionPlayer::new(player.clone());
                                group.session.add_member(session_player);
                                info!("Added user {} to session, now has {} players", user_id, group.session.count());

                                // If we have enough players, update session status
                                if group.session.count() >= 8 && !matches!(group.session.status, SessionStatus::Hot) {
                                    group.session.status = SessionStatus::Hot;
                                    info!("Session is now HOT with {} players", group.session.count());

                                    // Collect information required for the async notification
                                    notify_dashboard = Some((
                                        group.dashboard,
                                        group.session.id.clone(),
                                        group.session.count(),
                                    ));
                                }
                            }
                            break; // We found the group, exit the loop
                        }
                    }
                    // `pugmanager` lock is released here when it goes out of scope
                }

                // After releasing the lock, perform any async operations if needed
                if let Some((dashboard_channel_id, session_id_parts, session_count)) = notify_dashboard {
                    let channel = ChannelId::new(dashboard_channel_id);
                    if let Ok(msg) = channel
                        .send_message(
                            &ctx.http,
                            CreateMessage::new().embed(
                                CreateEmbed::new()
                                    .title("🎮 Session Ready!")
                                    .description(format!(
                                        "Session {} is ready with {} players!",
                                        session_id_parts
                                            .iter()
                                            .map(|&id| id.to_string())
                                            .collect::<Vec<_>>()
                                            .join("-"),
                                        session_count
                                    ))
                                    .color(0x00ff00)
                                    .footer(CreateEmbedFooter::new("React with ✅ to accept")),
                            ),
                        )
                        .await
                    {
                        if let Err(e) = msg.react(&ctx.http, '✅').await {
                            error!("Failed to add reaction: {}", e);
                        }
                    }
                }
            }
        }
    }
}

impl Handler {
    /// Sends a notification to the dashboard channel when a session is ready
    async fn send_session_ready_notification(&self, ctx: &Context, group: &Group) {
        let dashboard_channel = ChannelId::new(group.dashboard);
        
        // Ensure there are at least 8 players before slicing
        let mut player_mentions = Vec::new();
        let player_count = group.session.count();
        let players_to_mention = if player_count >= 8 { 8 } else { player_count };
        
        for player in &group.session.players[..players_to_mention] {
            player_mentions.push(format!("<@{}>", player.player.discord_id));
        }
        
        let embed = CreateEmbed::new()
            .title("🔔 SESSION READY!")
            .description(format!(
                "**8 players in queue channel!**\n\n{}\n\nUse `/shuffle` to generate teams.",
                player_mentions.join(" ")
            ))
            .color(0xffd43b)
            .footer(CreateEmbedFooter::new("Awaiting team generation..."));
        
        // Send the message to the dashboard channel
        if let Err(e) = dashboard_channel.send_message(
            &ctx.http, 
            CreateMessage::new().embed(embed)
        ).await {
            error!("Failed to send session ready notification: {:?}", e);
        } else {
            info!("Sent session ready notification to dashboard channel");
        }
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

    // Define TypeMapKey for PugManager
    struct PugManagerKey;
    impl TypeMapKey for PugManagerKey {
        type Value = Arc<Mutex<PugManager>>;
    }
    
    info!("Starting bot...");
    
    // Hardcoded for testing
    let group = Group::new(
        ID_DASHBOARD,
        ID_CHAT,
        ID_QUEUE,
        ID_RED,
        ID_BLU,
    );
    
    // Create PugManager once
    let pugmanager = Arc::new(Mutex::new(PugManager::new(group)));
    
    // Create client
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler { 
            database: database.clone(),
            pugmanager: pugmanager.clone(),
        })
        .await
        .expect("Error creating client");
    
    // Set the pugmanager in the client data for global access
    client.data.write().await.insert::<PugManagerKey>(pugmanager.clone());
    
    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }

    Ok(())
}
