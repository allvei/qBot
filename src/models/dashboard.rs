use anyhow::{anyhow, Error, Result};
use std::{collections::HashSet, sync::Arc, time::{Duration, SystemTime}};
use crate::{QueueToggleType, log_queue_toggle, models::constants::DEFAULT_TIMEOUT};
use serenity::{all::{
    ButtonStyle as BS, ChannelId as CI, Context, CreateActionRow as CAR, CreateButton as CB,
    CreateEmbed as CE, CreateMessage as CM, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, EditMessage, Message,
}};
use tracing::{error, info, warn};
use tokio::sync::mpsc;

use crate::models::{ComponentContext as CC, DashboardQueueKey, Group, SessionStatus};

/// Helper function to format team players as a string for embed fields
async fn format_team_field(team: &[crate::models::SessionPlayer], db: &crate::Database, guild_id: u64) -> String {
    let mut lines = Vec::new();
    for player in team {
        let elo_str = if let Some(rank) = player.player.rank {
            let elo = rank.elo_from_config(db, guild_id).await;
            format!("[**{elo}**] ")
        } else {
            String::new()
        };
        lines.push(format!("{elo_str}<@{}>", player.player.discord_id));
    }
    lines.join("\n")
}

/// Helper function to split pool into teams by actual team assignments and sort by ELO descending
fn get_sorted_teams(pool: &[crate::models::SessionPlayer], quota: usize) -> (Vec<crate::models::SessionPlayer>, Vec<crate::models::SessionPlayer>) {
    // Filter players by their actual team assignment (not by position!)
    let mut team_red: Vec<_> = pool.iter()
        .take(quota)
        .filter(|p| p.team == Some(crate::models::Team::Red))
        .cloned()
        .collect();

    let mut team_blu: Vec<_> = pool.iter()
        .take(quota)
        .filter(|p| p.team == Some(crate::models::Team::Blu))
        .cloned()
        .collect();

    // Sort both teams by ELO descending
    let sort_by_elo = |a: &crate::models::SessionPlayer, b: &crate::models::SessionPlayer| {
        let elo_a = a.player.rank.map(|r| r.elo()).unwrap_or(0);
        let elo_b = b.player.rank.map(|r| r.elo()).unwrap_or(0);
        elo_b.cmp(&elo_a)
    };

    team_red.sort_by(sort_by_elo);
    team_blu.sort_by(sort_by_elo);

    (team_red, team_blu)
}

/// Represents different types of button interactions in the Discord bot
#[derive(Debug, Clone, PartialEq)]
pub enum ButtonType {
    // Setup flow buttons
    SetupDashboard,
    SetupQueue,
    SetupQueueVc,
    SetupRed,
    SetupBlue,
    SetupRunner,
    SetupAdmin,

    // Init group flow buttons
    InitDashboard,
    InitQueue,
    InitQueueVc,
    InitRed,
    InitBlue,
    InitRunner,
    InitAdmin,
    InitQuota,

    // Group link buttons
    GroupLinkDashboard,
    GroupLinkQueue,
    GroupLinkQueueVc,
    GroupLinkRed,
    GroupLinkBlue,

    // Dashboard action buttons
    DashboardJoin,
    DashboardLeave,
    DashboardShuffle,
    DashboardStart,
    DashboardEnd,

    // Permission confirmation button
    ConfirmPermissions,

    // Rank role creation buttons
    CreateRankRolesYes,
    CreateRankRolesNo,

    // Unknown button type
    Unknown(String),
}

impl ButtonType {
    /// Parse a custom_id string into a ButtonType
    pub fn parse(custom_id: &str) -> Self {
        match custom_id {
            // Setup buttons
            "setup_dashboard" => Self::SetupDashboard,
            "setup_queue"     => Self::SetupQueue,
            "setup_queuevc"   => Self::SetupQueueVc,
            "setup_red"       => Self::SetupRed,
            "setup_blue"      => Self::SetupBlue,
            "setup_runner"    => Self::SetupRunner,
            "setup_admin"     => Self::SetupAdmin,

            // Init buttons
            "init_dashboard"  => Self::InitDashboard,
            "init_queue"      => Self::InitQueue,
            "init_queuevc"    => Self::InitQueueVc,
            "init_red"        => Self::InitRed,
            "init_blue"       => Self::InitBlue,
            "init_runner"     => Self::InitRunner,
            "init_admin"      => Self::InitAdmin,
            "init_quota"      => Self::InitQuota,

            // Group link buttons
            "grouplink_dashboard" => Self::GroupLinkDashboard,
            "grouplink_queue"     => Self::GroupLinkQueue,
            "grouplink_queuevc"   => Self::GroupLinkQueueVc,
            "grouplink_red"       => Self::GroupLinkRed,
            "grouplink_blue"      => Self::GroupLinkBlue,

            // Dashboard buttons
            "join_queue"      => Self::DashboardJoin,
            "leave_queue"     => Self::DashboardLeave,
            "shuffle_teams"   => Self::DashboardShuffle,
            "start_match"     => Self::DashboardStart,
            "end_match"       => Self::DashboardEnd,

            // Permission confirmation
            "confirm_permissions" => Self::ConfirmPermissions,

            // Rank role creation
            "create_rank_roles_yes" => Self::CreateRankRolesYes,
            "create_rank_roles_no"  => Self::CreateRankRolesNo,

            // Unknown
            _ => Self::Unknown(custom_id.to_string()),
        }
    }

    /// Check if this button type requires setup handling
    pub fn is_setup_button(&self) -> bool {
        matches!(
            self,
            Self::SetupDashboard |
            Self::SetupQueue     |
            Self::SetupQueueVc   |
            Self::SetupRed       |
            Self::SetupBlue      |
            Self::SetupRunner    |
            Self::SetupAdmin     |
            Self::InitDashboard  |
            Self::InitQueue      |
            Self::InitQueueVc    |
            Self::InitRed        |
            Self::InitBlue       |
            Self::InitRunner     |
            Self::InitAdmin      |
            Self::InitQuota      |
            Self::GroupLinkDashboard |
            Self::GroupLinkQueue     |
            Self::GroupLinkQueueVc   |
            Self::GroupLinkRed       |
            Self::GroupLinkBlue
        )
    }

