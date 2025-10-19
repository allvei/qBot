use std::{str::FromStr, sync::Arc};

use anyhow::Error;
// CHECK ME
use serde::{ Deserialize, Serialize };
use serenity::all::{ButtonStyle, Cache, ChannelId as CI, Context, CreateActionRow, CreateButton, CreateMessage, GuildId as GI, MessageId as MI, RoleId as RI, UserId};
use sqlx::FromRow;
use tracing::{error, info};

use crate::{handlers::dashboard, Database, models::Player};

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

// Manager
#[derive(Default)]
pub struct Manager {
    pub guilds: Vec<Guild>,
}

impl Manager {
    pub fn new(
    ) -> Self {
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
        Self {
            guilds,
        }
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
    pub key:         String,
    pub value:       Option<String>,
    pub description: Option<String>,
}

// Guild
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub guild_id: GI,
    pub roles:    Roles,
    pub groups:   Vec<Group>,
}

impl Guild {
    pub fn new(guild_id: GI, roles: Roles) -> Self {
        Self {guild_id, roles, groups: Vec::new()}
    }

    pub fn add_group(&mut self, group: Group) {
        self.groups.push(group);
    }

    pub fn empty(guild_id: GI) -> Self {
        Self {guild_id, roles: Roles::empty(), groups: Vec::new()}
    }
}

// Group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub group_id:  u8,
    pub timeout:   u16,
    pub quota:     u8,
    pub dashboard: Dashboard,
    pub channels:  Channels,
    pub sessions:  Vec<Session>,
}

impl Group {
    pub fn new(
        group_id:  u8,
        quota:     u8,
        timeout:   u16,
        dashboard: Dashboard,
        channels:  Channels,
        sessions:  Vec<Session>,
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
        self.sessions.push(Session::new(SessionStatus::Idle, Vec::new()));
    }

    pub fn end_session(&mut self,) -> bool {
        info!("Attempting to end session");
        if let Some(pos) = self.sessions.iter().position(|s| s.status == SessionStatus::Idle) {
            self.sessions.remove(pos);
            info!("Session successfully ended and removed");
            true
        } else {
            info!("Failed to end session: Session not found");
            false
        }
    }

    pub fn get_sessions_by_status(&mut self, status: &SessionStatus) -> Vec<&mut Session> {
        self.sessions.iter_mut().filter(|s| s.status == *status).collect()
    }

    pub fn get_user_session(&mut self, discord_id: UserId) -> Option<Session> {
        self.sessions.iter().find(|s| s.pool.iter().any(|p| p.player.discord_id == discord_id)).cloned()
    }

    pub async fn has_dashboard(&self,ctx: &Context) -> bool {
        let channel = CI::new(self.dashboard.ch.into());
        let message = channel.message(&ctx.http, self.dashboard.msg).await;
        message.is_ok()
    }

    pub async fn init_dashboard(&self, ctx: &Context, _db: &Arc<Database>, _dashboard_id: CI) -> Result<bool, anyhow::Error> {
        info!("Checking if dashboard exists for channel ID: {}", _dashboard_id);
        // Only check if dashboard exists, don't create new ones
        // Dashboard creation should be handled separately via admin commands
        Ok(self.has_dashboard(ctx).await)
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
            ("join",    "Join Queue",  ButtonStyle::Secondary, false),
            ("leave",   "Leave Queue", ButtonStyle::Secondary, false),
            ("shuffle", "Shuffle",     ButtonStyle::Secondary, !has_ready_session),
            ("start",   "Start Match", ButtonStyle::Secondary, !has_live_session),
            ("end",     "End Match",   ButtonStyle::Secondary, !has_live_session)
        ];
        
        // Generate buttons from configurations
        button_configs.into_iter().map(|(action, label, style, disabled)| {
            
            // Create the button with all specified properties
            CreateButton::new(action)
                .label(label)
                .style(style)
                .disabled(disabled)
        }).collect()
    }
    
    pub fn empty(group_id: u8) -> Self {
        Self {
            group_id,
            quota: 1,
            timeout: 120,
            dashboard: Dashboard::empty(),
            channels: Channels::empty(),
            sessions: Vec::new(),
        }
    }
}

// Channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    pub queue:    CI,
    pub queue_vc: CI,
    pub teams:    Vec<Teams>,
}

impl Channels {
    pub fn new(queue: CI, queue_vc: CI, teams: Vec<Teams>) -> Self {
        Self {queue, queue_vc, teams}
    }

    /// Pushs a red and blue channel to the vector
    pub fn add_team_channel_pair(&mut self, red_vc: CI, blu_vc: CI) {
        self.teams.push(Teams { red_vc, blu_vc });
    }

    pub fn empty() -> Self {
        Self {queue: CI::new(1), queue_vc: CI::new(1), teams: Vec::new()}
    }
}

// Teams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Teams {
    pub red_vc: CI,
    pub blu_vc: CI,
}

impl Teams {
    pub fn new(red_vc: CI, blu_vc: CI) -> Self {
        Self {red_vc, blu_vc}
    }

    pub fn empty() -> Self {
        Self {red_vc: CI::new(1), blu_vc: CI::new(1)}
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
    pub admin:  RI,
}

impl Roles {
    pub fn new(runner: RI, admin: RI) -> Self {
        Self {runner, admin}
    }
    pub fn empty() -> Self {
        Self {runner: RI::new(1), admin: RI::new(1)}
    }
}

// Dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub ch:  CI,
    pub msg: MI,
}

impl Dashboard {
    pub fn new(ch: CI, msg: MI) -> Self {
        Self {ch, msg}
    }
    pub fn empty() -> Self {
        Self {ch: CI::new(1), msg: MI::new(1)}
    }
}

// Session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub status:   SessionStatus,
    pub pool:     Vec<SessionPlayer>,
}

impl Session {
    pub fn new(status: SessionStatus, pool: Vec<SessionPlayer>) -> Self {
        Self {status, pool}
    }

    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    pub fn empty() -> Self {
        Self {status: SessionStatus::Idle, pool: Vec::new()}
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
        matches!(self, SessionStatus::Push | SessionStatus::Live | SessionStatus::Pull)
    }
}

// SessionPlayer
#[derive(Debug, Clone, Copy, FromRow, Serialize, Deserialize,)]
pub struct SessionPlayer {
    pub player:    Player,
    pub team:      Option<Team>,
    pub buffered:  Option<Player>,
    pub queue_vc:  bool,
    pub queue_cmd: bool,
}

impl SessionPlayer {
    pub fn construct(
        player:   Player,
    ) -> Self {
        Self {
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