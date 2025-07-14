//! # Main Module
//!
//! This is the entry point for the Discord bot application.
//! It sets up the Discord client, registers event handlers,
//! and initializes the database connection.

mod database;
mod discord;
mod error;
mod events;
mod handlers;
mod models;

use error::{AppError, AppResult};

use std::env;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton as CB};
use serenity::async_trait;
use serenity::builder::{
    CreateCommand as CC, CreateCommandOption as CCO, CreateEmbed as CE, CreateEmbedFooter as CEF, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateMessage as CM,
};
use serenity::model::application::{Command, CommandOptionType as COT, Interaction};
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::model::voice::VoiceState;
use serenity::prelude::*;
use tracing::{error, info};

use database::Database;
use handlers::{admin, queue, session};
use models::command::CommandContext;
use models::config::{ID_BLU, ID_CHAT, ID_DASHBOARD, ID_QUEUE, ID_RED};
use models::group::Group;
use models::manager::Manager;
use models::player::Player;
use models::session::SessionStatus;

use crate::models::Session;

fn cmd(
    name: impl Into<String>,
    desc: impl Into<String>,
) -> CC {
    CC::new(name.into()).description(desc.into())
}

pub trait CmdOp: Sized {
    fn op(
        self,
        name: impl Into<String>,
        desc: impl Into<String>,
        req: bool,
    ) -> Self;
}

impl CmdOp for CC {
    /// Adds an option to the command
    ///
    /// ### Arguments
    /// * `name`
    /// * `desc`
    /// * `req` - Is it required?
    ///
    /// ### Returns
    /// The command with the added option
    fn op(
        self,
        name: impl Into<String>,
        desc: impl Into<String>,
        req: bool,
    ) -> Self {
        self.add_option(CCO::new(COT::String, name, desc).required(req))
    }
}

struct Handler {
    database: Arc<Database>,
    guild_id: Arc<Mutex<Manager>>,
}

/// Handler for Discord events with database access
#[async_trait]
impl EventHandler for Handler {
    async fn ready(
        &self,
        ctx: Context,
        ready: Ready,
    ) {
        info!("{} is connected!", ready.user.name);

        let guild_count = ctx.cache.guilds().len();
        println!("Connected to {} guilds", guild_count);
        for guild in ctx.cache.guilds() {
            if let Some(guild_data) = ctx.cache.guild(guild) {
                println!("{}: {}", guild_data.name, guild_data.id);
            }
        }

        // Register slash commands globally or for specific guild
        let cmds = vec![
            cmd("join", "Join the queue"),
            cmd("leave", "Leave the queue"),
            cmd("status", "Check queue status"),
            cmd("shuffle", "Generate teams from queue"),
            cmd("accept", "Accept/confirm generated teams").op("id", "Session ID to accept (optional)", false),
            cmd("end", "End a session").op("id", "Session ID to end (optional)", false),
            cmd("buffer", "Buffer a player").op("user", "User to buffer", true),
            cmd("config", "View or set bot configuration")
                .op("key", "Configuration key", false)
                .op("value", "Configuration value", false),
        ];

        if let Err(why) = Command::set_global_commands(&ctx.http, cmds).await {
            error!("Failed to register commands: {}", why);
        }
    }