    /// Check if this button type is a dashboard action
    pub fn is_dashboard_action(&self) -> bool {
        matches!(
            self,
            Self::DashboardJoin   |
            Self::DashboardLeave  |
            Self::DashboardShuffle     |
            Self::DashboardStart       |
            Self::DashboardEnd
        )
    }

    /// Get the setup step name (for setup/init buttons)
    pub fn setup_step(&self) -> Option<&str> {
        match self {
            Self::SetupDashboard | Self::InitDashboard => Some("dashboard"),
            Self::SetupQueue     | Self::InitQueue     => Some("queue"),
            Self::SetupRed       | Self::InitRed       => Some("red"),
            Self::SetupBlue      | Self::InitBlue      => Some("blue"),
            Self::InitQueueVc => Some("queuevc"),
            Self::SetupRunner => Some("runner"),
            Self::SetupAdmin  => Some("admin"),
            Self::InitQuota   => Some("quota"),
            _ => None,
        }
    }
}

impl Group {
    /// Get the dashboard message
    pub async fn dash_get(&self, ctx: &Context) -> Result<Message> {
        let ch = CI::new(self.channels.dashboard.into());
        let msg = ch.message(&ctx.http, self.dashboard_msg).await;
        match msg {
            Ok(msg) => Ok(msg),
            Err(e)  => Err(anyhow!("Failed to get dashboard message: {e}")),
        }
    }

    /// Creates buttons for the dashboard
    pub async fn create_dashboard_buttons(&self) -> Result<Vec<CAR>> {
        let _quota = self.quota as usize;

        // Check if any session is hot AND still has enough players
        let is_hot  = self.sessions.iter().any(|s| s.is_hot());
        let is_live = self.sessions.iter().any(|s| s.is_active());

        let buttons = vec![
            CAR::Buttons(vec![
                Self::gen_button(("join_queue",     "Join",              BS::Success,   true)),
                Self::gen_button(("leave_queue",    "Leave",             BS::Danger,    true)),
                Self::gen_button(("change_expiry",  "Change expiry time", BS::Secondary, true)),
            ]),
            CAR::Buttons(vec![
                Self::gen_button(("show_settings",  "Settings",          BS::Secondary, true)),
                Self::gen_button(("shuffle_teams",  "Shuffle",           BS::Secondary, is_hot)),
            ]),
            CAR::Buttons(vec![
                Self::gen_button(("start_match", "Start", BS::Primary, is_hot)),
                Self::gen_button(("end_match",   "End",   BS::Danger,  is_live)),
            ]),
        ];

        Ok(buttons)
    }

    fn gen_button(config: (&'static str, &'static str, BS, bool)) -> CB {
        let (action, label, style, enabled) = config;
        CB::new(action).label(label).style(style).disabled(!enabled)
    }

    fn gen_buttons(button_configs: Vec<(&'static str, &'static str, BS, bool)>) -> Vec<CB> {
        button_configs.into_iter().map(|(action, label, style, enabled)| {
            CB::new(action).label(label).style(style).disabled(!enabled)
        }).collect()
    }

    pub async fn has_dashboard(&self, ctx: &Context) -> bool {
        let ch = CI::new(self.channels.dashboard.into());
        let msg = ch.message(&ctx.http, self.dashboard_msg).await;
        msg.is_ok()
    }

    pub async fn dash_publish(&mut self, ctx: &Context, channel: CI, db: &crate::Database, guild_id: u64) -> Result<(), Error>{
        // Create new dashboard message (don't check if it exists - caller should check)
        let msg = channel.send_message(&ctx.http, self.dash_init(db, guild_id).await?).await;
        if let Ok(msg) = msg {
            self.dashboard_msg = msg.id;
            Ok(())
        } else {
            let channel_name = channel.name(&ctx.http).await.unwrap_or_else(|_| format!("#{channel}"));
            error!("Failed to send dashboard message in #{}", channel_name);
            Err(anyhow!("Failed to send dashboard message in #{}: {}", channel_name, msg.unwrap_err()))
        }
    }

