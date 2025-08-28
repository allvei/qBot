// CHECK ME
mod database;
mod handlers;
mod models;

use std::env;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serenity::all::*;
use serenity::async_trait;
use serenity::builder::{
    CreateCommand as CC, CreateCommandOption as CCO, CreateEmbed as CE, CreateEmbedFooter as CEF, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM,
    CreateMessage as CM,
};
use serenity::model::application::{Command, CommandOptionType as COT, Interaction};
use serenity::model::gateway::Ready;
use serenity::model::voice::VoiceState;
use serenity::prelude::*;
use tracing::{error, info, warn};

use database::{Database, migrations::DatabaseMigrations};
use handlers::{admin, player, dashboard};
use models::command::CommandContext;
use models::data::{Group, SessionPlayer, SessionStatus};
use models::manager::Manager as SessionManager;

use crate::models::ComponentContext;

fn cmd(name: impl Into<String,>,desc: impl Into<String,>,) -> CC {
    CC::new(name.into(),).description(desc.into(),)
}

pub trait CmdOp:
    Sized {
    fn op(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
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
    fn op(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::String, name, desc).required(req))
    }
}

struct Handler {
    database: Arc<Database,>,
    guild_id: Arc<Mutex<SessionManager,>,>,
}

/// Handler for Discord events with database access
#[async_trait]
impl EventHandler
    for Handler {
    async fn ready(&self,ctx: Context,ready: Ready,) {
        info!("{} online!", ready.user.name);

        let guild_count = ctx.cache.guilds().len();
        info!("Connected guilds: {}", guild_count);

        // Register slash commands globally or for specific guild
        let cmds = vec![
            cmd("join",      "Join the queue"),
            cmd("leave",     "Leave the queue"),
            cmd("status",    "Check queue status"),
            cmd("shuffle",   "Generate teams from queue"),
            cmd("accept",    "Accept/confirm generated teams").op("id",   "Session ID to accept (optional)", false),
            cmd("end",       "End a session")                 .op("id",   "Session ID to end (optional)",    false),
            cmd("buffer",    "Buffer a player")               .op("user", "User to buffer",                  true),
            cmd("config",    "View or set bot configuration")
                .op("key",   "Configuration key",   false)
                .op("value", "Configuration value", false),
        ];

        if let Err(why) = Command::set_global_commands(&ctx.http, cmds).await {
            error!("Failed to register commands: {}", why);
        }
    }

    async fn guild_create(&self,ctx: Context, guild: Guild, _is_new: Option<bool>,) {
        let guild_id = guild.id.get();
        match self.database.get_config(guild_id).await {
            Ok(_config) => {
                info!("{} connected successfully!", guild.name);
                
                // Validate that groups exist for this guild
                let group_repo = crate::database::repositories::GroupRepository::new(self.database.pool().clone());
                match group_repo.group_exists_for_guild(guild_id).await {
                    Ok(true) => {
                        info!("Guild {} has group configurations", guild.name);
                    },
                    Ok(false) => {
                        warn!("Guild {} has no group configurations. Bot commands may not work properly.", guild.name);
                        info!("Consider creating a group configuration for guild_id: {}", guild_id);
                    },
                    Err(e) => error!("Failed to check group configurations for guild {}: {}", guild.name, e),
                }
            },
            Err(e) => error!("Failed to load config for guild {}: {}", guild.name, e),
        }
    }

    /// Handles interaction create events
    async fn interaction_create(&self,ctx: Context,pl: Interaction,) {
        match pl {
            Interaction::Command(command) => {
                let user_name = &command.user.name;
                let cmd_ctx = CommandContext {
                    ctx:   &ctx,
                    intax: &command,
                    db:    self.database.clone(),
                    manager: self.guild_id.clone(),
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
                        player::queue(&cmd_ctx).await
                    }
                    "status" => {
                        info();
                        player::status(&cmd_ctx).await
                    }
                    "shuffle" => {
                        info();
                        player::shuffle(&cmd_ctx).await
                    }
                    "accept" => {
                        info();
                        player::accept(&cmd_ctx, &get_arg("id")).await
                    }
                    "end" => {
                        info();
                        player::end(&cmd_ctx, get_arg("id")).await
                    }
                    "buffer" => {
                        info();
                        if let Some(user_option) = cdo.first() {
                            if let Some(user_id) = user_option.value.as_str() {
                                admin::cmd_buffer(&cmd_ctx, user_id.to_string()).await.expect("Failed to buffer player")
                            }
                        }
                        Ok(())
                    }
                    "config" => {
                        info();
                        let key   = cdo.iter().find(|opt| opt.name == "key")  .and_then(|opt| opt.value.as_str()).unwrap_or("").to_string();
                        let value = cdo.iter().find(|opt| opt.name == "value").and_then(|opt| opt.value.as_str()).map(|s| s.to_string());

                        admin::cmd_config(&cmd_ctx, key, value).await
                    }
                    "init_dashboard" => {
                        info();
                        admin::cmd_init_dashboard(&cmd_ctx).await
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
            },
            Interaction::Component(component) => {
                // Handle button interactions
                let user_name = &component.user.name;
                info!("{} clicked button: {}", user_name, component.data.custom_id);
                
                // Create component context similar to command context
                let comp_ctx = ComponentContext {
                    ctx:       &ctx,
                    component: &component,
                    db:        self.database.clone(),
                };
                
                // Handle different button actions based on custom_id
                let result = dashboard::handle_button_interaction(&comp_ctx).await;
                
                if let Err(e) = result {
                    error!("Error handling button '{}': {}", component.data.custom_id, e);
                    
                    // Try to respond with an error message if we haven't responded yet
                    let error_response = CIR::Message(CIRM::new().content("An error occurred while processing your button click").ephemeral(true));
                    
                    if let Err(response_err) = component.create_response(&ctx.http, error_response).await {
                        error!("Failed to send error response: {}", response_err);
                    }
                }
            },
            _ => {
                // Other interaction types not handled yet
            }
        }
    }

    async fn voice_state_update(&self,ctx: Context,old: Option<VoiceState>,new: VoiceState,) {
        let user_id     = new.user_id;
        let user        = &ctx.http.get_user(user_id).await.unwrap();
        let user_name   = user.display_name();
        let channel     = new.channel_id;
        let old_channel = old.map(|s| s.channel_id);

        if channel.is_none() && old_channel.is_some() {
            info!("{} left {} VC", user_name, old_channel.unwrap().unwrap().name(&ctx.http).await.unwrap());

            // TODO: create a function to get the session by channel
            return;
        }

        // Handle user joining a queue channel
        if let Some(new_tc_id) = channel {
            info!("{} joined {} VC", user_name, new_tc_id.name(&ctx.http).await.unwrap());
            // First, get the player data without holding the lock
            let player = match self.database.get_user(user_id).await {
                Ok(user) => {
                    info!("Loaded user: {}", user_name);
                    user
                },
                Err(_) => match self.database.new_user(user_id).await {
                    Ok(new_user) => {
                        info!("New user: {}", user_name);
                        new_user
                    },
                    Err(e) => {
                        error!("Failed to create new user: {}", e);
                        return;
                    }
                },
            };

            // We'll store notification information to use after the mutex is released
            let mut dashboard_channel = None;
            let mut player_count = 0;
            let mut should_notify = false;

            // Scope for the mutex lock
            {
                let mut manager = self.guild_id.lock().unwrap();
                
                // Find the guild by ID and check if the new channel is a queue channel in any group
                if let Some(server) = manager.servers.iter_mut().find(|s| s.guild_id == new.guild_id.unwrap()) {
                    for group in server.groups.iter_mut() {
                        if group.channels.queue == channel.expect("Channel ID is None").get() {
                            // User joined queue channel
                            info!("{} joined queue channel {}", user_name, channel.expect("Channel ID is None"));
                            // Ensure there is at least one active session
                            if group.sessions.is_empty() {
                                info!("No active session, creating one");
                                group.create_session();
                            }
                            // Get the current session (last in the vector)
                            if let Some(session) = group.sessions.last_mut() {
                                // Skip if user is already in the session
                                if session.pool.iter().any(|sp| sp.player.discord_id == user_id.get()) {
                                    info!("User {} is already in session", user_name);
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
                                let _session_player = SessionPlayer::construct(player);
                                session.pool.push(_session_player);
                                info!("Added {} to session, now has {} players", user_name, session.pool.len());
                                // If we have enough players, update session status
                                if session.pool.len() >= 8 && !matches!(session.status, SessionStatus::Hot) {
                                    session.status = SessionStatus::Hot;
                                    info!("Session is now HOT with {} players", session.pool.len());
                                    // Store notification info to use after releasing the lock
                                    dashboard_channel = Some(group.dashboard.ch);
                                    player_count = session.pool.len();
                                    should_notify = true;
                                }
                            }
                            break; // We found the group, exit the loop
                        }
                    }
                } // Close the if let Some(guild) block
            } // Mutex guard is released here

            // Now perform async operations with the data we collected
            if should_notify {
                if let Some(dashboard_id) = dashboard_channel {
                info!("Sending session ready notification: dashboard={}, players={}", dashboard_id, player_count);
                let channel = dashboard_id;

                // Create an embed message for the session ready notification
                let embed = CE::new()
                    .title("SESSION READY!")
                    .description(format!("A match is ready to start with {} players!", player_count))
                    .footer(CEF::new("Awaiting team generation..."));

                // Create buttons for actions
                let components = vec![CreateActionRow::Buttons(vec![CreateButton::new("shuffle")
                    .style(ButtonStyle::Primary)
                    .label("Shuffle Teams")])];

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
}

impl Handler {
    /// Sends a notification to the dashboard channel when a session is ready
    async fn notify(&self,ctx: &Context,group: &Group,) {
        let dashboard_channel = group.dashboard.ch;

        // Ensure there are at least 8 players before slicing
        let mut player_mentions = Vec::new();
        
        // Get count from the latest session if available
        let player_count = if let Some(session) = group.sessions.last() { session.pool.len() } else { 0 };
        let players_to_mention = if player_count >= 8 { 8 } else { player_count };

        // Access players in the latest session if available
        if let Some(session) = group.sessions.last() {
            for player in &session.pool[..players_to_mention] {
                player_mentions.push(format!("<@{}>", player.player.discord_id));
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
async fn main(
) -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenvy::dotenv().ok();
    let token        = env::var("DISCORD_TOKEN").expect("Expected a Discord token in the environment");
    let db_file      = env::var("DATABASE_URL").unwrap_or_else(|_| "./pfpug.db".to_string());
    let database_url = format!("sqlite:{}",db_file);

    // Initialize database connection
    let db = Arc::new(Database::new(&database_url).await?);

    // Run database migrations
    let migrations = DatabaseMigrations::new(db.pool());
    migrations.run_all().await?;
    
    // Validate database schema integrity
    migrations.validate_schema().await?;

    // Configure the client with the framework and intents
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILDS;

    // Define TypeMapKey for Guild
    struct GuildKey;
    impl TypeMapKey
        for GuildKey {
        type Value = Arc<Mutex<SessionManager>>;
    }


    // Init manager
    let manager = Arc::new(Mutex::new(SessionManager::default()));

    // Init client
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler {
            database: db,
            guild_id: manager.clone(),
        })
        .await
        .expect("Failed to create client");

    // Set the manager in the client data for global access
    client.data.write().await.insert::<GuildKey>(manager.clone());

    // Start listening for events by starting a single shard
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }

    Ok(())
}
