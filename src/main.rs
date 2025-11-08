// CHECK ME
mod console;

use std::env;
use std::sync::Arc;

use anyhow::Result;
use pf_pug_bot::player::queue;
use serenity::all::{
    Client, Command, CommandOptionType as COT, Context, EventHandler, Guild, GuildId,
    GatewayIntents, Interaction, Ready, VoiceState,
};
use serenity::prelude::TypeMapKey;
use serenity::async_trait;
use serenity::builder::{
    CreateCommand as CC, CreateCommandOption as CCO, CreateEmbed as CE,
    CreateEmbedFooter as CEF, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateMessage as CM,
};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use pf_pug_bot::database::migrations::DatabaseMigrations;
use pf_pug_bot::database::repositories::GroupRepository;
use pf_pug_bot::handlers::{admin, player};
use pf_pug_bot::{ButtonType, CommandContext, ComponentContext, Database, Group, Manager, Roles, Server, SessionStatus};

fn cmd(name: impl Into<String,>,desc: impl Into<String,>,) -> CC {
    CC::new(name.into(),).description(desc.into(),)
}

pub trait CmdOp:
    Sized {
    fn op(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
}

impl CmdOp for CC {
    /// Adds an option to the command
    fn op(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::String, name, desc).required(req))
    }
}

struct Handler {
    database: Arc<Database>,
    manager:  Arc<Mutex<Manager>>,
}