    /// Builds dashboard embed and components based on current group state
    pub async fn build_dashboard_content(&self, db: &crate::Database, guild_id: u64) -> Result<(CE, Vec<CAR>)> {
        let quota     = self.quota as usize;
        let inactives = self.get_inactives();
        let actives   = self.get_actives();

        let mut embed = CE::new().title("PUG Dashboard");
        let mut description = String::new();

        // Show active games first (Hot/Push/Live/Pull)
        if !actives.is_empty() {
            description.push_str("**Current Match:**\n");
            for session in &actives {
                // Only show missing players check for Hot sessions
                // For Push/Live/Pull, players are in team channels, not queue VC
                if session.is_hot() {
                    // Check for players who have NEVER joined the VC (not just currently not in VC)
                    let players_never_joined: Vec<_> = session.pool.iter()
                        .take(quota)
                        .filter(|p| !p.in_queue_vc)
                        .collect();

                    // Only show countdown and missing players if there are players who have never joined
                    if !players_never_joined.is_empty() {
                        // Display countdown timer
                        if let Some(ready_at) = session.ready_at {
                            if let Ok(duration_since_epoch) = ready_at.duration_since(SystemTime::UNIX_EPOCH) {
                                let ready_timestamp = duration_since_epoch.as_secs();
                                let deadline_timestamp = ready_timestamp + DEFAULT_TIMEOUT as u64;
                                description.push_str(&format!("Join deadline: <t:{deadline_timestamp}:R>\n"));
                                description.push_str("Missing players will be removed. Overflow players will take their spots.\n\n");
                            }
                        }

                        description.push_str("**Missing players:**\n");
                        for player in players_never_joined {
                            let elo_str = if let Some(rank) = player.player.rank {
                                let elo = rank.elo_from_config(db, guild_id).await;
                                format!("[**{elo}**] ")
                            } else {
                                String::new()
                            };
                            description.push_str(&format!("  • {elo_str}<@{}>\n", player.player.discord_id));
                        }
                        description.push_str("\n\n");
                    }
                } else {
                    // For Push/Live/Pull sessions, just show status
                    let status_text = match session.status {
                        SessionStatus::Push => "Moving players to team channels...",
                        SessionStatus::Live => "Match in progress",
                        SessionStatus::Pull => "Moving players back to queue...",
                        _ => "Match active"
                    };
                    description.push_str(&format!("• {status_text} ({} players)\n", session.pool.len()));
                }
            }
            description.push('\n');
        }
        // Show idle session
        else {
            // No active games - show queue status or hot game info
            if let Some(current_session) = inactives.first() {
                // If session is Hot, show missing players info
                if current_session.is_hot() {
                    // Check for players who have NEVER joined the VC (not just currently not in VC)
                    let players_never_joined: Vec<_> = current_session.pool.iter()
                        .take(quota)
                        .filter(|p| !p.in_queue_vc)
                        .collect();

                    // Only show countdown and missing players if there are players who have never joined
                    if !players_never_joined.is_empty() {
                        // Display countdown timer
                        if let Some(ready_at) = current_session.ready_at {
                            if let Ok(duration_since_epoch) = ready_at.duration_since(SystemTime::UNIX_EPOCH) {
                                let ready_timestamp = duration_since_epoch.as_secs();
                                let deadline_timestamp = ready_timestamp + DEFAULT_TIMEOUT as u64;
                                description.push_str(&format!("Join deadline: <t:{deadline_timestamp}:R>\n"));
                                description.push_str("Missing players will be removed. Overflow players will take their spots.\n\n");
                            }
                        }

                        description.push_str("**Missing players:**\n");
                        for player in players_never_joined {
                            let elo_str = if let Some(rank) = player.player.rank {
                                let elo = rank.elo_from_config(db, guild_id).await;
                                format!("[**{elo}**] ")
                            } else {
                                String::new()
                            };
                            description.push_str(&format!("  • {elo_str}<@{}>\n", player.player.discord_id));
                        }
                        description.push_str("\n\n");
                    }
                    // Note: Next Queue for overflow will be shown after teams
                } else {
                    // Session is Idle - show normal queue
                    let queue_players = current_session.pool.len();
                    description.push_str(&format!("**Queue ({queue_players}/{quota})**\n"));

                    if queue_players == 0 {
                        description.push_str("*No players in queue. Join to get started!*\n");
                    }
                    description.push('\n');
                }
            } else {
                description.push_str("**Queue status**\n* Empty. Join to get started!*\n\n");
            }
        }

        embed = embed.description(description);

        // Add queue fields for idle session (before quota is met)
        if actives.is_empty() {
            if let Some(current_session) = inactives.first() {
                if !current_session.is_hot() && current_session.pool.len() > 0 && current_session.pool.len() < quota {
                    let mut players_field = String::new();
                    let mut timers_field  = String::new();

                    for player in current_session.pool.iter() {
                        // Build player list with ELO
                        let elo_str = if let Some(rank) = player.player.rank {
                            let elo = rank.elo_from_config(db, guild_id).await;
                            format!("[**{elo}**] ")
                        } else {
                            String::new()
                        };
                        players_field.push_str(&format!("{elo_str}<@{}>\n", player.player.discord_id));

                        // Build auto-remove timer list
                        // Use per-instance expiry_duration if set, otherwise get user's setting
                        let expiry_duration = if let Some(duration) = player.expiry_duration {
                            duration
                        } else if let Ok(settings) = db.users.get_settings(player.player.discord_id).await {
                            settings.expiry_duration
                        } else {
                            std::time::Duration::ZERO
                        };

                        if expiry_duration.as_secs() > 0 {
                            if let Ok(join_time) = player.joined_at.duration_since(std::time::SystemTime::UNIX_EPOCH) {
                                let expiry_timestamp = join_time.as_secs() + expiry_duration.as_secs();
                                timers_field.push_str(&format!("<t:{}:R>\n", expiry_timestamp));
                            } else {
                                timers_field.push_str("-\n");
                            }
                        } else {
                            timers_field.push_str("-\n");
                        }
                    }

                    embed = embed.field("Players",        players_field, true);
                    embed = embed.field("Auto-remove at", timers_field,  true);
                }
            }
        }

        // Add connect info if available
        if let Some(ref connect_info) = self.connect_info {
            let mut field_value = format!("```{connect_info}```");

            // Try to extract IP:PORT and create a Steam link
            // Discord doesn't support steam:// protocol links, so display as copyable text
            if let Some(steam_link) = Self::extract_steam_link(connect_info) {
                field_value.push_str(&format!("\n**Steam Link:**\n```{steam_link}```"));
            }

            embed = embed.field("Server connect info:", field_value, false);
        }

        // Add team fields for inactive sessions with enough players
        if let Some(current_session) = inactives.first() {
            let queue_players = current_session.pool.len();
            if queue_players >= quota {
                let (team_red, team_blu) = get_sorted_teams(&current_session.pool, quota);
                embed = embed.field("🔴 RED", format_team_field(&team_red, db, guild_id).await, true);
                embed = embed.field("🔵 BLU", format_team_field(&team_blu, db, guild_id).await, true);

                // Show fatkidded players AFTER teams if there are overflow players
                if current_session.is_hot() && queue_players > quota {
                    let overflow_count = queue_players - quota;
                    let mut fatkid = format!("**Waiting for next game ({overflow_count}/{quota}):**\n");
                    for player in current_session.pool.iter().skip(quota) {
                        let elo_str = if let Some(rank) = player.player.rank {
                            let elo = rank.elo_from_config(db, guild_id).await;
                            format!("[**{elo}**] ")
                        } else {
                            String::new()
                        };
                        fatkid.push_str(&format!("{elo_str}<@{}>\n", player.player.discord_id));
                    }
                    embed = embed.field("\u{200B}", fatkid, false); // Full-width field
                }
            }
        }

        // Add team fields for active sessions
        for session in &actives {
            if session.pool.len() >= quota {
                let (team_red, team_blu) = get_sorted_teams(&session.pool, quota);
                embed = embed.field("🔴 RED", format_team_field(&team_red, db, guild_id).await, true);
                embed = embed.field("🔵 BLU", format_team_field(&team_blu, db, guild_id).await, true);
            }
        }

        // Show fatkidded players for idle session (when there's an active game)
        if !actives.is_empty() {
            if let Some(next_session) = inactives.first() {
                if !next_session.pool.is_empty() {
                    let mut fatkid = format!("**Waiting for next game ({quota}/{}):**\n", next_session.pool.len());
                    for player in next_session.pool.iter() {
                        let elo_str = if let Some(rank) = player.player.rank {
                            let elo = rank.elo_from_config(db, guild_id).await;
                            format!("[**{elo}**] ")
                        } else {
                            String::new()
                        };
                        fatkid.push_str(&format!("{elo_str}<@{}>\n", player.player.discord_id));
                    }
                    embed = embed.field("\u{200B}", fatkid, false); // Full-width field
                }
            }
        }

        let buttons = self.create_dashboard_buttons().await.unwrap();

        Ok((embed, buttons))
    }

