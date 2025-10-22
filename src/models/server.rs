//! # Server Module
//!
//! This module defines the Server struct and its related functionality.
//! A Server represents a Discord guild with associated groups and sessions.

use serde::{Deserialize, Serialize};
use serenity::all::{parse_user_mention, ButtonStyle, Context, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter as CEF, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateMessage as CM, Message};
use serenity::all::{GuildId as GI, RoleId as RI, ChannelId as CI, MessageId as MI, UserId as UI};
use tracing::info;
use anyhow::{anyhow, Error, Result};

use crate::handlers::player::check_role;
use crate::models::data::*;
use crate::models::session::*;
use crate::models::{CommandContext, ComponentContext};

macro_rules! list_players {
    ($desc:ident, $team:ident) => {
        for (i, player) in $team.iter().enumerate() {
            $desc.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
        }
    };
}


/// Represents a game server with IP and name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameServer {
    /// IP address of the game server
    pub ip: String,
    /// Name of the game server
    pub name: String,
}

// Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub guild_id: GI,
    pub roles:    Roles,
    pub groups:   Vec<Group>,
}

impl Server {
    pub fn new(
        guild_id: GI,
        roles: Roles,
    ) -> Self {
        Self {
            guild_id,
            roles,
            groups: Vec::new(),
        }
    }

    pub fn add_group(
        &mut self,
        group: Group,
    ) {
        self.groups.push(group);
    }

    pub fn empty(guild_id: GI) -> Self {
        Self {
            guild_id,
            roles: Roles::empty(),
            groups: Vec::new(),
        }
    }

    pub fn get_group(
        &mut self,
        channel_id: CI,
    ) -> Result<&mut Group> {
        match self.groups.iter_mut().find(|group| group.contains_channel(channel_id)) {
            Some(group) => Ok(group),
            None => Err(anyhow!("Group not found")),
        }
    }
}

// Group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub group_id:      u8,
    pub timeout:       u16,
    pub quota:         u8,
    pub dashboard_msg: MI,
    pub channels:      Channels,
    pub sessions:      Vec<Session>,
}

impl Group {
    pub fn new(
        group_id:      u8,
        quota:         u8,
        timeout:       u16,
        dashboard_msg: MI,
        channels:      Channels,
        sessions:      Vec<Session>,
    ) -> Self {
        Self {
            group_id,
            quota,
            timeout,
            dashboard_msg,
            channels,
            sessions,
        }
    }

    pub fn create_session(&mut self) {
        info!("Creating new session");
        self.sessions
            .push(Session::new(SessionStatus::Idle, Vec::new()));
    }

    pub fn end_session(&mut self) -> bool {
        info!("Attempting to end session");
        if let Some(pos) = self
            .sessions
            .iter()
            .position(|s| s.status == SessionStatus::Idle)
        {
            self.sessions.remove(pos);
            info!("Session successfully ended and removed");
            true
        } else {
            info!("Failed to end session: Session not found");
            false
        }
    }

    pub fn get_sessions_by_status(
        &mut self,
        status: &SessionStatus,
    ) -> Vec<&mut Session> {
        self.sessions
            .iter_mut()
            .filter(|s| s.status == *status)
            .collect()
    }

    pub fn get_user_session(
        &mut self,
        discord_id: UI,
    ) -> Option<Session> {
        self.sessions
            .iter()
            .find(|s| s.pool.iter().any(|p| p.player.discord_id == discord_id))
            .cloned()
    }

    /// Checks if this group contains the given channel_id in any of its channels
    pub fn contains_channel(
        &self,
        channel_id: CI,
    ) -> bool {
        self.channels.contains_channel(channel_id)
    }

    pub async fn has_dashboard(
        &self,
        ctx: &Context,
    ) -> bool {
        let channel = CI::new(self.channels.dashboard.into());
        let message = channel.message(&ctx.http, self.dashboard_msg).await;
        message.is_ok()
    }

