use std::str::FromStr;

use anyhow::Error;
// CHECK ME
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serenity::all::parse_user_mention;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::{
    ButtonStyle, Cache, ChannelId, ChannelId as CI, Context, CreateButton, CreateEmbed,
    CreateEmbedFooter as CEF, CreateMessage as CM, GuildId as GI, Message, MessageId, RoleId as RI,
    UserId,
};
use sqlx::FromRow;
use tracing::{error, info};

use crate::handlers::role::check_role;
use crate::models::player::Role;
use crate::{models::player::Player, models::command::*};
use serenity::all::CreateInteractionResponse as CIR;
use serenity::all::CreateInteractionResponseMessage as CIRM;

// Example usage
// define_global_ids! {
//     ID_RUNNER => 1386951114225746040,
//     ID_ADMIN  => 1386951155052974141,
// }
macro_rules! define_global_ids {
    (
        $(
            $(#[$meta: meta])*
            $const: ident => $value: expr
        ),*
        $(,)?
    ) => {
        $(
            $(#[$meta])*
            pub const $const: u64 = $value;
        )*
    };
}

define_global_ids! {
  RUNNER_R_ID          => 1386951114225746040,
  ADMIN_R_ID           => 1386951155052974141,

  EU_BEGINNER_R_ID     => 1386989827307606107,
  EU_NEWCOMER_R_ID     => 1386951211109974066,
  EU_NOVICE_R_ID       => 1386951241539784827,
  EU_APPRENTICE_R_ID   => 1386951264117592097,
  EU_JOURNEYMAN_R_ID   => 1386951275056201820,
  EU_MASTER_R_ID       => 1386951316143734814,
  EU_MASTER_ELITE_R_ID => 1386951327711494204,
  EU_GRANDMASTER_R_ID  => 1386951360594837544,

  DASHBOARD_TC_ID    => 1385894822992281701,
  CHAT_TC_ID         => 1388643261543088208,
  QUEUE_TC_ID        => 1385893666010300436,
  RED_VC_ID          => 1385464431185494086,
  BLU_VC_ID          => 1385464563448680578,
  // ID_NA_BEGINNER     => 0,
  // ID_NA_NEWCOMER     => 0,
  // ID_NA_NOVICE       => 0,
  // ID_NA_APPRENTICE   => 0,
  // ID_NA_JOURNEYMAN   => 0,
  // ID_NA_MASTER       => 0,
  // ID_NA_MASTER_ELITE => 0,
  // ID_NA_GRANDMASTER  => 0,
}

macro_rules! list_players {
    ($desc:ident, $team:ident) => {
        for (i, player) in $team.iter().enumerate() {
            $desc.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
        }
    };
}

// Manager
#[derive(Default)]
pub struct Manager {
    pub guilds: Vec<Guild>,
}

impl Manager {
    pub fn new() -> Self {
        Self { guilds: Vec::new() }
    }

    pub fn pull_list(
        &mut self,
        cache: &Cache,
    ) -> Self {
        let mut guilds = Vec::new();
        cache.guilds().iter().for_each(|g| {
            guilds.push(Guild::new(*g, Roles::empty()));
        });
        Self { guilds }
    }
}

// DivName
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DivName {
    Newcomer,
    Journey,
}

// ConfigFormat
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConfigFormat {
    pub key: String,
    pub value: Option<String>,
    pub description: Option<String>,
}

// Guild
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub guild_id: GI,
    pub roles: Roles,
    pub groups: Vec<Group>,
}

impl Guild {
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
}

// Group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub group_id: u8,
    pub timeout: u16,
    pub quota: u8,
    pub dashboard: Dashboard,
    pub channels: Channels,
    pub sessions: Vec<Session>,
}

