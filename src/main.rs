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
use pf_pug_bot::{ButtonType, CommandContext, ComponentContext, Database, Group, Manager, Roles, Server, SessionStatus, VoiceStateUpdate};
use pf_pug_bot::models::constants::VoiceStateUpdate::*;

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
        let guild_count = ctx.cache.guilds().len();

        // Spawn console command handler in a separate task
        let console_handler = command::ConsoleHandler::new(
            self.manager.clone(),
            self.database.pool().clone(),
            Arc::new(ctx.clone()),
        );

        tokio::spawn(async move {
            console_handler.start_console_loop().await;
        });

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
            cmd("roles",     "Manage runner and admin roles") .op("type",  "Role type (runner/admin)",       false)
                                                              .op("role",  "Discord role to assign",          false),
            cmd("dashboard", "Create/update interactive dashboard"),
            cmd("initgroup", "Initialize group"),
            cmd("setup",     "Run guild setup wizard"),
            cmd("checkranks", "Check and create missing rank roles"),
            cmd("setquota",  "Set the queue quota")              .op("quota", "Number of players required (2-100)", true),
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
                    Ok(groups) if !groups.is_empty() => {
                        let mut manager = self.manager.lock().await;
                        if manager.get_server(guild.id).is_err() {
                            let mut server = Server::new(guild.id, Roles::empty());
                            for group in groups {
                                if let Err(e) = server.add_group(group) {
                                    error!("Failed to add group: {}", e);
                                }
                            }
                            let groups_len = server.groups.len();
                            manager.servers.push(server);

                            self.check_existing_voice_users(&ctx, &guild, &mut manager).await;
                            self.create_guild_dashboard_from_manager(&ctx, &guild, &mut manager).await;
                        }
                    },
                    Ok(_) => {
                        warn!("{} has no group configurations. ID: {}", guild.name, guild_id);
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
                    "roles" => {
                        info();
                        let role_type = cdo.iter().find(|opt| opt.name == "type").and_then(|opt| opt.value.as_str()).unwrap_or("").to_string();
                        let role      = cdo.iter().find(|opt| opt.name == "role").and_then(|opt| opt.value.as_str()).map(|s| s.to_string());
                        admin::cmd_roles(&cmd_ctx, role_type, role).await
                    }
                    "checkranks" => {
                        info();
                        admin::cmd_check_ranks(&cmd_ctx).await
                    }
                    "setquota" => {
                        info();
                        let quota = cdo.iter()
                            .find(|opt| opt.name == "quota")
                            .and_then(|opt| opt.value.as_str())
                            .and_then(|s| s.parse::<i64>().ok())
                            .unwrap_or(0);
                        admin::cmd_set_quota(&cmd_ctx, quota).await
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

                // Handle permission confirmation button
                if matches!(button_type, ButtonType::ConfirmPermissions) {
                    let guild_id = itx.guild_id.unwrap();
                    let user_id = itx.user.id;

                    // Check if user is an admin
                    let is_admin = match guild_id.member(&ctx.http, user_id).await {
                        Ok(member) => {
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
                        Err(_) => false,
                    };

                    if !is_admin {
                        let error_response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content("❌ Only administrators can confirm bot permissions.")
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
                                .content(format!("❌ Still missing permissions: {}", missing_perms))
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
                                .content("✅ Permissions confirmed! Setting up dashboard...")
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
                        info!("Group not found in manager for channel {}, attempting recovery from database", channel_id);

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
                                    info!("Found group in database for dashboard channel {}", channel_id);

                                    // Update the dashboard message ID in the database
                                    if let Err(e) = group_repo.update_dashboard_msg(guild_id_u64, channel_id_u64, message_id_u64).await {
                                        error!("Failed to update dashboard message ID: {}", e);
                                    } else {
                                        info!("Updated dashboard message ID to {} in database", message_id_u64);
                                        // Update the in-memory group too
                                        recovered_group.dashboard_msg = message_id;
                                    }

                                    // Add the recovered group to the manager
                                    let server = manager.get_server(guild_id);
                                    if let Ok(server) = server {
                                        server.groups.push(recovered_group);
                                        info!("Recovered group added to manager");

                                        // Now get the group from the manager
                                        manager.get_group(guild_id, channel_id).unwrap()
                                    } else {
                                        error!("Could not get server for guild {}", guild_id);
                                        let error_response = CIR::Message(
                                            CIRM::new()
                                                .content("⚠️ Dashboard state was lost. Please run `/setup` to reconfigure.")
                                                .ephemeral(true)
                                        );
                                        if let Err(e) = itx.create_response(&ctx.http, error_response).await {
                                            error!("Failed to send error response: {}", e);
                                        }
                                        return;
                                    }
                                } else {
                                    error!("No group found in database for dashboard channel {}", channel_id);
                                    let error_response = CIR::Message(
                                        CIRM::new()
                                            .content("⚠️ Dashboard configuration not found. Please run `/setup` to configure this channel.")
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
                                        .content("⚠️ Failed to access database. Please contact an administrator.")
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
        let user_name   = user.display_name();
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
                    info!("{} left the queue", user_name);

                    let quota = group.quota as usize;
                    // Get session index before mutable borrow
                    let session_idx = group.sessions.iter()
                        .position(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

                    let should_regenerate = if let Ok(sesh) = group.get_user_session(user_id).await {
                        if !sesh.is_active() {
                            let was_hot = sesh.is_hot();
                            sesh.remove_player(user_id);
                            let pool_len = sesh.pool.len();

                            // If session was hot, check what to do next
                            if was_hot {
                                if pool_len < quota {
                                    // Dropped below quota, transition back to idle
                                    if let Some(idx) = session_idx {
                                        info!("[Session {}] Dropped below quota, transitioning from Hot to Idle", idx);
                                    } else {
                                        info!("Session dropped below quota, transitioning from Hot to Idle");
                                    }
                                    sesh.idle();
                                    false
                                } else {
                                    // Still at or above quota, regenerate teams
                                    if let Some(idx) = session_idx {
                                        info!("[Session {}] Still meets quota after player left, regenerating teams", idx);
                                    } else {
                                        info!("Session still meets quota after player left, regenerating teams");
                                    }
                                    true
                                }
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
                        group.generate_teams(&ctx).await;
                    }
                    group.dash_update(&ctx).await;
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
                    info!("{} left the queue", user_name);

                    let quota = group.quota as usize;
                    // Get session index before mutable borrow
                    let session_idx = group.sessions.iter()
                        .position(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

                    let should_regenerate = if let Ok(sesh) = group.get_user_session(user_id).await {
                        if !sesh.is_active() {
                            let was_hot = sesh.is_hot();
                            sesh.remove_player(user_id);
                            let pool_len = sesh.pool.len();

                            // If session was hot, check what to do next
                            if was_hot {
                                if pool_len < quota {
                                    // Dropped below quota, transition back to idle
                                    if let Some(idx) = session_idx {
                                        info!("[Session {}] Dropped below quota, transitioning from Hot to Idle", idx);
                                    } else {
                                        info!("Session dropped below quota, transitioning from Hot to Idle");
                                    }
                                    sesh.idle();
                                    false
                                } else {
                                    // Still at or above quota, regenerate teams
                                    if let Some(idx) = session_idx {
                                        info!("[Session {}] Still meets quota after player left, regenerating teams", idx);
                                    } else {
                                        info!("Session still meets quota after player left, regenerating teams");
                                    }
                                    true
                                }
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
                        group.generate_teams(&ctx).await;
                    }
                    group.dash_update(&ctx).await;
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

        let channel_name = new.channel_id.unwrap().name(&ctx.http).await.unwrap_or_else(|_| "Unknown".to_string());
        info!("{} joined {}", user_name, channel_name);

        // Get player data
        match self.database.get_user(user_id).await {
            Ok(user) => user,
            Err(_) => match self.database.new_user(user_id).await {
                    Ok(new_user) => new_user,
                    Err(e) => {
                        error!("Failed to create new user: {}", e);
                        return;
                    }
            },
        };

        // Mutex scope
        {
            info!("[DEBUG] Acquiring manager lock for player addition...");
            let mut manager = self.manager.lock().await;
            info!("[DEBUG] Manager lock acquired, looking up group for channel {:?}", new.channel_id);

            // Find the guild by ID and check if the new channel is a queue voice channel in any group
            match manager.get_group(server, new.channel_id.unwrap()) {
                Ok(group) => {
                    info!("[DEBUG] Group found, checking if channel is queue_vc. queue_vc={:?}, joined_channel={:?}", group.channels.queue_vc, new.channel_id);
                    if group.channels.queue_vc == new.channel_id.unwrap() {
                        info!("[DEBUG] Player joined queue VC, processing...");
                        // Check if player is already in any session and mark them as in VC
                        if let Ok(session) = group.get_user_session(user_id).await {
                            if let Some(player) = session.pool.iter_mut().find(|p| p.player.discord_id == user_id) {
                                player.in_queue_vc = true;
                                group.dash_update(&ctx).await;
                            }
                        } else {
                            // Player not in session yet, try to add them if idle session available
                            if group.get_inactives().is_empty() {
                                error!("No idle sessions present.");
                            } else {
                                // Get or assign player rank (auto-creates ranks and assigns Apprentice if needed)
                                use pf_pug_bot::handlers::player::get_or_assign_player_rank;
                                match get_or_assign_player_rank(&ctx, &self.database, server, user_id).await {
                                    Ok(rank) => {
                                        // Use queue_player_with_vc_status to set in_queue_vc BEFORE quota check/notification
                                        if let Err(e) = group.queue_player_with_vc_status(user_id, rank, &ctx, Some(server), Some(&self.database), true).await {
                                            error!("Failed to add player to queue: {}", e);
                                        }

                                        group.dash_update(&ctx).await;
                                    },
                                    Err(e) => {
                                        warn!("{} failed to get or assign rank: {}", user_name, e);
                                    }
                                }
                            }
                        }

                        if group.check_hot_timeout(&ctx).await {
                            info!("Hot session timeout detected, updating dashboard");
                            group.dash_update(&ctx).await;
                        }
                    }
                },
                Err(e) => {
                    error!("[DEBUG] Failed to get group: {}", e);
                }
            }
            info!("[DEBUG] Releasing manager lock after player addition check");
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

                    let user_name = match ctx.http.get_user(*user_id).await {
                        Ok(user) => user.display_name().to_string(),
                        Err(_)   => user_id.to_string(),
                    };

                    match get_or_assign_player_rank(ctx, &self.database, guild.id, *user_id).await {
                        Ok(rank) => {
                            players_to_add.push((*user_id, rank, user_name));
                        },
                        Err(e) => {
                            warn!("Failed to get or assign rank for existing user {}: {}", user_name, e);
                        }
                    }
                }
            }

            // Add all players to the session WITHOUT quota check
            if let Ok(session) = group.get_queue().await {
                for (user_id, rank, user_name) in &players_to_add {
                    session.add_player(*user_id, *rank);
                    // Mark them as in VC
                    if let Some(player) = session.pool.iter_mut().find(|p| p.player.discord_id == *user_id) {
                        player.in_queue_vc = true;
                    }
                    info!("Added {} ({:?})", user_name, rank);
                }
            }

            let users_added = players_to_add.len();
            if users_added > 0 {
                info!("Added {} user(s) to queue for group in channel {}", users_added, queue_vc_id);

                // NOW check quota once after all players added
                if group.is_quota() {
                    if let Err(e) = group.hot(ctx, Some(guild.id), Some(&self.database)).await {
                        error!("Failed to transition to hot: {}", e);
                    }
                }

                // Update the dashboard to reflect the new users
                group.dash_update(ctx).await;
            }
        }
    }

    /// Creates dashboard for a guild using in-memory groups from manager
    async fn create_guild_dashboard_from_manager(&self, ctx: &Context, guild: &Guild, manager: &mut Manager) {
        info!("Creating dashboard for guild: {}", guild.name);

        // FIRST: Check bot permissions
        let (has_perms, missing_perms) = self.check_bot_permissions(ctx, guild).await;

        if !has_perms {
            warn!("Bot is missing permissions in guild {}: {}", guild.name, missing_perms);

            // Create a warning dashboard in the first available text channel
            if let Some(channel) = guild.channels.values().find(|c| c.kind == serenity::all::ChannelType::Text) {
                let warning_embed = serenity::all::CreateEmbed::new()
                    .title("⚠️ Missing Bot Permissions")
                    .description(format!(
                        "The bot is missing required permissions to function properly.\n\n\
                        **Missing Permissions:**\n{}\n\n\
                        Please grant these permissions to the bot and click the button below to confirm.",
                        missing_perms
                    ))
                    .color(0xFF0000);

                let button = serenity::all::CreateButton::new("confirm_permissions")
                    .label("✅ Confirm Permissions")
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
            // Check if dashboard already exists
            if group.has_dashboard(ctx).await {
                group.dash_update(ctx).await;
                continue;
            }
            // Create dashboard for each group's queue channel
            let channel_id   = group.channels.queue_chat;
            let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| "Unknown".to_string());

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
