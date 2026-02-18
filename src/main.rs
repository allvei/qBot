use std::env;
use std::sync::Arc;

use time::macros::format_description;
use tracing_subscriber::fmt::time::UtcTime;
use anyhow::Result;
use pf_pug_bot::{RED, commands, log_prefix_category};
use serenity::all::{
    Client, GatewayIntents, EventHandler, Ready, Guild,Interaction,
    VoiceState, Command, Context, CommandOptionType as COT,
    CreateEmbed, EditMessage, CommandInteraction, CommandDataOption
};
use serenity::prelude::TypeMapKey;
use serenity::async_trait;
use serenity::builder::{
    CreateCommand as CC, CreateCommandOption as CCO, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use pf_pug_bot::db::migrations::DatabaseMigrations;
use pf_pug_bot::db::repo::CategoryRepository;
use pf_pug_bot::handlers::{self, admin};
use pf_pug_bot::{ButtonType, CommandContext, ComponentContext, DashboardQueueKey, DashboardUpdateQueue, DmMessageTracker, DmTrackerKey, Database, Manager, QueueToggleType::{self, *}, Roles, Server, SessionStatus, VoiceStateUpdate, log_queue_toggle};

fn cmd(name: impl Into<String,>,desc: impl Into<String,>,) -> CC {
    CC::new(name.into(),).description(desc.into(),)
}

pub trait CmdOp:
    Sized {
    fn op_int(   self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
    fn op_string(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
    fn op_user(  self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
    fn op_role(  self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
}

async fn send_error_response(itx: &serenity::all::CommandInteraction, ctx: &Context, message: &str) -> Result<()> {
    let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
    itx.create_response(&ctx.http, response).await?;
    Ok(())
}

async fn send_component_error_response(itx: &serenity::all::ComponentInteraction, ctx: &Context, message: &str) {
    let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
    if let Err(e) = itx.create_response(&ctx.http, response).await {
        // Check if this is an "Unknown interaction" error, which means the interaction was already handled
        if e.to_string().contains("Unknown interaction") {
            debug!("Could not send error response - interaction already handled: {} | User: {} | Message: {}", 
                e, itx.user.id, itx.message.id);
        } else {
            error!("Failed to send error response: {e} | User: {} | Guild: {} | Message: {} | Interaction type: {}", 
                itx.user.id, itx.guild_id.unwrap_or_default(), itx.message.id, 
                std::any::type_name_of_val(itx));
        }
    }
}

impl CmdOp for CC {
    /// Adds an integer option to the command
    fn op_int(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::Integer, name, desc).required(req))
    }

    /// Adds a string option to the command
    fn op_string(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::String, name, desc).required(req))
    }

    /// Adds a user option to the command
    fn op_user(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::User, name, desc).required(req))
    }

    fn op_role(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::Role, name, desc).required(req))
    }
}

// Helper methods to reduce code duplication

/// Extract user ID from command options with error handling
async fn extract_user_option(cdo: &[CommandDataOption], command_name: &str) -> Option<serenity::all::UserId> {
    if let Some(user_option) = cdo.first() {
        if let Some(user_id) = user_option.value.as_user_id() {
            Some(user_id)
        } else {
            error!("Failed to parse user ID from {} command", command_name);
            None
        }
    } else {
        error!("No user option provided for {} command", command_name);
        None
    }
}

/// Get server with error handling - returns a mutable reference to the server
async fn get_server_with_error<'a>(
    manager: &'a mut tokio::sync::MutexGuard<'_, Manager>,
    guild_id: serenity::all::GuildId,
    itx: &CommandInteraction,
    ctx: &Context,
) -> Result<&'a mut pf_pug_bot::models::Server> {
    match manager.get_server(guild_id) {
        Ok(s) => Ok(s),
        Err(e) => {
            error!("Server not found: {e}");
            let _ = send_error_response(itx, ctx, "Server not configured. Please use `/config` to create roles and categories.").await;
            Err(anyhow::anyhow!("Server not found"))
        }
    }
}

/// Log command usage with proper context
async fn log_command_usage(
    ctx: &Context,
    itx: &CommandInteraction,
    tag: &str,
    command_name: &str,
) {
    let guild_name = pf_pug_bot::guild_name(ctx, itx.guild_id.unwrap());
    info!("[{}] {} used /{}", guild_name, tag, command_name);
}

/// Check if interaction is still valid for response
fn is_interaction_valid(interaction: &serenity::all::Interaction) -> bool {
    match interaction {
        serenity::all::Interaction::Component(itx) => {
            itx.id.get() != 0 && itx.message.id.get() != 0
        },
        serenity::all::Interaction::Command(itx) => {
            itx.id.get() != 0
        },
        _ => true,
    }
}

struct Handler {
    db:        Arc<Database>,
    manager:         Arc<Mutex<Manager>>,
    dashboard_queue: Arc<tokio::sync::Mutex<Option<DashboardUpdateQueue>>>,
}

