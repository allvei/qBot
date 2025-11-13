use anyhow::{anyhow, Error, Result};
use serenity::all::{
    ButtonStyle as BS, ChannelId as CI, Context, CreateActionRow as CAR, CreateButton as CB,
    CreateEmbed as CE, CreateEmbedFooter as CEF, CreateMessage as CM,
    EditMessage, Message, Reaction,
};
use tracing::{error, info, warn};

use crate::models::{ComponentContext as CC, Group, Session, SessionStatus};

macro_rules! list_players {
    ($desc:ident, $team:ident) => {
        for (i, player) in $team.iter().enumerate() {
            let elo_str = player.player.rank.map(|r| format!("**[{}]** ", r.elo())).unwrap_or_default();
            $desc.push_str(&format!("{}. {}<@{}>\n", i + 1, elo_str, player.player.discord_id));
        }
    };
}

/// Helper function to format team players as a string for embed fields
fn format_team_field(team: &[crate::models::SessionPlayer]) -> String {
    team.iter()
        .enumerate()
        .map(|(i, player)| {
            let elo_str = player.player.rank.map(|r| format!("**[{}]** ", r.elo())).unwrap_or_default();
            format!("{}. {}<@{}>", i + 1, elo_str, player.player.discord_id)
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    InitQuota,
    
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
            "init_quota"      => Self::InitQuota,
            
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
            Self::InitQuota
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
    pub async fn create_dashboard_buttons(&mut self) -> Result<Vec<CAR>> {
        let quota = self.quota as usize;
        
        // Check if any session is hot AND still has enough players
        let is_hot  = self.sessions.iter().any(|s| s.is_hot() && s.pool.len() >= quota);
        let is_live = self.sessions.iter().any(|s| s.is_active());

        let bs = BS::Secondary;
        let buttons = vec![
            ("toggle_queue", "Join/Leave", bs, true),
            ("shuffle_teams", "Shuffle", bs, is_hot),
            ("start_match",   "Start",   bs, is_hot),
            ("end_match",     "End",     bs, is_live),
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
        let msg = channel.send_message(&ctx.http, self.dash_init().await.unwrap()).await;
        if let Ok(msg) = msg {
            self.dashboard_msg = msg.id;
            Ok(())
        } else {
            error!("Failed to send game ready notification");
            Err(anyhow!("Failed to send game ready notification"))
        }
    }

    /// Builds dashboard embed and components based on current group state
    async fn build_dashboard_content(&mut self) -> Result<(CE, Vec<CAR>)> {
        let mut emb  = CE::new().title("PUG Dashboard");
        let mut desc = String::new();
        
        let quota = self.quota as usize;
        let games_idle: Vec<&Session> = self.get_sessions_by_status(&SessionStatus::Idle);
        let games_hot:  Vec<&Session> = self.get_sessions_by_status(&SessionStatus::Hot);
        let games_live: Vec<&Session> = self.get_sessions_by_status(&SessionStatus::Live);
        
        if let Some(game_current) = games_idle.first() {
            let queue_players = game_current.pool.len();
            
            desc.push_str(&format!("**Current Queue ({}/{}):**\n",queue_players, quota));
            
            if game_current.pool.is_empty() {
                desc.push_str("*None*\n");
            } else {
                match queue_players.cmp(&quota) {
                    std::cmp::Ordering::Less => {
                        for (i, player) in game_current.pool.iter().enumerate() {
                            let elo_str = player.player.rank.map(|r| format!("**[{}]** ", r.elo())).unwrap_or_default();
                            desc.push_str(&format!("{}. {}<@{}>\n", i + 1, elo_str, player.player.discord_id));
                        }
                    }
                    std::cmp::Ordering::Equal => {
                        desc.push_str("**🔥 READY TO START! 🔥**\n");
                    }
                    std::cmp::Ordering::Greater => {
                        desc.push_str("**🔥 MATCH READY! 🔥**\n");

                        let extra_players = &game_current.pool[quota..];
                        if !extra_players.is_empty() {
                            desc.push_str(&format!("\n**⏳ Queued for Next ({}):**\n",extra_players.len()));
                            list_players!(desc, extra_players);
                        }
                    }
                }
            }
        } else if !games_hot.is_empty() {
            // Show hot game status with players not in VC yet
            desc.push_str("**🔥 Match Ready to Start!**\n");
            for game in &games_hot {
                let players_in_vc = game.pool.iter().filter(|p| p.in_queue_vc).count();
                let players_not_in_vc: Vec<_> = game.pool.iter()
                    .filter(|p| !p.in_queue_vc)
                    .collect();
                
                desc.push_str(&format!("• {}/{} players in voice channel\n", players_in_vc, game.pool.len()));
                
                if !players_not_in_vc.is_empty() {
                    desc.push_str("⏳ **Waiting for:**\n");
                    for player in players_not_in_vc {
                        let elo_str = player.player.rank.map(|r| format!("**[{}]** ", r.elo())).unwrap_or_default();
                        desc.push_str(&format!("  • {}<@{}>\n", elo_str, player.player.discord_id));
                    }
                }
            }
            desc.push('\n');
        } else if games_live.is_empty() {
            // Only show "no active games" if there are no games at all
            desc.push_str("**📋 Queue Status**\n*No active games. Join the queue to get started!*\n\n");
        }

        emb = emb.description(desc);
        
        // Add team fields for idle games with enough players
        if let Some(game_current) = games_idle.first() {
            let queue_players = game_current.pool.len();
            if queue_players >= quota {
                let team_size = quota / 2;
                // Sort teams by rank descending before display
                let mut team_red: Vec<_> = game_current.pool[0..team_size].to_vec();
                let mut team_blu: Vec<_> = game_current.pool[team_size..quota].to_vec();
                team_red.sort_by(|a, b| {
                    let elo_a = a.player.rank.map(|r| r.elo()).unwrap_or(0);
                    let elo_b = b.player.rank.map(|r| r.elo()).unwrap_or(0);
                    elo_b.cmp(&elo_a) // Descending order
                });
                team_blu.sort_by(|a, b| {
                    let elo_a = a.player.rank.map(|r| r.elo()).unwrap_or(0);
                    let elo_b = b.player.rank.map(|r| r.elo()).unwrap_or(0);
                    elo_b.cmp(&elo_a) // Descending order
                });
                emb = emb.field("🔴 Red", format_team_field(&team_red), true);
                emb = emb.field("🔵 Blue", format_team_field(&team_blu), true);
            }
        }
        
        // Add team fields for hot games
        for game in games_hot {
            if game.pool.len() >= quota {
                let team_size = quota / 2;
                // Sort teams by rank descending before display
                let mut team_red: Vec<_> = game.pool[0..team_size].to_vec();
                let mut team_blu: Vec<_> = game.pool[team_size..quota].to_vec();
                team_red.sort_by(|a, b| {
                    let elo_a = a.player.rank.map(|r| r.elo()).unwrap_or(0);
                    let elo_b = b.player.rank.map(|r| r.elo()).unwrap_or(0);
                    elo_b.cmp(&elo_a) // Descending order
                });
                team_blu.sort_by(|a, b| {
                    let elo_a = a.player.rank.map(|r| r.elo()).unwrap_or(0);
                    let elo_b = b.player.rank.map(|r| r.elo()).unwrap_or(0);
                    elo_b.cmp(&elo_a) // Descending order
                });
                emb = emb.field("🔴 Red", format_team_field(&team_red), true);
                emb = emb.field("🔵 Blue", format_team_field(&team_blu), true);
            }
        }

        let buttons = self.create_dashboard_buttons().await.unwrap();

        Ok((emb, buttons))
    }

    /// Initializes a dashboard based on current group state
    pub async fn dash_init(&mut self) -> Result<CM> {
        let (embed, buttons) = self.build_dashboard_content().await?;
        let message = CM::new().embed(embed).components(buttons);
        Ok(message)
    }

    /// Updates a dashboard based on current group state
    pub async fn dash_update(&mut self, ctx: &Context) -> Result<(), Error> {
        let mut dash = match self.dash_get(ctx).await {
            Ok(msg) => msg,
            Err(e) => {
                warn!("Failed to get dashboard message: {}", e);
                return Err(e);
            }
        };
        let (embed, buttons) = self.build_dashboard_content().await?;
        
        match dash.edit(&ctx.http, EditMessage::new().embed(embed).components(buttons)).await {
            Ok(_) => {Ok(())},
            Err(e) => {Err(e.into())}
        }
    }

    /// Handles the toggle queue button (combines join/leave)
    async fn dash_toggle_queue(&mut self, cc: &CC<'_>) -> Result<()> {
        let user_id = cc.component.user.id;
        let quota = self.quota as usize;

        // Check if player is already in a game
        if let Ok(session) = self.get_user_session(user_id).await {
            // Player is in queue, remove them
            session.remove_player(user_id);
            let pool_len = session.pool.len();
            info!("Removed player from game. Queue now has {} players", pool_len);
            
            // If session was hot but now below quota, transition back to idle
            if session.is_hot() && pool_len < quota {
                info!("Session dropped below quota, transitioning from Hot to Idle");
                session.idle();
            }
        } else {
            // Player is not in queue, add them
            
            // Ensure we have an idle session (create if needed)
            let idle_sessions = self.get_sessions_by_status(&SessionStatus::Idle);
            if idle_sessions.is_empty() {
                info!("No idle session found, creating one");
                self.create_session();
            } else if idle_sessions.len() > 1 {
                return Err(anyhow::anyhow!("Multiple idle games found: {}", idle_sessions.len()));
            } else {
                info!("Found one existing idle game");
            }

            // Get or assign player's rank (auto-creates ranks and assigns Apprentice if needed)
            use crate::handlers::player::get_or_assign_player_rank;
            if let Some(guild_id) = cc.component.guild_id {
                match get_or_assign_player_rank(cc.ctx, &cc.db, guild_id, user_id).await {
                    Ok(rank) => {
                        self.queue_player(user_id, rank, cc.ctx).await;
                    },
                    Err(e) => {
                        return Ok(());
                    }
                }
            } else {
                cc.reply("❌ This command can only be used in a server.").await?;
                return Ok(());
            }
        }

        // Always acknowledge and update dashboard
        cc.acknowledge().await;
        if let Err(e) = self.dash_update(cc.ctx).await {
            warn!("Failed to update dashboard after toggle_queue: {}", e);
        }

        Ok(())
    }

    /// Handles the shuffle teams button
    async fn dash_shuffle(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        let mut is_shuffled = false;
        let quota = self.quota as usize;

        // Find the game to shuffle
        if let Some(game) = self.sessions.iter_mut().find(|s| s.status == SessionStatus::Idle && s.pool.len() >= quota)
        {
            // Shuffle the players using rand crate
            use rand::seq::SliceRandom;
            game.pool.shuffle(&mut rand::rng());
            is_shuffled = true;
            info!("Teams shuffled for game with {} players",game.pool.len());
        }

        if is_shuffled {
            cc.acknowledge().await;
            if let Err(e) = self.dash_update(cc.ctx).await {
                warn!("Failed to update dashboard after shuffle: {}", e);
            }
        } else {
            cc.reply(&format!("❌ No game ready for shuffling. Need at least {} players in queue.", quota)).await?;
        }

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
                cc.reply("❌ Only runners can start matches.").await?;
                return Ok(());
            },
            Err(e) => {
                warn!("Failed to check runner role: {}", e);
                cc.reply("❌ Failed to verify permissions.").await?;
                return Ok(());
            }
        }
        
        // Check if there's a hot game to start
        let has_hot_game = self.sessions.iter().any(|s| s.is_hot());
        
        if !has_hot_game {
            cc.reply("❌ No hot game ready to start.").await?;
            return Ok(());
        }

        // Move players to team channels (Hot → Push → Live)
        match self.push(cc.ctx).await {
            Ok(_) => {
                info!("Players moved to team channels and game is now live");
                cc.acknowledge().await;
                Ok(())
            }
            Err(e) => {
                error!("Failed to start match: {}", e);
                cc.reply(&format!("❌ Failed to start match: {}", e)).await?;
                Ok(())
            }
        }
    }

    /// Handles the end match button
    async fn dash_end(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        // Check if there's an active game to end
        let has_active_game = self.sessions.iter().any(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Live);
        
        if !has_active_game {
            cc.reply("❌ No active match to end.").await?;
            return Ok(());
        }

        // Move players back to queue channel (Hot/Live → Pull → Idle)
        match self.pull(cc.ctx).await {
            Ok(_) => {
                info!("Match ended, players moved back to queue");
                cc.acknowledge().await;
                Ok(())
            }
            Err(e) => {
                error!("Failed to end match: {}", e);
                cc.reply(&format!("❌ Failed to end match: {}", e)).await?;
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

        match action {
            "toggle_queue" => self.dash_toggle_queue(cc).await,
            "shuffle_teams" => self.dash_shuffle(cc, game_id).await,
            "start_match"   => self.dash_start(cc,   game_id).await,
            "end_match"     => self.dash_end(cc,     game_id).await,
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