    /// Handles interaction create events
    async fn interaction_create(
        &self,
        ctx: Context,
        pl: Interaction,
    ) {
        if let Interaction::Command(command) = pl {
            let user_name = &command.user.name;
            let cmd_ctx = CommandContext {
                ctx:   &ctx,
                intax: &command,
                db:    self.database.clone(),
            };
            let cd = &command.data;
            let cdo = &cd.options;

            let info = || {
                info!("{}: /{}", user_name, command.data.name);
            };

            let get_arg = |name: &str| -> Option<String> { command.data.options.iter().find(|opt| opt.name == name).and_then(|opt| opt.value.as_str()).map(|s| s.to_string()) };

            let result = match cd.name.as_str() {
                "join" | "leave" => {
                    info();
                    queue::queue(&cmd_ctx).await
                }
                "status" => {
                    info();
                    queue::status(&cmd_ctx).await
                }
                "shuffle" => {
                    info();
                    session::shuffle(&cmd_ctx).await
                }
                "accept" => {
                    info();
                    session::accept(&cmd_ctx, &get_arg("id")).await
                }
                "end" => {
                    info();
                    session::end(&cmd_ctx, get_arg("id")).await
                }
                "buffer" => {
                    info();
                    if let Some(user_option) = cdo.first() {
                        if let Some(user_id) = user_option.value.as_str() {
                            let _ = admin::buffer(&cmd_ctx, user_id.to_string()).await;
                        }
                    }
                    Ok(())
                }
                "config" => {
                    info();
                    let key = get_arg("key").unwrap_or("".to_string());
                    let value = get_arg("value").map(|s| s.to_string());

                    admin::config(&cmd_ctx, key, value).await
                }
                _ => {
                    let response = CIR::Message(CIRM::new().content("Unknown command").ephemeral(true));
                    command.create_response(&ctx.http, response).await.map_err(|e| e.into())
                }
            };

            if let Err(e) = result {
                error!("Error handling command '{}': {}", command.data.name, e);

                // Try to respond with an error message if we haven't responded yet
                let error_response = CIR::Message(CIRM::new().content("An error occurred while processing your command").ephemeral(true));

                if let Err(response_err) = command.create_response(&ctx.http, error_response).await {
                    error!("Failed to send error response: {}", response_err);
                }
            }
        }
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        let guild_id = new.guild_id;
        let channel = new.channel_id;
        let user = new.user_id;
        let old_channel = old.map(|s| s.channel_id);

        {
            let mut mgr = self.guild_id.lock().unwrap();
            for server in mgr.servers.iter_mut() {
                if let Some(group) = server.find_group_by_queue_channel_mut(channel.expect("Channel ID expected").get()) {
                    if let Some(session) = group.session.iter_mut().find(|s| s.status == SessionStatus::Idle) {
                        info!("User {} left voice channel", user);
                        if let Err(e) = session.remove_player(user.get()) {
                            info!("Error removing player from session: {}", e);
                        }
                        return;
                    }
                }
            }
        }

        // Handle user joining a queue channel
        if let Some(new_channel) = channel {
            info!("User {} joined voice channel {}", user, new_channel.get());
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
                let mut guild = self.guild_id.lock().unwrap();

                // Check if the new channel is a queue channel in any group
                for server in guild.servers.iter_mut() {
                    for group in server.groups.iter_mut() {
                        if group.queue == new_channel.get() {
                            // User joined queue channel
                            info!("User {} joined queue channel {}", user, new_channel);

                            // Ensure there is at least one active session
                            if group.session.is_empty() {
                                info!("No active session, creating one");
                                group.create_session();
                            }

                            // Get the current session (last in the vector)
                            if let Some(session) = group.session.last_mut() {
                                // Skip if user is already in the session
                                if session.pool.iter().any(|sp| sp.discord_id == user.get()) {
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

                                // Add player to session with backreferences
                                match session.add_player(&player) {
                                    Ok(_) => {
                                        info!("Added user {} to session, now has {} players", user, session.pool.len());

                                        // If we have enough players, update session status
                                        if session.pool.len() >= 8 && !matches!(session.status, SessionStatus::Hot) {
                                            session.status = SessionStatus::Hot;
                                            info!("Session is now HOT with {} players", session.pool.len());

                                            // Store notification info to use after releasing the lock
                                            dashboard_channel = Some(group.dashboard);
                                            session_info = Some((session.id, session.pool.len()));
                                        }
                                    }
                                    Err(e) => {
                                        info!("Failed to add user {} to session: {}", user, e);
                                    }
                                }
                            }
                            break; // We found the group, exit the loop
                        }
                    }
                }
            } // Mutex guard is released here

            // Now perform async operations with the data we collected
            if let (Some(dashboard_id), Some((session_id, player_count))) = (dashboard_channel, session_info) {
                info!("Sending session ready notification: dashboard={}, session_id={}, players={}", dashboard_id, session_id, player_count);
                let channel = ChannelId::new(dashboard_id);

                // Create an embed message for the session ready notification
                let embed = CE::new()
                    .title("SESSION READY!")
                    .description(format!("A match with ID: {} is ready to start with {} players!", session_id, player_count))
                    .footer(CEF::new("Awaiting team generation..."));

                // Create buttons for actions
                let components = vec![CreateActionRow::Buttons(vec![CB::new(format!("shuffle:{}", session_id))
                    .style(ButtonStyle::Primary)
                    .label("Shuffle Teams")
                    .emoji('🎲')])];

                // Send the message with both embed and components
                if let Ok(msg) = channel.send_message(&ctx.http, CM::new().embed(embed).components(components)).await {
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
    async fn notify(
        &self,
        ctx: &Context,
        group: &Group,
    ) {
        let dashboard_channel = ChannelId::new(group.dashboard);

        // Ensure there are at least 8 players before slicing
        let mut player_mentions = Vec::new();
        // Get count from the latest session if available
        let player_count = if let Some(session) = group.session.last() { session.pool.len() } else { 0 };
        let players_to_mention = if player_count >= 8 { 8 } else { player_count };

        // Access players in the latest session if available
        if let Some(session) = group.session.last() {
            for player in &session.pool[..players_to_mention] {
                player_mentions.push(format!("<@{}>", player.discord_id));
            }
        }

        let embed = CE::new()
            .title("SESSION READY!")
            .description(format!(
                // TODO: format according to group quota
                "**8 players in queue channel!**\n\n{}\n\nUse `/shuffle` to generate teams.",
                player_mentions.join(" ")
            ))
            .footer(CEF::new("Awaiting team generation..."));

        // Send the message to the dashboard channel
        if let Err(e) = dashboard_channel.send_message(&ctx.http, CM::new().embed(embed)).await {
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
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILDS;

    // Define TypeMapKey for Guild
    struct GuildKey;
    impl TypeMapKey for GuildKey {
        type Value = Arc<Mutex<Manager>>;
    }

    // Check if tables exist, create them if not
    db.init_db().await?;

    info!("Starting bot...");

    // Create a manager
    let manager = Arc::new(Mutex::new(Manager::default()));

    // Create client
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler {
            database: db.clone(),
            guild_id: manager.clone(),
        })
        .await
        .expect("Error creating client");

    // Set the manager in the client data for global access
    client.data.write().await.insert::<GuildKey>(manager.clone());

    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }

    Ok(())
}