    /// Extract IP:PORT and password from connect info and create a steam:// link
    /// Supports formats like:
    /// - "connect 1.1.1.1:27015"
    /// - "1.1.1.1:27015"
    /// - "connect 1.1.1.1:27015; password 1234"
    /// - "1.1.1.1:27015; password mypass"
    fn extract_steam_link(connect_info: &str) -> Option<String> {
        // Manual parsing without regex crate dependency

        // Find IP:PORT pattern
        let mut ip_start = None;
        let mut ip_end = None;

        let chars: Vec<char> = connect_info.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            // Look for digit that could start an IP address
            if chars[i].is_ascii_digit() {
                let start = i;
                let mut dots = 0;
                let mut has_port = false;

                // Try to match IP:PORT pattern
                while i < chars.len() {
                    if chars[i].is_ascii_digit() {
                        i += 1;
                    } else if chars[i] == '.' {
                        dots += 1;
                        i += 1;
                    } else if chars[i] == ':' && dots == 3 {
                        has_port = true;
                        i += 1;
                        // Continue to parse port
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        break;
                    } else {
                        break;
                    }
                }

                if dots == 3 && has_port {
                    ip_start = Some(start);
                    ip_end = Some(i);
                    break;
                }
            }
            i += 1;
        }

        let ip_port = if let (Some(start), Some(end)) = (ip_start, ip_end) {
            connect_info[start..end].to_string()
        } else {
            return None;
        };

        // Look for password
        let password = if let Some(pwd_pos) = connect_info.to_lowercase().find("password") {
            let after_password = &connect_info[pwd_pos + 8..].trim_start();
            // Extract password (everything after "password" until semicolon, newline, or end)
            after_password
                .split(&[';', '\n', '\r'][..])
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };

