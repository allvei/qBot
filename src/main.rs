mod command;

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
use pf_pug_bot::{ButtonType, CommandContext, ComponentContext, DashboardQueueKey, DashboardUpdateQueue, Database, Group, Manager, QueueToggleType::{self, *}, Roles, Server, SessionStatus, VoiceStateUpdate, log_queue_toggle};
use pf_pug_bot::models::constants::VoiceStateUpdate::*;

fn cmd(name: impl Into<String,>,desc: impl Into<String,>,) -> CC {
    CC::new(name.into(),).description(desc.into(),)
}

pub trait CmdOp:
    Sized {
    fn op(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
    fn op_user(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self;
}

impl CmdOp for CC {
    /// Adds a string option to the command
    fn op(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::String, name, desc).required(req))
    }
    
    /// Adds a user option to the command
    fn op_user(self,name: impl Into<String,>,desc: impl Into<String,>,req: bool,) -> Self {
        self.add_option(CCO::new(COT::User, name, desc).required(req))
    }
}

struct Handler {
    database: Arc<Database>,
    manager:  Arc<Mutex<Manager>>,
    dashboard_queue: Arc<tokio::sync::Mutex<Option<DashboardUpdateQueue>>>,
}

/// Handler for Discord events
#[async_trait]
impl EventHandler for Handler {
    /// When the bot is ready
    async fn ready(&self,ctx: Context,ready: Ready,) {
        let guild_count = ctx.cache.guilds().len();

        // Initialize dashboard update queue (done once on ready)
        {
            let mut queue_lock = self.dashboard_queue.lock().await;
            if queue_lock.is_none() {
                let queue = DashboardUpdateQueue::new(ctx.clone(), self.manager.clone(), self.database.clone());
                let queue_arc = Arc::new(queue.clone());
                *queue_lock = Some(queue);
                
                // Store in Context data for global access
                ctx.data.write().await.insert::<DashboardQueueKey>(queue_arc);
            }
        }

        // Spawn console command handler in a separate task
        let console_handler = command::ConsoleHandler::new(
            self.manager.clone(),
            self.database.clone(),
            Arc::new(ctx.clone()),
        );

        tokio::spawn(async move {
            console_handler.start_console_loop().await;
        });

        // Register slash commands globally or for specific guild
        let cmds = vec![
            // Player commands
            cmd("buffer", "Buffer a player").op_user("user",  "User to buffer", true),
            
            // Setup commands
            cmd("setupadd",   "Create roles and group (full setup)"),
            cmd("setuplink",  "Guide to link existing roles and channels"),
            
            // Role commands
            cmd("roleadd",    "Create runner and admin roles"),
            cmd("rolelink",   "Link existing runner and admin roles").op("runner_role", "Runner role to link", false)
                                                                     .op("admin_role",  "Admin role to link",  false),
            cmd("roledel", "Remove role configuration")              .op("role_type",   "Role type: runner, admin, or both", true),

            // Rank commands
            cmd("rankadd",    "Add Discord role(s) to a rank (supports multiple roles)")      .op("rank", "Rank name", true)
                                                                                             .op("role", "Discord roles to add", true),
            cmd("rankremove", "Remove Discord role(s) from a rank (supports multiple roles)").op("rank", "Rank name", true)
                                                                                             .op("role", "Discord roles to remove", true),
            cmd("ranklist",   "List all role mappings for ranks")                            .op("rank", "Rank name to filter (optional)", false),

            // Group commands
            cmd("groupadd",    "Create a new category with all group channels"),
            cmd("grouplink",   "Link existing channels to a group"),
            cmd("groupremove", "Remove a group")                     .op("group_id", "Group ID to remove (defaults to current channel's group)", false),

            // Admin commands
            cmd("quotaset",    "Set the queue quota")            .op("quota", "Number of players required (2-100)", true),
            cmd("connectadd",  "Set server connection info")     .op("connect_info", "Server connect command (e.g., connect 192.168.10.10:27015)", true),
            cmd("clear",       "Clear all players from the queue"),
            cmd("ranksetelo",  "Set custom ELO value for a rank").op("rank_role", "The rank role (mention or ID)", true)
                                                                 .op("elo", "ELO value (1-100)", true),
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
                // Load groups from database into manager
                let group_repo = GroupRepository::new(self.database.pool().clone());
                match group_repo.get_groups_for_guild(guild_id).await {
                    Ok(groups) => {
                        let mut manager = self.manager.lock().await;
                        if manager.get_server(guild.id).is_err() {
                            let mut server = Server::new(guild.id, guild.name.clone(), Roles::empty());
                            for group in groups {
                                if let Err(e) = server.add_group(group) {
                                    error!("Failed to add group: {}", e);
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
                let discord_tag = match self.database.get_user(itx.user.id).await {
                    Ok(player) => player.discord_tag.unwrap_or_else(|| itx.user.name.clone()),
                    Err(_) => itx.user.name.clone(),
                };
                let cmd_ctx     = CommandContext {
                    ctx:     &ctx,
                    intax:   &itx,
                    db:      self.database.clone(),
                    manager: &self.manager.clone(),
                };
                let cd          = &itx.data;
                let cdo         = &cd.options;

                let info = || {
                    let guild_name = itx.guild_id.and_then(|gid| ctx.cache.guild(gid).map(|g| g.name.clone())).unwrap_or_else(|| "DM".to_string());
                    info!("[{}] {} used /{}", guild_name, discord_tag, itx.data.name);
                };

                // Handle commands that don't need a server/group first
                let result = match cd.name.as_str() {
                    "setup" => {
                        info();
                        admin::cmd_setup(&cmd_ctx).await
                    }
                    "roleadd" => {
                        info();
                        pf_pug_bot::handlers::role_commands::cmd_role_add(&cmd_ctx).await
                    }
                    "rolelink" => {
                        info();
                        let runner_role = cdo.iter().find(|opt| opt.name == "runner_role").and_then(|opt| opt.value.as_str()).map(|s| s.to_string());
                        let admin_role  = cdo.iter().find(|opt| opt.name == "admin_role").and_then(|opt| opt.value.as_str()).map(|s| s.to_string());
                        pf_pug_bot::handlers::role_commands::cmd_role_link(&cmd_ctx, runner_role, admin_role).await
                    }
                    "roleremove" => {
                        info();
                        let role_type = cdo.iter().find(|opt| opt.name == "role_type").and_then(|opt| opt.value.as_str()).unwrap_or("both").to_string();
                        pf_pug_bot::handlers::role_commands::cmd_role_remove(&cmd_ctx, role_type).await
                    }
                    "setupadd" => {
                        info();
                        let mut manager = self.manager.lock().await;
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(_) => {
                                // Create new server if it doesn't exist
                                let guild_id = itx.guild_id.unwrap();
                                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                let new_server = Server::new(guild_id, guild_name, Roles::empty());
                                manager.servers.push(new_server);
                                manager.servers.last_mut().unwrap()
                            }
                        };
                        pf_pug_bot::handlers::setup_commands::cmd_setup_add(&cmd_ctx, server).await
                    }
                    "setuplink" => {
                        info();
                        pf_pug_bot::handlers::setup_commands::cmd_setup_link(&cmd_ctx).await
                    }
                    "rankadd" => {
                        info();
                        let rank_name = cdo.iter()
                            .find(|opt| opt.name == "rank")
                            .and_then(|opt| opt.value.as_str())
                            .unwrap_or("")
                            .to_string();
                        let role_mention = cdo.iter()
                            .find(|opt| opt.name == "role")
                            .and_then(|opt| opt.value.as_str())
                            .unwrap_or("")
                            .to_string();
                        pf_pug_bot::handlers::role_commands::cmd_rank_add(&cmd_ctx, rank_name, role_mention).await
                    }
                    "rankremove" => {
                        info();
                        let rank_name = cdo.iter()
                            .find(|opt| opt.name == "rank")
                            .and_then(|opt| opt.value.as_str())
                            .unwrap_or("")
                            .to_string();
                        let role_mention = cdo.iter()
                            .find(|opt| opt.name == "role")
                            .and_then(|opt| opt.value.as_str())
                            .unwrap_or("")
                            .to_string();
                        pf_pug_bot::handlers::role_commands::cmd_rank_remove(&cmd_ctx, rank_name, role_mention).await
                    }
                    "ranklist" => {
                        info();
                        let rank_name = cdo.iter()
                            .find(|opt| opt.name == "rank")
                            .and_then(|opt| opt.value.as_str())
                            .map(|s| s.to_string());
                        pf_pug_bot::handlers::role_commands::cmd_rank_list(&cmd_ctx, rank_name).await
                    }
                    "quotaset" => {
                        info();
                        let quota = cdo.iter()
                            .find(|opt| opt.name == "quota")
                            .and_then(|opt| opt.value.as_str())
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
                        admin::cmd_set_quota(&cmd_ctx, quota).await
                    }
                    "connectadd" => {
                        info();
                        let connect_info = cdo.iter()
                            .find(|opt| opt.name == "connect_info")
                            .and_then(|opt| opt.value.as_str())
                            .unwrap_or("")
                            .to_string();
                        admin::cmd_add_connect(&cmd_ctx, connect_info).await
                    }
                    "groupadd" => {
                        info();
                        let mut manager = self.manager.lock().await;
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(_) => {
                                // Create new server if it doesn't exist
                                let guild_id = itx.guild_id.unwrap();
                                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                let new_server = Server::new(guild_id, guild_name, Roles::empty());
                                manager.servers.push(new_server);
                                manager.servers.last_mut().unwrap()
                            }
                        };
                        admin::cmd_group_add(&cmd_ctx, server).await
                    }
                    "grouplink" => {
                        info();
                        let mut manager = self.manager.lock().await;
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(_) => {
                                // Create new server if it doesn't exist
                                let guild_id = itx.guild_id.unwrap();
                                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                let new_server = Server::new(guild_id, guild_name, Roles::empty());
                                manager.servers.push(new_server);
                                manager.servers.last_mut().unwrap()
                            }
                        };
                        admin::cmd_group_link(&cmd_ctx, server).await
                    }
                    "groupremove" => {
                        info();
                        let mut manager = self.manager.lock().await;
                        let group_id = cdo.iter()
                            .find(|opt| opt.name == "group_id")
                            .and_then(|opt| opt.value.as_str())
                            .and_then(|s| s.parse::<u8>().ok())
                            .unwrap_or(0);
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Server not found: {}", e);
                                let response = CIR::Message(CIRM::new().content("No groups found. Please create one with `/groupadd` first.").ephemeral(true));
                                let _ = itx.create_response(&ctx.http, response).await;
                                return;
                            }
                        };
                        admin::cmd_group_remove(&cmd_ctx, server, group_id).await
                    }
                    _ => {
                        // All other commands need a server
                        let mut manager = self.manager.lock().await;
                        let server = match manager.get_server(itx.guild_id.unwrap()) {
                            Ok(s) => s,
                            Err(e) => {
                                error!("Server not found: {}", e);
                                let response = CIR::Message(CIRM::new().content("Server not configured. Please run `/setupadd` or `/groupadd` first.").ephemeral(true));
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
                            "clear" => {
                                info();
                                admin::cmd_clear_queue(&cmd_ctx, server).await
                            }
                            "ranksetelo" => {
                                info();
                                let rank_role = cdo.iter()
                                    .find(|opt| opt.name == "rank_role")
                                    .and_then(|opt| opt.value.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let elo = cdo.iter()
                                    .find(|opt| opt.name == "elo")
                                    .and_then(|opt| opt.value.as_str())
                                    .and_then(|s| s.parse::<i64>().ok())
                                    .unwrap_or(0);
                                admin::cmd_rank_set_elo(&cmd_ctx, rank_role, elo).await
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
                let discord_tag = match self.database.get_user(itx.user.id).await {
                    Ok(player) => player.discord_tag.unwrap_or_else(|| itx.user.name.clone()),
                    Err(_) => itx.user.name.clone(),
                };
                let button_type = ButtonType::parse(&itx.data.custom_id);

                // Handle permission confirmation button
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
                            match self.database.config.get_config_value("admin_role", guild_id.get()).await {
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
                            error!("Failed to send error response: {}", e);
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
                                .content(format!("Still missing permissions: {}", missing_perms))
                                .ephemeral(true)
                        );
                        if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                            error!("Failed to send error response: {}", e);
                        }
                    } else {
                        // Permissions granted! Delete the warning message and create dashboard
                        if let Err(e) = itx.message.delete(&ctx.http).await {
                            error!("Failed to delete permission warning: {}", e);
                        }

                        let success_response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content("Permissions confirmed! Setting up dashboard...")
                                .ephemeral(true)
                        );
                        if let Err(e) = itx.create_response(&ctx.http, success_response).await {
                            error!("Failed to send success response: {}", e);
                        }

                        // Now create the dashboard
                        let mut manager = self.manager.lock().await;
                        self.create_guild_dashboard_from_manager(&ctx, &guild, &mut manager).await;
                    }
                    return;
                }

                // Handle rank role creation buttons
                if matches!(button_type, ButtonType::CreateRankRolesYes | ButtonType::CreateRankRolesNo) {
                    let create = matches!(button_type, ButtonType::CreateRankRolesYes);
                    let result = admin::handle_create_rank_roles(&ctx, &self.database, &itx, create).await;
                    if let Err(e) = result {
                        error!("Error handling rank role creation: {}", e);
                    }
                    return;
                }

                // Handle setup/init interactions first (no group needed)
                if button_type.is_setup_button() {
                    let result = admin::handle_setup_interaction(&ctx, &itx, &self.database, &self.manager).await;
                    if let Err(e) = result {
                        error!("Error handling setup interaction: {}", e);
                    }
                    return;
                }

                // For dashboard button interactions, we need a group
                let mut manager = self.manager.lock().await;
                let guild_id = itx.guild_id.unwrap();
                let channel_id = itx.channel_id;

                // Try to get the group from the manager
                let group = match manager.get_group(guild_id, channel_id) {
                    Ok(group) => group,
                    Err(_) => {
                        // Group not in manager - try to recover from database
                        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                        let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{}", channel_id));
                        info!("[{}] Group not found in manager for #{}, attempting recovery from database", guild_name, channel_name);

                        // Get the message ID from the interaction
                        let message_id = itx.message.id;
                        let guild_id_u64 = guild_id.get();
                        let channel_id_u64 = channel_id.get();
                        let message_id_u64 = message_id.get();

                        // Load groups from database for this guild
                        let group_repo = GroupRepository::new(self.database.pool().clone());
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
                                        manager.get_group(guild_id, channel_id).unwrap()
                                    } else {
                                        error!("[{}] Could not get server from manager", guild_name);
                                        let error_response = CIR::Message(
                                            CIRM::new()
                                                .content("Dashboard state was lost. Please run `/setup` to reconfigure.")
                                                .ephemeral(true)
                                        );
                                        if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                            error!("Failed to send error response: {}", e);
                                        }
                                        return;
                                    }
                                } else {
                                    error!("[{}] No group found in database for #{}", guild_name, channel_name);
                                    let error_response = CIR::Message(
                                        CIRM::new()
                                            .content("Dashboard configuration not found. Please run `/setup` to configure this channel.")
                                            .ephemeral(true)
                                    );
                                    if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                        error!("Failed to send error response: {}", e);
                                    }
                                    return;
                                }
                            },
                            Err(e) => {
                                error!("Failed to load groups from database: {}", e);
                                let error_response = CIR::Message(
                                    CIRM::new()
                                        .content("Failed to access database. Please contact an administrator.")
                                        .ephemeral(true)
                                );
                                if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                    error!("Failed to send error response: {}", e);
                                }
                                return;
                            }
                        }
                    }
                };

                // Create component context similar to command context
                let comp_ctx = ComponentContext {
                    ctx:       &ctx,
                    component: &itx,
                    db:        self.database.clone(),
                    manager:   &self.manager,
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
        let state   = VoiceStateUpdate::get(&old, &new);
        let user_id = new.user_id;
        let user    = match ctx.http.get_user(user_id).await {
            Ok(u) => u,
            Err(e) => {
                error!("Failed to get user {}: {}", user_id, e);
                return;
            }
        };
        
        // Get player discord_tag from database
        let discord_tag = match self.database.get_user(user_id).await {
            Ok(player) => player.discord_tag.unwrap_or_else(|| user.display_name().to_string()),
            Err(_) => user.display_name().to_string(),
        };
        
        let server      = match new.guild_id {
            Some(s) => s,
            None => {return;}
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

        let group = match manager.get_group(server, lookup_channel) {
            Ok(g) => g,
            Err(_) => return, // Channel not configured for pug queue
        };

        match state {
            VoiceStateUpdate::Disconnected => {
                if group.channels.queue_vc == lookup_channel {
                    let guild_name = ctx.cache.guild(server).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                    let group_name = ctx.cache.channel(group.channels.dashboard)
                        .map(|ch| ch.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    log_queue_toggle(&guild_name, &group_name, &discord_tag, VL);

                    let quota = group.quota as usize;
                    // Get session index before mutable borrow
                    let session_idx = group.sessions.iter()
                        .position(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

                    let should_regenerate = if let Ok(sesh) = group.get_user_session(user_id).await {
                        if !sesh.is_active() {
                            let was_hot = sesh.is_hot();
                            
                            // Remove player from session when they leave VC
                            sesh.remove_player(user_id);
                            
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
                        group.generate_teams(&ctx, server, Some(&self.database)).await;
                    }
                    group.queue_dash_update(&ctx, server.get()).await;
                }
            },
            VoiceStateUpdate::Connected => {
                // Player addition is handled in the later section (lines 680+)
                // which properly uses get_or_assign_player_rank
                // This just ensures a session exists for them to join
                if group.get_inactives().is_empty() {
                    group.create_session();
                }
            },
            VoiceStateUpdate::Moved => {
                if group.channels.queue_vc == lookup_channel {
                    let guild_name = ctx.cache.guild(server).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                    let group_name = ctx.cache.channel(group.channels.dashboard)
                        .map(|ch| ch.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    log_queue_toggle(&guild_name, &group_name, &discord_tag, VL);

                    let quota = group.quota as usize;
                    // Get session index before mutable borrow
                    let session_idx = group.sessions.iter()
                        .position(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

                    let should_regenerate = if let Ok(sesh) = group.get_user_session(user_id).await {
                        if !sesh.is_active() {
                            let was_hot = sesh.is_hot();
                            
                            // Remove player from session when they move out of queue VC
                            sesh.remove_player(user_id);
                            
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
                        group.generate_teams(&ctx, server, Some(&self.database)).await;
                    }
                    group.queue_dash_update(&ctx, server.get()).await;
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
        let player = match self.database.get_user_with_tag(user_id, &ctx).await {
            Ok(user) => user,
            Err(_) => match self.database.new_user_with_tag(user_id, &ctx).await {
                    Ok(new_user) => new_user,
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
            match manager.get_group(server, new.channel_id.unwrap()) {
                Ok(group) => {
                    if group.channels.queue_vc == new.channel_id.unwrap() {
                        // Check if player is already in any session and mark them as in VC
                        if let Ok(session) = group.get_user_session(user_id).await {
                            let was_hot = session.is_hot();
                            if let Some(player) = session.pool.iter_mut().find(|p| p.player.discord_id == user_id) {
                                let was_missing = !player.in_queue_vc;
                                player.in_queue_vc = true;
                                
                                // Update dashboard if player was missing in a hot session
                                // This removes them from the "Missing players" list
                                if was_hot && was_missing {
                                    info!("{} joined VC during hot session, updating dashboard", discord_tag);
                                    group.queue_dash_update(&ctx, server.get()).await;
                                }
                            }
                        } else {
                            // Player not in session yet, add them
                            if group.get_inactives().is_empty() {
                                error!("No idle sessions present.");
                            } else {
                                // Get or assign player rank (auto-creates ranks and assigns Apprentice if needed)
                                use pf_pug_bot::handlers::player::get_or_assign_player_rank;
                                match get_or_assign_player_rank(&ctx, &self.database, server, user_id).await {
                                    Ok(rank) => {
                                        // Use queue_player_with_vc_status to set in_queue_vc BEFORE quota check/notification
                                        if let Err(e) = group.queue_player_with_vc_status(player.clone(), rank, &ctx, Some(server), Some(&self.database), Some(self.manager.clone()), true).await {
                                            error!("Failed to add player to queue: {}", e);
                                        } else {
                                            // Log successful queue join via voice channel
                                            let guild_name = ctx.cache.guild(server).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                            let group_name = ctx.cache.channel(group.channels.dashboard)
                                                .map(|ch| ch.name.clone())
                                                .unwrap_or_else(|| "Unknown".to_string());
                                            log_queue_toggle(&guild_name, &group_name, &discord_tag, QueueToggleType::VJ);
                                        }

                                        group.queue_dash_update(&ctx, server.get()).await;
                                    },
                                    Err(e) => {
                                        warn!("{} failed to get or assign rank: {}", discord_tag, e);
                                    }
                                }
                            }
                        }

                        if group.check_hot_timeout(&ctx, server).await {
                            info!("Hot session timeout detected, updating dashboard");
                            group.queue_dash_update(&ctx, server.get()).await;
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
                error!("Failed to get bot member: {}", e);
                return (false, "Unable to check bot permissions".to_string());
            }
        };

        // Get bot's guild-level permissions
        let guild_permissions = guild.member_permissions(&bot_member);

        // Check required permissions
        if !guild_permissions.contains(Permissions::MOVE_MEMBERS) {
            missing_perms.push("Move Members");
        }
        if !guild_permissions.contains(Permissions::SEND_MESSAGES) {
            missing_perms.push("Send Messages");
        }
        if !guild_permissions.contains(Permissions::EMBED_LINKS) {
            missing_perms.push("Embed Links");
        }
        if !guild_permissions.contains(Permissions::VIEW_CHANNEL) {
            missing_perms.push("View Channels");
        }
        if !guild_permissions.contains(Permissions::MANAGE_CHANNELS) {
            missing_perms.push("Manage Channels");
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
                error!("Failed to get server from manager: {}", e);
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
            let mut players_to_add = Vec::new();
            for (user_id, voice_state) in &guild.voice_states {
                // Check if user is in this queue voice channel
                if voice_state.channel_id == Some(queue_vc_id) {
                    if group.get_user_session(*user_id).await.is_ok() {
                        info!("User {} already in session, skipping", user_id);
                        continue;
                    }

                    let discord_tag = if let Ok(player) = self.database.get_user(*user_id).await {
                        if let Some(tag) = player.discord_tag {
                            tag
                        } else {
                            match ctx.http.get_user(*user_id).await {
                                Ok(user) => user.display_name().to_string(),
                                Err(_) => user_id.to_string(),
                            }
                        }
                    } else {
                        match ctx.http.get_user(*user_id).await {
                            Ok(user) => user.display_name().to_string(),
                            Err(_) => user_id.to_string(),
                        }
                    };

                    match get_or_assign_player_rank(ctx, &self.database, guild.id, *user_id).await {
                        Ok(rank) => {
                            players_to_add.push((*user_id, rank, discord_tag));
                        },
                        Err(e) => {
                            warn!("Failed to get or assign rank for existing user {}: {}", discord_tag, e);
                        }
                    }
                }
            }

            // Add all players to the session WITHOUT quota check
            if let Ok(session) = group.get_queue().await {
                // Get server and group names for logging
                let guild_name = guild.name.clone();
                let group_name = ctx.cache.channel(dashboard_channel)
                    .map(|ch| ch.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                for (user_id, rank, discord_tag) in &players_to_add {
                    // Fetch player from database to preserve discord_tag
                    let player = match self.database.get_user_with_tag(*user_id, ctx).await {
                        Ok(p) => p,
                        Err(_) => match self.database.new_user_with_tag(*user_id, ctx).await {
                            Ok(p) => p,
                            Err(e) => {
                                warn!("Failed to get or create player {}: {}", discord_tag, e);
                                continue;
                            }
                        }
                    };
                    session.add_player_in_vc(player, *rank);
                    log_queue_toggle(&guild_name, &group_name, &discord_tag, QueueToggleType::VJ);
                }
            }

            let users_added = players_to_add.len();
            if users_added > 0 {

                // NOW check quota once after all players added
                if group.is_quota() {
                    if let Err(e) = group.hot(ctx, Some(guild.id), Some(&self.database), Some(self.manager.clone())).await {
                        error!("Failed to transition to hot: {}", e);
                    }
                }

                // Update the dashboard to reflect the new users
                group.queue_dash_update(ctx, guild.id.get()).await;
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
                    .title("Missing Bot Permissions")
                    .description(format!(
                        "The bot is missing required permissions to function properly.\n\n\
                        **Missing Permissions:**\n{}\n\n\
                        Please grant these permissions to the bot and click the button below to confirm.",
                        missing_perms
                    ))
                    .color(0xFF0000);

                let button = serenity::all::CreateButton::new("confirm_permissions")
                    .label("Confirm Permissions")
                    .style(serenity::all::ButtonStyle::Success);

                let action_row = serenity::all::CreateActionRow::Buttons(vec![button]);

                let msg = serenity::all::CreateMessage::new()
                    .embed(warning_embed)
                    .components(vec![action_row]);

                if let Err(e) = channel.id.send_message(&ctx.http, msg).await {
                    error!("Failed to send permission warning: {}", e);
                }
            }
            return;
        }

        // Get server from manager (already has groups with existing users loaded)
        let server = match manager.get_server(guild.id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get server from manager: {}", e);
                return;
            }
        };

        for group in &mut server.groups {
            // Validate that the dashboard channel still exists
            let channel_id = group.channels.dashboard;
            let channel_exists = match ctx.http.get_channel(channel_id).await {
                Ok(_) => true,
                Err(e) => {
                    if e.to_string().contains("10003") || e.to_string().contains("Unknown Channel") {
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
                group.queue_dash_update(ctx, guild.id.get()).await;
                continue;
            }
            
            // Create dashboard for each group's dashboard channel
            let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| "Unknown".to_string());

            // Create dashboard in the dashboard channel
            match group.dash_publish(ctx, channel_id).await {
                Ok(_) => {
                    info!("Dashboard created successfully for channel {}", channel_name);
                    
                    // Persist the dashboard message ID to database
                    let dashboard_msg_id = group.dashboard_msg.get();
                    if let Err(e) = self.database.groups.update_dashboard_msg(
                        guild.id.get(),
                        channel_id.get(),
                        dashboard_msg_id
                    ).await {
                        warn!("Failed to persist dashboard message ID to database: {}", e);
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
                "**{} players in queue channel!**\n\n{}\n\nUse `/shuffle` to generate teams.",
                group.quota,
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
    // Initialize tracing with minimal, colored format
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_level(true)
        .compact()
        .init();

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
            database: db.clone(),
            manager: manager.clone(),
            dashboard_queue: Arc::new(tokio::sync::Mutex::new(None)),
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