/// Handler for Discord events
#[async_trait]
impl EventHandler for Handler {
    /// When the bot is ready
    async fn ready(&self,ctx: Context, _ready: Ready,) {
        let _guild_count = ctx.cache.guilds().len();

        // Initialize dashboard update queue (done once on ready)
        {
            let mut queue_lock = self.dashboard_queue.lock().await;
            if queue_lock.is_none() {
                let queue = DashboardUpdateQueue::new(ctx.clone(), self.manager.clone(), self.db.clone());
                let queue_arc = Arc::new(tokio::sync::Mutex::new(queue.clone()));
                *queue_lock = Some(queue);

                // Store in Context data for global access
                ctx.data.write().await.insert::<DashboardQueueKey>(queue_arc);
            }
        }

        // Initialize DM message tracker and start cleanup task
        {
            let dm_tracker = Arc::new(DmMessageTracker::new());
            ctx.data.write().await.insert::<DmTrackerKey>(dm_tracker.clone());

            // Start the cleanup background task
            dm_tracker.start_cleanup_task(ctx.http.clone());
        }

        // Start timeout background task
        {
            let manager   = self.manager.clone();
            let database  = self.db.clone();
            let ctx_clone = ctx.clone();

            tokio::spawn(async move {
                use tokio::time::{interval, Duration};
                let mut check_interval = interval(Duration::from_secs(60)); // Check every minute

                loop {
                    check_interval.tick().await;

                    let mut manager_lock = manager.lock().await;
                    
                    // Check all categories in all servers
                    for server in manager_lock.servers.iter_mut() {
                        let guild_id = server.guild_id;
                        for category in server.categories.iter_mut() {
                            if category.check_timeout(&database, &ctx_clone, guild_id).await {
                                // Players were removed, update dashboard
                                category.queue_dash_update(&ctx_clone, guild_id).await;
                            }
                        }
                    }
                }
            });
        }

        // Spawn console command handler in a separate task
        //let console_handler = command::ConsoleHandler::new(
        //    self.manager.clone(),
        //    self.database.clone(),
        //    Arc::new(ctx.clone()),
        //);

        //tokio::spawn(async move {
        //    console_handler.start_console_loop().await;
        //});

        // Register slash commands globally or for specific guild
        let cmds = vec![
            // Player commands
            cmd("buffer",        "Move a player to the start of the queue")
                .op_user("user", "User to buffer", true),
            cmd("fatkid",        "Move a player to the end of the queue")
                .op_user("user", "User to fatkid", true),

            cmd("remove",        "Remove all players from the queue, or a specific player")
                .op_user("user", "User to remove (optional)", false),
            cmd("elo",           "View ELO and rank information for a player")
                .op_user("user", "The Discord user (mention or ID, optional)", false),
            cmd("prefs",         "Open your preferences"),
            cmd("config",        "Open server settings"),
            cmd("edit",    "Open player menu")
                .op_user("user", "The Discord user to edit", true),
            cmd("migrate", "Bulk-assign ELO to all members with a role")
                .op_role("role", "The role to migrate", true)
                .op_int("elo", "The ELO value to assign", true),
        ];

        if let Err(why) = Command::set_global_commands(&ctx.http, cmds).await {
            error!("Failed to register commands: {}", why);
        }
    }

