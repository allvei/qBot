use anyhow::{anyhow, Error, Result};
use serenity::all::{
    ButtonStyle as BS, ChannelId as CI, Context, CreateActionRow as CAR, CreateButton as CB,
    CreateEmbed as CE, CreateEmbedFooter as CEF, CreateMessage as CM,
    EditMessage, Message, Reaction,
};
use tracing::{error, info};

use crate::models::{ComponentContext as CC, Group, Session, SessionStatus};

macro_rules! list_players {
    ($desc:ident, $team:ident) => {
        for (i, player) in $team.iter().enumerate() {
            $desc.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
        }
    };
}

/// Helper function to format team players as a string for embed fields
fn format_team_field(team: &[crate::models::SessionPlayer]) -> String {
    team.iter()
        .enumerate()
        .map(|(i, player)| format!("{}. <@{}>", i + 1, player.player.discord_id))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Represents different types of button interactions in the Discord bot
#[derive(Debug, Clone, PartialEq)]
pub enum ButtonType {
    // Setup flow buttons
    SetupDashboard,
    SetupQueue,
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
        if let Some(game_current) = games_idle.first() {
            let queue_players = game_current.pool.len();
            
            desc.push_str(&format!("**Current Queue ({}/{}):**\n",queue_players, quota));
            
            if game_current.pool.is_empty() {
                desc.push_str("*None*\n");
            } else {
                match queue_players.cmp(&quota) {
                    std::cmp::Ordering::Less => {
                        for (i, player) in game_current.pool.iter().enumerate() {
                            desc.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
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
        } else {
            desc.push_str("**📋 Queue Status**\n*No active games. Join the queue to get started!*\n\n");
        }

        let games_hot:  Vec<&Session> = self.get_sessions_by_status(&SessionStatus::Hot);
        let games_live: Vec<&Session> = self.get_sessions_by_status(&SessionStatus::Live);
        
        // Show hot games status
        if !games_hot.is_empty() {
            desc.push_str("\n**🔥 Ready to Start:**\n");
        }

        // Show live matches
        if !games_live.is_empty() {
            desc.push_str("**🎮 Live Matches:**\n");
            for _game in games_live {
                desc.push_str("• In Progress\n");
            }
        }

        emb = emb.description(desc);
        
        // Add team fields for idle games with enough players
        if let Some(game_current) = games_idle.first() {
            let queue_players = game_current.pool.len();
            if queue_players >= quota {
                let team_red = &game_current.pool[0..4];
                let team_blu = &game_current.pool[4..8];
                emb = emb.field("🔴 Red", format_team_field(team_red), true);
                emb = emb.field("🔵 Blue", format_team_field(team_blu), true);
            }
        }
        
        // Add team fields for hot games
        for game in games_hot {
            if game.pool.len() >= 8 {
                let team_red = &game.pool[0..4];
                let team_blu = &game.pool[4..8];
                emb = emb.field("🔴 Red", format_team_field(team_red), true);
                emb = emb.field("🔵 Blue", format_team_field(team_blu), true);
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
        let mut dash = self.dash_get(ctx).await.unwrap();
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
            
            // Check if we have idle sessions
            match self.get_sessions_by_status(&SessionStatus::Idle).len() {
                0 => {
                    info!("No idle games found, creating a new game");
                    self.create_session();
                }
                1 => info!("Found one existing idle game"),
                n => return Err(anyhow::anyhow!("Multiple idle games found: {}", n)),
            };

            self.queue_player(user_id, cc.ctx).await;
        }

        // Always acknowledge and update dashboard
        cc.acknowledge().await;
        self.dash_update(cc.ctx).await;

        Ok(())
    }

    /// Handles the shuffle teams button
    async fn dash_shuffle(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        let mut is_shuffled = false;

        // Find the game to shuffle
        if let Some(game) = self.sessions.iter_mut().find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8)
        {
            // Shuffle the players using rand crate
            use rand::seq::SliceRandom;
            game.pool.shuffle(&mut rand::rng());
            is_shuffled = true;
            info!("Teams shuffled for game with {} players",game.pool.len());
        }

        if is_shuffled {
            cc.acknowledge().await;
            self.dash_update(cc.ctx).await;
        } else {
            cc.reply("❌ No game ready for shuffling. Need at least 8 players in queue.").await?;
        }

        Ok(())
    }

    /// Handles the start match button
    async fn dash_start(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        let mut is_live = false;

        // Find the game to start
        if let Some(game) = self.sessions.iter_mut()
            .find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8)
        {
            // Change game status to Hot (ready to start)
            game.status   = SessionStatus::Hot;
            is_live = true;
            info!("Match started for game with {} players",game.pool.len());
        }

        if is_live {
            cc.acknowledge().await;
            self.dash_update(cc.ctx).await;
        } else {
            cc.reply("❌ No game ready to start. Need at least 8 players and shuffled teams.").await?;
        }

        Ok(())
    }

    /// Handles the end match button
    async fn dash_end(&mut self, cc: &CC<'_>, _game_id: Option<String>) -> Result<()> {
        let mut match_ended = false;

        // Find active games to end
        for game in &mut self.sessions {
            if game.status == SessionStatus::Hot || game.status == SessionStatus::Live {
                // Clear the game and reset to idle
                game.pool.clear();
                game.status = SessionStatus::Idle;
                match_ended = true;
                info!("Match ended and game reset");
                break;
            }
        }

        if match_ended {
            cc.acknowledge().await;
            self.dash_update(cc.ctx).await;
        } else {
            cc.reply("❌ No active match to end.").await?;
        }

        Ok(())
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
        let mut dash = self.dash_get(cc.ctx).await.unwrap();
        let buttons = self.create_dashboard_buttons().await.unwrap();
        dash.edit(&cc.ctx.http, EditMessage::new().components(buttons)).await;
        Ok(())
    }

    pub async fn unlock_button(&mut self, cc: &CC<'_>) -> Result<()> {
        let mut dash = self.dash_get(cc.ctx).await.unwrap();
        let buttons = self.create_dashboard_buttons().await.unwrap();
        dash.edit(&cc.ctx.http, EditMessage::new().components(buttons)).await;
        Ok(())
    }
}