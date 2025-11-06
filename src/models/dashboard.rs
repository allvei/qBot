use anyhow::{anyhow,Error, Result};
use serenity::all::{ButtonStyle, ChannelId as CI, Context, CreateActionRow, CreateButton, CreateEmbed as CE, CreateEmbedFooter as CEF, CreateMessage as CM, EditMessage, Message, Reaction};
use tracing::{error, info};

use crate::{models::{session::{SessionStatus, Session}, server::Group}, ComponentContext};

macro_rules! list_players {
    ($desc:ident, $team:ident) => {
        for (i, player) in $team.iter().enumerate() {
            $desc.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
        }
    };
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
    DashboardJoin,
    DashboardLeave,
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
            "join_queue"      => Self::DashboardJoin,
            "leave_queue"     => Self::DashboardLeave,
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
            Self::DashboardJoin    |
            Self::DashboardLeave   |
            Self::DashboardShuffle |
            Self::DashboardStart   |
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
        let channel = CI::new(self.channels.dashboard.into());
        let message = channel.message(&ctx.http, self.dashboard_msg).await;
        match message {
            Ok(msg) => Ok(msg),
            Err(e)  => Err(anyhow!("Failed to get dashboard message: {}", e)),
        }
    }

    /// Creates buttons for the dashboard
    pub async fn create_dashboard_buttons(&self) -> Result<Vec<CreateActionRow>> {
        info!("Creating dashboard buttons for group {}", self.group_id);
        // Check if there's an live game to enable/disable buttons
        let has_live_game  = !self.games.is_empty();
        let has_ready_game = self.games.iter().any(|s| s.pool.len() >= 8);

        // Define button configurations - this makes it easy to add/remove buttons
        let button_configs = vec![
            // (custom_id, label, style, disabled)
            ("join",    "Join queue",  ButtonStyle::Secondary, false),
            ("leave",   "Leave queue", ButtonStyle::Secondary, false),
            ("shuffle", "Shuffle",     ButtonStyle::Secondary, !has_ready_game),
            ("start",   "Start match", ButtonStyle::Secondary, !has_live_game),
            ("end",     "End match",   ButtonStyle::Secondary, !has_live_game),
        ];

        // Generate buttons from configurations
        let buttons = Self::gen_buttons(button_configs);

        Ok(vec![CreateActionRow::Buttons(buttons)])
    }

    fn gen_buttons(button_configs: Vec<(&'static str, &'static str, ButtonStyle, bool)>) -> Vec<CreateButton> {
        button_configs.into_iter().map(|(action, label, style, disabled)| {
                // Create the button with all specified properties
                CreateButton::new(action).label(label).style(style).disabled(disabled)
            }).collect()
    }

    pub async fn has_dashboard(&self, ctx: &Context) -> bool {
        let channel = CI::new(self.channels.dashboard.into());
        let message = channel.message(&ctx.http, self.dashboard_msg).await;
        message.is_ok()
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
    async fn build_dashboard_content(&mut self) -> Result<(CE, Vec<CreateActionRow>)> {
        let mut embed = CE::new().title("PUG Dashboard");
        let mut desc = String::new();
        
        let quota = self.quota as usize;
        let games_idle: Vec<&Session> = self.get_games_by_status(&SessionStatus::Idle);
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
                        
                        let team_red = &game_current.pool[0..4];
                        desc.push_str("**🔴 Red:**\n");
                        list_players!(desc, team_red);

                        let team_blu = &game_current.pool[4..8];
                        desc.push_str("\n**🔵 Blue:**\n");
                        list_players!(desc, team_blu);

                        desc.push('\n');
                    }
                    std::cmp::Ordering::Greater => {
                        desc.push_str("**🔥 MATCH READY! 🔥**\n");

                        if queue_players >= 8 {
                            let team_red = &game_current.pool[0..4];
                            desc.push_str("**🔴 Red:**\n");
                            list_players!(desc, team_red);

                            let team_blu = &game_current.pool[4..8];
                            desc.push_str("\n**🔵 Blue:**\n");
                            list_players!(desc, team_blu);
                        }

                        let extra_players = &game_current.pool[quota..];
                        if !extra_players.is_empty() {
                            desc.push_str(&format!("\n**⏳ Queued for Next ({}):**\n",extra_players.len()));
                            list_players!(desc, extra_players);
                        }
                        desc.push('\n');
                    }
                }
            }
        } else {
            desc.push_str("**📋 Queue Status**\n*No active games. Join the queue to get started!*\n\n");
        }

        let games_hot:  Vec<&Session> = self.get_games_by_status(&SessionStatus::Hot);
        // Show hot games (waiting to start)
        if !games_hot.is_empty() {
            desc.push_str("**🔥 Ready games:**\n");
            for _game in games_hot {
                desc.push_str("• Ready to start!\n");
            }
            desc.push('\n');
        }

        let games_live: Vec<&Session> = self.get_games_by_status(&SessionStatus::Live);
        // Show live matches
        if !games_live.is_empty() {
            desc.push_str("**⚡ Live Matches:**\n");
            for _game in games_live {
                desc.push_str("• Live\n");
            }
            desc.push('\n');
        }

        embed = embed.description(desc);
        // embed = embed.footer(CEF::new("Use the buttons below to manage the queue and matches"));

        let buttons = self.create_dashboard_buttons().await.unwrap();

        Ok((embed, buttons))
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

    /// Handles the join queue button
    async fn dash_join_queue(&mut self,cc: &ComponentContext<'_>) -> Result<()> {
        let user = cc.component.user.id;

        let mut already_in_queue = false;

        // Check if we have idle games
        match self.get_games_by_status(&SessionStatus::Idle).len() {
            0 => {
                info!("No idle games found, creating a new game");
                self.create_game();
            }
            1 => {
                info!("Found one existing idle game");
            }
            n => {
                return Err(anyhow::anyhow!("Multiple idle games found: {}", n));
            }
        };

        // Check if player is already in game
        if self.get_user_game(user).is_ok() {
            info!("Player {} is already in a game", user);
            already_in_queue = true;
        } else {
            // Add player to the idle game
            if let Some(game) = self.games.iter_mut().find(|g| g.status == SessionStatus::Idle) {
                game.add_player(user);
                info!("Added player to game. Queue now has {} players", game.pool.len());
            }
        }

        if already_in_queue {
            cc.create_bot_reply("You are already in the queue!").await;
        } else {
            // Update dashboard to reflect new state
            match self.dash_update(cc.ctx).await {
                Ok(_) => {},
                Err(e) => {
                    error!("Failed to update dashboard: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Handles the leave queue button
    async fn dash_leave_queue(&mut self,cc: &ComponentContext<'_>) -> Result<()> {
        let user    = cc.component.user.id;

        let mut found = false;

        // Find and remove player from any game
        for game in &mut self.games {
            if game.status == SessionStatus::Idle {
                let initial_len = game.pool.len();
                game.pool.retain(|p| p.player.discord_id != user);
                if game.pool.len() < initial_len {
                    found = true;
                    info!("Removed player from game. Queue now has {} players", game.pool.len());
                    break;
                }
            }
        }

        if found {
            // Update dashboard
            self.dash_update(cc.ctx).await;
        } else {
            cc.create_bot_reply("You are not in the queue!").await?;
        }

        Ok(())
    }

    /// Handles the shuffle teams button
    async fn dash_shuffle(&mut self, cc: &ComponentContext<'_>, _game_id: Option<String>) -> Result<()> {

        let mut shuffled = false;

        // Find the game to shuffle
        if let Some(game) = self.games.iter_mut().find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8)
        {
            // Shuffle the players using rand crate
            use rand::seq::SliceRandom;
            game.pool.shuffle(&mut rand::rng());
            shuffled = true;
            info!("Teams shuffled for game with {} players",game.pool.len());
        }

        if shuffled {
            cc.create_bot_reply("🔀 Teams shuffled! Check the dashboard for new team assignments.").await?;

            // Update dashboard to show shuffled teams
            self.dash_update(cc.ctx).await;
        } else {
            cc.create_bot_reply("❌ No game ready for shuffling. Need at least 8 players in queue.").await?;
        }

        Ok(())
    }

    /// Handles the start match button
    async fn dash_start(&mut self, cc: &ComponentContext<'_>, _game_id: Option<String>) -> Result<()> {
        let mut match_started = false;

        // Find the game to start
        if let Some(game) = self.games.iter_mut()
            .find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8)
        {
            // Change game status to Hot (ready to start)
            game.status   = SessionStatus::Hot;
            match_started = true;
            info!("Match started for game with {} players",game.pool.len());
        }

        if match_started {
            cc.create_bot_reply("🔥 Match started! Teams are now ready to play.").await?;

            // Update dashboard to show match status
            self.dash_update(cc.ctx).await;
        } else {
            cc.create_bot_reply("❌ No game ready to start. Need at least 8 players and shuffled teams.").await?;
        }

        Ok(())
    }

    /// Handles the end match button
    async fn dash_end(&mut self, cc: &ComponentContext<'_>, _game_id: Option<String>) -> Result<()> {
        let mut match_ended = false;

        // Find active games to end
        for game in &mut self.games {
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
            cc.create_bot_reply("✅ Match ended! Sesh has been reset and is ready for new players.").await?;

            // Update dashboard to show reset state
            self.dash_update(cc.ctx).await;
        } else {
            cc.create_bot_reply("❌ No active match to end.").await?;
        }

        Ok(())
    }

    /// Handles button interaction events from the dashboard
    ///
    /// Processes all button interactions in a modular way
    ///
    /// * `cc` - The component context with button information
    pub async fn dash_handle_button_interaction(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
        let custom_id = &cc.component.data.custom_id;

        // Log the button click
        info!("Button clicked: {}", custom_id);

        // Split the custom_id to extract action and optional game ID
        // Format: "action:game_id" or just "action"
        let parts: Vec<&str> = custom_id.split(':').collect();
        let action = parts[0];
        let game_id = parts.get(1).map(|s| s.to_string());

        match action {
            "join"    => self.dash_join_queue(cc).await,
            "leave"   => self.dash_leave_queue(cc).await,
            "shuffle" => self.dash_shuffle(cc, game_id).await,
            "start"   => self.dash_start(cc, game_id).await,
            "end"     => self.dash_end(cc, game_id).await,
            _ => {
                cc.create_bot_reply(&format!("Unknown button action: {}", action))
                    .await?;
                Ok(())
            }
        }
    }
}