impl Group {
    pub fn new(
        group_id: u8,
        quota: u8,
        timeout: u16,
        dashboard: Dashboard,
        channels: Channels,
        sessions: Vec<Session>,
    ) -> Self {
        Self {
            group_id,
            quota,
            timeout,
            dashboard,
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
        discord_id: UserId,
    ) -> Option<Session> {
        self.sessions
            .iter()
            .find(|s| s.pool.iter().any(|p| p.player.discord_id == discord_id))
            .cloned()
    }

    pub fn get_dashboard(&self) -> &Dashboard {
        &self.dashboard
    }

    pub fn channel_exists(
        &self,
        channel_id: &ChannelId,
    ) -> bool {
        self.channels
            .teams
            .iter()
            .any(|team| team.red_vc == *channel_id || team.blu_vc == *channel_id)
            || self.channels.queue_vc == *channel_id
    }

    pub async fn has_dashboard(
        &self,
        ctx: &Context,
    ) -> bool {
        let channel = CI::new(self.dashboard.channel_id.into());
        let message = channel.message(&ctx.http, self.dashboard.msg).await;
        message.is_ok()
    }

    pub async fn dash_init(
        &self,
        ctx: &Context,
    ) -> Result<(), Error> {
        let embed = Dashboard::update(&self).await?;
        Dashboard::send(&self.dashboard, &ctx, embed).await;
        Ok(())
    }

    /// Creates buttons for the dashboard in a modular way
    /// Makes it easy to add or remove buttons
    pub fn create_dashboard_buttons(&self) -> Vec<CreateButton> {
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
        button_configs
            .into_iter()
            .map(|(action, label, style, disabled)| {
                // Create the button with all specified properties
                CreateButton::new(action)
                    .label(label)
                    .style(style)
                    .disabled(disabled)
            })
            .collect()
    }

    /// `/buffer`
    ///
    /// * `user_mention` - The user mention to buffer.
    pub async fn cmd_buffer(cc: &CommandContext<'_>,user_mention: &str,) -> Result<()> {
        info!("Processing buffer command for user mention: {}", user_mention);
        let user_id = parse_user_mention(user_mention);
        if !check_role(cc, &Role::Admin).await? {
            let response = CIR::Message(CIRM::new().content("Only admins can buffer players!").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }

        
    
        // TODO: Actually buffer the player
        Ok(())
    }
}

// Channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    pub queue: CI,
    pub queue_vc: CI,
    pub teams: Vec<TeamChannel>,
}

impl Channels {
    pub fn new(
        queue: CI,
        queue_vc: CI,
        teams: Vec<TeamChannel>,
    ) -> Self {
        Self {
            queue,
            queue_vc,
            teams,
        }
    }

    /// Pushs a red and blue channel to the vector
    pub fn add_team_channel_pair(
        &mut self,
        red_vc: CI,
        blu_vc: CI,
    ) {
        self.teams.push(TeamChannel { red_vc, blu_vc });
    }

    pub fn empty() -> Self {
        Self {
            queue: CI::new(1),
            queue_vc: CI::new(1),
            teams: Vec::new(),
        }
    }
}

// Teams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannel {
    pub red_vc: CI,
    pub blu_vc: CI,
}

impl TeamChannel {
    pub fn new(
        red_vc: CI,
        blu_vc: CI,
    ) -> Self {
        Self { red_vc, blu_vc }
    }

    pub fn empty() -> Self {
        Self {
            red_vc: CI::new(1),
            blu_vc: CI::new(1),
        }
    }
}

// Team
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Team {
    Red,
    Blu,
}

impl FromStr for Team {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RED" => Ok(Team::Red),
            "BLU" => Ok(Team::Blu),
            _ => Err(Error::msg(format!("Unknown : {}", s))),
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

// Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub msg: MessageId,
    pub channel_id: ChannelId,
}

impl Dashboard {
    pub fn new(
        msg: MessageId,
        channel_id: ChannelId,
    ) -> Self {
        Self { msg, channel_id }
    }

    pub async fn send(
        &self,
        ctx: &Context,
        embed: CreateEmbed,
    ) -> Result<Message> {
        Ok(self
            .channel_id
            .send_message(&ctx.http, CM::new().embed(embed))
            .await?)
    }