        // Build Steam link
        if let Some(pwd) = password {
            Some(format!("steam://connect/{ip_port}/{pwd}"))
        } else {
            Some(format!("steam://connect/{ip_port}"))
        }
    }

    /// Initializes a dashboard based on current group state
    pub async fn dash_init(&mut self, db: &crate::Database, guild_id: u64) -> Result<CM> {
        let (embed, buttons) = self.build_dashboard_content(db, guild_id).await?;
        let message = CM::new().embed(embed).components(buttons);
        Ok(message)
    }

    /// Updates a dashboard based on current group state
    /// Auto-recovers by creating a new dashboard if the current one is missing/deleted
    pub async fn dash_update(&mut self, ctx: &Context) -> Result<(), Error> {
        match self.dash_get(ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Dashboard message not found (may have been deleted): {e}");
                info!("Auto-recovering: creating new dashboard message");

                // Dashboard is missing - create a new one
                // Note: We can't call dash_publish here because we don't have db and guild_id
                // This path should not be hit since the queue processor handles recreation
                return Err(anyhow!("Dashboard message missing - requires recreation with db/guild_id"));
                /* match self.dash_publish(ctx, self.channels.dashboard).await {
                    Ok(_) => {
                        info!("Successfully created new dashboard message with ID {}", self.dashboard_msg);
                        // Return early since dash_publish already creates the dashboard with current content
                        return Ok(());
                    },
                    Err(publish_err) => {
                        error!("Failed to auto-recover dashboard: {}", publish_err);
                        return Err(publish_err);
                    }
                } */
            }
        };

        // Note: This function is deprecated and should not be called directly
        // Use the dashboard queue processor which has access to db and guild_id
        Err(anyhow!("dash_update requires db and guild_id - use dashboard queue"))
    }

    /// Queue a dashboard update (non-blocking, batched)
    /// Requires guild_id to be passed since Group doesn't store it
    pub async fn queue_dash_update(&self, ctx: &Context, guild_id: u64) {
        //
        // Try to get queue from context data using the key from models module
        let data = ctx.data.read().await;
        if let Some(queue) = data.get::<DashboardQueueKey>() {
            queue.request_update(guild_id, self.group_id as u64);
            //
        } else {
            warn!("Dashboard queue not initialized in Context");
            // Note: Can't fallback to dash_update here because we'd need &mut self
            // The dashboard queue should always be initialized, so this is just a safety check
        }
    }

    /// Handles the join queue button
    async fn dash_join_queue(&mut self, cc: &CC<'_>) -> Result<()> {
        let user_id = cc.component.user.id;
        //

        // Store channel IDs before any borrows
        let dashboard_channel = self.channels.dashboard;

        // Check if player is already in queue
        if self.get_user_session(user_id).await.is_ok() {
            //
            cc.reply("You are already in the queue!").await?;
            return Ok(());
        }

        // Check if we have an idle or hot session to join
        let has_joinable_session = self.sessions.iter().any(|s|
            s.status == SessionStatus::Idle || s.status == SessionStatus::Hot
        );
        //

        if !has_joinable_session {
            cc.reply("Cannot join - match is in progress. Please wait.").await?;
            return Ok(());
        }

        // Defer update now that we know we'll succeed
        //
        cc.defer_update().await?;
        //

        // Get or assign player's rank (auto-creates ranks and assigns Apprentice if needed)
        use crate::handlers::player::get_or_assign_player_rank;
        if let Some(guild_id) = cc.component.guild_id {
            //
            // Get player object from database (without fetching discord tag for performance)
            let mut player = match cc.db.get_user(user_id).await {
                Ok(p) => p,
                Err(_) => match cc.db.new_user(user_id).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to get or create player: {e}");
                        return Ok(());
                    }
                }
            };

            // Fetch discord tag from component user for performance (avoid extra API call)
            player.discord_tag = Some(cc.component.user.tag());

            match get_or_assign_player_rank(cc.ctx, &cc.db, guild_id, user_id).await {
                Ok(rank) => {
                    //
                    // Refresh player rank from current Discord roles before queueing
                    player.rank = Some(rank);

                    // Save rank for announcement (player will be moved)
                    let player_rank = rank;

                    //
                    if let Err(e) = self.queue_player(player, rank, cc.ctx, Some(guild_id), Some(&cc.db), Some(cc.manager.clone())).await {
                        warn!("Failed to queue player: {e}");
                    } else {
                        //
                        // Log successful queue join via button
                        let server_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                        let group_name = cc.ctx.cache.channel(dashboard_channel)
                            .map(|ch| ch.name.clone())
                            .unwrap_or_else(|| "Unknown".to_string());
                        let username = cc.ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string());
                        log_queue_toggle(&server_name, &group_name, &username, QueueToggleType::BJ);

                        // Send join announcement using shared function
                        if let Ok(settings) = cc.db.users.get_settings(user_id).await {
                            use serenity::all::CreateMessage;
                            use crate::handlers::settings::build_join_announcement_embed;

                            let embed = build_join_announcement_embed(
                                cc.ctx,
                                user_id,
                                Some(guild_id),
                                &settings,
                                &player_rank.name()
                            ).await;

                            let queue_chat = self.channels.queue_chat;
                            let _ = queue_chat.send_message(&cc.ctx.http, CreateMessage::new().embed(embed)).await;
                        }
                    }
                },
                Err(e) => {
                    warn!("Failed to get/assign rank: {e}");
                    return Ok(());
                }
            }
        } else {
            cc.reply("This command can only be used in a server.").await?;
            return Ok(());
        }

        // Update dashboard to reflect changes
        //
        self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap().get()).await;
        //

        Ok(())
    }

    /// Handles the leave queue button
    async fn dash_leave_queue(&mut self, cc: &CC<'_>) -> Result<()> {
        let user_id = cc.component.user.id;

        let quota = self.quota as usize;

        // Store channel IDs before any borrows
        let dashboard_channel = self.channels.dashboard;
        let queue_chat = self.channels.queue_chat;

        // Get session index before mutable borrow
        let session_idx = self.sessions.iter()
            .position(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

        // Check if player is in queue
        let should_regenerate_teams = if let Ok(session) = self.get_user_session(user_id).await {
            // Check if player is physically in the queue VC
            let player_in_vc = if let Some(player) = session.pool.iter().find(|p| p.player.discord_id == user_id) {
                player.in_queue_vc
            } else {
                false
            };

            // If player is in VC, check if they want to be disconnected
            if player_in_vc {

                // Check user's VC disconnect preference
                let settings = cc.db.users.get_settings(user_id).await.unwrap_or_default();

                if settings.vc_kick {

                    // Acknowledge button press before disconnecting
                    cc.defer_update().await?;

                    // User wants to be disconnected from VC
                    if let Some(guild_id) = cc.component.guild_id {
                        use serenity::all::EditMember;
                        let _ = cc.ctx.http.edit_member(
                            guild_id,
                            user_id,
                            &EditMember::new().disconnect_member(),
                            Some("Player left queue via dashboard button")
                        ).await;
                    }
                    // The voice_state_update handler will handle removing them from the queue
                    return Ok(());
                }
                // User wants to stay in VC, manually remove them from queue
                // Fall through to remove them manually below
            }

            // Player is in queue but not in VC (or wants to stay in VC), remove them manually
            // Defer update immediately

            cc.defer_update().await?;

            let was_hot = session.is_hot();
            let username = cc.ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string());
            session.remove_player(user_id);
            let pool_len = session.pool.len();

            // Log with server and group context
            let guild_id = cc.component.guild_id.unwrap();
            let server_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
            let group_name = cc.ctx.cache.channel(dashboard_channel)
                .map(|ch| ch.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            log_queue_toggle(&server_name, &group_name, &username, QueueToggleType::BL);

            // Send leave announcement using shared function
            if let Ok(settings) = cc.db.users.get_settings(user_id).await {
                use serenity::all::CreateMessage;
                use crate::handlers::settings::build_leave_announcement_embed;

                let embed = build_leave_announcement_embed(
                    cc.ctx,
                    user_id,
                    Some(guild_id),
                    &settings
                ).await;

                let _ = queue_chat.send_message(&cc.ctx.http, CreateMessage::new().embed(embed)).await;
            }

            // If session was hot, check what to do next
            if was_hot {
                if pool_len < quota {
                    // Dropped below quota, transition back to idle
                    if let Some(idx) = session_idx {

                    } else {
                        info!("Session dropped below quota, transitioning from Hot to Idle");
                    }
                    session.idle();
                    false
                } else {
                    // Still at or above quota, need to regenerate teams
                    if let Some(idx) = session_idx {

                    } else {
                        info!("Session still meets quota after player left, will regenerate teams");
                    }
                    true
                }
            } else {
                false
            }
        } else {
            // Player not in queue

            cc.reply("You are not in the queue!").await?;
            return Ok(());
        };

        // Regenerate teams if needed (outside the session borrow scope)
        if should_regenerate_teams {
            self.generate_teams(cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;
        }

        // Update dashboard to reflect changes (queue count now only shown in dashboard)
        self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap().get()).await;

        Ok(())
    }

    /// Handles the shuffle teams button
    async fn dash_shuffle(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        let quota = self.quota as usize;

        // Find the game to shuffle - can be Idle (if quota met) or Hot
        let session = self.sessions.iter_mut().find(|s|
            (s.status == SessionStatus::Idle || s.status == SessionStatus::Hot) && s.pool.len() >= quota
        );

        if session.is_none() {
            cc.reply(&format!("No game ready for shuffling. Need at least {quota} players in queue.")).await?;
            return Ok(());
        }

        // Defer update now that we know we have a game to shuffle
        cc.defer_update().await?;

        // Refresh player ranks from Discord roles before shuffling teams
        if let Some(guild_id) = cc.component.guild_id {
            self.refresh_player_ranks(cc.ctx, guild_id, &cc.db).await;
        }

        // Call the same team generation logic used by generate_teams
        // This ensures balanced teams using the BCH algorithm
        self.generate_teams(cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;

        // Update dashboard to show new teams
        self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap().get()).await;

        Ok(())
    }

    /// Handles the start match button
    async fn dash_start(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        // Check if user has Runner role
        use crate::handlers::player::check_component_role;
        use crate::models::Role;

        match check_component_role(cc, &Role::Runner).await {
            Ok(true) => {
                // User has Runner role, proceed
            },
            Ok(false) => {
                cc.reply("Only runners can start matches.").await?;
                return Ok(());
            },
            Err(e) => {
                warn!("Failed to check runner role: {e}");
                cc.reply("Failed to verify permissions.").await?;
                return Ok(());
            }
        }

        // Check if there's a hot game to start
        let has_hot_game = self.sessions.iter().any(|s| s.is_hot());

        if !has_hot_game {
            cc.reply("No hot game ready to start.").await?;
            return Ok(());
        }

        // Defer update now that we're going to start the match
        cc.defer_update().await?;

        // Move players to team channels (Hot → Push → Live)
        match self.push(cc.ctx, cc.component.guild_id.unwrap()).await {
            Ok(_) => {
                info!("Players moved to team channels and game is now live");
                // Update dashboard to reflect Live status
                self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap().get()).await;
                Ok(())
            }
            Err(e) => {
                error!("Failed to start match: {e}");
                Ok(())
            }
        }
    }

    /// Handles the end match button - directly ends the match
    async fn dash_end(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        // Check if there's an active game to end
        let has_active_game = self.sessions.iter().any(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Live);

        if !has_active_game {
            cc.reply("No active match to end.").await?;
            return Ok(());
        }

        // Defer update now that we're going to end the match
        cc.defer_update().await?;

        let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

        // Move players back to queue channel (Hot/Live → Pull → Idle)
        match self.pull(cc.ctx, guild_id, &cc.db, Some(cc.manager.clone())).await {
            Ok(_) => {
                info!("Match ended, players moved back to queue");
                Ok(())
            }
            Err(e) => {
                error!("Failed to end match: {e}");
                Ok(())
            }
        }
    }

    /// Handles button interaction events from the dashboard
    ///
    /// Processes all button interactions in a modular way
    ///
    /// * `cc` - The component context with button information
    pub async fn dash_handle_button_interaction(&mut self, cc: &CC<'_>) -> Result<()> {
        let custom_id = &cc.component.data.custom_id;

        let parts: Vec<&str> = custom_id.split(':').collect();
        let action  = parts[0];
        let game_id = parts.get(1).map(|s| s.to_string());

        // Get server and group names for logging - store channel ID before any mut borrows
        let guild_id = cc.component.guild_id.unwrap();
        let dashboard_channel = self.channels.dashboard;
        let server_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        let group_name = cc.ctx.cache.channel(dashboard_channel)
            .map(|ch| ch.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let username = cc.ctx.cache.user(cc.component.user.id).map(|u| u.name.clone()).unwrap_or_else(|| cc.component.user.id.to_string());

        match action {
            "join_queue"        => {
                // Log will be done inside dash_join_queue with proper context
                self.dash_join_queue(cc).await
            },
            "leave_queue"       => {
                // Log will be done inside dash_leave_queue with proper context
                self.dash_leave_queue(cc).await
            },
            "change_expiry"     => {
                info!("[{}][{}] {} requested expiry time change", server_name, group_name, username);
                self.dash_change_expiry(cc).await
            },
            "set_expiry"        => {
                info!("[{}][{}] {} changed expiry time", server_name, group_name, username);
                self.dash_set_expiry(cc, parts.get(1).copied()).await
            },
            "show_settings"     => {
                info!("[{}][{}] {} requested settings", server_name, group_name, username);
                self.dash_show_settings(cc).await
            },
            "shuffle_teams"     => {
                info!("[{}][{}] {} used Shuffle", server_name, group_name, username);
                self.dash_shuffle(cc, game_id).await
            },
            "start_match"       => {
                info!("[{}][{}] {} used Start", server_name, group_name, username);
                self.dash_start(cc,   game_id).await
            },
            "end_match"         => {
                info!("[{}][{}] {} used End", server_name, group_name, username);
                self.dash_end(cc,     game_id).await
            },
            _ => {
                cc.reply(&format!("Unknown button action: {action}"))
                    .await?;
                Ok(())
            }
        }
    }

    /// Show expiry time options
    async fn dash_change_expiry(&mut self, cc: &CC<'_>) -> Result<()> {
        use serenity::all::{CreateButton as CB, ButtonStyle as BS};

        // Check if user is in queue
        let user_id = cc.component.user.id;
        let is_in_queue = self.sessions.iter()
            .any(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

        if !is_in_queue {
            cc.reply("You must be in the queue to change your expiry time.").await?;
            return Ok(());
        }

        // Create buttons for time options: 30m, 1h, 2h, 3h, 4h
        let time_buttons = vec![
            CB::new("set_expiry:30m").label("30 minutes").style(BS::Secondary),
            CB::new("set_expiry:1h") .label("1 hour")    .style(BS::Secondary),
            CB::new("set_expiry:2h") .label("2 hours")   .style(BS::Secondary),
            CB::new("set_expiry:3h") .label("3 hours")   .style(BS::Secondary),
            CB::new("set_expiry:4h") .label("4 hours")   .style(BS::Secondary),
        ];

        let response = CIR::Message(
            CIRM::new()
                .content("Select your expiry time for this queue instance:")
                .components(vec![CAR::Buttons(time_buttons)])
                .ephemeral(true)
        );

        cc.component.create_response(&cc.ctx.http, response).await?;
        Ok(())
    }

    /// Set expiry duration for user in current queue
    async fn dash_set_expiry(&mut self, cc: &CC<'_>, duration_str: Option<&str>) -> Result<()> {
        let user_id = cc.component.user.id;
        
        // Parse duration string
        let duration = match duration_str {
            Some("30m") => std::time::Duration::from_secs(30 * 60),
            Some("1h")  => std::time::Duration::from_secs(60 * 60),
            Some("2h")  => std::time::Duration::from_secs(2 * 60 * 60),
            Some("3h")  => std::time::Duration::from_secs(3 * 60 * 60),
            Some("4h")  => std::time::Duration::from_secs(4 * 60 * 60),
            _ => {
                cc.reply("Invalid expiry duration.").await?;
                return Ok(());
            }
        };

        // Find and update the player's expiry duration in any session they're in
        let mut found = false;
        for session in self.sessions.iter_mut() {
            if let Some(player) = session.pool.iter_mut().find(|p| p.player.discord_id == user_id) {
                player.expiry_duration = Some(duration);
                found = true;
                break;
            }
        }

        if !found {
            cc.reply("You are not in the queue.").await?;
            return Ok(());
        }

        // Delete the ephemeral message by updating it
        let response = CIR::UpdateMessage(
            CIRM::new()
                .content(format!("Expiry time set to {} for this queue instance.", duration_str.unwrap_or("unknown")))
                .components(vec![]) // Remove buttons
        );
        cc.component.create_response(&cc.ctx.http, response).await?;

        // Update the dashboard
        self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap().get()).await;

        Ok(())
    }

    /// Show user settings as ephemeral embed in dashboard channel
    async fn dash_show_settings(&mut self, cc: &CC<'_>) -> Result<()> {
        use crate::handlers::settings::build_settings_embed;
        
        let user_id = cc.component.user.id;
        
        // Get user's settings
        let settings = match cc.db.users.get_settings(user_id).await {
            Ok(s) => s,
            Err(e) => {
                cc.reply(&format!("Failed to load settings: {}", e)).await?;
                return Ok(());
            }
        };

        // Build settings embed (without buttons for read-only view)
        let embed = build_settings_embed(&settings);

        // Send ephemeral message with settings embed
        let response = CIR::Message(
            CIRM::new()
                .content("**Your current settings:**\nUse `/settings` in DMs to modify these settings.")
                .embed(embed)
                .ephemeral(true)
        );

        cc.component.create_response(&cc.ctx.http, response).await?;
        Ok(())
    }

    pub async fn lock_button(&mut self, cc: &CC<'_>) -> Result<()> {
        let mut dash = match self.dash_get(cc.ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to get dashboard message for lock_button: {e}");
                return Err(e);
            }
        };
        let buttons = self.create_dashboard_buttons().await?;
        dash.edit(&cc.ctx.http, EditMessage::new().components(buttons)).await?;
        Ok(())
    }

    pub async fn unlock_button(&mut self, cc: &CC<'_>) -> Result<()> {
        let mut dash = match self.dash_get(cc.ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to get dashboard message for unlock_button: {e}");
                return Err(e);
            }
        };
        let buttons = self.create_dashboard_buttons().await?;
        dash.edit(&cc.ctx.http, EditMessage::new().components(buttons)).await?;
        Ok(())
    }
}

