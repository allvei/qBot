// CHECK ME
mod database;
mod handlers;
mod models;

use std::{
    env,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use serenity::{
    all::{ButtonStyle, CreateActionRow, CreateButton},
    async_trait,
    builder::{
        CreateCommand, CreateCommandOption, CreateEmbed, CreateEmbedFooter,
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    },
    model::{
        application::{CommandOptionType, Interaction},
        gateway::Ready,
        id::ChannelId,
        voice::VoiceState,
    },
    prelude::*,
};
use tracing::{error, info};

use database::Database;
use handlers::{admin, queue, session};
use models::{
    command::CommandContext,
    config::{ID_BLU, ID_CHAT, ID_DASHBOARD, ID_QUEUE, ID_RED},
    session::{Group, Manager, SessionPlayer, SessionStatus},
};

struct Handler {
    database: Arc<Database>,
    guild:    Arc<Mutex<Manager>>,
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
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "join",
                    "Join the queue",
                ))
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "leave",
                    "Leave the queue",
                ))
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "status",
                    "Check queue status",
                )),
            CreateCommand::new("shuffle").description("Generate teams from queue"),
            CreateCommand::new("accept")
                .description("Accept/confirm generated teams")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "session_id",
                        "Session ID to accept (optional)",
                    )
                    .required(false),
                ),
            CreateCommand::new("end")
                .description("End a session")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "session_id",
                        "Session ID to end (optional)",
                    )
                    .required(false),
                ),
            CreateCommand::new("buffer")
                .description("Buffer a player")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "user", "User to buffer")
                        .required(true),
                ),
            CreateCommand::new("config")
                .description("View or set bot configuration")
                .add_option(
                    CreateCommandOption::new(CommandOptionType::String, "key", "Configuration key")
                        .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "value",
                        "Configuration value",
                    )
                    .required(false),
                ),
        ];

        if let Err(why) =
            serenity::model::application::Command::set_global_commands(&ctx.http, commands).await
        {
            error!("Cannot register global slash commands: {}", why);
        } else {
            info!("Registered global slash commands");
        }
    }

    /// Handles interaction create events
    async fn interaction_create(&self, ctx: Context, pl: Interaction) {
        if let Interaction::Command(command) = pl {
            let user_name = &command.user.name;
            let cmd_ctx = CommandContext {
                ctx:   &ctx,
                intax: &command,
                db:    self.database.clone(),
            };

            let result = match command.data.name.as_str() {
                "queue" => {
                    if let Some(subcommand) = command.data.options.first() {
                        info!("{}: /{} {}", user_name, command.data.name, subcommand.name);
                        match subcommand.name.as_str() {
                            "join" | "leave" => queue::handle_queue_command(&cmd_ctx).await,
                            "status" => queue::handle_queue_status_command(&cmd_ctx).await,
                            _ => Ok(()),
                        }
                    } else {
                        Ok(())
                    }
                }
                "shuffle" => {
                    info!("{}: /{}", user_name, command.data.name);
                    session::handle_shuffle_command(&cmd_ctx).await
                }
                "accept" => {
                    info!("{}: /{}", user_name, command.data.name);
                    let session_id = command
                        .data
                        .options
                        .iter()
                        .find(|opt| opt.name == "session_id")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    session::handle_accept_command(&cmd_ctx, &session_id).await
                }
                "end" => {
                    info!("{}: /{}", user_name, command.data.name);
                    let session_id = command
                        .data
                        .options
                        .iter()
                        .find(|opt| opt.name == "session_id")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());
                    session::handle_end_command(&cmd_ctx, session_id).await
                }
                "buffer" => {
                    info!("{}: /{}", user_name, command.data.name);
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
                    info!("{}: /{}", user_name, command.data.name);
                    let key = command
                        .data
                        .options
                        .iter()
                        .find(|opt| opt.name == "key")
                        .and_then(|opt| opt.value.as_str())
                        .unwrap_or("")
                        .to_string();
                    let value = command
                        .data
                        .options
                        .iter()
                        .find(|opt| opt.name == "value")
                        .and_then(|opt| opt.value.as_str())
                        .map(|s| s.to_string());

                    admin::handle_config_command(&cmd_ctx, key, value).await
                }
                _ => {
                    let response = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Unknown command")
                            .ephemeral(true),
                    );
                    command
                        .create_response(&ctx.http, response)
                        .await
                        .map_err(|e| e.into())
                }
            };

            if let Err(e) = result {
                error!("Error handling command '{}': {}", command.data.name, e);

                // Try to respond with an error message if we haven't responded yet
                let error_response = CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("An error occurred while processing your command")
                        .ephemeral(true),
                );

                if let Err(response_err) = command.create_response(&ctx.http, error_response).await
                {
                    error!("Failed to send error response: {}", response_err);
                }
            }
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let user = new.user_id;
        let channel = new.channel_id;
        let old_channel = old.map(|s| s.channel_id);

        if channel.is_none() && old_channel.is_some() {
            info!("User {} left voice channel", user);
            let session = Session::get_session_by_channel(old_channel.unwrap().get()).await;
            if let Some(session) = session {
                session.remove_player(user.get()).await;
            }
            return;
        }

        

        // Handle user joining a queue channel
        if let Some(new_channel_id) = channel {
            info!(
                "User {} joined voice channel {}",
                user,
                new_channel_id.get()
            );
            // First, get the player data without holding the lock
            info!("Fetching player data for user {}", user.get());
            let player = match self.database.get_user(user.get()).await {
                Ok(user) => user,
                Err(_) => match self.database.new_user(user.get()).await {
                    Ok(new_user) => new_user,
                    Err(e) => {
                        error!("Failed to create new user: {}", e);
                        return;
                    }
                },
            };

            // We'll store notification information to use after the mutex is released
            let mut dashboard_channel = None;
            let mut session_info = None;

            // Scope for the mutex lock
            {
                let mut guild = self.guild.lock().unwrap();

                // Check if the new channel is a queue channel in any group
                for server in guild.servers.iter_mut() {
                    for group in server.groups.iter_mut() {
                        if group.queue == new_channel_id.get() {
                            // User joined queue channel
                            info!("User {} joined queue channel {}", user, new_channel_id);

                            // Ensure there is at least one active session
                            if group.session.is_empty() {
                                info!("No active session, creating one");
                                group.create_session();
                            }

                            // Get the current session (last in the vector)
                            if let Some(session) = group.session.last_mut() {
                                // Skip if user is already in the session
                                if session
                                    .pool
                                    .iter()
                                    .any(|sp| sp.player.i_discord == user.get())
                                {
                                    info!("User {} is already in session", user);
                                    break;
                                }

                                // Check if the session has space and is accepting players
                                if session.pool.len() >= 12 {
                                    info!("Session is full, cannot add more players");
                                    break;
                                }

                                if matches!(session.status, SessionStatus::Live) {
                                    info!("Session is already playing, cannot add more players");
                                    break;
                                }

                                // Add player to session
                                let _session_player = SessionPlayer::new(player.clone());
                                session.add_player(&player);
                                info!(
                                    "Added user {} to session, now has {} players",
                                    user,
                                    session.pool.len()
                                );

                                // If we have enough players, update session status
                                if session.pool.len() >= 8
                                    && !matches!(session.status, SessionStatus::Hot)
                                {
                                    session.status = SessionStatus::Hot;
                                    info!("Session is now HOT with {} players", session.pool.len());

                                    // Store notification info to use after releasing the lock
                                    dashboard_channel = Some(group.dashboard);
                                    session_info = Some((session.id, session.pool.len()));
                                }
                            }
                            break; // We found the group, exit the loop
                        }
                    }
                }
            } // Mutex guard is released here

            // Now perform async operations with the data we collected
            if let (Some(dashboard_id), Some((session_id, player_count))) =
                (dashboard_channel, session_info)
            {
                info!(
                    "Sending session ready notification: dashboard={}, session_id={}, players={}",
                    dashboard_id, session_id, player_count
                );
                let channel = ChannelId::new(dashboard_id);

                // Create an embed message for the session ready notification
                let embed = CreateEmbed::new()
                    .title("SESSION READY!")
                    .description(format!(
                        "A match with ID: {} is ready to start with {} players!",
                        session_id, player_count
                    ))
                    .color(0xffd43b)
                    .footer(CreateEmbedFooter::new("Awaiting team generation..."));

                // Create buttons for actions
                let components = vec![CreateActionRow::Buttons(vec![CreateButton::new(format!(
                    "shuffle:{}",
                    session_id
                ))
                .style(ButtonStyle::Primary)
                .label("Shuffle Teams")
                .emoji('🎲')])];

                // Send the message with both embed and components
                if let Ok(msg) = channel
                    .send_message(
                        &ctx.http,
                        CreateMessage::new().embed(embed).components(components),
                    )
                    .await
                {
                    // Add a reaction to the message
                    if let Err(e) = msg.react(&ctx.http, '✅').await {
                        error!("Failed to add reaction: {}", e);
                    }
                } else {
                    error!("Failed to send session ready notification");
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
        // Get count from the latest session if available
        let player_count = if let Some(session) = group.session.last() {
            session.pool.len()
        } else {
            0
        };
        let players_to_mention = if player_count >= 8 { 8 } else { player_count };

        // Access players in the latest session if available
        if let Some(session) = group.session.last() {
            for player in &session.pool[..players_to_mention] {
                player_mentions.push(format!("<@{}>", player.player.i_discord));
            }
        }

        let embed = CreateEmbed::new()
            .title("SESSION READY!")
            .description(format!(
                "**8 players in queue channel!**\n\n{}\n\nUse `/shuffle` to generate teams.",
                player_mentions.join(" ")
            ))
            .color(0xffd43b)
            .footer(CreateEmbedFooter::new("Awaiting team generation..."));

        // Send the message to the dashboard channel
        if let Err(e) = dashboard_channel
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await
        {
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

    let token = env::var("DISCORD_TOKEN").expect("Expected a Discord token in the environment");

    let db_file = env::var("DATABASE_URL").unwrap_or_else(|_| "./pfpug.db".to_string());
    let database_url = format!("sqlite:{}", db_file);

    // Initialize database
    info!("Connecting to database: {}", database_url);
    let db = Arc::new(Database::new(&database_url).await?);

    // Configure the client with the framework and intents
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILDS;

    // Define TypeMapKey for Guild
    struct GuildKey;
    impl TypeMapKey for GuildKey {
        type Value = Arc<Mutex<Manager>>;
    }

    info!("Starting bot...");

    // Check if tables exist, create them if not
    db.init_db().await?;

    // Hardcoded for testing
    let group = db
        .new_group(ID_DASHBOARD, ID_CHAT, ID_QUEUE, ID_RED, ID_BLU, 8)
        .await?;

    // Create a manager
    let manager = Arc::new(Mutex::new(Manager::default()));

    // Create client
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler {
            database: db.clone(),
            guild:    manager.clone(),
        })
        .await
        .expect("Error creating client");

    // Set the manager in the client data for global access
    client
        .data
        .write()
        .await
        .insert::<GuildKey>(manager.clone());

    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }

    Ok(())
}