/// Handler for Discord events
#[async_trait]
impl EventHandler for Handler {
    /// When the bot is ready
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
            cmd("accept",    "Accept/confirm generated teams").op("id",    "Game ID to accept (optional)", false),
            cmd("end",       "End a game")                    .op("id",    "Game ID to end (optional)",    false),
            cmd("buffer",    "Buffer a player")               .op("user",  "User to buffer",                  true),
            cmd("config",    "View or set bot configuration") .op("key",   "Configuration key",               false)
                                                              .op("value", "Configuration value",             false),
            cmd("dashboard", "Create/update interactive dashboard"),
            cmd("initgroup", "Initialize group"),
            cmd("setup",     "Run guild setup wizard"),
        ];

        if let Err(why) = Command::set_global_commands(&ctx.http, cmds).await {
            error!("Failed to register commands: {}", why);
        }
    }

    /// When the bot is connected to a new guild
    async fn guild_create(&self,ctx: Context, guild: Guild, _is_new: Option<bool>,) {
        let guild_id = guild.id.get();
        match self.database.get_config(guild_id).await {
            Ok(_config) => {
                info!("{} connected successfully!", guild.name);
                
                // Load groups from database into manager
                let group_repo = GroupRepository::new(self.database.pool().clone());
                match group_repo.get_groups_for_guild(guild_id).await {
                    Ok(groups) if !groups.is_empty() => {
                        info!("Guild {} has {} group configuration(s)", guild.name, groups.len());
                        
                        // Add server to manager if it doesn't exist
                        let mut manager = self.manager.lock().await;
                        
                        // Check if server already exists, if not create it
                        if manager.get_server(guild.id).is_err() {
                            let mut server = Server::new(guild.id, Roles::empty());
                            // Add all groups to the server
                            for group in groups {
                                server.add_group(group);
                            }
                            manager.servers.push(server);
                            info!("Loaded {} group(s) into memory for guild {}", manager.servers.last().unwrap().groups.len(), guild.name);
                        }
                        
                        // Create dashboard for the guild automatically
                        self.create_guild_dashboard(&ctx, &guild).await;
                    },
                    Ok(_) => {
                        warn!("{} has no group configurations. ID: {}", guild.name, guild_id);
            
                        // Add server to manager
                        let mut manager = self.manager.lock().await;
                        if manager.get_server(guild.id).is_err() {
                            let server = Server::empty(guild.id);
                            manager.servers.push(server);
                        }
                    },
                    Err(e) => error!("Failed to load groups for guild {}: {}", guild.name, e),
                }
            },
            Err(e) => error!("Failed to load config for guild {}: {}", guild.name, e),
        }
    }

    /// When an interaction is created
    async fn interaction_create(&self,ctx: Context,pl: Interaction,) {
        match pl {
            Interaction::Command(itx) => {
                let user_name   = &itx.user.name;
                let cmd_ctx     = CommandContext {
                    ctx:     &ctx,
                    intax:   &itx,
                    db:      self.database.clone(),
                    manager: &self.manager.clone(),
                };
                let cd          = &itx.data;
                let cdo         = &cd.options;
                
                let info = || {
                    info!("{}: /{}", user_name, itx.data.name);
                };
                
                // Handle commands that don't need a server/group first
                let result = match cd.name.as_str() {
                    "setup" => {
                        info();
                        admin::cmd_setup(&cmd_ctx).await
                    }
                    "config" => {
                        info();
                        let key   = cdo.iter().find(|opt| opt.name == "key")  .and_then(|opt| opt.value.as_str()).unwrap_or("").to_string();
                        let value = cdo.iter().find(|opt| opt.name == "value").and_then(|opt| opt.value.as_str()).map(|s| s.to_string());
                        admin::cmd_config(&cmd_ctx, key, value).await
                    }
                    _ => {
                        // All other commands need a server
                        let mut manager = self.manager.lock().await;
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Server not found: {}", e);
                                let response = CIR::Message(CIRM::new().content("Server not configured. Please run `/setup` first.").ephemeral(true));
                                let _ = itx.create_response(&ctx.http, response).await;
                                return;
                            }
                        };

                        match cd.name.as_str() {
                            "join" | "leave" => {
                                info();
                                player::queue(&cmd_ctx, server).await
                            }
                            "status" => {
                                info();
                                player::status(&cmd_ctx, server).await
                            }
                            "shuffle" => {
                                info();
                                player::shuffle(&cmd_ctx, server).await
                            }
                            "accept" => {
                                info();
                                player::accept(&cmd_ctx, server).await
                            }
                            "end" => {
                                info();
                                player::end(&cmd_ctx, server).await
                            }
                            "buffer" => {
                                info();
                                if let Some(user_option) = cdo.first() {
                                    if let Some(user_id) = user_option.value.as_str() {
                                        Group::cmd_buffer(&cmd_ctx, user_id).await.expect("Failed to buffer player")
                                    }
                                }
                                Ok(())
                            }
                            "initgroup" => {
                                info();
                                admin::cmd_init_group(&cmd_ctx, server).await
                            }
                            "dashboard" => {
                                info();
                                admin::cmd_dashboard(&cmd_ctx, server).await
                            }
                            _ => {
                                let response = CIR::Message(CIRM::new().content("Unknown command").ephemeral(true));
                                itx.create_response(&ctx.http, response).await.map_err(|e| e.into())
                            }
                        }
                    }
                };

                if let Err(e) = result {
                    error!("Error handling command '{}': {}", itx.data.name, e);

                    // Try to respond with an error message if we haven't responded yet
                    let error_response = CIR::Message(CIRM::new().content("An error occurred while processing your command").ephemeral(true));

                    if let Err(response_err) = itx.create_response(&ctx.http, error_response).await {
                        error!("Failed to send error response: {}", response_err);
                    }
                }
            },
            Interaction::Component(itx) => {
                // Handle button interactions
                let user_name   = &itx.user.name;
                let button_type = ButtonType::parse(&itx.data.custom_id);
                info!("{} clicked button: {:?}", user_name, button_type);
                
                // Handle setup/init interactions first (no group needed)
                if button_type.is_setup_button() {
                    let result = admin::handle_setup_interaction(&ctx, &itx, &self.database).await;
                    if let Err(e) = result {
                        error!("Error handling setup interaction: {}", e);
                    }
                    return;
                }
                
                // For dashboard button interactions, we need a group
                let mut manager = self.manager.lock().await;
                let group       = manager.get_group(itx.guild_id.unwrap(), itx.channel_id).unwrap();
                
                // Create component context similar to command context
                let comp_ctx = ComponentContext {
                    ctx:       &ctx,
                    component: &itx,
                    db:        self.database.clone(),
                };
                
                // Handle different button actions based on custom_id
                let result = group.dash_handle_button_interaction(&comp_ctx).await;
                
                if let Err(e) = result {
                    error!("Error handling button '{}': {}", itx.data.custom_id, e);
                    
                    // Try to respond with an error message if we haven't responded yet
                    let error_response = CIR::Message(CIRM::new().content("An error occurred while processing your button click").ephemeral(true));
                    
                    if let Err(response_err) = itx.create_response(&ctx.http, error_response).await {
                        error!("Failed to send error response: {}", response_err);
                    }
                }
            },
            _ => {
                // Other interaction types not handled yet
            }
        }
    }

    /// When a user joins or leaves a voice channel
    async fn voice_state_update(&self,ctx: Context,old: Option<VoiceState>,new: VoiceState,) {
        let user_id     = new.user_id;
        let user        = &ctx.http.get_user(user_id).await.unwrap();
        let user_name   = user.display_name();
        let old_channel = old.map(|s| s.channel_id);
        let server      = new.guild_id.unwrap();

        // Handle user leaving a vc
        if new.channel_id.is_none() && old_channel.is_some() {
            let left_channel_id = old_channel.unwrap().unwrap();
            info!("{} left {} VC", user_name, left_channel_id.name(&ctx.http).await.unwrap());
            
            // Check if they left a queue voice channel and remove them from the game
            let mut manager = self.manager.lock().await;
            if let Ok(group) = manager.get_group(server, left_channel_id) {
                if group.channels.queue_vc == left_channel_id {
                    info!("{} left queue voice channel, removing from game", user_name);
                    
                    let mut player_removed = false;
                    for game in &mut group.sessions {
                        if game.status == SessionStatus::Idle {
                            let initial_len = game.pool.len();
                            game.pool.retain(|p| p.player.discord_id != user_id);
                            if game.pool.len() < initial_len {
                                player_removed = true;
                                info!("Removed {} from game. Pool size: {}", user_name, game.pool.len());
                                break;
                            }
                        }
                    }
                    
                    // Update dashboard after player leaves
                    if player_removed {
                        if let Err(e) = group.dash_update(&ctx).await {
                            error!("Failed to update dashboard: {}", e);
                        }
                    }
                }
            }
            return;
        }
        
        // Handle user joining a vc
        let channel_id = match new.channel_id {
            Some(id) => id,
            None => {
                error!("Voice state update with no channel_id for joining case");
                return;
            }
        };
        
        info!("{} joined {}", user_name, channel_id.name(&ctx.http).await.unwrap());
        
        // Get player data
        match self.database.get_user(user_id).await {
            Ok(user) => {
                    user
            },
            Err(_) => match self.database.new_user(user_id).await {
                    Ok(new_user) => {
                        new_user
                    },
                    Err(e) => {
                        error!("Failed to create new user: {}", e);
                        return;
                    }
            },
        };

        // Mutex scope
        {
            let mut manager = self.manager.lock().await;
            
            // Find the guild by ID and check if the new channel is a queue voice channel in any group
            match manager.get_group(server, channel_id) {
                Ok(group) => {
                    if group.channels.queue_vc == channel_id {
                        info!("{} joined queue voice channel {}", user_name, channel_id);
                        if group.sessions.is_empty() { group.create_game(); }
                        
                        // Extract immutable data before mutable iteration
                        let dashboard_channel = group.channels.dashboard;
                        let player_exists     = group.get_player(user_id).is_ok();
                        
                        if player_exists {
                            error!("{} is already in the game", user_name);
                        } else {
                            group.queue_player(user_id, &ctx).await;
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to get group: {}", e);
                }
            }
        }
    }
}

impl Handler {
    /// Creates dashboard for a guild automatically when bot connects
    async fn create_guild_dashboard(&self, ctx: &Context, guild: &Guild) {
        info!("Creating dashboard for guild: {}", guild.name);
        
        // Get all groups for this guild from database
        let group_repo = GroupRepository::new(self.database.pool().clone());
        match group_repo.get_groups_for_guild(guild.id.get()).await {
            Ok(groups) => {
                for mut group in groups {
                    // Check if dashboard already exists
                    if group.has_dashboard(ctx).await {
                        group.dash_update(ctx).await;
                        continue;
                    }
                    // Create dashboard for each group's queue channel
                    let channel_id   = group.channels.queue_chat;
                    let channel_name = channel_id.name(&ctx.http).await.unwrap();
                    
                    // Create dashboard in the queue channel
                    match group.dash_publish(ctx, channel_id).await {
                        Ok(_) => {
                            info!("Dashboard created successfully for channel {}", channel_name);
                        },
                        Err(e) => {
                            error!("Failed to create dashboard for channel {}: {}", channel_name, e);
                        }
                    }
                }
            },
            Err(e) => error!("Failed to get groups for guild {}: {}", guild.name, e),
        }
    }

    /// Sends a notification to the dashboard channel when a game is ready
    async fn notify(&self,ctx: &Context,group: &Group,) {
        let dashboard_channel = group.channels.dashboard;

        // Ensure there are at least 8 players before slicing
        let mut player_mentions = Vec::new();
        
        // Get count from the latest game if available
        let player_count = if let Some(game) = group.sessions.last() { game.pool.len() } else { 0 };
        let players_to_mention = if player_count >= 8 { 8 } else { player_count };

        // Access players in the latest game if available
        if let Some(game) = group.sessions.last() {
            for player in &game.pool[..players_to_mention] {
                player_mentions.push(format!("<@{}>", player.player.discord_id));
            }
        }

        let embed = CE::new()
            .title("GAME READY!")
            .description(format!(
                // TODO: format according to group quota
                "**8 players in queue channel!**\n\n{}\n\nUse `/shuffle` to generate teams.",
                player_mentions.join(" ")
            ))
            .footer(CEF::new("Awaiting team generation..."));

        // Send the message to the dashboard channel
        if let Err(e) = dashboard_channel.send_message(&ctx.http, CM::new().embed(embed)).await {
            error!("Failed to send game ready notification: {:?}", e);
        } else {
            info!("Sent ready notification to dashboard channel");
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
    let db_file      = env::var("DATABASE_URL").unwrap_or_else(|_| "./pf_pug_bot.db".to_string());
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
        type Value = Arc<Mutex<Manager>>;
    }


    // Init manager
    let manager = Arc::new(Mutex::new(Manager::default()));

    // Init client
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler {
            database: db.clone(),
            manager: manager.clone(),
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