    pub async fn dash_init(
        &self,
        ctx: &Context,
    ) -> Result<(), Error> {
        let embed = Group::dash_update(&self).await?;
        match self.dash_send(&ctx, embed).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Creates buttons for the dashboard
    pub fn create_dashboard_buttons(&self) -> Vec<CreateActionRow> {
        // Check if there's an live session to enable/disable buttons
        let has_live_session = !self.sessions.is_empty();
        let has_ready_session = self.sessions.iter().any(|s| s.pool.len() >= 8);

        // Define button configurations - this makes it easy to add/remove buttons
        let button_configs = vec![
            // (custom_id, label, style, disabled, emoji_option)
            ("join", "Join Queue", ButtonStyle::Secondary, false),
            ("leave", "Leave Queue", ButtonStyle::Secondary, false),
            (
                "shuffle",
                "Shuffle",
                ButtonStyle::Secondary,
                !has_ready_session,
            ),
            (
                "start",
                "Start Match",
                ButtonStyle::Secondary,
                !has_live_session,
            ),
            (
                "end",
                "End Match",
                ButtonStyle::Secondary,
                !has_live_session,
            ),
        ];

        // Generate buttons from configurations
        let buttons: Vec<CreateButton> = button_configs
            .into_iter()
            .map(|(action, label, style, disabled)| {
                // Create the button with all specified properties
                CreateButton::new(action)
                    .label(label)
                    .style(style)
                    .disabled(disabled)
            })
            .collect();

        vec![CreateActionRow::Buttons(buttons)]
    }

    /// `/buffer`
    ///
    /// * `user_mention` - The user mention to buffer.
    pub async fn cmd_buffer(cc: &CommandContext<'_>,user_mention: &str,) -> Result<()> {
        info!("Processing buffer command for user mention: {}", user_mention);
        let _user_id = parse_user_mention(user_mention);
        if !check_role(cc, &Role::Admin).await? {
            let response = CIR::Message(CIRM::new().content("Only admins can buffer players!").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }

        
    
        // TODO: Actually buffer the player
        Ok(())
    }


    pub async fn dash_send(
        &self,
        ctx: &Context,
        embed: CreateEmbed,
    ) -> Result<Message> {
        Ok(self
            .channels.dashboard
            .send_message(&ctx.http, CM::new().embed(embed))
            .await?)
    }

    /// Initializes a dashboard based on current group state
    pub async fn dash_update(&self) -> Result<CreateEmbed> {
        let mut embed = CreateEmbed::new().title("PUG Dashboard");

        let sessions_idle: Vec<&Session> = self.sessions.iter().filter(|s| s.status == SessionStatus::Idle).collect();
        let sessions_hot:  Vec<&Session> = self.sessions.iter().filter(|s| s.status == SessionStatus::Hot) .collect();
        let sessions_live: Vec<&Session> = self.sessions.iter().filter(|s| s.status == SessionStatus::Live).collect();

        let mut desc = String::new();

        if let Some(session_current) = sessions_idle.first() {
            let queue_players = session_current.pool.len();
            let quota = self.quota as usize;

            desc.push_str(&format!(
                "**📋 Current Queue ({}/{})**\n",
                queue_players, quota
            ));

            if queue_players < quota {
                desc.push_str("**Players:**\n");

                for (i, player) in session_current.pool.iter().enumerate() {
                    desc.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
                }
                desc.push_str(&format!(
                    "\n*Need {} more players to start*\n\n",
                    quota - queue_players
                ));
            } else if queue_players == quota {
                desc.push_str("**🔥 READY TO START! 🔥**\n");

                let team_red = &session_current.pool[0..4];
                desc.push_str("**🔴 Red:**\n");
                list_players!(desc, team_red);

                let team_blu = &session_current.pool[4..8];
                desc.push_str("\n**🔵 Blue:**\n");
                list_players!(desc, team_blu);

                desc.push_str("\n");
            } else {
                desc.push_str("**🔥 MATCH READY! 🔥**\n");

                if queue_players >= 8 {
                    let team_red = &session_current.pool[0..4];
                    desc.push_str("**🔴 Red:**\n");
                    list_players!(desc, team_red);

                    let team_blu = &session_current.pool[4..8];
                    desc.push_str("\n**🔵 Blue:**\n");
                    list_players!(desc, team_blu);
                }

                let extra_players = &session_current.pool[quota..];
                if !extra_players.is_empty() {
                    desc.push_str(&format!(
                        "\n**⏳ Queued for Next ({}):**\n",
                        extra_players.len()
                    ));
                    list_players!(desc, extra_players);
                }
                desc.push('\n');
            }
        } else {
            desc.push_str(
                "**📋 Queue Status**\n*No active sessions. Join the queue to get started!*\n\n",
            );
        }

        // Show hot sessions (waiting to start)
        if !sessions_hot.is_empty() {
            desc.push_str("**🔥 Ready sessions:**\n");
            for _session in sessions_hot {
                desc.push_str("• Ready to start!\n");
            }
            desc.push('\n');
        }

        // Show live matches
        if !sessions_live.is_empty() {
            desc.push_str("**⚡ Live Matches:**\n");
            for _session in sessions_live {
                desc.push_str("• Live\n");
            }
            desc.push('\n');
        }

        embed = embed.description(desc);
        embed = embed.footer(CEF::new(
            "Use the buttons below to manage the queue and matches",
        ));

        Ok(embed)
    }

    /// Handles the join queue button
    async fn dash_join_queue(
        &mut self,
        cc: &ComponentContext<'_>) -> Result<()> {
        let user = cc.component.user.id;

        // Get player info or create a new one
        let player = match cc.db.get_user(user).await {
            Ok(player) => {
                info!("Found user in db!");
                player
            }
            Err(_) => {
                info!("Creating new user in db!");
                cc.db.new_user(user).await?
            }
        };

        let mut queue_count = 0;
        let mut already_in_queue = false;

        // Check if we have idle sessions
        match self.get_sessions_by_status(&SessionStatus::Idle).len() {
            0 => {
                info!("No idle sessions found, creating a new session");
                self.create_session();
            }
            1 => {
                info!("Found one existing idle session");
            }
            n => {
                return Err(anyhow::anyhow!("Multiple idle sessions found: {}", n));
            }
        };

        // Check if player is already in session
        match self.get_user_session(user) {
            Some(_session) => {
                info!("Player is already in session");
                already_in_queue = true;
            }
            None => {
                info!("Player is not in session");
            }
        };

        // Check if we have idle sessions
        match self.get_sessions_by_status(&SessionStatus::Idle).len() {
            0 => {
                info!("No idle sessions found, creating a new session");
                self.create_session();
            }
            1 => {
                info!("Found one existing idle session");
            }
            n => {
                return Err(anyhow::anyhow!(
                    "Found more than one idle session ({}). This is unexpected.",
                    n
                ));
            }
        }

        // Check if player is already in session
        if self.get_user_session(user).is_some() {
            info!("Player {} is already in a session", player.discord_id);
            already_in_queue = true;
        } else {
            // Add player to the session
            if let Some(session) = self.sessions.last_mut() {
                if session.status == SessionStatus::Idle {
                    session.pool.push(SessionPlayer::construct(player));
                    queue_count = session.pool.len();
                    info!(
                        "Added player to session. Queue now has {} players",
                        queue_count
                    );
                }
            }
        }

        if already_in_queue {
            cc.create_bot_reply("You are already in the queue!").await?;
        } else {
            cc.create_bot_reply(&format!(
                "✅ Joined the queue! ({}/12 players)",
                queue_count
            ))
            .await?;

            // Update dashboard to reflect new state
            match self.dash_update().await {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
        }

        Ok(())
    }

    /// Handles the leave queue button
    async fn dash_leave_queue(
        &mut self,
        cc: &ComponentContext<'_>) -> Result<()> {
        let user    = cc.component.user.id;

        let mut found = false;
        let mut queue_count = 0;

        // Find and remove player from any session
        for session in &mut self.sessions {
            if session.status == SessionStatus::Idle {
                let initial_len = session.pool.len();
                session.pool.retain(|p| p.player.discord_id != user);
                if session.pool.len() < initial_len {
                    found = true;
                    queue_count = session.pool.len();
                    info!(
                        "Removed player from session. Queue now has {} players",
                        queue_count
                    );
                    break;
                }
            }
        }

        if found {
            cc.create_bot_reply(&format!("❌ Left the queue! ({}/12 players)", queue_count))
                .await?;

            // Update dashboard to reflect new state
            self.dash_update().await?;
        } else {
            cc.create_bot_reply("You are not in the queue!").await?;
        }

        Ok(())
    }

    /// Handles the shuffle teams button
    async fn dash_shuffle(
        &mut self,
        cc: &ComponentContext<'_>,
        _session_id: Option<String>,
    ) -> Result<()> {

        let mut shuffled = false;

        // Find the session to shuffle
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8)
        {
            // Shuffle the players using rand crate
            use rand::seq::SliceRandom;
            session.pool.shuffle(&mut rand::rng());
            shuffled = true;
            info!(
                "Teams shuffled for session with {} players",
                session.pool.len()
            );
        }

        if shuffled {
            cc.create_bot_reply("🔀 Teams shuffled! Check the dashboard for new team assignments.")
                .await?;

            // Update dashboard to show shuffled teams
            self.dash_update().await?;
        } else {
            cc.create_bot_reply(
                "❌ No session ready for shuffling. Need at least 8 players in queue.",
            )
            .await?;
        }

        Ok(())
    }

    /// Handles the start match button
    async fn dash_start(
        &mut self,
        cc: &ComponentContext<'_>,
        _session_id: Option<String>,
    ) -> Result<()> {
        let mut match_started = false;

        // Find the session to start
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8)
        {
            // Change session status to Hot (ready to start)
            session.status = SessionStatus::Hot;
            match_started = true;
            info!(
                "Match started for session with {} players",
                session.pool.len()
            );
        }

        if match_started {
            cc.create_bot_reply("🔥 Match started! Teams are now ready to play.")
                .await?;

            // Update dashboard to show match status
            self.dash_update().await?;
        } else {
            cc.create_bot_reply(
                "❌ No session ready to start. Need at least 8 players and shuffled teams.",
            )
            .await?;
        }

        Ok(())
    }

    /// Handles the end match button
    async fn dash_end(
        &mut self,
        cc: &ComponentContext<'_>,
        _session_id: Option<String>,
    ) -> Result<()> {
        let mut match_ended = false;

        // Find active sessions to end
        for session in &mut self.sessions {
            if session.status == SessionStatus::Hot || session.status == SessionStatus::Live {
                // Clear the session and reset to idle
                session.pool.clear();
                session.status = SessionStatus::Idle;
                match_ended = true;
                info!("Match ended and session reset");
                break;
            }
        }

        if match_ended {
            cc.create_bot_reply(
                "✅ Match ended! Sesh has been reset and is ready for new players.",
            )
            .await?;

            // Update dashboard to show reset state
            self.dash_update().await?;
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
    pub async fn dash_handle_button_interaction(
        &mut self,
        cc: &ComponentContext<'_>,

    ) -> Result<()> {
        let custom_id = &cc.component.data.custom_id;

        // Log the button click
        info!("Button clicked: {}", custom_id);

        // Split the custom_id to extract action and optional session ID
        // Format: "action:session_id" or just "action"
        let parts: Vec<&str> = custom_id.split(':').collect();
        let action = parts[0];
        let session_id = parts.get(1).map(|s| s.to_string());

        match action {
            "join"    => self.dash_join_queue(cc).await,
            "leave"   => self.dash_leave_queue(cc).await,
            "shuffle" => self.dash_shuffle(cc, session_id).await,
            "start"   => self.dash_start(cc, session_id).await,
            "end"     => self.dash_end(cc, session_id).await,
            _ => {
                cc.create_bot_reply(&format!("Unknown button action: {}", action))
                    .await?;
                Ok(())
            }
        }
    }
}

// Roles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roles {
    pub runner: RI,
    pub admin: RI,
}

impl Roles {
    pub fn new(
        runner: RI,
        admin: RI,
    ) -> Self {
        Self { runner, admin }
    }
    pub fn empty() -> Self {
        Self {
            runner: RI::new(1),
            admin: RI::new(1),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    Runner,
    Admin,
}

impl Role {
    pub fn id(&self) -> RI {
        match self {
            Role::Runner => RUNNER_R_ID.into(),
            Role::Admin  => ADMIN_R_ID .into(),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Role::Runner => "Runner",
            Role::Admin  => "Admin",
        }
    }
}

// Divisons
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Divisons {
    Newcomer,
    Journey,
}

// Channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    pub queue:     CI,
    pub queue_vc:  CI,
    pub teams:     Vec<TeamChannel>,
    pub dashboard: CI,
}

impl Channels {
    pub fn new(
        queue:     CI,
        queue_vc:  CI,
        teams:     Vec<TeamChannel>,
        dashboard: CI,
    ) -> Self {
        Self {
            queue,
            queue_vc,
            teams,
            dashboard,
        }
    }

    /// Pushs a red and blue channel to the vector
    pub fn add_team_channel_pair(
        &mut self,
        red_vc: CI,
        blu_vc: CI,
    ) {
        self.teams.push(TeamChannel::new(red_vc, blu_vc));
    }

    pub fn empty() -> Self {
        Self {
            queue:     CI::new(1),
            queue_vc:  CI::new(1),
            teams:     Vec::new(),
            dashboard: CI::new(1),
        }
    }

    /// Checks if this Channels struct contains the given channel_id
    /// in any of its channel fields (queue, queue_vc, dashboard, or team channels)
    pub fn contains_channel(
        &self,
        channel_id: CI,
    ) -> bool {
        self.queue == channel_id
            || self.queue_vc  == channel_id
            || self.dashboard == channel_id
            || self.teams.iter().any(|team| team.contains_channel(channel_id))
    }
}