//
// Dashboard update queue
//

/// Request to update a specific group's dashboard
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DashboardUpdateRequest {
    pub guild_id: u64,
    pub group_id: u64,
}

/// Dashboard update queue that batches updates to reduce API calls
pub struct DashboardUpdateQueue {
    sender: mpsc::UnboundedSender<DashboardUpdateRequest>,
}

impl Clone for DashboardUpdateQueue {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl DashboardUpdateQueue {
    /// Create a new dashboard update queue and spawn the batching task
    pub fn new(ctx: Context, manager: Arc<tokio::sync::Mutex<crate::models::Manager>>, database: Arc<crate::Database>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        // Spawn the batching task
        tokio::spawn(Self::batch_processor(receiver, ctx, manager, database));

        Self { sender }
    }

    /// Request a dashboard update for a specific group
    pub fn request_update(&self, guild_id: u64, group_id: u64) {
        let request = DashboardUpdateRequest { guild_id, group_id };
        if let Err(e) = self.sender.send(request) {
            warn!("Failed to queue dashboard update: {e}");
        }
    }

    /// Background task that batches and processes dashboard updates
    ///
    /// This uses a HashSet to automatically deduplicate update requests for the same group.
    /// Since dashboards show current state, only the latest update matters - all previous
    /// requests for the same group are redundant and automatically discarded.
    async fn batch_processor(
        mut receiver: mpsc::UnboundedReceiver<DashboardUpdateRequest>,
        ctx: Context,
        manager: Arc<tokio::sync::Mutex<crate::models::Manager>>,
        database: Arc<crate::Database>,
    ) {
        let batch_window = Duration::from_millis(200); // Wait 200ms to batch updates
        // HashSet automatically deduplicates - if 10 updates come in for the same group,
        // we only keep one entry and process it once with the current state
        let mut pending_updates: HashSet<DashboardUpdateRequest> = HashSet::new();

        loop {
            // Wait for the first update request
            match receiver.recv().await {
                Some(request) => {
                    pending_updates.insert(request);

                    // Now wait for the batch window, collecting more updates
                    let deadline = tokio::time::Instant::now() + batch_window;

                    loop {
                        match tokio::time::timeout_at(deadline, receiver.recv()).await {
                            Ok(Some(request)) => {
                                // Got another update, add it to the batch
                                pending_updates.insert(request);
                            }
                            Ok(None) => {
                                // Channel closed, process remaining and exit
                                Self::process_batch(&pending_updates, &ctx, manager.clone(), database.clone()).await;
                                return;
                            }
                            Err(_) => {
                                // Timeout - batch window expired, process the batch
                                break;
                            }
                        }
                    }

                    // Process the batched updates
                    if !pending_updates.is_empty() {
                        // info!("Processing batch of {} dashboard updates", pending_updates.len());
                        Self::process_batch(&pending_updates, &ctx, manager.clone(), database.clone()).await;
                        pending_updates.clear();
                    }
                }
                None => {
                    // Channel closed, exit
                    return;
                }
            }
        }
    }