    /// When the bot is connected to a new guild
    async fn guild_create(&self,ctx: Context, guild: Guild, _is_new: Option<bool>,) {
        let guild_id = guild.id;
        match self.db.get_config(guild_id).await {
            Ok(_config) => {
                // Load categories from database into manager
                let category_repo = CategoryRepository::new(self.db.pool().clone());
                match category_repo.get_categories_for_guild(guild_id).await {
                    Ok(categories) => {
                        let mut manager = self.manager.lock().await;
                        if manager.get_server(guild.id).is_err() {
                            let mut server = Server::new(guild.id, guild.name.clone(), Roles::empty());
                            for mut category in categories {
                                // Update guild name in database if it's missing
                                if category.guild_name.is_none() {
                                    if let Err(e) = self.db.categories.update_guild_name(guild_id, category.category_id, &guild.name).await {
                                        error!("Failed to update guild name for category {}: {}", category.category_id, e);
                                    } else {
                                        category.guild_name = Some(guild.name.clone());
                                    }
                                }
                                if let Err(e) = server.add_category(category) {
                                    error!("Failed to add category: {e}");
                                }
                            }
                            // Clean up orphaned dynamic VCs from previous bot runs
                            for category in &mut server.categories {
                                category.cleanup_orphaned_vcs(&ctx, &self.db).await;
                            }

                            let categories_len = server.categories.len();
                            manager.servers.push(server);

                            if categories_len > 0 {
                                self.check_existing_voice_users(&ctx, &guild, &mut manager).await;
                                self.create_guild_dashboard_from_manager(&ctx, &guild, &mut manager).await;
                            } else {
                                warn!("[{}] No valid category configurations.", guild.name);
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to load categories for guild {}: {}", guild.name, e);
                        // Still create an empty server so commands can work
                        let mut manager = self.manager.lock().await;
                        if manager.get_server(guild.id).is_err() {
                            let server = Server::empty(guild.id, guild.name.clone());
                            manager.servers.push(server);
                        }
                    }
                }
            },
            Err(e) => error!("Failed to load config for guild {}: {}", guild.name, e),
        }
    }

    /// When an interaction is created
    async fn interaction_create(&self,ctx: Context,pl: Interaction,) {
        match pl {
            Interaction::Command(itx) => {
                let tag = pf_pug_bot::log::get_user_tag(&ctx, itx.user.id, &self.db).await;
                let cmd_ctx     = CommandContext {
                    ctx:     &ctx,
                    intax:   &itx,
                    db:      self.db.clone(),
                    manager: &self.manager.clone(),
                };
                let cd  = &itx.data;
                let cdo = &cd.options;

                let info = || {
                    let guild_name = pf_pug_bot::guild_name(&ctx, itx.guild_id.unwrap());
                    info!("[{}] {} used /{}", guild_name, tag, itx.data.name);
                };

                // Handle commands that don't need a server/category first
                let result = match cd.name.as_str() {
                    "prefs" => {
                        info();
                        commands::cmd_prefs(&cmd_ctx).await
                    }
                    "config" => {
                        info();
                        commands::cmd_config(&cmd_ctx).await
                    }
                    "edit" => {
                        info();
                        commands::cmd_edit_player(&cmd_ctx).await
                    }
                    "migrate" => {
                        info();
                        commands::cmd_migrate(&cmd_ctx).await
                    }
                    _ => {
                        // All other commands need a server
                        let mut manager = self.manager.lock().await;
                        let server = match get_server_with_error(&mut manager, itx.guild_id.unwrap(), &itx, &ctx).await {
                            Ok(s) => s,
                            Err(_) => return, // Error already handled by helper
                        };

                        match cd.name.as_str() {
                            "buffer" => {
                                log_command_usage(&ctx, &itx, &tag, "buffer").await;
                                if let Some(user_id) = extract_user_option(cdo, "buffer").await {
                                    admin::cmd_buffer(&cmd_ctx, server, user_id).await
                                } else {
                                    Ok(())
                                }
                            }
                            "fatkid" => {
                                log_command_usage(&ctx, &itx, &tag, "fatkid").await;
                                if let Some(user_id) = extract_user_option(cdo, "fatkid").await {
                                    admin::cmd_fatkid(&cmd_ctx, server, user_id).await
                                } else {
                                    Ok(())
                                }
                            }
                            "remove" => {
                                log_command_usage(&ctx, &itx, &tag, "remove").await;
                                admin::cmd_remove_queue(&cmd_ctx, server, cdo.first()).await
                            }
                            "elo" => {
                                log_command_usage(&ctx, &itx, &tag, "elo").await;
                                if let Some(user_option) = cdo.first() {
                                    if let Some(user_id) = user_option.value.as_user_id() {
                                        match ctx.http.get_user(user_id).await {
                                            Ok(user) => admin::cmd_get_player_elo(&cmd_ctx, Some(user)).await,
                                            Err(_) => {
                                                let _ = send_error_response(&itx, &ctx, "Failed to get user").await;
                                                Ok(())
                                            }
                                        }
                                    } else {
                                        let _ = send_error_response(&itx, &ctx, "Invalid user specified").await;
                                        Ok(())
                                    }
                                } else {
                                    let _ = send_error_response(&itx, &ctx, "No user specified").await;
                                    Ok(())
                                }
                            }
                            _ => {
                                send_error_response(&itx, &ctx, "Unknown command").await
                            }
                        }
                    }
                };

                if let Err(e) = result {
                    error!("Error handling command '{}': {}", itx.data.name, e);
                    let _ = send_error_response(&itx, &ctx, "An error occurred while processing your command").await;
                }
            },
            Interaction::Component(ref itx) => {
                let button_type = ButtonType::parse(&itx.data.custom_id);

                if matches!(button_type, ButtonType::ConfirmPermissions) {
                    let guild_id = itx.guild_id.unwrap();
                    let user_id = itx.user.id;

                    // Check if user is an admin
                    // Try cache first (fast path)
                    let member_opt = if let Some(guild) = ctx.cache.guild(guild_id) {
                        guild.members.get(&user_id).cloned()
                    } else {
                        None
                    };

                    // Fallback to HTTP if not in cache
                    let member = match member_opt {
                        Some(m) => Some(m),
                        None => guild_id.member(&ctx.http, user_id).await.ok()
                    };

                    let is_admin = match member {
                        Some(member) => {
                            match self.db.config.get_admin_role_id(guild_id).await {
                                Ok(Some(admin_role)) => member.roles.contains(&admin_role),
                                _ => {
                                    // If no admin role configured, check for server Administrator permission
                                    if let Some(guild_ref) = guild_id.to_guild_cached(&ctx.cache) {
                                        let perms = guild_ref.member_permissions(&member);
                                        perms.contains(serenity::all::Permissions::ADMINISTRATOR)
                                    } else {
                                        false
                                    }
                                }
                            }
                        }
                        None => false,
                    };

                    if !is_admin {
                        send_component_error_response(&itx, &ctx, "Only administrators can confirm bot permissions.").await;
                        return;
                    }

                    // Clone the guild data to avoid Send issues
                    let guild = match guild_id.to_guild_cached(&ctx.cache) {
                        Some(g) => g.clone(),
                        None => {
                            error!("Failed to get guild from cache");
                            return;
                        }
                    };

                    // Re-check permissions
                    let (has_perms, missing_perms) = self.check_bot_permissions(&ctx, &guild).await;

                    if !has_perms {
                        // Still missing permissions
                        send_component_error_response(&itx, &ctx, &format!("Still missing permissions: {missing_perms}")).await;
                    } else {
                        // Permissions granted! Delete the warning message and create dashboard
                        if let Err(e) = itx.message.delete(&ctx.http).await {
                            error!("Failed to delete permission warning: {e}");
                        }

                        let success_response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content("Permissions confirmed! Setting up dashboard...")
                                .ephemeral(true)
                        );
                        if let Err(e) = itx.create_response(&ctx.http, success_response).await {
                            error!("Failed to send success response: {e}");
                        }

                        // Now create the dashboard
                        let mut manager = self.manager.lock().await;
                        self.create_guild_dashboard_from_manager(&ctx, &guild, &mut manager).await;
                    }
                    return;
                }

                if matches!(button_type, ButtonType::CreateRankRolesYes | ButtonType::CreateRankRolesNo) {
                    let create = matches!(button_type, ButtonType::CreateRankRolesYes);
                    let result = admin::handle_create_rank_roles(&ctx, &self.db, &itx, create).await;
                    if let Err(e) = result {
                        error!("Error handling rank role creation: {e}");
                    }
                    return;
                }

                if button_type.is_setup_button() {
                    let result = admin::handle_setup_interaction(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling setup interaction: {e}");
                    }
                    return;
                }

                // Handle settings buttons (user settings)
                if itx.data.custom_id.starts_with("settings_") {
                    let result = handlers::handle_settings_button(&ctx, &itx, &self.db).await;
                    if let Err(e) = result {
                        error!("Error handling settings interaction: {e}");
                    }
                    return;
                }

                // Handle server settings buttons (including link channel flow)
                if itx.data.custom_id.starts_with("server_settings_") || itx.data.custom_id.starts_with("server_cfg_") || itx.data.custom_id.starts_with("link_ch_") {
                    let result = handlers::handle_server_settings_button(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling server settings interaction: {e}");
                    }
                    return;
                }

                // Handle category settings select menu
                if itx.data.custom_id == "category_settings_select" {
                    let result = handlers::handle_category_settings_select(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling category settings select: {e}");
                    }
                    return;
                }

                // Handle server-level team balance method select
                if itx.data.custom_id == "server_settings_balance" {
                    let result = handlers::handle_server_settings_balance_select(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling server settings balance select: {e}");
                    }
                    return;
                }

                // Handle player settings rank selection
                if itx.data.custom_id.starts_with("player_settings_rank_select_") {
                    let result = handlers::handle_player_settings_rank_select(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling player settings rank select: {e}");
                    }
                    return;
                }

                // Handle category settings buttons (including link message, format, and elo gate buttons)
                if itx.data.custom_id.starts_with("category_settings_") || itx.data.custom_id.starts_with("category_link_msg_") || itx.data.custom_id.starts_with("category_sg_") || itx.data.custom_id.starts_with("elo_gate_") {
                    let result = handlers::handle_category_settings_button(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling category settings interaction: {e}");
                    }
                    return;
                }

                // Handle ELO change confirmation buttons
                if itx.data.custom_id.starts_with("confirm_elo_change_") || itx.data.custom_id.starts_with("cancel_elo_change_") {
                    let result = handlers::handle_elo_change_confirmation(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling ELO change confirmation: {e}");
                    }
                    return;
                }

                // Handle player settings buttons
                if itx.data.custom_id.starts_with("player_settings_") {
                    let result = handlers::handle_player_settings_button(&ctx, &itx, &self.db).await;
                    if let Err(e) = result {
                        error!("Error handling player settings interaction: {e}");
                    }
                    return;
                }

                let mut manager = self.manager.lock().await;
                let guild_id = itx.guild_id.unwrap();
                let channel_id = itx.channel_id;

                let category = match manager.get_category_by_channel(guild_id, channel_id) {
                    Ok(category) => category,
                    Err(_) => {
                        // Category not in manager - try to recover from database
                        let guild_name = pf_pug_bot::guild_name(&ctx, guild_id);
                        let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{channel_id}"));
                        info!("[{}] Category not found in manager for #{}, attempting recovery from database", guild_name, channel_name);

                        // Get the message ID from the interaction
                        let message_id = itx.message.id;
                        let guild_id_u64 = guild_id;
                        let channel_id_u64 = channel_id.get();
                        let message_id_u64 = message_id.get();

                        // Load categories from database for this guild
                        let category_repo = CategoryRepository::new(self.db.pool().clone());
                        match category_repo.get_categories_for_guild(guild_id_u64).await {
                            Ok(categories) => {
                                // Find the category that matches this dashboard channel
                                if let Some(mut recovered_category) = categories.into_iter()
                                    .find(|g| g.channels.dashboard.get() == channel_id_u64)
                                {
                                    info!("[{}] Found category in database for #{}", guild_name, channel_name);

                                    // Update the dashboard message ID in the database
                                    if let Err(e) = category_repo.update_dashboard_msg(guild_id_u64, channel_id_u64, message_id_u64).await {
                                        error!("[{}] Failed to update dashboard message ID: {}", guild_name, e);
                                    } else {
                                        info!("[{}] Updated dashboard message ID in database", guild_name);
                                        // Update the in-memory category too
                                        recovered_category.dashboard_msg = message_id;
                                    }

                                    // Add the recovered category to the manager
                                    let server = manager.get_server(guild_id);
                                    if let Ok(server) = server {
                                        if let Err(e) = server.add_category(recovered_category) {
                                            error!("[{}] Failed to add recovered category: {}", guild_name, e);
                                        } else {
                                            info!("[{}] Recovered category added to manager", guild_name);
                                        }

                                        // Now get the category from the manager
                                        manager.get_category_by_channel(guild_id, channel_id).unwrap()
                                    } else {
                                        error!("[{}] Could not get server from manager", guild_name);
                                        send_component_error_response(&itx, &ctx, "Dashboard state was lost. Please run `/setup` to reconfigure.").await;
                                        return;
                                    }
                                } else {
                                    error!("[{}] No category found in database for #{}", guild_name, channel_name);
                                    send_component_error_response(&itx, &ctx, "Dashboard configuration not found. Please run `/config` to configure this server.").await;
                                    return;
                                }
                            },
                            Err(e) => {
                                error!("Failed to load categories from database: {e}");
                                send_component_error_response(&itx, &ctx, "Failed to access database. Please contact an administrator.").await;
                                return;
                            }
                        }
                    }
                };

                let comp_ctx = ComponentContext {
                    ctx:       &ctx,
                    component: &itx,
                    db:        self.db.clone(),
                    manager:   &self.manager,
                };

                let _button_id = &itx.data.custom_id;
                let _user_id = itx.user.id;

                debug!("Handling button interaction: '{}' | User: {} | Message: {} | Token: {:?}", 
                    itx.data.custom_id, itx.user.id, itx.message.id, 
                    itx.token);

                let result = category.dash_handle_button_interaction(&comp_ctx).await;

                if let Err(e) = result {
                    error!("Error handling button '{}': {} | User: {} | Guild: {} | Message: {} | Token: {:?}", 
                        itx.data.custom_id, e, itx.user.id, itx.guild_id.unwrap_or_default(), 
                        itx.message.id, itx.token);
                    if is_interaction_valid(&pl) {
                        send_component_error_response(&itx, &ctx, "An error occurred while processing your button click").await;
                    } else {
                        error!("Interaction no longer valid for button '{}'", itx.data.custom_id);
                    }
                } else {
                    debug!("Successfully handled button '{}': User: {} | Message: {}", 
                        itx.data.custom_id, itx.user.id, itx.message.id);
                }
            },
            Interaction::Modal(itx) => {
                // Handle modal submissions for user settings
                if itx.data.custom_id.starts_with("settings_modal_") {
                    let result = handlers::handle_settings_modal(&ctx, &itx, &self.db).await;
                    if let Err(e) = result {
                        error!("Error handling settings modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for server settings
                if itx.data.custom_id.starts_with("server_settings_modal_") 
                    || itx.data.custom_id.starts_with("server_settings_rank_modal_")
                    || itx.data.custom_id.starts_with("server_settings_category_modal_") 
                {
                    let result = handlers::handle_server_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling server settings modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for category settings (including format modals)
                if itx.data.custom_id.starts_with("category_settings_modal_") || itx.data.custom_id.starts_with("category_sg_modal_") {
                    let result = handlers::handle_category_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling category settings modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for linking dashboard message
                if itx.data.custom_id.starts_with("category_link_msg_modal_") {
                    let result = handlers::handle_category_link_msg_modal(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling category link message modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for player settings
                if itx.data.custom_id.starts_with("player_settings_modal_") {
                    let result = handlers::handle_player_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling player settings modal '{}': {}", itx.data.custom_id, e);
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
        let state   = VoiceStateUpdate::get(&old, &new);
        let user_id = new.user_id;
        let user    = match ctx.http.get_user(user_id).await {
            Ok(u) => u,
            Err(e) => {
                error!("Failed to get user {}: {}", user_id, e);
                return;
            }
        };

        let server      = match new.guild_id {
            Some(s) => s,
            None => {return;}
        };

        // Get player tag from database (primary source)
        let tag = match self.db.get_user(user_id, &ctx).await {
            Ok(player) => player.tag,
            Err(_) => user.display_name().to_string(),
        };

        // First manager lock scope
        let left_team_vc = {
            let mut manager = self.manager.lock().await;

            // Determine which channel to use for category lookup based on state
            // For disconnects/moves, use old channel; for connects, use new channel
            let lookup_channel = match state {
                VoiceStateUpdate::Disconnected | VoiceStateUpdate::Moved => {
                    match &old {
                        Some(s) => match s.channel_id {
                            Some(ch) => ch,
                            None => return,
                        },
                        None => return,
                    }
                },
                VoiceStateUpdate::Connected => {
                    match new.channel_id {
                        Some(ch) => ch,
                        None => return,
                    }
                },
                VoiceStateUpdate::Reconnected => return,
            };

            let category = match manager.get_category_by_channel(server, lookup_channel) {
                Ok(g) => g,
                Err(_) => return,
            };

            match state {
                VoiceStateUpdate::Disconnected => {
                    let was_team_vc = category.is_team_vc(lookup_channel);
                    self.handle_player_leave_vc(&ctx, category, server, user_id, &tag).await;
                    category.check_team_vc_cleanup_on_leave(&ctx).await;
                    category.queue_dash_update(&ctx, server).await;
                    was_team_vc
                },
                VoiceStateUpdate::Connected => {
                    if category.get_inactives().is_empty() {
                        if let Err(e) = category.create_session() {
                            warn!("Failed to create session on VC connect: {e}");
                        }
                    }
                    false
                },
                VoiceStateUpdate::Moved => {
                    let was_team_vc = category.is_team_vc(lookup_channel);
                    if category.channels.queue_vc == lookup_channel {
                        self.handle_player_leave_vc(&ctx, category, server, user_id, &tag).await;
                        category.queue_dash_update(&ctx, server).await;
                    }
                    was_team_vc
                },
                VoiceStateUpdate::Reconnected => {
                    return;
                }
            }
        }; // Release first manager lock here

        // If a player left a team VC, check if all team VCs are now empty
        // and auto-end the game if so. Requires a separate lock scope since
        // pull needs the manager Arc.
        if left_team_vc {
            let mut manager = self.manager.lock().await;
            if let Ok(category) = manager.get_category_by_channel(server, {
                match &old {
                    Some(s) => s.channel_id.unwrap(),
                    None => return,
                }
            }) {
                category.check_team_vc_empty_auto_end(&ctx, server, &self.db, Some(self.manager.clone())).await;
            }
        }

        // Only process joining logic if player is joining a channel (not disconnecting)
        if new.channel_id.is_none() {
            return;
        }

        // Note: Join logging is done inside the queue VC check below to avoid logging unrelated channel joins

        // Get player data
        let _player = match self.db.get_user(user_id, &ctx).await {
            Ok(user) => user,
            Err(_) => match self.db.new_user(user_id, &ctx).await {
                    Ok(new_user) => new_user,
                    Err(e) => {
                        error!("Failed to create new user: {e}");
                        return;
                    }
            }
        };

        // Mutex scope
        {
            let mut manager = self.manager.lock().await;

            // Find the guild by ID and check if the new channel is a queue voice channel in any category
            match manager.get_category_by_channel(server, new.channel_id.unwrap()) {
                Ok(category) => {
                    if category.channels.queue_vc == new.channel_id.unwrap() {
                        // Check if player is already in any session and mark them as in VC
                        if let Ok(session) = category.get_user_session(user_id).await {
                            let was_hot = session.is_hot();
                            if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                                let was_missing = !player.in_queue_vc;
                                player.in_queue_vc = true;

                                // Update dashboard if player was missing in a hot session
                                // This removes them from the "Missing players" list
                                if was_hot && was_missing {
                                    info!("{} joined VC during hot session, updating dashboard", tag);
                                    category.queue_dash_update(&ctx, server).await;
                                }
                            }
                        } else {
                            // Player not in session yet - check if they want auto-queue
                            let user_prefs = self.db.users.get_prefs(user_id).await.unwrap_or_default();
                            if !user_prefs.vc_auto_join {
                                // User has disabled VC auto-queue, log that they joined VC but didn't join queue
                                let guild_name = pf_pug_bot::guild_name(&ctx, server);
                                let category_name = category.name.as_deref().unwrap_or("Unknown");
                                info!("{} {} joined queue VC but did not join the queue (auto-queue disabled)", log_prefix_category(&guild_name, &category_name), tag);
                                return;
                            }

                            // Ensure a session exists before trying to add player
                            if category.get_inactives().is_empty() {
                                warn!("No idle sessions present when player {} joined VC, creating one", tag);
                                if let Err(e) = category.create_session() {
                                    error!("Failed to create session for player {}: {}", tag, e);
                                    let guild_name = pf_pug_bot::guild_name(&ctx, server);
                                    let category_name = category.name.as_deref().unwrap_or("Unknown");
                                    error!("{} {} joined VC but could not be added to queue (failed to create session)", 
                                          log_prefix_category(&guild_name, &category_name), tag);
                                    return;
                                }
                            }

                            // Now we're guaranteed to have a session
                            {
                                use pf_pug_bot::handlers::player::resolve_player_for_queue;

                                let (player, discord_rank) = match resolve_player_for_queue(&ctx, &self.db, server, user_id).await {
                                    Ok(result) => result,
                                    Err(e) => {
                                        error!("Failed to resolve player for queue: {e}");
                                        return;
                                    }
                                };

                                // Use queue_player_with_vc_status to set in_queue_vc BEFORE quota check/notification
                                let queue_ctx = pf_pug_bot::models::server::QueueContext {
                                    ctx: &ctx,
                                    guild_id: Some(server),
                                    db: Some(&self.db),
                                    manager: Some(self.manager.clone()),
                                };
                                if let Err(e) = category.queue_player_with_vc_status(player.clone(), discord_rank, queue_ctx, true).await {
                                    error!("Failed to add player to queue: {e}");
                                } else {
                                    let guild_name = pf_pug_bot::guild_name(&ctx, server);
                                    let category_name = category.name.as_deref().unwrap_or("Unknown");
                                    let pool_len: usize = category.formats[0].sessions.iter().map(|s| s.pool.len()).sum();
                                    let sg_name = category.formats.first().map(|sg| sg.name.as_str());
                                    
                                    // Check if queue was already full when this player joined
                                    if pool_len > category.quota() as usize {
                                        warn!("{} {} joined VC and was added to queue, but queue exceeded quota ({} > {})", 
                                              log_prefix_category(&guild_name, &category_name), tag, pool_len, category.quota());
                                    }
                                    
                                    log_queue_toggle(&guild_name, &category_name, &tag, QueueToggleType::VJ, Some((pool_len, category.quota() as usize)), sg_name, Some(pool_len));
                                }

                                category.queue_dash_update(&ctx, server).await;
                            }
                        }

                        // Get post-game timeout from database
                        let post_game_timeout = self.db.config.get_post_game_timeout(server).await.ok();
                        
                        if category.check_hot_timeout(&ctx, server, post_game_timeout).await {
                            info!("Hot session timeout detected, updating dashboard");
                            category.queue_dash_update(&ctx, server).await;
                        }
                    }
                },
                Err(_) => {
                    // Silently ignore - not a queue channel (expected for non-queue VCs)
                }
            }
        }
    }
}

impl Handler {
    /// Handle a player leaving the queue VC (disconnect or move away).
    /// Checks auto-leave preference, removes or resets timeout, and regenerates teams if needed.
    async fn handle_player_leave_vc(
        &self,
        ctx: &Context,
        category: &mut pf_pug_bot::models::Category,
        guild_id: serenity::all::GuildId,
        user_id: serenity::all::UserId,
        tag: &str,
    ) {
        let guild_name = pf_pug_bot::guild_name(&ctx, guild_id);
        let category_name = category.name.as_deref().unwrap_or("Unknown").to_string();
        let sg_name = category.get_user_sg_name(user_id);

        let quota = category.quota() as usize;

        let should_regenerate = if let Ok(sesh) = category.get_user_session(user_id).await {
            if sesh.is_active() {
                // Player is in an active session (Push/Live) - bot is moving them, not a voluntary leave
                return;
            }

            let was_hot = sesh.is_hot();

            let should_remove_player = {
                // Check if this is a post-game scenario (session was recently active)
                let is_post_game = sesh.ready_at.is_none() && !sesh.is_hot();
                
                if is_post_game {
                    // Post-game behavior: check server-wide post_game_auto_leave setting
                    self.db.config.get_bool(guild_id, "post_game_auto_leave", true).await.unwrap_or(true)
                } else if sesh.is_hot() {
                    // Hot game behavior: check user's vc_auto_leave preference
                    if let Ok(settings) = self.db.users.get_prefs(user_id).await {
                        settings.vc_auto_leave
                    } else {
                        false
                    }
                } else {
                    // Regular idle session: don't auto-remove
                    false
                }
            };

            if !should_remove_player {
                if let Some(player) = sesh.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                    player.joined_at = std::time::SystemTime::now();
                    player.in_queue_vc = false;
                }
            }

            // Capture position before removal for logging
            let position_before_removal = sesh.pool.iter().position(|p| p.player.user_id == user_id).map(|p| p + 1);
            
            if should_remove_player {
                sesh.remove_player(user_id);
            }

            // Log after removal so pool count is accurate, but use position before removal
            log_queue_toggle(&guild_name, &category_name, tag, VL, Some((sesh.pool.len(), quota)), sg_name.as_deref(), position_before_removal);

            if was_hot && sesh.pool.len() >= quota {
                true
            } else if was_hot && sesh.pool.len() < quota {
                sesh.idle();
                false
            } else {
                false
            }
        } else {
            // Player not found in any session
            log_queue_toggle(&guild_name, &category_name, tag, VL, None, sg_name.as_deref(), None);
            false
        };

        if should_regenerate {
            category.generate_teams(ctx, guild_id, Some(&self.db)).await;
        }
    }

    /// Check if bot has necessary permissions in the guild
    async fn check_bot_permissions(&self, ctx: &Context, guild: &Guild) -> (bool, String) {
        use serenity::all::Permissions;

        let mut missing_perms = Vec::new();

        // Get bot's member object in the guild
        let bot_user_id = ctx.cache.current_user().id;
        let bot_member = match guild.id.member(&ctx.http, bot_user_id).await {
            Ok(member) => member,
            Err(e) => {
                error!("Failed to get bot member: {e}");
                return (false, "Unable to check bot permissions".to_string());
            }
        };

        // Get bot's guild-level permissions
        let guild_permissions = guild.member_permissions(&bot_member);

        // Check required permissions
        let required_perms = [
            (Permissions::MOVE_MEMBERS,    "Move members"),
            (Permissions::SEND_MESSAGES,   "Send messages"),
            (Permissions::EMBED_LINKS,     "Embed links"),
            (Permissions::VIEW_CHANNEL,    "View channels"),
            (Permissions::MANAGE_CHANNELS, "Manage channels"),
        ];

        for (perm, name) in required_perms {
            if !guild_permissions.contains(perm) {
                missing_perms.push(name);
            }
        }

        if missing_perms.is_empty() {
            (true, String::new())
        } else {
            (false, missing_perms.join(", "))
        }
    }

    /// Check for users already in queue voice channels and add them to the queue
    async fn check_existing_voice_users(&self, ctx: &Context, guild: &Guild, manager: &mut Manager) {
        // Get the server from the manager
        let server = match manager.get_server(guild.id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get server from manager: {e}");
                return;
            }
        };

        // Iterate through all categories and check their queue voice channels
        for category in &mut server.categories {
            let queue_vc_id = category.channels.queue_vc;
            let _dashboard_channel = category.channels.dashboard;

            // Check if there's an idle session available
            let has_idle_session = !category.get_sessions_by_status(&SessionStatus::Idle).is_empty();

            if !has_idle_session {
                info!("No idle session available for existing users in {}", queue_vc_id);
                continue;
            }

            // Collect all players to add first (to avoid quota check per player)
            let mut players_to_add: Vec<(serenity::all::UserId, String)> = Vec::new();
            for (user_id, voice_state) in &guild.voice_states {
                // Check if user is in this queue voice channel
                if voice_state.channel_id == Some(queue_vc_id) {
                    if category.get_user_session(*user_id).await.is_ok() {
                        info!("User {} already in session, skipping", user_id);
                        continue;
                    }

                    let tag = if let Ok(player) = self.db.get_user(*user_id, ctx).await {
                        player.tag
                    } else {
                        "Unknown".to_string()
                    };

                    players_to_add.push((*user_id, tag));
                }
            }

            // Add all players to the session WITHOUT quota check
            let sg_name_owned = category.formats.first().map(|sg| sg.name.clone());
            let category_name = category.name.as_deref().unwrap_or("Unknown").to_string();
            if let Ok(session) = category.get_queue().await {
                // Get server and category names for logging
                let guild_name = guild.name.clone();

                use pf_pug_bot::handlers::player::resolve_player_for_queue;

                for (user_id, _tag) in &players_to_add {
                    let user_id = *user_id;

                    let player = match resolve_player_for_queue(ctx, &self.db, guild.id, user_id).await {
                        Ok((p, _rank)) => p,
                        Err(e) => {
                            error!("Failed to resolve player {} for queue: {e}", user_id);
                            continue;
                        }
                    };

                    let position = session.pool.len() + 1;
                    log_queue_toggle(&guild_name, &category_name, &player.tag.clone(), QueueToggleType::VJ, None, sg_name_owned.as_deref(), Some(position));
                    session.add_player(player);
                }
            }

            let users_added = players_to_add.len();
            if users_added > 0 {

                // NOW check quota once after all players added
                if category.is_quota() {
                    if let Err(e) = category.hot(ctx, Some(guild.id), Some(&self.db), Some(self.manager.clone())).await {
                        error!("Failed to transition to hot: {e}");
                    }
                }

                // Update the dashboard to reflect the new users
                category.queue_dash_update(ctx, guild.id).await;
            }
        }
    }

    /// Creates dashboard for a guild using in-memory categories from manager
    async fn create_guild_dashboard_from_manager(&self, ctx: &Context, guild: &Guild, manager: &mut Manager) {
        // FIRST: Check bot permissions
        let (has_perms, missing_perms) = self.check_bot_permissions(ctx, guild).await;

        if !has_perms {
            warn!("Bot is missing permissions in guild {}: {}", guild.name, missing_perms);

            // Create a warning dashboard in the first available text channel
            if let Some(channel) = guild.channels.values().find(|c| c.kind == serenity::all::ChannelType::Text) {
                let warning_embed = serenity::all::CreateEmbed::new()
                    .title("Missing bot permissions")
                    .description(format!(
                        "The bot is missing required permissions to function properly.\n\n\
                        **Missing Permissions:**\n{missing_perms}\n\n\
                        Please grant these permissions to the bot and click the button below to confirm.",
                    ))
                    .color(RED);

                let button = serenity::all::CreateButton::new("confirm_permissions")
                    .label("Confirm permissions")
                    .style(serenity::all::ButtonStyle::Success);

                let action_row = serenity::all::CreateActionRow::Buttons(vec![button]);

                let msg = serenity::all::CreateMessage::new()
                    .embed(warning_embed)
                    .components(vec![action_row]);

                if let Err(e) = channel.id.send_message(&ctx.http, msg).await {
                    error!("Failed to send permission warning: {e}");
                }
            }
            return;
        }

        // Get server from manager (already has categories with existing users loaded)
        let server = match manager.get_server(guild.id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get server from manager: {e}");
                return;
            }
        };

        for category in &mut server.categories {
            // Validate that the dashboard channel still exists
            let channel_id = category.channels.dashboard;
            let channel_exists = match ctx.http.get_channel(channel_id).await {
                Ok(_) => true,
                Err(e) => {
                    if e.to_string().contains("10003") || e.to_string().contains("Unknown channel") {
                        warn!("[{}] Dashboard channel {} no longer exists, skipping category", guild.name, channel_id);
                        false
                    } else {
                        warn!("[{}] Error checking dashboard channel {}: {}", guild.name, channel_id, e);
                        false
                    }
                }
            };

            if !channel_exists {
                continue;
            }

            // Check if dashboard already exists
            if category.has_dashboard(ctx).await {
                category.queue_dash_update(ctx, guild.id).await;
                continue;
            }

            // Create dashboard for each category's dashboard channel
            let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| "Unknown".to_string());

            // Create dashboard in the dashboard channel
            match category.dash_publish(ctx, channel_id, &self.db, guild.id).await {
                Ok(_) => {
                    info!("Dashboard created successfully for channel {}", channel_name);

                    // Persist the dashboard message ID to database
                    let dashboard_msg_id = category.dashboard_msg.get();
                    if let Err(e) = self.db.categories.update_dashboard_msg(
                        guild.id,
                        channel_id.get(),
                        dashboard_msg_id
                    ).await {
                        warn!("Failed to persist dashboard message ID to database: {e}");
                    } else {
                        info!("Persisted dashboard message ID {} to database", dashboard_msg_id);
                    }
                },
                Err(e) => {
                    error!("Failed to create dashboard for channel {}: {}", channel_name, e);
                }
            }
        }
    }
}

/// Main entry point for the PUG bot application.
/// Sets up tracing, loads environment variables, initializes the database connection,
/// configures the Discord client with necessary intents, and starts the bot.
#[tokio::main]
async fn main(
) -> Result<()> {
    // Initialize tracing with minimal, colored format
    let timer = UtcTime::new(format_description!("[hour]:[minute]:[second]"));
    tracing_subscriber::fmt()
        .with_ansi(true)
        .with_target(false)
        .with_timer(timer)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(true)
        .with_line_number(true)
        .with_level(false)
        .compact()
        .init();

    // Load environment variables
    dotenvy::dotenv().ok();
    let token        = env::var("DISCORD_TOKEN").expect("Expected a Discord token in the environment");
    let db_file      = env::var("DATABASE_URL").unwrap_or_else(|_| "./pf_pug_bot.db".to_string());
    let database_url = format!("sqlite:{db_file}");

    // Initialize database connection
    let db = Arc::new(Database::new(&database_url).await?);

    // Run database migrations
    let migrations = DatabaseMigrations::new(db.pool());
    migrations.create_tables().await?;
    migrations.verify_schemas().await?;

    // Configure the client with the framework and intents
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;

    // Define TypeMapKey for Manager
    struct GuildKey;
    impl TypeMapKey
        for GuildKey {
        type Value = Arc<Mutex<Manager>>;
    }

    // Init manager
    let manager = Arc::new(Mutex::new(Manager::default()));

    // Init dashboard queue (shared with Handler and shutdown handler)
    let dashboard_queue = Arc::new(tokio::sync::Mutex::new(None));

    // Init client
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler {
            db:              db.clone(),
            manager:         manager.clone(),
            dashboard_queue: dashboard_queue.clone(),
        })
        .await
        .expect("Failed to create client");

    // Set the manager in the client data for global access
    client.data.write().await.insert::<GuildKey>(manager.clone());

    // Set up signal handling for graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    
    // Clone necessary data for signal handler
    let manager_for_shutdown = manager.clone();
    let cache_for_shutdown = client.cache.clone();
    let http_for_shutdown = client.http.clone();
    let dashboard_queue_for_shutdown = dashboard_queue.clone();
    
    // Spawn signal handler task
    tokio::spawn(async move {
        use tokio::signal;
        
        // Wait for either SIGINT (Ctrl+C) or SIGTERM
        let sigint = async {
            signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
        };
        
        let sigterm = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");
                sigterm.recv().await;
                #[cfg(not(unix))]
                {
                    // On non-Unix systems, we'll never receive SIGTERM
                    std::future::pending::<()>().await;
                }
            }
        };
        
        // Wait for either signal
        tokio::select! {
            _ = sigint => {
                info!("Received Ctrl+C, shutting down gracefully...");
            }
            _ = sigterm => {
                info!("Received SIGTERM, shutting down gracefully...");
            }
        }
        
        // Stop the dashboard update queue to prevent race conditions
        // (voice state updates during shutdown could overwrite the offline message)
        {
            let mut queue_lock = dashboard_queue_for_shutdown.lock().await;
            let _ = queue_lock.take(); // Drop the queue, closing the channel
        }
        // Wait for any in-flight batch to finish
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Clean up empty team VCs before shutting down
        if let Ok(mut manager_lock) = manager_for_shutdown.try_lock() {
            for server in &mut manager_lock.servers {
                let guild_id = server.guild_id;

                // Collect voice channel members for this guild
                let vc_members: std::collections::HashSet<u64> = cache_for_shutdown.guild(guild_id)
                    .map(|g| g.voice_states.values()
                        .filter_map(|vs| vs.channel_id.map(|c| c.get()))
                        .collect())
                    .unwrap_or_default();

                for category in &mut server.categories {
                    let mut kept = Vec::new();
                    for tc in &category.channels.teams {
                        let red_empty = !vc_members.contains(&tc.red_vc.get());
                        let blu_empty = !vc_members.contains(&tc.blu_vc.get());

                        if red_empty && blu_empty {
                            let _ = tc.red_vc.delete(&http_for_shutdown).await;
                            let _ = tc.blu_vc.delete(&http_for_shutdown).await;
                        } else {
                            kept.push(tc.clone());
                        }
                    }
                    category.channels.teams = kept;
                }
            }
        } else {
            warn!("Could not acquire manager lock for team VC cleanup");
        }

        // Mark all dashboards as offline before shutting down
        if let Ok(mut manager_lock) = manager_for_shutdown.try_lock() {
            info!("Marking all dashboards as offline...");
            
            for server in &mut manager_lock.servers {
                let guild_id = server.guild_id;
                let guild_name = cache_for_shutdown.guild(guild_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                
                for category in &mut server.categories {
                    // Create offline dashboard embed
                    use serenity::all::CreateEmbedFooter;
                    
                    let offline_embed = CreateEmbed::new()
                        .title("🔴 qBot is offline...")
                        .color(0xFF0000) // Red color
                        .footer(CreateEmbedFooter::new(format!("Shutdown at {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"))));
                    
                    // Try to update the existing dashboard message
                    let channel_id = category.channels.dashboard;
                    let message_id = category.dashboard_msg;
                    
                    match channel_id.edit_message(&http_for_shutdown, message_id, EditMessage::new().embed(offline_embed).components(vec![])).await {
                        Ok(_) => {
                            info!("[{}] Marked dashboard for category {} as offline", guild_name, category.category_id);
                        }
                        Err(e) => {
                            warn!("[{}] Failed to update dashboard for category {}: {}", guild_name, category.category_id, e);
                        }
                    }
                }
            }
        } else {
            warn!("Could not acquire manager lock for graceful shutdown");
        }
        
        // Send shutdown signal to main task
        let _ = shutdown_tx.send(());
    });

    // Start listening for events by starting a single shard
    // Use select! to handle both client events and shutdown signal
    tokio::select! {
        result = client.start() => {
            if let Err(why) = result {
                error!("Client error: {:?}", why);
            }
        }
        _ = shutdown_rx => {
            info!("Shutting down client...");
        }
    }

    Ok(())
}
