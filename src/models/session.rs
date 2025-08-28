// CHECK ME

use std::str::FromStr;
use anyhow::{anyhow, Error};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::{ButtonStyle, Cache, ChannelId, Context, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter, CreateMessage, GuildId};
use sqlx::FromRow;
use tracing::{debug, error, info};

use crate::models::{Player, Rank};

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
)]
pub enum DivName {
    Newcomer,
    Journey,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot, // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

impl SessionStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, SessionStatus::Push | SessionStatus::Live | SessionStatus::Pull)
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
)]
pub enum Team {
    Red,
    Blu,
}

impl FromStr
    for Team {
    type Err =
        Error;

    fn from_str(
        s: &str,
    ) -> Result<
        Self,
        Self::Err,
    > {
        match s {
            "RED" => Ok(Team::Red),
            "BLU" => Ok(Team::Blu),
            _ => Err(Error::msg(format!("Unknown : {}", s))),
        }
    }
}

#[derive(
    Default,
)]
pub struct Manager {
    pub servers:
        Vec<Server>,
}

impl Manager {
    pub fn new(
    ) -> Self {
        Self { servers: Vec::new() }
    }

    pub fn pull_list(
        &mut self,
        cache: &Cache,
    ) -> Self {
        let mut servers = Vec::new();
        cache.guilds().iter().for_each(|g| {
            servers.push(Server::new(*g, None));
        });
        Self {
            servers,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
)]
pub struct Server {
    pub guild_id:
        GuildId,
    pub groups:
        Vec<Group>,
}

impl Server {
    pub fn new(
        guild_id: GuildId,
        group: Option<Group>,
    ) -> Self {
        info!("New server created for {}", guild_id);
        Self {
            guild_id,
            groups: vec![group.unwrap()],
        }
    }

    pub fn groups(
        &self,
    ) -> &Vec<Group> {
        &self.groups
    }

    pub fn groups_mut(&mut self) -> &mut Vec<Group>{
        &mut self
            .groups
    }

    pub fn find_group_by_queue_channel(
        &self,
        channel_id: u64,
    ) -> Option<
        &Group,
    > {
        self.groups.iter().find(|g| g.queue_id == channel_id)
    }

    pub fn find_group_by_queue_tc_mut(
        &mut self,
        channel_id: u64,
    ) -> Option<
        &mut Group,
    > {
        self.groups.iter_mut().find(|g| g.queue_id == channel_id)
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
)]
pub struct Group {
    pub guild_id:          u64,
    pub dashboard_id:      u64,
    pub queue_chat_id:     u64,
    pub queue_id:          u64,
    pub teams:             Vec<TeamChannels>,
    pub sessions:          Vec<Session>,
    pub session_increment: u16,
    pub quota:             u8,
}

impl Group {
    pub fn new(
        guild_id: u64,
        dashboard_tc_id: u64,
        queue_chat_id: u64,
        queue_vc_id: u64,
        red_vc_id: u64,
        blu_vc_id: u64,
        session_quota: u8,
    ) -> Self {
        info!("New group created for {}", dashboard_tc_id);
        Self {
            guild_id,
            dashboard_id: dashboard_tc_id,
            queue_chat_id,
            queue_id: queue_vc_id,
            teams: vec![TeamChannels { red_vc_id, blu_vc_id }],
            sessions: Vec::new(),
            session_increment: 0,
            quota: session_quota,
        }
    }

    pub fn add_team_channels(
        &mut self,
        red: u64,
        blu: u64,
    ) {
        self.teams.push(TeamChannels { red_vc_id: red, blu_vc_id: blu });
    }

    pub fn get_sessions_by_status(&mut self, status: &SessionStatus) -> Vec<&mut Session> {
        self.sessions.iter_mut().filter(|s| s.status == *status).collect()
    }

