use anyhow::{anyhow, Error, Result};
use std::time::SystemTime;
use std::cmp::Ordering::*;
use crate::{QueueToggleType, log_queue_toggle, models::constants::DEFAULT_TIMEOUT};
use serenity::all::{
    ButtonStyle as BS, ChannelId as CI, Context, CreateActionRow as CAR, CreateButton as CB,
    CreateEmbed as CE, CreateEmbedFooter as CEF, CreateMessage as CM,
    EditMessage, Message, Reaction,
};
use tracing::{error, info, warn};

use crate::models::{ComponentContext as CC, DashboardQueueKey, Group, Session, SessionStatus};

macro_rules! list_players {
    ($desc:ident, $team:ident) => {
        for (i, player) in $team.iter().enumerate() {
            let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
            $desc.push_str(&format!("{}<@{}>\n", elo_str, player.player.discord_id));
        }
    };
}

/// Helper function to format team players as a string for embed fields
fn format_team_field(team: &[crate::models::SessionPlayer]) -> String {
    team.iter()
        .enumerate()
        .map(|(i, player)| {
            let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
            format!("{}<@{}>", elo_str, player.player.discord_id)
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    DashboardToggleQueue,
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
            "toggle_queue"    => Self::DashboardToggleQueue,
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
            Self::DashboardToggleQueue |
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
            Err(e)  => Err(anyhow!("Failed to get dashboard message: {}", e)),
        }
    }

    /// Creates buttons for the dashboard
    pub async fn create_dashboard_buttons(&self) -> Result<Vec<CAR>> {
        let quota = self.quota as usize;

        // Check if any session is hot AND still has enough players
        let is_hot  = self.sessions.iter().any(|s| s.is_hot());
        let is_live = self.sessions.iter().any(|s| s.is_active());

        let buttons = vec![
            ("toggle_queue", "Join/Leave", BS::Primary, true),
            ("shuffle_teams", "Shuffle", BS::Secondary, is_hot),
            ("start_match",   "Start",   BS::Success, is_hot),
            ("end_match",     "End",     BS::Danger, is_live),
        ];

        Ok(vec![CAR::Buttons(Self::gen_buttons(buttons))])
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

    pub async fn dash_publish(&mut self, ctx: &Context, channel: CI) -> Result<(), Error>{
        // Create new dashboard message (don't check if it exists - caller should check)
        let msg = channel.send_message(&ctx.http, self.dash_init().await?).await;
        if let Ok(msg) = msg {
            self.dashboard_msg = msg.id;
            Ok(())
        } else {
            let channel_name = channel.name(&ctx.http).await.unwrap_or_else(|_| format!("#{}", channel));
            error!("Failed to send dashboard message in #{}", channel_name);
            Err(anyhow!("Failed to send dashboard message in #{}: {}", channel_name, msg.unwrap_err()))
        }
    }

    /// Builds dashboard embed and components based on current group state
    pub async fn build_dashboard_content(&self) -> Result<(CE, Vec<CAR>)> {
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
                                description.push_str(&format!("Join deadline: <t:{}:R>\n", deadline_timestamp));
                                description.push_str("Missing players will be removed. Overflow players will take their spots.\n\n");
                            }
                        }

                        description.push_str("**Missing players:**\n");
                        for player in players_never_joined {
                            let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
                            description.push_str(&format!("  • {}<@{}>\n", elo_str, player.player.discord_id));
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
                    description.push_str(&format!("• {} ({} players)\n", status_text, session.pool.len()));
                }
            }
            description.push('\n');
        } else {
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
                                description.push_str(&format!("Join deadline: <t:{}:R>\n", deadline_timestamp));
                                description.push_str("Missing players will be removed. Overflow players will take their spots.\n\n");
                            }
                        }

                        description.push_str("**Missing players:**\n");
                        for player in players_never_joined {
                            let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
                            description.push_str(&format!("  • {}<@{}>\n", elo_str, player.player.discord_id));
                        }
                        description.push_str("\n\n");
                    }
                    // Note: Next Queue for overflow will be shown after teams
                } else {
                    // Session is Idle - show normal queue
                    let queue_players = current_session.pool.len();
                    description.push_str(&format!("**Queue ({}/{})**\n", queue_players, quota));

                    if queue_players > 0 {
                        for (i, player) in current_session.pool.iter().enumerate() {
                            let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
                            description.push_str(&format!("{}<@{}>\n", elo_str, player.player.discord_id));
                        }
                    } else {
                        description.push_str("*No players in queue. Join to get started!*\n");
                    }
                    description.push('\n');
                }
            } else {
                description.push_str("**Queue status**\n* Empty. Join to get started!*\n\n");
            }
        }

        embed = embed.description(description);

        // Add connect info if available
        if let Some(ref connect_info) = self.connect_info {
            let mut field_value = format!("```{}```", connect_info);
            
            // Try to extract IP:PORT and create a Steam link
            // Discord doesn't support steam:// protocol links, so display as copyable text
            if let Some(steam_link) = Self::extract_steam_link(connect_info) {
                field_value.push_str(&format!("\n**Steam Link:**\n```{}```", steam_link));
            }
            
            embed = embed.field("Server connect info:", field_value, false);
        }

        // Add team fields for inactive sessions with enough players
        if let Some(current_session) = inactives.first() {
            let queue_players = current_session.pool.len();
            if queue_players >= quota {
                let (team_red, team_blu) = get_sorted_teams(&current_session.pool, quota);
                embed = embed.field("🔴 RED", format_team_field(&team_red), true);
                embed = embed.field("🔵 BLU", format_team_field(&team_blu), true);

                // Show fatkidded players AFTER teams if there are overflow players
                if current_session.is_hot() && queue_players > quota {
                    let overflow_count = queue_players - quota;
                    let mut fatkid = format!("**Waiting for next game ({}/{}):**\n", overflow_count, quota);
                    for (i, player) in current_session.pool.iter().skip(quota).enumerate() {
                        let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
                        fatkid.push_str(&format!("{}<@{}>\n", elo_str, player.player.discord_id));
                    }
                    embed = embed.field("\u{200B}", fatkid, false); // Full-width field
                }
            }
        }

        // Add team fields for active sessions
        for session in &actives {
            if session.pool.len() >= quota {
                let (team_red, team_blu) = get_sorted_teams(&session.pool, quota);
                embed = embed.field("🔴 RED", format_team_field(&team_red), true);
                embed = embed.field("🔵 BLU", format_team_field(&team_blu), true);
            }
        }

        // Show fatkidded players for idle session (when there's an active game)
        if !actives.is_empty() {
            if let Some(next_session) = inactives.first() {
                if !next_session.pool.is_empty() {
                    let mut fatkid = format!("**Waiting for next game ({}/{}):**\n", next_session.pool.len(), quota);
                    for (i, player) in next_session.pool.iter().enumerate() {
                        let elo_str = player.player.rank.map(|r| format!("[**{}**] ", r.elo())).unwrap_or_default();
                        fatkid.push_str(&format!("{}<@{}>\n", elo_str, player.player.discord_id));
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
            Some(format!("steam://connect/{}/{}", ip_port, pwd))
        } else {
            Some(format!("steam://connect/{}", ip_port))
        }
    }

    /// Initializes a dashboard based on current group state
    pub async fn dash_init(&mut self) -> Result<CM> {
        let (embed, buttons) = self.build_dashboard_content().await?;
        let message = CM::new().embed(embed).components(buttons);
        Ok(message)
    }

    /// Updates a dashboard based on current group state
    /// Auto-recovers by creating a new dashboard if the current one is missing/deleted
    pub async fn dash_update(&mut self, ctx: &Context) -> Result<(), Error> {
        let mut dash = match self.dash_get(ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Dashboard message not found (may have been deleted): {}", e);
                info!("Auto-recovering: creating new dashboard message");
                
                // Dashboard is missing - create a new one
                match self.dash_publish(ctx, self.channels.dashboard).await {
                    Ok(_) => {
                        info!("Successfully created new dashboard message with ID {}", self.dashboard_msg);
                        // Return early since dash_publish already creates the dashboard with current content
                        return Ok(());
                    },
                    Err(publish_err) => {
                        error!("Failed to auto-recover dashboard: {}", publish_err);
                        return Err(publish_err);
                    }
                }
            }
        };
        
        let (embed, buttons) = self.build_dashboard_content().await?;
        match dash.edit(&ctx.http, EditMessage::new().embed(embed).components(buttons)).await {
            Ok(_) => {Ok(())},
            Err(e) => {
                error!("Failed to update dashboard: {}", e);
                Err(e.into())
            }
        }
    }
    
    /// Queue a dashboard update (non-blocking, batched)
    /// Requires guild_id to be passed since Group doesn't store it
    pub async fn queue_dash_update(&self, ctx: &Context, guild_id: u64) {
        // Try to get queue from context data using the key from models module
        let data = ctx.data.read().await;
        if let Some(queue) = data.get::<DashboardQueueKey>() {
            queue.request_update(guild_id, self.group_id as u64);
        } else {
            warn!("Dashboard queue not initialized in Context");
            // Note: Can't fallback to dash_update here because we'd need &mut self
            // The dashboard queue should always be initialized, so this is just a safety check
        }
    }

    /// Handles the toggle queue button (combines join/leave)
    async fn dash_toggle_queue(&mut self, cc: &CC<'_>) -> Result<()> {
        let user_id = cc.component.user.id;
        let quota = self.quota as usize;
        
        // Store channel IDs before any borrows
        let dashboard_channel = self.channels.dashboard;

        // Get session index before mutable borrow
        let session_idx = self.sessions.iter()
            .position(|s| s.pool.iter().any(|p| p.player.discord_id == user_id));

        // Check if player is already in a game (this is the initial state check)
        let player_was_in_queue = self.get_user_session(user_id).await.is_ok();
        
        // Check if player is already in a game
        let should_regenerate_teams = if let Ok(session) = self.get_user_session(user_id).await {
            // Check if player is physically in the queue VC
            let player_in_vc = if let Some(player) = session.pool.iter().find(|p| p.player.discord_id == user_id) {
                player.in_queue_vc
            } else {
                false
            };

            // If player is in VC, disconnect them from voice channel
            if player_in_vc {
                // Acknowledge button press before disconnecting
                cc.defer_update().await?;
                
                let username = cc.ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string());
                
                // Disconnect player from voice channel
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

            // Player is in queue but not in VC, remove them
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

            // If session was hot, check what to do next
            if was_hot {
                if pool_len < quota {
                    // Dropped below quota, transition back to idle
                    if let Some(idx) = session_idx {
                        info!("[Session {}] Dropped below quota, transitioning from Hot to Idle", idx);
                    } else {
                        info!("Session dropped below quota, transitioning from Hot to Idle");
                    }
                    session.idle();
                    false
                } else {
                    // Still at or above quota, need to regenerate teams
                    if let Some(idx) = session_idx {
                        info!("[Session {}] Still meets quota after player left, will regenerate teams", idx);
                    } else {
                        info!("Session still meets quota after player left, will regenerate teams");
                    }
                    true
                }
            } else {
                false
            }
        } else {
            false
        };

        // Regenerate teams if needed (outside the session borrow scope)
        if should_regenerate_teams {
            self.generate_teams(cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;
        }

        // If player was in queue, we removed them above - update dashboard and return
        if player_was_in_queue {
            self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap().get()).await;
            return Ok(());
        }

        // Only add player if they were NOT originally in the queue
        if !player_was_in_queue {
            // Player is not in queue, add them

            // Check if we have an idle or hot session to join
            let has_joinable_session = self.sessions.iter().any(|s|
                s.status == SessionStatus::Idle || s.status == SessionStatus::Hot
            );

            if !has_joinable_session {
                cc.reply("Cannot join - match is in progress. Please wait.").await?;
                return Ok(());
            }
            
            // Defer update now that we know we'll succeed
            cc.defer_update().await?;

            // Get or assign player's rank (auto-creates ranks and assigns Apprentice if needed)
            use crate::handlers::player::get_or_assign_player_rank;
            if let Some(guild_id) = cc.component.guild_id {
                // Get player object from database (without fetching discord tag for performance)
                let mut player = match cc.db.get_user(user_id).await {
                    Ok(p) => p,
                    Err(_) => match cc.db.new_user(user_id).await {
                        Ok(p) => p,
                        Err(e) => {
                            warn!("Failed to get or create player: {}", e);
                            return Ok(());
                        }
                    }
                };
                
                // Fetch discord tag from component user for performance (avoid extra API call)
                player.discord_tag = Some(cc.component.user.tag());
                
                match get_or_assign_player_rank(cc.ctx, &cc.db, guild_id, user_id).await {
                    Ok(rank) => {
                        // Refresh player rank from current Discord roles before queueing
                        player.rank = Some(rank);
                        if let Err(e) = self.queue_player(player, rank, cc.ctx, Some(guild_id), Some(&cc.db), Some(cc.manager.clone())).await {
                            warn!("Failed to queue player: {}", e);
                        } else {
                            // Log successful queue join via button
                            let server_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                            let group_name = cc.ctx.cache.channel(dashboard_channel)
                                .map(|ch| ch.name.clone())
                                .unwrap_or_else(|| "Unknown".to_string());
                            let username = cc.ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string());
                            log_queue_toggle(&server_name, &group_name, &username, QueueToggleType::BJ);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to get/assign rank: {}", e);
                        return Ok(());
                    }
                }
            } else {
                cc.reply("This command can only be used in a server.").await?;
                return Ok(());
            }
        }

        // Update dashboard to reflect changes
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
            cc.reply(&format!("No game ready for shuffling. Need at least {} players in queue.", quota)).await?;
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
                warn!("Failed to check runner role: {}", e);
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
                error!("Failed to start match: {}", e);
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
                error!("Failed to end match: {}", e);
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
            "toggle_queue"      => {
                // Log will be done inside dash_toggle_queue with proper context
                self.dash_toggle_queue(cc).await
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
                cc.reply(&format!("Unknown button action: {}", action))
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn lock_button(&mut self, cc: &CC<'_>) -> Result<()> {
        let mut dash = match self.dash_get(cc.ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to get dashboard message for lock_button: {}", e);
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
                warn!("Failed to get dashboard message for unlock_button: {}", e);
                return Err(e);
            }
        };
        let buttons = self.create_dashboard_buttons().await?;
        dash.edit(&cc.ctx.http, EditMessage::new().components(buttons)).await?;
        Ok(())
    }
}