    /// Initializes a dashboard based on current group state
    pub async fn update(group: &Group) -> Result<CreateEmbed> {
        let mut embed = CreateEmbed::new().title("PUG Dashboard");

        let sessions_idle: Vec<&Session> = group
            .sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Idle)
            .collect();
        let sessions_hot: Vec<&Session> = group
            .sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Hot)
            .collect();
        let sessions_live: Vec<&Session> = group
            .sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Live)
            .collect();

        let mut desc = String::new();

        if let Some(session_current) = sessions_idle.first() {
            let queue_players = session_current.pool.len();
            let quota = group.quota as usize;

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
    pub async fn join_queue(cc: &ComponentContext<'_>) -> Result<()> {
        let user = cc.component.user.id;
        let channel = cc.component.channel_id;

        // Get group from database as base configuration
        let base_group = cc.db.get_group_by_channel(channel).await?;

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
        let mut manager = cc.manager.lock().await;
        let group = manager.get_or_create_group(channel, &base_group);

        // Check if we have idle sessions
        match group.get_sessions_by_status(&SessionStatus::Idle).len() {
            0 => {
                info!("No idle sessions found, creating a new session");
                group.create_session();
            }
            1 => {
                info!("Found one existing idle session");
            }
            n => {
                return Err(anyhow::anyhow!("Multiple idle sessions found: {}", n));
            }
        };

        // Check if player is already in session
        match group.get_user_session(user) {
            Some(_session) => {
                info!("Player is already in session");
                already_in_queue = true;
            }
            None => {
                info!("Player is not in session");
            }
        };

        // Check if we have idle sessions
        match group.get_sessions_by_status(&SessionStatus::Idle).len() {
            0 => {
                info!("No idle sessions found, creating a new session");
                group.create_session();
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
        if group.get_user_session(user).is_some() {
            info!("Player {} is already in a session", player.discord_id);
            already_in_queue = true;
        } else {
            // Add player to the session
            if let Some(session) = group.sessions.last_mut() {
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
            Dashboard::update(&base_group).await?;
        }

        Ok(())
    }

    /// Handles the leave queue button
    async fn leave_queue(cc: &ComponentContext<'_>) -> Result<()> {
        let user = cc.component.user.id;
        let channel = cc.component.channel_id;

        // Get group from database as base configuration
        let base_group = cc.db.get_group_by_channel(channel).await?;

        let mut found = false;
        let mut queue_count = 0;

        // Scope the manager lock to avoid Send issues
        {
            let mut manager = cc.manager.lock().await;
            let group = manager.get_or_create_group(channel, &base_group);

            // Find and remove player from any session
            for session in &mut group.sessions {
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
        } // Manager lock is dropped here

        if found {
            cc.create_bot_reply(&format!("❌ Left the queue! ({}/12 players)", queue_count))
                .await?;

            // Update dashboard to reflect new state
            Dashboard::update(&base_group).await?;
        } else {
            cc.create_bot_reply("You are not in the queue!").await?;
        }

        Ok(())
    }

    /// Handles the shuffle teams button
    async fn shuffle(
        cc: &ComponentContext<'_>,
        _session_id: Option<String>,
    ) -> Result<()> {
        let channel = cc.component.channel_id;
        let base_group = cc.db.get_group_by_channel(channel).await?;

        let mut shuffled = false;

        // Scope the manager lock to avoid Send issues
        {
            let mut manager = cc.manager.lock().await;
            let group = manager.get_or_create_group(channel, &base_group);

            // Find the session to shuffle
            if let Some(session) = group
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
        } // Manager lock is dropped here

        if shuffled {
            cc.create_bot_reply("🔀 Teams shuffled! Check the dashboard for new team assignments.")
                .await?;

            // Update dashboard to show shuffled teams
            Dashboard::update(&base_group).await?;
        } else {
            cc.create_bot_reply(
                "❌ No session ready for shuffling. Need at least 8 players in queue.",
            )
            .await?;
        }

        Ok(())
    }

    /// Handles the start match button
    async fn start(
        cc: &ComponentContext<'_>,
        _session_id: Option<String>,
    ) -> Result<()> {
        let channel = cc.component.channel_id;
        let base_group = cc.db.get_group_by_channel(channel).await?;

        let mut match_started = false;

        // Scope the manager lock to avoid Send issues
        {
            let mut manager = cc.manager.lock().await;
            let group = manager.get_or_create_group(channel, &base_group);

            // Find the session to start
            if let Some(session) = group
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
        } // Manager lock is dropped here

        if match_started {
            cc.create_bot_reply("🔥 Match started! Teams are now ready to play.")
                .await?;

            // Update dashboard to show match status
            Dashboard::update(&base_group).await?;
        } else {
            cc.create_bot_reply(
                "❌ No session ready to start. Need at least 8 players and shuffled teams.",
            )
            .await?;
        }

        Ok(())
    }

    /// Handles the end match button
    async fn end(
        cc: &ComponentContext<'_>,
        _session_id: Option<String>,
    ) -> Result<()> {
        let channel = cc.component.channel_id;
        let base_group = cc.db.get_group_by_channel(channel).await?;

        let mut match_ended = false;

        // Scope the manager lock to avoid Send issues
        {
            let mut manager = cc.manager.lock().await;
            let group = manager.get_or_create_group(channel, &base_group);

            // Find active sessions to end
            for session in &mut group.sessions {
                if session.status == SessionStatus::Hot || session.status == SessionStatus::Live {
                    // Clear the session and reset to idle
                    session.pool.clear();
                    session.status = SessionStatus::Idle;
                    match_ended = true;
                    info!("Match ended and session reset");
                    break;
                }
            }
        } // Manager lock is dropped here

        if match_ended {
            cc.create_bot_reply(
                "✅ Match ended! Sesh has been reset and is ready for new players.",
            )
            .await?;

            // Update dashboard to show reset state
            Dashboard::update(&base_group).await?;
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
    pub async fn handle_button_interaction(
        &self,
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
            "join" => Dashboard::join_queue(cc).await,
            "leave" => Dashboard::leave_queue(cc).await,
            "shuffle" => Dashboard::shuffle(cc, session_id).await,
            "start" => Dashboard::start(cc, session_id).await,
            "end" => Dashboard::end(cc, session_id).await,
            _ => {
                cc.create_bot_reply(&format!("Unknown button action: {}", action))
                    .await?;
                Ok(())
            }
        }
    }
}

// Session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub status: SessionStatus,
    pub pool: Vec<SessionPlayer>,
}

impl Session {
    pub fn get_user(&self, discord_id: UserId) -> Result<Player> {
        match self.pool.iter().find(|p| p.player.discord_id == discord_id) {
            Some(player) => Ok(player.player),
            None => Err(anyhow::anyhow!("User not found")),
        }
    }

    pub fn new(
        status: SessionStatus,
        pool: Vec<SessionPlayer>,
    ) -> Self {
        Self { status, pool }
    }

    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    pub fn empty() -> Self {
        Self {
            status: SessionStatus::Idle,
            pool: Vec::new(),
        }
    }
}

// SessionStatus
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Idle, // Waiting for enough players to join
    Hot,  // Waiting for runners to start the session
    Push, // Moving players to the team channels
    Live, // Game is active
    Pull, // Moving players back to the queue
}

impl SessionStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            SessionStatus::Push | SessionStatus::Live | SessionStatus::Pull
        )
    }
}

// SessionPlayer
#[derive(Debug, Clone, Copy, FromRow, Serialize, Deserialize)]
pub struct SessionPlayer {
    pub player: Player,
    pub team: Option<Team>,
    pub buffered: Option<Player>,
    pub queue_vc: bool,
    pub queue_cmd: bool,
}

impl SessionPlayer {
    pub fn construct(player: Player) -> Self {
        Self {
            player,
            team: None,
            buffered: None,
            queue_vc: false,
            queue_cmd: false,
        }
    }

    pub fn buff(
        &mut self,
        buffered: Option<Player>,
    ) {
        self.buffered = buffered;
    }

    pub fn unbuff(&mut self) {
        self.buffered = None;
    }

    pub fn team(
        &mut self,
        team: Team,
    ) {
        self.team = Some(team);
    }

    pub fn in_queue(&self) -> bool {
        self.queue_vc || self.queue_cmd
    }
}