    /// Process a batch of dashboard updates
    async fn process_batch(
        updates: &HashSet<DashboardUpdateRequest>,
        ctx: &Context,
        manager: Arc<tokio::sync::Mutex<crate::models::Manager>>,
        database: Arc<crate::Database>,
    ) {
        // Process updates concurrently (Discord allows multiple requests in parallel)
        let mut tasks = Vec::new();

        for update in updates {
            let ctx = ctx.clone();
            let manager = manager.clone();
            let database = database.clone();
            let guild_id = update.guild_id;
            let group_id = update.group_id;

            // Spawn a task for each dashboard update
            let task = tokio::spawn(async move {
                //
                // Acquire lock briefly to get CURRENT dashboard data
                // This ensures we always show the latest state, regardless of how many
                // update requests were queued - they all get collapsed into this one update
                let (channel_id, dashboard_channel_id, message_id, embed, buttons, guild_name, pool_size) = {
                    let mut manager_lock = manager.lock().await;

                    let server = match manager_lock.get_server(serenity::all::GuildId::new(guild_id)) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Failed to get server for dashboard update: {e}");
                            return;
                        }
                    };

                    let guild_name = server.guild_name.clone();

                    let group = match server.groups.iter_mut().find(|g| g.group_id == group_id as u8) {
                        Some(g) => g,
                        None => {
                            warn!("[{}] Failed to find group {} for dashboard update", guild_name, group_id);
                            return;
                        }
                    };

                    // Log current session state
                    let pool_size = group.sessions.first().map(|s| s.pool.len()).unwrap_or(0);
                    //

                    // Refresh player ranks from Discord to ensure dashboard shows current ranks
                    // This prevents desync when players are promoted while sitting in queue
                    group.refresh_player_ranks(&ctx, serenity::all::GuildId::new(guild_id), &database).await;

                    // Validate VC status to ensure accurate display of who is in voice chat
                    // This prevents desync where flags don't match Discord's actual voice states
                    group.validate_vc_status(&ctx, serenity::all::GuildId::new(guild_id)).await;

                    // Get dashboard message info
                    let channel_id = group.channels.dashboard;
                    let dashboard_channel_id = channel_id.get();
                    let message_id = group.dashboard_msg;

                    // Generate dashboard content
                    let (embed, buttons) = match group.build_dashboard_content(&database, guild_id).await {
                        Ok(content) => content,
                        Err(e) => {
                            warn!("[{}] Failed to build dashboard content for group {}: {}", guild_name, group_id, e);
                            return;
                        }
                    };

                    (channel_id, dashboard_channel_id, message_id, embed, buttons, guild_name, pool_size)
                }; // Release lock here

                // Update the dashboard message WITHOUT holding any locks
                use serenity::all::EditMessage;
                let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{channel_id}"));
                //
                match channel_id.edit_message(&ctx.http, message_id, EditMessage::new().embed(embed.clone()).components(buttons.clone())).await {
                    Ok(_) => {
                        //
                    }
                    Err(e) => {
                        // Check if message was deleted (404 error)
                        if e.to_string().contains("404") || e.to_string().contains("Unknown Message") {
                            warn!("[{}] Dashboard message was deleted in #{}, recreating...", guild_name, channel_name);

                            // Recreate the dashboard message
                            use serenity::all::CreateMessage;
                            match channel_id.send_message(&ctx.http, CreateMessage::new().embed(embed).components(buttons)).await {
                                Ok(new_msg) => {
                                    info!("[{}] Recreated dashboard in #{}", guild_name, channel_name);

                                    // Update the stored message ID in memory
                                    let mut manager_lock = manager.lock().await;
                                    if let Ok(server) = manager_lock.get_server(serenity::all::GuildId::new(guild_id)) {
                                        if let Some(group) = server.groups.iter_mut().find(|g| g.group_id == group_id as u8) {
                                            group.dashboard_msg = new_msg.id;
                                        }
                                    }
                                    drop(manager_lock);

                                    // Persist to database
                                    if let Err(e) = database.groups.update_dashboard_msg(guild_id, dashboard_channel_id, new_msg.id.get()).await {
                                        warn!("Failed to update dashboard message ID in database: {e}");
                                    }
                                }
                                Err(create_err) => {
                                    warn!("[{}] Failed to recreate dashboard in #{}: {}", guild_name, channel_name, create_err);
                                }
                            }
                        } else {
                            warn!("[{}] Failed to update dashboard in #{}: {}", guild_name, channel_name, e);
                        }
                    }
                }
            });

            tasks.push(task);
        }

        // Wait for all updates to complete
        for task in tasks {
            let _ = task.await;
        }
    }
}