    pub fn is_player_in_session(&self, player: &Player) -> bool {
        self.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.discord_id == player.discord_id))
    }

    pub fn get_player_session_status(&self, player: &Player) -> Result<SessionStatus, Error> {
        self.sessions
            .iter()
            .find(|s| s.pool.iter().any(|p| p.player.discord_id == player.discord_id))
            .map(|s| Ok(s.status.clone()))
            .unwrap_or(Err(anyhow!("Player not found in any session")))
    }

    pub fn create_session(&mut self) {
        self.session_increment += 1;
        info!("Creating new session with ID: {}", self.session_increment);
        self.sessions.push(Session::new(self.session_increment, self.guild_id, self.queue_id));
    }

    pub fn end_session(
        &mut self,
        session_id: u16,
    ) -> bool {
        info!("Attempting to end session with ID: {}", session_id);
        if let Some(pos) = self.sessions.iter().position(|s| s.session_id == session_id) {
            self.sessions.remove(pos);
            info!("Session {} successfully ended and removed", session_id);
            true
        } else {
            info!("Failed to end session {}: Session not found", session_id);
            false
        }
    }

    pub async fn init_dashboard(
        &self,
        ctx: &Context,
        dashboard_id: u64,
    ) -> Result<bool, anyhow::Error> {
        info!("Initializing dashboard for channel ID: {}", dashboard_id);
        if self.dashboard_id != dashboard_id {
            return Ok(false);
        }
        
        let channel = ChannelId::new(dashboard_id);
        let embed = CreateEmbed::new()
            .title("PUG Dashboard")
            .description("No active sessions. Join the queue to get started!")
            .footer(CreateEmbedFooter::new("Use /join to join the queue"));
        
        // Create buttons in a modular way for easy addition/removal
        let buttons = self.create_dashboard_buttons();
        let action_row = CreateActionRow::Buttons(buttons);
            
        match channel.send_message(
            &ctx.http, 
            CreateMessage::new()
                .embed(embed)
                .components(vec![action_row])
        ).await {
            Ok(_) => {
                info!("Dashboard initialized successfully");
                Ok(true)
            },
            Err(e) => {
                error!("Failed to initialize dashboard: {:?}", e);
                Err(anyhow::anyhow!("Failed to initialize dashboard: {:?}", e))
            }
        }
    }
    
    /// Creates buttons for the dashboard in a modular way
    /// Makes it easy to add or remove buttons
    fn create_dashboard_buttons(&self) -> Vec<CreateButton> {
        // Get the latest session ID if available
        let session_id = self.sessions.last().map(|s| s.session_id.to_string());
        
        // Check if there's an active session to enable/disable buttons
        let has_active_session = !self.sessions.is_empty();
        let has_ready_session = self.sessions.iter().any(|s| s.pool.len() >= 8);
        
        // Define button configurations - this makes it easy to add/remove buttons
        let button_configs = vec![
            // (custom_id, label, style, disabled, emoji_option)
            ("join_leave", "Join Queue",    ButtonStyle::Success,   false,               None),
            ("shuffle",    "Shuffle Teams", ButtonStyle::Primary,   !has_ready_session,  Some('🎲')),
            ("start",      "Start Match",   ButtonStyle::Secondary, !has_active_session, Some('▶')),
            ("end",        "End Match",     ButtonStyle::Danger,    !has_active_session, Some('⏹'))
        ];
        
        // Generate buttons from configurations
        button_configs.into_iter().map(|(action, label, style, disabled, emoji)| {
            // Create button with the appropriate custom_id
            let custom_id = if let Some(id) = &session_id {
                // For buttons that need a session ID
                if action != "join_leave" {
                    format!("{action}:{id}")
                } else {
                    action.to_string()
                }
            } else {
                // No session ID available
                action.to_string()
            };
            
            // Create the button with all specified properties
            let mut button = CreateButton::new(custom_id)
                .label(label)
                .style(style)
                .disabled(disabled);
                
            // Add emoji if specified
            if let Some(emoji_char) = emoji {
                button = button.emoji(emoji_char);
            }
            
            button
        }).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Session {
    pub guild_id:      u64,
    pub queue_chat_id: u64,
    pub session_id:    u16,
    pub status:        SessionStatus,
    pub pool:          Vec<SessionPlayer>,
}

impl Session {
    pub fn new(
        session_id:    u16,
        guild_id:      u64,
        queue_chat_id: u64,
    ) -> Self {
        let session = Self {
            guild_id,
            queue_chat_id,
            session_id,
            pool:   Vec::new(),
            status: SessionStatus::Idle,
        };
        info!("New session started with ID: {}", session_id);
        session
    }

    pub fn hot(&mut self) {
        // send notif
        info!("Changing session {} status to HOT", self.session_id);
        self.status = SessionStatus::Hot;
    }

    pub fn push(&mut self) {
        info!("Changing session {} status to PUSH", self.session_id);
        self.status = SessionStatus::Push;
    }

    pub fn live(&mut self) {
        info!("Changing session {} status to LIVE", self.session_id);
        self.status = SessionStatus::Live;
    }

    pub fn pull(&mut self) {
        info!("Changing session {} status to PULL", self.session_id);
        self.status = SessionStatus::Pull;
    }

    pub fn generate_teams(&mut self) {
        info!("Generating teams for session {}", self.session_id);
        debug!("Cloned {} players for team assignment", self.pool.len());
        let mut rng = rand::rng();
        let mut players = self.pool.clone();

        // 1. First add buffered players to genpool (priority)
        let buffered_players = self.pool.iter().filter(|p| p.buffered.is_some()).cloned().collect::<Vec<_>>();

        // 2. Fill remaining slots with non-buffered players
        let remaining_slots = 8 - buffered_players.len(); // Assuming 8 players per match
        let mut non_buffered = self.pool.iter().filter(|p| p.buffered.is_none()).cloned().collect::<Vec<_>>();

        // Take only what we need
        if non_buffered.len() > remaining_slots {
            non_buffered.truncate(remaining_slots);
        }

        // 3. Sort players by ELO in descending order
        players.sort_by(|a, b| {
            let a_elo = a.player.rank.unwrap_or(Rank::Beginner).elo();
            let b_elo = b.player.rank.unwrap_or(Rank::Beginner).elo();

            // Randomize order for players with identical ELO
            if a_elo == b_elo {
                if rng.random::<bool>() {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                b_elo.cmp(&a_elo)
            }
        });

        // 4. Distribute players in snake draft pattern (ABBAABBA)
        let mut team_a = Vec::new();
        let mut team_b = Vec::new();

        for (i, player) in players.iter().enumerate() {
            let mut player_clone = player.clone();

            // Snake draft pattern: 0->A, 1->B, 2->B, 3->A, 4->A, 5->B, 6->B, 7->A
            match i % 4 {
                0 | 3 => {
                    player_clone.team(Team::Red);
                    team_a.push(player_clone);
                }
                1 | 2 => {
                    player_clone.team(Team::Blu);
                    team_b.push(player_clone);
                }
                _ => unreachable!(),
            }
        }

        // 5. Update the original pool with team assignments
        for player in &team_a {
            if let Some(p) = self.pool.iter_mut().find(|p| p.player.discord_id == player.player.discord_id) {
                p.team = Some(Team::Red);
            }
        }

        for player in &team_b {
            if let Some(p) = self.pool.iter_mut().find(|p| p.player.discord_id == player.player.discord_id) {
                p.team = Some(Team::Blu);
            }
        }
    }

    pub fn count(&self) -> usize {
        self.pool.len()
    }

    pub fn get_members(&self) -> Vec<Player> {
        self.pool.iter().map(|m| m.player.clone()).collect()
    }

    pub fn add_player(&mut self, player: &Player) {
        // Clone players for team assignment
        let session_player = SessionPlayer::construct(player.clone(), self.guild_id, self.session_id);
        info!("Player {} added to session {}. Total players: {}", player.discord_id, self.session_id, self.pool.len());
        self.pool.push(session_player);
    }

    pub fn remove_player(&mut self, player: &SessionPlayer) {
        let before_count = self.pool.len();
        self.pool.retain(|p| p.player.discord_id != player.player.discord_id);
        let after_count = self.pool.len();

        if before_count == after_count {
            info!("Player {} not found in session {}", player.player.discord_id, self.session_id);
        } else {
            info!("Player {} removed from session {}. Remaining players: {}", player.player.discord_id, self.session_id, after_count);
        }
    }

    pub fn buff(&mut self, user_id: u64, buffered: Option<Player>) {
        self.pool.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().buff(buffered);
    }

    pub fn unbuff(&mut self, user_id: u64) {
        self.pool.iter_mut().find(|m| m.player.discord_id == user_id).unwrap().unbuff();
    }

    pub fn update_queue_status(&mut self,
        user:      Player,
        queue_vc:  Option<bool>,
        queue_cmd: Option<bool>,
    ) {
        // if session is closed, ignore
        if !matches!(self.status, SessionStatus::Idle | SessionStatus::Pull) {
            return;
        }

        let uid = user.discord_id;
        // find existing entry
        if let Some(sp) = self.pool.iter_mut().find(|sp| sp.player.discord_id == uid) {
            if let Some(v) = queue_vc {
                sp.queue_vc = v;
            }
            if let Some(c) = queue_cmd {
                sp.queue_cmd = c;
            }

            // if neither flag is true any more, remove them
            if !sp.in_queue() {
                self.pool.retain(|p| p.player.discord_id != uid);
            }
        } else if queue_vc.unwrap_or(false) || queue_cmd.unwrap_or(false) {
            // first‐time join
            let mut sp = SessionPlayer::construct(user, self.guild_id, self.session_id);
            sp.queue_vc = queue_vc.unwrap_or(false);
            sp.queue_cmd = queue_cmd.unwrap_or(false);
            self.pool.push(sp);
        }
    }

    /// Helper so you don’t have to remember SessionPlayer::new everywhere
    pub fn on_voice_state_change(&mut self, user: Player, joined: bool) {
        self.update_queue_status(user, Some(joined), None);
    }

    pub fn on_command_join(&mut self, user: Player, joined: bool) {
        self.update_queue_status(user, None, Some(joined));
    }

    /// Notify session ready when the queue quota is reached.
    ///
    /// * `ctx`        - Ref to the Serenity context.
    /// * `db`         - Ref to the database.
    /// * `group`      - The group containing the session.
    pub async fn notify_session_ready(
        &self,
        ctx: &Context,
    ) -> Result<(), Error> {
        // Send notification to log channel
        if self.queue_chat_id != 0 {
            let channel = ChannelId::new(self.queue_chat_id);
            let mut player_mentions = Vec::new();
            // Take at most 8 players
            let pool_len = self.pool.len().min(8);
            for member in &self.pool[..pool_len] {
                player_mentions.push(format!("<@{}>", member.player.discord_id));
            }
            
            let embed = CreateEmbed::new()
                .title("QUOTA REACHED!")
                .description(
                    format!(
                        "**8 players ready for pickup!**\n\n{}\n\nPlayers have 2 minutes to confirm.", // TODO: Use Discord datetime feature
                        player_mentions.join(" ")
                    )
                )
                .footer(CreateEmbedFooter::new("Awaiting team generation..."));
            channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await?;
        } else {
            error!("Queue chat ID is not set in the config")
        }
        Ok(())
    }

}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize,)]
pub struct SessionPlayer {
    pub guild_id:   u64,
    pub session_id: u16,
    pub player:     Player,
    pub team:       Option<Team>,
    pub buffered:   Option<Player>,
    pub queue_vc:   bool,
    pub queue_cmd:  bool,
}

impl SessionPlayer {
    pub fn construct(
        player:     Player,
        guild_id:   u64,
        session_id: u16,
    ) -> Self {
        Self {
            guild_id,
            session_id,
            player,
            team:      None,
            buffered:  None,
            queue_vc:  false,
            queue_cmd: false,
        }
    }

    pub fn buff(&mut self, buffered: Option<Player>,) {
        self.buffered = buffered;
    }

    pub fn unbuff(&mut self,) {
        self.buffered = None;
    }

    pub fn team(&mut self, team: Team,) {
        self.team = Some(team);
    }

    pub fn in_queue(&self) -> bool {
        self.queue_vc || self.queue_cmd
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannels {
    pub red_vc_id: u64,
    pub blu_vc_id: u64,
}
