use std::env;
use std::sync::Arc;

use time::macros::format_description;
use tracing_subscriber::fmt::time::UtcTime;
use anyhow::Result;
use pf_pug_bot::{Player, RED, commands, Rank};
use serenity::all::{
    Client, GatewayIntents, EventHandler, Ready, Guild,Interaction,
    VoiceState, Command, Context, User, CommandOptionType as COT,
    CreateEmbed, EditMessage, UserId
};
use serenity::prelude::TypeMapKey;
use serenity::async_trait;
use serenity::builder::{
    CreateCommand as CC, CreateCommandOption as CCO, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use pf_pug_bot::database::migrations::DatabaseMigrations;
use pf_pug_bot::database::repositories::GroupRepository;
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
            info!("DM tracker initialized with 10-minute cleanup");
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
                    
                    // Check all groups in all servers
                    for server in manager_lock.servers.iter_mut() {
                        let guild_id = server.guild_id;
                        for group in server.groups.iter_mut() {
                            if group.check_timeout(&database, &ctx_clone, guild_id).await {
                                // Players were removed, update dashboard
                                group.queue_dash_update(&ctx_clone, guild_id).await;
                            }
                        }
                    }
                }
            });
            info!("Timeout background task started (checks every 60 seconds)");
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

            cmd("clear",         "Clear all players from the queue"),
            cmd("elo",           "View ELO and rank information for a player")
                .op_user("user", "The Discord user (mention or ID, optional)", false),
            cmd("prefs",         "Open your preferences"),
            cmd("config",        "Open server settings"),
            cmd("editplayer",    "Open player menu")
                .op_user("user", "The Discord user to edit", true),
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
                // Load groups from database into manager
                let group_repo = GroupRepository::new(self.db.pool().clone());
                match group_repo.get_groups_for_guild(guild_id).await {
                    Ok(groups) => {
                        let mut manager = self.manager.lock().await;
                        if manager.get_server(guild.id).is_err() {
                            let mut server = Server::new(guild.id, guild.name.clone(), Roles::empty());
                            for group in groups {
                                if let Err(e) = server.add_group(group) {
                                    error!("Failed to add group: {e}");
                                }
                            }
                            let groups_len = server.groups.len();
                            manager.servers.push(server);

                            if groups_len > 0 {
                                self.check_existing_voice_users(&ctx, &guild, &mut manager).await;
                                self.create_guild_dashboard_from_manager(&ctx, &guild, &mut manager).await;
                            } else {
                                warn!("[{}] No valid group configurations.", guild.name);
                            }
                        }
                    },
                    Err(e) => {
                        error!("Failed to load groups for guild {}: {}", guild.name, e);
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
                let tag = match self.db.get_user(itx.user.id, &ctx).await {
                    Ok(player) => player.tag,
                    Err(_) => itx.user.name.clone(),
                };
                let cmd_ctx     = CommandContext {
                    ctx:     &ctx,
                    intax:   &itx,
                    db:      self.db.clone(),
                    manager: &self.manager.clone(),
                };
                let cd  = &itx.data;
                let cdo = &cd.options;

                let info = || {
                    let guild_name = itx.guild_id.and_then(|gid| ctx.cache.guild(gid).map(|g| g.name.clone())).unwrap_or_else(|| "DM".to_string());
                    info!("[{}] {} used /{}", guild_name, tag, itx.data.name);
                };

                // Handle commands that don't need a server/group first
                let result = match cd.name.as_str() {
                    "prefs" => {
                        info();
                        commands::cmd_prefs(&cmd_ctx).await
                    }
                    "config" => {
                        info();
                        commands::cmd_config(&cmd_ctx).await
                    }
                    "editplayer" => {
                        info();
                        commands::cmd_edit_player(&cmd_ctx).await
                    }
                    _ => {
                        // All other commands need a server
                        let mut manager = self.manager.lock().await;
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Server not found: {e}");
                                let response = CIR::Message(CIRM::new().content("Server not configured. Please use `/config` to create roles and groups.").ephemeral(true));
                                let _ = itx.create_response(&ctx.http, response).await;
                                return;
                            }
                        };

                        match cd.name.as_str() {
                            "buffer" => {
                                info();
                                if let Some(user_option) = cdo.first() {
                                    if let Some(user_id) = user_option.value.as_user_id() {
                                        admin::cmd_buffer(&cmd_ctx, server, user_id).await.expect("Failed to buffer player")
                                    } else {
                                        error!("Failed to parse user ID from buffer command");
                                    }
                                }
                                Ok(())
                            }
                            "fatkid" => {
                                info();
                                if let Some(user_option) = cdo.first() {
                                    if let Some(user_id) = user_option.value.as_user_id() {
                                        admin::cmd_fatkid(&cmd_ctx, server, user_id).await.expect("Failed to fatkid player")
                                    } else {
                                        error!("Failed to parse user ID from fatkid command");
                                    }
                                }
                                Ok(())
                            }
                            "clear" => {
                                info();
                                admin::cmd_clear_queue(&cmd_ctx, server).await
                            }
                            "elo" => {
                                info();
                                if let Some(user_option) = cdo.first() {
                                    if let Some(user_id) = user_option.value.as_user_id() {
                                        let user: Result<User, serenity::Error> = ctx.http.get_user(user_id).await;
                                        if let Ok(user) = user {
                                            admin::cmd_get_player_elo(&cmd_ctx, Some(user)).await
                                        } else {
                                            let response = CIR::Message(CIRM::new().content("Failed to get user").ephemeral(true));
                                            itx.create_response(&ctx.http, response).await.map_err(|e| e.into())
                                        }
                                    } else {
                                        let response = CIR::Message(CIRM::new().content("Invalid user specified").ephemeral(true));
                                        itx.create_response(&ctx.http, response).await.map_err(|e| e.into())
                                    }
                                } else {
                                    let response = CIR::Message(CIRM::new().content("No user specified").ephemeral(true));
                                    itx.create_response(&ctx.http, response).await.map_err(|e| e.into())
                                }
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
                        error!("Failed to send error response: {response_err}");
                    }
                }
            },
            Interaction::Component(itx) => {
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
                            // Get admin role from database config
                            match self.db.config.get_config_item("admin_role", guild_id).await {
                                Ok(Some(admin_role_str)) => {
                                    if let Ok(admin_role_id) = admin_role_str.parse::<u64>() {
                                        let admin_role = serenity::all::RoleId::new(admin_role_id);
                                        member.roles.contains(&admin_role)
                                    } else {
                                        false
                                    }
                                }
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
                        let error_response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content("Only administrators can confirm bot permissions.")
                                .ephemeral(true)
                        );
                        if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                            error!("Failed to send error response: {e}");
                        }
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
                        let error_response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content(format!("Still missing permissions: {missing_perms}"))
                                .ephemeral(true)
                        );
                        if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                            error!("Failed to send error response: {e}");
                        }
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
                if itx.data.custom_id.starts_with("server_settings_") || itx.data.custom_id.starts_with("link_ch_") {
                    let result = handlers::handle_server_settings_button(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling server settings interaction: {e}");
                    }
                    return;
                }

                // Handle group settings select menu
                if itx.data.custom_id == "group_settings_select" {
                    let result = handlers::handle_group_settings_select(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling group settings select: {e}");
                    }
                    return;
                }

                // Handle group settings team balance method select
                if itx.data.custom_id.starts_with("group_settings_balance_") {
                    let result = handlers::handle_group_settings_balance_select(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling group settings balance select: {e}");
                    }
                    return;
                }

                // Handle player settings rank selection
                if itx.data.custom_id.starts_with("player_settings_rank_select_") {
                    let result = handlers::handle_player_settings_rank_select(&ctx, &itx, &self.db).await;
                    if let Err(e) = result {
                        error!("Error handling player settings rank select: {e}");
                    }
                    return;
                }

                // Handle group settings buttons (including link message buttons)
                if itx.data.custom_id.starts_with("group_settings_") || itx.data.custom_id.starts_with("group_link_msg_") {
                    let result = handlers::handle_group_settings_button(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling group settings interaction: {e}");
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

                let group = match manager.get_group_by_channel(guild_id, channel_id) {
                    Ok(group) => group,
                    Err(_) => {
                        // Group not in manager - try to recover from database
                        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                        let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{channel_id}"));
                        info!("[{}] Group not found in manager for #{}, attempting recovery from database", guild_name, channel_name);

                        // Get the message ID from the interaction
                        let message_id = itx.message.id;
                        let guild_id_u64 = guild_id;
                        let channel_id_u64 = channel_id.get();
                        let message_id_u64 = message_id.get();

                        // Load groups from database for this guild
                        let group_repo = GroupRepository::new(self.db.pool().clone());
                        match group_repo.get_groups_for_guild(guild_id_u64).await {
                            Ok(groups) => {
                                // Find the group that matches this dashboard channel
                                if let Some(mut recovered_group) = groups.into_iter()
                                    .find(|g| g.channels.dashboard.get() == channel_id_u64)
                                {
                                    info!("[{}] Found group in database for #{}", guild_name, channel_name);

                                    // Update the dashboard message ID in the database
                                    if let Err(e) = group_repo.update_dashboard_msg(guild_id_u64, channel_id_u64, message_id_u64).await {
                                        error!("[{}] Failed to update dashboard message ID: {}", guild_name, e);
                                    } else {
                                        info!("[{}] Updated dashboard message ID in database", guild_name);
                                        // Update the in-memory group too
                                        recovered_group.dashboard_msg = message_id;
                                    }

                                    // Add the recovered group to the manager
                                    let server = manager.get_server(guild_id);
                                    if let Ok(server) = server {
                                        if let Err(e) = server.add_group(recovered_group) {
                                            error!("[{}] Failed to add recovered group: {}", guild_name, e);
                                        } else {
                                            info!("[{}] Recovered group added to manager", guild_name);
                                        }

                                        // Now get the group from the manager
                                        manager.get_group_by_channel(guild_id, channel_id).unwrap()
                                    } else {
                                        error!("[{}] Could not get server from manager", guild_name);
                                        let error_response = CIR::Message(
                                            CIRM::new()
                                                .content("Dashboard state was lost. Please run `/setup` to reconfigure.")
                                                .ephemeral(true)
                                        );
                                        if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                            error!("Failed to send error response: {e}");
                                        }
                                        return;
                                    }
                                } else {
                                    error!("[{}] No group found in database for #{}", guild_name, channel_name);
                                    let error_response = CIR::Message(
                                        CIRM::new()
                                            .content("Dashboard configuration not found. Please run `/config` to configure this server.")
                                            .ephemeral(true)
                                    );
                                    if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                        error!("Failed to send error response: {e}");
                                    }
                                    return;
                                }
                            },
                            Err(e) => {
                                error!("Failed to load groups from database: {e}");
                                let error_response = CIR::Message(
                                    CIRM::new()
                                        .content("Failed to access database. Please contact an administrator.")
                                        .ephemeral(true)
                                );
                                if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                    error!("Failed to send error response: {e}");
                                }
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

                let result = group.dash_handle_button_interaction(&comp_ctx).await;

                if let Err(e) = result {
                    error!("Error handling button '{}': {}", itx.data.custom_id, e);

                    // Try to respond with an error message if we haven't responded yet
                    let error_response = CIR::Message(CIRM::new().content("An error occurred while processing your button click").ephemeral(true));

                    if let Err(response_err) = itx.create_response(&ctx.http, error_response).await {
                        error!("Failed to send error response: {response_err}");
                    }
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
                    || itx.data.custom_id.starts_with("server_settings_group_modal_") 
                {
                    let result = handlers::handle_server_settings_modal(&ctx, &itx, &self.db).await;
                    if let Err(e) = result {
                        error!("Error handling server settings modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for group settings
                if itx.data.custom_id.starts_with("group_settings_modal_") {
                    let result = handlers::handle_group_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling group settings modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for linking dashboard message
                if itx.data.custom_id.starts_with("group_link_msg_modal_") {
                    let result = handlers::handle_group_link_msg_modal(&ctx, &itx, &self.db, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling group link message modal '{}': {}", itx.data.custom_id, e);
                    }
                }
                // Handle modal submissions for player settings
                if itx.data.custom_id.starts_with("player_settings_modal_") {
                    let result = handlers::handle_player_settings_modal(&ctx, &itx, &self.db).await;
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

        // First manager lock scope - released before line 660
        {
            let mut manager = self.manager.lock().await;

            // Determine which channel to use for group lookup based on state
        // For disconnects/moves, use old channel; for connects, use new channel
        let lookup_channel = match state {
            VoiceStateUpdate::Disconnected | VoiceStateUpdate::Moved => {
                // Extract old channel for these events
                match &old {
                    Some(s) => match s.channel_id {
                        Some(ch) => ch,
                        None => return, // No old channel to process
                    },
                    None => return, // No old state
                }
            },
            VoiceStateUpdate::Connected => {
                match new.channel_id {
                    Some(ch) => ch,
                    None => return, // Can't join nothing
                }
            },
            VoiceStateUpdate::Reconnected => return, // Early return for reconnects
        };

        let group = match manager.get_group_by_channel(server, lookup_channel) {
            Ok(g) => g,
            Err(_) => return, // Channel not configured for pug queue
        };

        match state {
            VoiceStateUpdate::Disconnected => {
                // Player disconnected from queue VC (not moved, actually left)
                let guild_name = ctx.cache.guild(server).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                let group_name = ctx.cache.channel(group.channels.dashboard)
                    .map(|ch| ch.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                log_queue_toggle(&guild_name, &group_name, &tag, VL);

                let quota = group.quota as usize;
                // Get session index before mutable borrow
                let _ = group.sessions.iter()
                    .position(|s| s.pool.iter().any(|p| p.player.user_id == user_id));

                let should_regenerate = if let Ok(sesh) = group.get_user_session(user_id).await {
                    if !sesh.is_active() {
                        let was_hot = sesh.is_hot();

                        // Check if player has auto-leave disabled
                        let should_remove_player = if let Ok(settings) = self.db.users.get_prefs(user_id).await {
                            if settings.vc_auto_leave {
                                // Auto-leave enabled - remove player from queue
                                true
                            } else {
                                // Auto-leave disabled - reset timeout and keep player in queue
                                if let Some(player) = sesh.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                                    // Reset the join time to restart the timeout
                                    player.joined_at = std::time::SystemTime::now();
                                    // Mark as not in VC anymore
                                    player.in_queue_vc = false;
                                    info!("Reset timeout for {} after leaving VC (auto-leave disabled)", tag);
                                }
                                false
                            }
                        } else {
                            // Default to removing player if settings can't be retrieved
                            true
                        };

                        if should_remove_player {
                            // Remove player from session when they disconnect
                            sesh.remove_player(user_id);
                        }

                        // If session was hot and still has enough players, regenerate teams
                        if was_hot && sesh.pool.len() >= quota {
                            true
                        } else if was_hot && sesh.pool.len() < quota {
                            // Dropped below quota, transition to Idle
                            sesh.idle();
                            false
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if should_regenerate {
                    group.generate_teams(&ctx, server, Some(&self.db)).await;
                }
                group.queue_dash_update(&ctx, server).await;
            },
            VoiceStateUpdate::Connected => {
                // Player addition is handled in the later section (lines 680+)
                // which properly uses get_or_assign_player_rank
                // This just ensures a session exists for them to join
                if group.get_inactives().is_empty() {
                    if let Err(e) = group.create_session() {
                        warn!("Failed to create session on VC connect: {e}");
                    }
                }
            },
            VoiceStateUpdate::Moved => {
                if group.channels.queue_vc == lookup_channel {
                    let guild_name = ctx.cache.guild(server).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                    let group_name = ctx.cache.channel(group.channels.dashboard)
                        .map(|ch| ch.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    log_queue_toggle(&guild_name, &group_name, &tag, VL);

                    let quota = group.quota as usize;
                    // Get session index before mutable borrow
                    let _ = group.sessions.iter()
                        .position(|s| s.pool.iter().any(|p| p.player.user_id == user_id));

                    let should_regenerate = if let Ok(sesh) = group.get_user_session(user_id).await {
                        if !sesh.is_active() {
                            let was_hot = sesh.is_hot();

                            // Check if player has auto-leave disabled
                            let should_remove_player = if let Ok(settings) = self.db.users.get_prefs(user_id).await {
                                if settings.vc_auto_leave {
                                    // Auto-leave enabled - remove player from queue
                                    true
                                } else {
                                    // Auto-leave disabled - reset timeout and keep player in queue
                                    if let Some(player) = sesh.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                                        // Reset the join time to restart the timeout
                                        player.joined_at = std::time::SystemTime::now();
                                        // Mark as not in VC anymore
                                        player.in_queue_vc = false;
                                        info!("Reset timeout for {} after leaving VC (auto-leave disabled)", tag);
                                    }
                                    false
                                }
                            } else {
                                // Default to removing player if settings can't be retrieved
                                true
                            };

                            if should_remove_player {
                                // Remove player from session when they move out of queue VC
                                sesh.remove_player(user_id);
                            }

                            // If session was hot and still has enough players, regenerate teams
                            if was_hot && sesh.pool.len() >= quota {
                                true
                            } else if was_hot && sesh.pool.len() < quota {
                                // Dropped below quota, transition to Idle
                                sesh.idle();
                                false
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if should_regenerate {
                        group.generate_teams(&ctx, server, Some(&self.db)).await;
                    }
                    // Queue count now only displayed in dashboard
                    group.queue_dash_update(&ctx, server).await;
                }
            },
            VoiceStateUpdate::Reconnected => {
                return;
            }
        }
        } // Release first manager lock here

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

            // Find the guild by ID and check if the new channel is a queue voice channel in any group
            match manager.get_group_by_channel(server, new.channel_id.unwrap()) {
                Ok(group) => {
                    if group.channels.queue_vc == new.channel_id.unwrap() {
                        // Check if player is already in any session and mark them as in VC
                        if let Ok(session) = group.get_user_session(user_id).await {
                            let was_hot = session.is_hot();
                            if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                                let was_missing = !player.in_queue_vc;
                                player.in_queue_vc = true;

                                // Update dashboard if player was missing in a hot session
                                // This removes them from the "Missing players" list
                                if was_hot && was_missing {
                                    info!("{} joined VC during hot session, updating dashboard", tag);
                                    group.queue_dash_update(&ctx, server).await;
                                }
                            }
                        } else {
                            // Player not in session yet - check if they want auto-queue
                            let user_prefs = self.db.users.get_prefs(user_id).await.unwrap_or_default();
                            if !user_prefs.vc_auto_join {
                                // User has disabled VC auto-queue, don't add them
                                return;
                            }

                            // Ensure a session exists before trying to add player
                            if group.get_inactives().is_empty() {
                                warn!("No idle sessions present when player {} joined VC, creating one", tag);
                                if let Err(e) = group.create_session() {
                                    error!("Failed to create session for player {}: {}", tag, e);
                                    return;
                                }
                            }

                            // Now we're guaranteed to have a session
                            {
                                // Get player rank: DB for speed, Discord roles for truth
                                use pf_pug_bot::handlers::player::{get_player_rank, get_user_rank_from_discord_roles, get_or_assign_player_rank};
                                
                                // First, get Discord role (source of truth) - returns GuildRank with ELO
                                let role_based_guild_rank = get_user_rank_from_discord_roles(&ctx, &self.db, server, user_id).await;
                                
                                // Convert to Rank struct and get ELO
                                let (discord_rank, rank_min_elo) = if let Some(db_rank) = get_player_rank(&self.db, server, user_id).await {
                                    // Player has existing rank in database
                                    if let Some(guild_rank) = &role_based_guild_rank {
                                        // Discord role exists - use its name to determine Rank struct
                                        let role_rank = Rank::from_name(&self.db, server, &guild_rank.name).await.unwrap_or(db_rank.clone());
                                        if role_rank != db_rank {
                                            info!("Voice join - Player {} Discord role '{}' (ELO {}) differs from DB {}, using Discord role", 
                                                  user_id, guild_rank.name, guild_rank.elo, db_rank.name);
                                        }
                                        (role_rank, guild_rank.elo)
                                    } else {
                                        // No Discord role, keep DB rank
                                        let elo = db_rank.elo;
                                        (db_rank, elo)
                                    }
                                } else {
                                    // No DB rank - check Discord roles before defaulting
                                    if let Some(guild_rank) = &role_based_guild_rank {
                                        // Has Discord role, use its name
                                        let role_rank = Rank::from_name(&self.db, server, &guild_rank.name).await.unwrap_or_else(|_| Rank {
                                            guild_id: server,
                                            role_id: guild_rank.role_id,
                                            name: guild_rank.name.clone(),
                                            elo: guild_rank.elo,
                                        });
                                        info!("Voice join - New player {} has Discord role '{}' (ELO {})", 
                                              user_id, guild_rank.name, guild_rank.elo);
                                        (role_rank, guild_rank.elo)
                                    } else {
                                        // No DB rank or Discord role, assign default
                                        match get_or_assign_player_rank(&self.db, server, user_id).await {
                                            Ok(rank) => {
                                                let elo = rank.elo;
                                                (rank, elo)
                                            },
                                            Err(e) => {
                                                error!("Failed to get or assign rank for user {}: {}", user_id, e);
                                                return;
                                            }
                                        }
                                    }
                                };
                                
                                // Get base player info
                                let mut player = match self.db.get_user(user_id, &ctx).await {
                                    Ok(p) => p,
                                    Err(_) => match self.db.new_user(user_id, &ctx).await {
                                        Ok(p) => p,
                                        Err(e) => {
                                            error!("Failed to create new user: {}", e);
                                            return;
                                        }
                                    }
                                };

                                // Get guild-specific ELO if exists
                                let existing_elo = self.db.elo.get_if_exists(user_id, server).await.ok().flatten();
                                
                                // Get the valid ELO range for the Discord rank
                                let rank_min_elo = discord_rank.elo;
                                let rank_max_elo = 101; // Simple upper bound
                                
                                if let Some(guild_elo) = existing_elo {
                                    if guild_elo.elo >= rank_min_elo && guild_elo.elo < rank_max_elo {
                                        // ELO is within the Discord rank's range - keep it
                                        info!("Voice join - Player {} ELO {} within {} range [{}, {}), keeping", 
                                              user_id, guild_elo.elo, discord_rank.name, rank_min_elo, rank_max_elo);
                                        player.elo = guild_elo.elo;
                                    } else {
                                        // ELO is outside the Discord rank's range - reset to rank default
                                        info!("Voice join - Player {} ELO {} outside {} range [{}, {}), resetting to {}", 
                                              user_id, guild_elo.elo, discord_rank.name, rank_min_elo, rank_max_elo, rank_min_elo);
                                        player.elo = rank_min_elo;
                                        if let Err(e) = self.db.elo.set(user_id, server, player.elo, discord_rank.clone()).await {
                                            warn!("Failed to update guild ELO: {}", e);
                                        }
                                    }
                                } else {
                                    // No ELO record - new player, use Discord rank's default ELO
                                    // Get the role_id for this rank for debugging
                                    let role_id_info = if let Ok(ranks) = self.db.ranks.get_ranks(server).await {
                                        ranks.iter()
                                            .find(|r| r.name == discord_rank.name)
                                            .map(|r| format!(" (role_id: {})", r.role_id))
                                            .unwrap_or_else(|| String::new())
                                    } else {
                                        String::new()
                                    };
                                    info!("Voice join - New player {} in guild, setting ELO to {} from Discord rank {}{}", 
                                          user_id, rank_min_elo, discord_rank.name, role_id_info);
                                    player.elo = rank_min_elo;
                                    if let Err(e) = self.db.elo.set(user_id, server, player.elo, discord_rank.clone()).await {
                                        warn!("Failed to initialize guild ELO: {}", e);
                                    }
                                }
                                player.rank = Some(discord_rank.clone());
                                
                                // Use queue_player_with_vc_status to set in_queue_vc BEFORE quota check/notification
                                let queue_ctx = pf_pug_bot::models::server::QueueContext {
                                    ctx: &ctx,
                                    guild_id: Some(server),
                                    db: Some(&self.db),
                                    manager: Some(self.manager.clone()),
                                };
                                if let Err(e) = group.queue_player_with_vc_status(player.clone(), discord_rank, queue_ctx, true).await {
                                    error!("Failed to add player to queue: {e}");
                                } else {
                                    // Log successful queue join via voice channel
                                    let guild_name = ctx.cache.guild(server).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                    let group_name = ctx.cache.channel(group.channels.dashboard)
                                        .map(|ch| ch.name.clone())
                                        .unwrap_or_else(|| "Unknown".to_string());
                                    log_queue_toggle(&guild_name, &group_name, &tag, QueueToggleType::VJ);
                                }

                                group.queue_dash_update(&ctx, server).await;
                            }
                        }

                        if group.check_hot_timeout(&ctx, server).await {
                            info!("Hot session timeout detected, updating dashboard");
                            group.queue_dash_update(&ctx, server).await;
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
        use pf_pug_bot::handlers::player::get_or_assign_player_rank;

        // Get the server from the manager
        let server = match manager.get_server(guild.id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get server from manager: {e}");
                return;
            }
        };

        // Iterate through all groups and check their queue voice channels
        for group in &mut server.groups {
            let queue_vc_id = group.channels.queue_vc;
            let dashboard_channel = group.channels.dashboard;

            // Check if there's an idle session available
            let has_idle_session = !group.get_sessions_by_status(&SessionStatus::Idle).is_empty();

            if !has_idle_session {
                info!("No idle session available for existing users in {}", queue_vc_id);
                continue;
            }

            // Collect all players to add first (to avoid quota check per player)
            let mut players_to_add: Vec<(serenity::all::UserId, String)> = Vec::new();
            for (user_id, voice_state) in &guild.voice_states {
                // Check if user is in this queue voice channel
                if voice_state.channel_id == Some(queue_vc_id) {
                    if group.get_user_session(*user_id).await.is_ok() {
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
            if let Ok(session) = group.get_queue().await {
                // Get server and group names for logging
                let guild_name = guild.name.clone();
                let group_name = ctx.cache.channel(dashboard_channel)
                    .map(|ch| ch.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                use pf_pug_bot::handlers::player::{get_player_rank, get_user_rank_from_discord_roles, get_or_assign_player_rank};
                
                for (user_id, _tag) in &players_to_add {
                    let user_id = *user_id;
                    // Use same rank detection as voice join: Discord roles for truth, DB for speed
                    let role_based_guild_rank = get_user_rank_from_discord_roles(&ctx, &self.db, guild.id, user_id).await;
                    
                    let (discord_rank, rank_min_elo) = if let Some(db_rank) = get_player_rank(&self.db, guild.id, user_id).await {
                        // Player has existing rank in database
                        if let Some(guild_rank) = &role_based_guild_rank {
                            // Discord role exists - use its name to determine Rank struct
                            let role_rank = Rank::from_name(&self.db, guild.id, &guild_rank.name).await.unwrap_or(db_rank.clone());
                            if role_rank != db_rank {
                                info!("Existing VC - Player {} Discord role '{}' (ELO {}) differs from DB {}, using Discord role", 
                                      user_id, guild_rank.name, guild_rank.elo, db_rank.name);
                            }
                            (role_rank, guild_rank.elo)
                        } else {
                            // No Discord role, keep DB rank
                            let elo = db_rank.elo;
                            (db_rank, elo)
                        }
                    } else {
                        // No DB rank - check Discord roles before defaulting
                        if let Some(guild_rank) = &role_based_guild_rank {
                            let role_rank = Rank::from_name(&self.db, guild.id, &guild_rank.name).await.unwrap_or_else(|_| Rank {
                                guild_id: guild.id,
                                role_id: guild_rank.role_id,
                                name: guild_rank.name.clone(),
                                elo: guild_rank.elo,
                            });
                            info!("Existing VC - Player {} has Discord role '{}' (ELO {})", user_id, guild_rank.name, guild_rank.elo);
                            (role_rank, guild_rank.elo)
                        } else {
                            // No DB rank or Discord role, assign default
                            match get_or_assign_player_rank(&self.db, guild.id, user_id).await {
                                Ok(rank) => {
                                    let elo = rank.elo;
                                    (rank, elo)
                                },
                                Err(e) => {
                                    error!("Failed to get or assign rank for user {}: {}", user_id, e);
                                    continue;
                                }
                            }
                        }
                    };
                    
                    // Get base player info
                    let mut player = match self.db.get_user(user_id, &ctx).await {
                        Ok(p) => p,
                        Err(_) => match self.db.new_user(user_id, &ctx).await {
                            Ok(p) => p,
                            Err(e) => {
                                error!("Failed to create new user: {}", e);
                                continue;
                            }
                        }
                    };
                    
                    // Get guild-specific ELO if exists
                    let existing_elo = self.db.elo.get_if_exists(user_id, guild.id).await.ok().flatten();
                    
                    // Get the valid ELO range for the Discord rank
                    let rank_min_elo = discord_rank.elo;
                    let rank_max_elo = 101; // Simple upper bound
                    
                    if let Some(guild_elo) = existing_elo {
                        if guild_elo.elo >= rank_min_elo && guild_elo.elo < rank_max_elo {
                            // ELO is within the Discord rank's range - keep it
                            player.elo = guild_elo.elo;
                        } else {
                            // ELO is outside the Discord rank's range - reset to rank default
                            info!("Existing VC - Player {} ELO {} outside {} range [{}, {}), resetting to {}", 
                                  user_id, guild_elo.elo, discord_rank.name, rank_min_elo, rank_max_elo, rank_min_elo);
                            player.elo = rank_min_elo;
                            if let Err(e) = self.db.elo.set(user_id, guild.id, player.elo, discord_rank.clone()).await {
                                warn!("Failed to update guild ELO: {}", e);
                            }
                        }
                    } else {
                        // No ELO record - use Discord rank's default ELO
                        // Get the role_id for this rank for debugging
                        let role_id_info = if let Ok(ranks) = self.db.ranks.get_ranks(guild.id).await {
                            ranks.iter()
                                .find(|r| r.name == discord_rank.name)
                                .map(|r| format!(" (role_id: {})", r.role_id))
                                .unwrap_or_else(|| String::new())
                        } else {
                            String::new()
                        };
                        info!("Existing VC - New player {} in guild, setting ELO to {} from Discord rank {}{}", 
                              user_id, rank_min_elo, discord_rank.name, role_id_info);
                        player.elo = rank_min_elo;
                        if let Err(e) = self.db.elo.set(user_id, guild.id, player.elo, discord_rank.clone()).await {
                            warn!("Failed to initialize guild ELO: {}", e);
                        }
                    }
                    player.rank = Some(discord_rank);
                    
                    log_queue_toggle(&guild_name, &group_name, &player.tag.clone(), QueueToggleType::VJ);
                    session.add_player(player);
                }
            }

            let users_added = players_to_add.len();
            if users_added > 0 {

                // NOW check quota once after all players added
                if group.is_quota() {
                    if let Err(e) = group.hot(ctx, Some(guild.id), Some(&self.db), Some(self.manager.clone())).await {
                        error!("Failed to transition to hot: {e}");
                    }
                }

                // Update the dashboard to reflect the new users
                group.queue_dash_update(ctx, guild.id).await;
            }
        }
    }

    /// Creates dashboard for a guild using in-memory groups from manager
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

        // Get server from manager (already has groups with existing users loaded)
        let server = match manager.get_server(guild.id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get server from manager: {e}");
                return;
            }
        };

        for group in &mut server.groups {
            // Validate that the dashboard channel still exists
            let channel_id = group.channels.dashboard;
            let channel_exists = match ctx.http.get_channel(channel_id).await {
                Ok(_) => true,
                Err(e) => {
                    if e.to_string().contains("10003") || e.to_string().contains("Unknown channel") {
                        warn!("[{}] Dashboard channel {} no longer exists, skipping group", guild.name, channel_id);
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
            if group.has_dashboard(ctx).await {
                group.queue_dash_update(ctx, guild.id).await;
                continue;
            }

            // Create dashboard for each group's dashboard channel
            let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| "Unknown".to_string());

            // Create dashboard in the dashboard channel
            match group.dash_publish(ctx, channel_id, &self.db, guild.id).await {
                Ok(_) => {
                    info!("Dashboard created successfully for channel {}", channel_name);

                    // Persist the dashboard message ID to database
                    let dashboard_msg_id = group.dashboard_msg.get();
                    if let Err(e) = self.db.groups.update_dashboard_msg(
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
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILDS;

    // Define TypeMapKey for Manager
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
            db:              db.clone(),
            manager:         manager.clone(),
            dashboard_queue: Arc::new(tokio::sync::Mutex::new(None)),
        })
        .await
        .expect("Failed to create client");

    // Set the manager in the client data for global access
    client.data.write().await.insert::<GuildKey>(manager.clone());

    // Set up signal handling for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
    
    // Clone necessary data for signal handler
    let manager_for_shutdown = manager.clone();
    let cache_for_shutdown = client.cache.clone();
    let http_for_shutdown = client.http.clone();
    
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
        
        // Mark all dashboards as offline before shutting down
        if let Ok(mut manager_lock) = manager_for_shutdown.try_lock() {
            info!("Marking all dashboards as offline...");
            
            for server in &mut manager_lock.servers {
                let guild_id = server.guild_id;
                let guild_name = cache_for_shutdown.guild(guild_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                
                for group in &mut server.groups {
                    // Create offline dashboard embed
                    use serenity::all::CreateEmbedFooter;
                    
                    let offline_embed = CreateEmbed::new()
                        .title("🔴 qBot is offline...")
                        .color(0xFF0000) // Red color
                        .footer(CreateEmbedFooter::new(format!("Shutdown at {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"))));
                    
                    // Try to update the existing dashboard message
                    let channel_id = group.channels.dashboard;
                    let message_id = group.dashboard_msg;
                    
                    match channel_id.edit_message(&http_for_shutdown, message_id, EditMessage::new().embed(offline_embed)).await {
                        Ok(_) => {
                            info!("[{}] Marked dashboard for group {} as offline", guild_name, group.group_id);
                        }
                        Err(e) => {
                            warn!("[{}] Failed to update dashboard for group {}: {}", guild_name, group.group_id, e);
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
