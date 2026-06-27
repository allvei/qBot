use crate::{get_user_tag, guild_name, log_prefix_category, log_prefix_guild};
use anyhow::{anyhow, Error, Result};
use serenity::all::{
  ButtonStyle as BS, ChannelId as CI, Context, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE, CreateInteractionResponse as CIR,
  CreateInteractionResponseMessage as CIRM, CreateMessage as CM, EditMessage, GuildId as GI, Message, UserId as UI,
};
use std::{
  collections::{HashMap, HashSet},
  sync::Arc,
  time::{Duration, SystemTime},
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::models::{Category, ComponentContext, DashboardQueueKey, Session, SessionPlayer, SessionStatus};

// Helper methods to reduce code duplication

/// Format team display for embed fields
async fn format_team_display(
  embed: serenity::all::CreateEmbed,
  pool: &[crate::models::SessionPlayer],
  label: &str,
  hide_elo: bool,
  dynamic_elo_active: bool,
) -> serenity::all::CreateEmbed {
  if pool.is_empty() {
    return embed;
  }

  let formatted_players: Vec<_> = pool
    .iter()
    .map(|p| {
      if hide_elo {
        format!("<@{}>", p.player.user_id)
      } else {
        let elo = if dynamic_elo_active { p.player.dynamic_elo.unwrap_or(p.player.elo) } else { p.player.elo };
        format!("‹**{}**› <@{}>", elo, p.player.user_id)
      }
    })
    .collect();

  embed.field(format!("{label} ({})", pool.len()), formatted_players.join("\n"), false)
}

/// Add waiting players field to embed
fn add_waiting_field(embed: serenity::all::CreateEmbed, fmt_label: &str, current: usize, quota: usize, message: &str) -> serenity::all::CreateEmbed {
  embed.field(format!("{fmt_label} - Idle ({current}/{quota})"), message, false)
}

/// Create a button interaction response with proper error handling
async fn create_button_response(ctx: &Context, interaction: &serenity::all::ComponentInteraction, response_type: ButtonResponseType, content: &str) -> Result<()> {
  let response = match response_type {
    ButtonResponseType::Acknowledge => CIR::Acknowledge,
    ButtonResponseType::Message => CIR::Message(CIRM::new().content(content).ephemeral(true)),
    ButtonResponseType::Update => CIR::UpdateMessage(CIRM::new().content(content)),
  };

  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

#[derive(Debug)]
enum ButtonResponseType {
  Acknowledge,
  Message,
  Update,
}

/// Helper struct to hold team data for display
struct TeamDisplay {
  red: Vec<crate::models::SessionPlayer>,
  blu: Vec<crate::models::SessionPlayer>,
  hide_elo: bool,
  dynamic_elo_active: bool,
}

impl TeamDisplay {
  /// Create new team display from sorted teams
  fn new(red: Vec<crate::models::SessionPlayer>, blu: Vec<crate::models::SessionPlayer>, hide_elo: bool, dynamic_elo_active: bool) -> Self {
    Self { red, blu, hide_elo, dynamic_elo_active }
  }

  /// Build team field headers with average ELO
  fn build_headers(&self) -> (String, String) {
    let red_header = if self.hide_elo { "🔴 RED".to_string() } else { format!("‹**{}**› 🔴 RED", get_avg_elo(&self.red, self.dynamic_elo_active)) };
    let blu_header = if self.hide_elo { "🔵 BLU".to_string() } else { format!("‹**{}**› 🔵 BLU", get_avg_elo(&self.blu, self.dynamic_elo_active)) };
    (red_header, blu_header)
  }

  /// Add both team fields to an embed in a single call
  /// Blue team is shown first, then red team
  async fn add_to_embed(self, embed: CE, db: &crate::Database, guild_id: GI) -> CE {
    let (red_header, blu_header) = self.build_headers();
    embed.field(blu_header, format_team_field(&self.blu, db, guild_id, self.hide_elo).await, true).field(
      red_header,
      format_team_field(&self.red, db, guild_id, self.hide_elo).await,
      true,
    )
  }
}

/// Helper function to calculate average ELO for a team
fn get_avg_elo(team: &[crate::models::SessionPlayer], dynamic_elo_active: bool) -> f64 {
  let sum: f64 = team
    .iter()
    .map(|p| {
      let elo = if dynamic_elo_active { p.player.dynamic_elo.unwrap_or(p.player.elo) } else { p.player.elo };
      elo as f64
    })
    .sum();
  (sum / team.len() as f64 * 10.0).round() / 10.0
}

/// Helper function to format team players as a string for embed fields
async fn format_team_field(team: &[crate::models::SessionPlayer], db: &crate::Database, guild_id: GI, hide_elo: bool) -> String {
  let dynamic_elo_active = db.config.get_active_elo(guild_id).await.unwrap_or(false);
  let mut lines = Vec::new();
  for player in team {
    if hide_elo {
      lines.push(format!("<@{}>", player.player.user_id));
    } else {
      let elo = if dynamic_elo_active { player.player.dynamic_elo.unwrap_or(player.player.elo) } else { player.player.elo };
      lines.push(format!("‹**{}**› <@{}>", elo, player.player.user_id));
    }
  }
  lines.join("\n")
}

/// Helper function to split pool into teams by actual team assignments and sort by ELO descending
fn get_sorted_teams(pool: &[crate::models::SessionPlayer], _quota: usize) -> (Vec<crate::models::SessionPlayer>, Vec<crate::models::SessionPlayer>) {
  // Filter players by their actual team assignment (not by position!)
  // Don't use .take(quota) here - we want all players with team assignments
  // This ensures new players who join after someone leaves are displayed
  let mut team_red: Vec<_> = pool.iter().filter(|p| p.team == Some(crate::models::Team::Red)).cloned().collect();

  let mut team_blu: Vec<_> = pool.iter().filter(|p| p.team == Some(crate::models::Team::Blu)).cloned().collect();

  // Sort both teams by ELO descending
  let sort_by_elo = |a: &crate::models::SessionPlayer, b: &crate::models::SessionPlayer| {
    let elo_a = a.player.elo;
    let elo_b = b.player.elo;
    elo_b.cmp(&elo_a)
  };

  team_red.sort_by(sort_by_elo);
  team_blu.sort_by(sort_by_elo);

  (team_red, team_blu)
}

/// Public wrapper for get_sorted_teams (used by runner_menu_end)
pub fn get_sorted_teams_pub(pool: &[crate::models::SessionPlayer], quota: usize) -> (Vec<crate::models::SessionPlayer>, Vec<crate::models::SessionPlayer>) {
  get_sorted_teams(pool, quota)
}

/// Build and add team fields to an embed using TeamDisplay (used by runner_menu_end)
pub async fn build_team_fields(
  embed: CE,
  team_red: Vec<crate::models::SessionPlayer>,
  team_blu: Vec<crate::models::SessionPlayer>,
  hide_elo: bool,
  dynamic_elo_active: bool,
  db: &crate::Database,
  guild_id: GI,
) -> CE {
  TeamDisplay::new(team_red, team_blu, hide_elo, dynamic_elo_active).add_to_embed(embed, db, guild_id).await
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

  // Init category flow buttons
  InitDashboard,
  InitQueue,
  InitQueueVc,
  InitRed,
  InitBlue,
  InitRunner,
  InitAdmin,
  InitQuota,

  // Category link buttons
  CategoryLinkDashboard,
  CategoryLinkQueue,
  CategoryLinkQueueVc,
  CategoryLinkRed,
  CategoryLinkBlue,

  // Dashboard action buttons
  DashboardJoin,
  DashboardLeave,
  DashboardShuffle,
  DashboardStart,
  DashboardEnd,
  DashboardCancel,
  DashboardReportScore,

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
  pub fn parse_button(custom_id: &str) -> Self {
    match custom_id {
      // Setup buttons
      "setup_dashboard" => Self::SetupDashboard,
      "setup_queue" => Self::SetupQueue,
      "setup_queuevc" => Self::SetupQueueVc,
      "setup_red" => Self::SetupRed,
      "setup_blue" => Self::SetupBlue,
      "setup_runner" => Self::SetupRunner,
      "setup_admin" => Self::SetupAdmin,

      // Init buttons
      "init_dashboard" => Self::InitDashboard,
      "init_queue" => Self::InitQueue,
      "init_queuevc" => Self::InitQueueVc,
      "init_red" => Self::InitRed,
      "init_blue" => Self::InitBlue,
      "init_runner" => Self::InitRunner,
      "init_admin" => Self::InitAdmin,
      "init_quota" => Self::InitQuota,

      // Category link buttons
      "categorylink_dashboard" => Self::CategoryLinkDashboard,
      "categorylink_queue" => Self::CategoryLinkQueue,
      "categorylink_queuevc" => Self::CategoryLinkQueueVc,
      "categorylink_red" => Self::CategoryLinkRed,
      "categorylink_blue" => Self::CategoryLinkBlue,

      // Dashboard buttons
      "join_queue" => Self::DashboardJoin,
      "leave_queue" => Self::DashboardLeave,
      "shuffle_teams" => Self::DashboardShuffle,
      "start_match" => Self::DashboardStart,
      "end_match" => Self::DashboardEnd,
      "cancel_match" => Self::DashboardCancel,
      "report_score" => Self::DashboardReportScore,

      // Permission confirmation
      "confirm_permissions" => Self::ConfirmPermissions,

      // Rank role creation
      "create_rank_roles_yes" => Self::CreateRankRolesYes,
      "create_rank_roles_no" => Self::CreateRankRolesNo,

      // Unknown
      _ => Self::Unknown(custom_id.to_string()),
    }
  }

  /// Check if this button type requires setup handling
  pub fn is_setup_button(&self) -> bool {
    matches!(
      self,
      Self::SetupDashboard
        | Self::SetupQueue
        | Self::SetupQueueVc
        | Self::SetupRed
        | Self::SetupBlue
        | Self::SetupRunner
        | Self::SetupAdmin
        | Self::InitDashboard
        | Self::InitQueue
        | Self::InitQueueVc
        | Self::InitRed
        | Self::InitBlue
        | Self::InitRunner
        | Self::InitAdmin
        | Self::InitQuota
        | Self::CategoryLinkDashboard
        | Self::CategoryLinkQueue
        | Self::CategoryLinkQueueVc
        | Self::CategoryLinkRed
        | Self::CategoryLinkBlue
    )
  }

  /// Check if this button type is a dashboard action
  pub fn is_dashboard_action(&self) -> bool {
    matches!(self, Self::DashboardJoin | Self::DashboardLeave | Self::DashboardShuffle | Self::DashboardStart | Self::DashboardEnd | Self::DashboardCancel)
  }

  /// Get the setup step name (for setup/init buttons)
  pub fn setup_step(&self) -> Option<&str> {
    match self {
      Self::SetupDashboard | Self::InitDashboard => Some("dashboard"),
      Self::SetupQueue | Self::InitQueue => Some("queue"),
      Self::SetupRed | Self::InitRed => Some("red"),
      Self::SetupBlue | Self::InitBlue => Some("blue"),
      Self::InitQueueVc => Some("queuevc"),
      Self::SetupRunner => Some("runner"),
      Self::SetupAdmin => Some("admin"),
      Self::InitQuota => Some("quota"),
      _ => None,
    }
  }
}

impl Category {
  /// Get the dashboard message
  pub async fn dash_get(&self, ctx: &Context) -> Result<Message> {
    let ch = CI::new(self.channels.dashboard.into());
    let msg = ch.message(&ctx.http, self.dashboard_msg).await;
    match msg {
      Ok(msg) => Ok(msg),
      Err(e) => Err(anyhow!("Failed to get dashboard message: {e}")),
    }
  }

  /// Creates buttons for the dashboard
  /// When multiple formats exist, each format gets its own button row.
  pub async fn create_dashboard_buttons(&self) -> Result<Vec<CAR>> {
    let has_multiple = self.formats.len() > 1;
    let mut buttons = Vec::new();

    for sg in &self.formats {
      let is_hot = sg.sessions.iter().any(|s| s.is_hot());
      let is_live = sg.sessions.iter().any(|s| s.is_active());
      let has_queued_players = sg.sessions.iter().any(|s| (s.is_idle() || s.is_hot()) && !s.pool.is_empty());

      let fmt_suffix = format!(":{}", sg.id);
      let join_label = if has_multiple { format!("Join {}", sg.name) } else { "Join".to_string() };

      // Row: Join {name} | [Leave | Edit timeout] | Start/End | [Shuffle]
      let mut row = if self.restarting {
        // When restarting, only show end button for live games
        vec![]
      } else {
        vec![CB::new(format!("join_queue{fmt_suffix}")).label(&join_label).style(BS::Success)]
      };

      if !self.restarting && has_queued_players {
        row.push(CB::new(format!("leave_queue{fmt_suffix}")).label("Leave").style(BS::Danger));
        row.push(CB::new(format!("change_expiry{fmt_suffix}")).label("Edit timeout").style(BS::Secondary));
      }
      if is_live {
        // Check if match can still be cancelled (less than 5 minutes)
        if let Some(live_session) = sg.sessions.iter().find(|s| s.is_active()) {
          if live_session.can_cancel_match() {
            row.push(CB::new(format!("cancel_match{fmt_suffix}")).label("Cancel game").style(BS::Danger));
          } else {
            let end_label = if self.require_score_report { "End & log score" } else { "End" };
            row.push(CB::new(format!("end_match{fmt_suffix}")).label(end_label).style(BS::Danger));
          }
        }
      }
      if !self.restarting && is_hot {
        row.push(CB::new(format!("start_match{fmt_suffix}")).label("Start").style(BS::Success));
        row.push(CB::new(format!("shuffle_teams{fmt_suffix}")).label("Shuffle").style(BS::Secondary));
      }
      buttons.push(CAR::Buttons(row));
    }

    // Last row: Preferences, Runner Menu, and Help
    if !self.restarting {
      buttons.push(Self::create_dashboard_footer_buttons());
    }

    Ok(buttons)
  }

  fn gen_button(config: (&'static str, &'static str, BS, bool)) -> CB {
    let (action, label, style, enabled) = config;
    CB::new(action).label(label).style(style).disabled(!enabled)
  }

  /// Create the standard footer buttons for the dashboard
  /// Includes Ping, Preferences, Runner Menu, and Help buttons
  fn create_dashboard_footer_buttons() -> CAR {
    CAR::Buttons(vec![
      Self::gen_button(("ping_players", "Ping", BS::Secondary, true)),
      Self::gen_button(("show_settings", "Preferences", BS::Secondary, true)),
      Self::gen_button(("show_runner_menu", "Runner menu", BS::Secondary, true)),
      Self::gen_button(("show_help", "Help", BS::Secondary, true)),
    ])
  }

  /// Record a match to the database with all player information. Returns the match ID if recorded.
  pub async fn record_match_to_database(
    db: &crate::Database,
    guild_id: GI,
    category_id: u8,
    format_id: u8,
    started_at: Option<std::time::SystemTime>,
    session_id: Option<String>,
    team_red: &[SessionPlayer],
    team_blu: &[SessionPlayer],
  ) -> Result<Option<i64>> {
    use crate::db::repo::MatchPlayerInsert;

    // Only record if match was actually started (has started_at timestamp)
    if let Some(started_at) = started_at {
      let ended_at = std::time::SystemTime::now();
      let duration_secs = ended_at.duration_since(started_at).map(|d| d.as_secs()).unwrap_or(0);

      // Reject matches shorter than 2 minutes (likely double-report or error)
      const MIN_MATCH_DURATION_SECS: u64 = 120;
      if duration_secs < MIN_MATCH_DURATION_SECS {
        warn!("Match duration too short ({}s), skipping database recording", duration_secs);
        return Ok(None);
      }

      // Insert match record
      match db.matches.insert_match(guild_id, category_id as i64, format_id as i64, session_id, started_at, ended_at, duration_secs).await {
        Ok(match_id) => {
          // Insert match players
          let mut players = Vec::new();

          for player in team_red {
            players.push(MatchPlayerInsert { user_id: player.player.user_id, team: "red".to_string(), elo_before: player.player.elo as i64 });
          }

          for player in team_blu {
            players.push(MatchPlayerInsert { user_id: player.player.user_id, team: "blu".to_string(), elo_before: player.player.elo as i64 });
          }

          if let Err(e) = db.matches.insert_match_players(match_id, players).await {
            error!("Failed to insert match players: {e}");
          }

          return Ok(Some(match_id));
        }
        Err(e) => {
          error!("Failed to insert match record: {e}");
        }
      }
    }

    Ok(None)
  }

  pub async fn has_dash(&self, ctx: &Context) -> bool {
    let ch = CI::new(self.channels.dashboard.into());
    let msg = ch.message(&ctx.http, self.dashboard_msg).await;
    msg.is_ok()
  }

  pub async fn dash_publish(&mut self, ctx: &Context, channel: CI, db: &crate::Database, guild_id: GI) -> Result<(), Error> {
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

  /// Builds dashboard embed and components based on current category state.
  /// Uses category name as title. Each format gets its own queue section.
  /// All content for a format is rendered together (header, players, teams)
  /// before moving to the next format.
  pub async fn build_dashboard_content(&self, db: &crate::Database, guild_id: GI, in_game_players: &HashMap<UI, (GI, String)>) -> Result<(CE, Vec<CAR>)> {
    let confirm_time_seconds = self.confirm_time as u64;
    let post_game_confirm_time = db.config.get_post_game_confirm_time(guild_id).await.unwrap_or(120) as u64;
    let hide_elo = db.config.get_bool(guild_id, "hide_elo", false).await.unwrap_or(false);
    let dynamic_elo_active = db.config.get_active_elo(guild_id).await.unwrap_or(false);
    let has_multiple = self.formats.len() > 1;

    let mut embed = CE::new().title(self.name());

    // Single loop: each format renders all its content together
    let fmt_count = self.formats.len();
    for (fmt_i, sg) in self.formats.iter().enumerate() {
      let quota = sg.quota as usize;
      let fmt_label = if has_multiple { format!("{} queue", sg.name) } else { "Queue".to_string() };

      // Categorize sessions
      let live_sessions: Vec<_> = sg.sessions.iter().filter(|s| s.is_active()).collect();
      let hot_sessions: Vec<_> = sg.sessions.iter().filter(|s| s.is_hot()).collect();
      let idle_sessions: Vec<_> = sg.sessions.iter().filter(|s| s.is_idle()).collect();
      let has_concurrent = live_sessions.len() + hot_sessions.len() > 1;

      // --- Live games (Push/Live/Pull) ---
      for (game_i, session) in live_sessions.iter().enumerate() {
        let started_time =
          session.started_at.and_then(|started_at| crate::timestamp_from_system_time(&started_at, crate::Style::Relative)).unwrap_or_else(|| "recently".to_string());

        let status_text = match session.status {
          SessionStatus::Push => "Moving players to team channels...".to_string(),
          SessionStatus::Live => format!("Started {}", started_time),
          SessionStatus::Pull => "Moving players back to queue...".to_string(),
          _ => String::new(),
        };

        let game_label = if has_concurrent { format!("Game {} - {}", game_i + 1, status_text) } else { format!("{fmt_label} - {}", status_text) };

        embed = embed.field(&game_label, "", false);

        if session.pool.len() >= quota {
          let (team_red, team_blu) = get_sorted_teams(&session.pool, quota);
          embed = TeamDisplay::new(team_red, team_blu, hide_elo, dynamic_elo_active).add_to_embed(embed, db, guild_id).await;
        }
      }

      // --- Hot sessions (Ready to start) ---
      for session in &hot_sessions {
        let hot_label = if has_concurrent { "Next game - Ready to start".to_string() } else { format!("{fmt_label} - Ready to start") };

        let mut hot_info = String::new();
        let missing_players: Vec<_> = session.pool.iter().take(quota).filter(|p| !p.in_vc).collect();

        if !missing_players.is_empty() {
          let base_time = session.match_ended_at.or(session.ready_at);
          if let Some(base_time) = base_time {
            let confirm_time_deadline = if session.match_ended_at.is_some() { post_game_confirm_time } else { confirm_time_seconds };
            if let Ok(d) = base_time.duration_since(SystemTime::UNIX_EPOCH) {
              let deadline = d.as_secs() + confirm_time_deadline;
              hot_info.push_str(&format!("Join deadline: {}\n", crate::timestamp_from_unix(deadline as i64, crate::Style::Relative)));
              hot_info.push_str("Missing players will be removed.\n\n");
            }
          }
          hot_info.push_str("**Missing players:**\n");
          for player in &missing_players {
            if hide_elo {
              hot_info.push_str(&format!("  • <@{}>\n", player.player.user_id));
            } else {
              let elo = if dynamic_elo_active { player.player.dynamic_elo.unwrap_or(player.player.elo) } else { player.player.elo };
              hot_info.push_str(&format!("  • ‹**{}**› <@{}>\n", elo, player.player.user_id));
            }
          }
        } else if !session.pool.is_empty() {
          hot_info.push_str("All players ready");
        }

        embed = embed.field(&hot_label, hot_info, false);

        if session.pool.len() >= quota {
          let (team_red, team_blu) = get_sorted_teams(&session.pool, quota);
          embed = TeamDisplay::new(team_red, team_blu, hide_elo, dynamic_elo_active).add_to_embed(embed, db, guild_id).await;

          // Overflow players in the hot session
          if session.pool.len() > quota {
            let overflow_count = session.pool.len() - quota;
            let fatkid: Vec<_> = session
              .pool
              .iter()
              .skip(quota)
              .map(|p| {
                if hide_elo {
                  format!("<@{}>", p.player.user_id)
                } else {
                  let elo = if dynamic_elo_active { p.player.dynamic_elo.unwrap_or(p.player.elo) } else { p.player.elo };
                  format!("‹**{}**› <@{}>", elo, p.player.user_id)
                }
              })
              .collect();
            embed = embed.field(format!("Waiting for next game ({overflow_count}/{quota})"), fatkid.join("\n"), false);
          }
        }
      }

      // --- Idle session (queue for next game) ---
      if let Some(idle_session) = idle_sessions.first() {
        let queue_players = idle_session.pool.len();

        if queue_players > 0 && (has_concurrent || !live_sessions.is_empty()) {
          // There are active/hot games — show idle players as waiting for next game
          embed = format_team_display(embed, &idle_session.pool, "Waiting for next game", hide_elo, dynamic_elo_active).await;
        } else if queue_players == 0 && live_sessions.is_empty() && hot_sessions.is_empty() {
          // No games at all — show empty queue
          embed = add_waiting_field(embed, &fmt_label, 0, quota, "*Join to get started!*");
        } else if queue_players > 0 && live_sessions.is_empty() && hot_sessions.is_empty() {
          // Only idle with players — show full player list with timers
          let mut players_field = String::new();
          let mut timers_field = String::new();

          for player in idle_session.pool.iter() {
            let elo_to_display = if dynamic_elo_active { player.player.dynamic_elo.unwrap_or(player.player.elo) } else { player.player.elo };
            let elo_str = if hide_elo { String::new() } else { format!("‹**{}**› ", elo_to_display) };
            players_field.push_str(&format!("{elo_str}<@{}>\n", player.player.user_id));

            if let Some((game_guild_id, fmt_name)) = in_game_players.get(&player.player.user_id) {
              if *game_guild_id == guild_id {
                timers_field.push_str(&format!("In {fmt_name} game\n"));
              } else {
                timers_field.push_str("In-game\n");
              }
            } else if player.in_vc {
              timers_field.push_str("VC\n");
            } else {
              let queue_expiration = player.queue_expiration;

              if queue_expiration > 0 {
                if let Ok(join_time) = player.joined_at.duration_since(std::time::SystemTime::UNIX_EPOCH) {
                  let expiry_timestamp = join_time.as_secs() + (queue_expiration as u64 * 60);
                  timers_field.push_str(&format!("Timeout {}\n", crate::timestamp_from_unix(expiry_timestamp as i64, crate::Style::Relative)));
                } else {
                  timers_field.push_str("-\n");
                }
              } else {
                timers_field.push_str("-\n");
              }
            }
          }

          embed = embed.field(format!("{fmt_label} - Idle ({queue_players}/{quota})"), players_field, true);
          embed = embed.field("Status", timers_field, true);
        }
      } else if live_sessions.is_empty() && hot_sessions.is_empty() {
        // No sessions at all
        embed = add_waiting_field(embed, &fmt_label, 0, quota, "*Empty, join to get started!*");
      }

      // Separator between formats
      if has_multiple && fmt_i < fmt_count - 1 {
        embed = embed.field("\u{200B}", "", false);
      }

      // Add connect info if available and non-empty
      if let Some(ref connect_info) = sg.connect_info.as_ref().filter(|s| !s.trim().is_empty()) {
        let label = if has_multiple { format!("{} - Connect info:", sg.name) } else { "Server connect info:".to_string() };
        let mut field_value = format!("```{connect_info}```");

        if let Some(steam_link) = Self::extract_steam_link(connect_info) {
          field_value.push_str(&format!("\n**Steam Link:**\n```{steam_link}```"));
        }

        embed = embed.field(label, field_value, false);
      }
    }

    // Add last action footer if available
    if let Some((user_tag, action_desc, timestamp)) = &self.last_action {
      // Only show if action was within the last 10 minutes
      if let Ok(elapsed) = SystemTime::now().duration_since(*timestamp) {
        if elapsed.as_secs() < 600 {
          embed = embed.footer(serenity::all::CreateEmbedFooter::new(format!("{} {}", user_tag, action_desc)));
        }
      }
    }

    let buttons = self.create_dashboard_buttons().await.unwrap();

    Ok((embed, buttons))
  }

  /// Set last action for dashboard footer
  pub fn set_last_action(&mut self, user_tag: String, action: &str) {
    self.last_action = Some((user_tag, action.to_string(), SystemTime::now()));
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
      after_password.split(&[';', '\n', '\r'][..]).next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
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

  /// Initializes a dashboard based on current category state
  pub async fn dash_init(&mut self, db: &crate::Database, guild_id: GI) -> Result<CM> {
    let (embed, buttons) = self.build_dashboard_content(db, guild_id, &HashMap::new()).await?;
    let message = CM::new().embed(embed).components(buttons);
    Ok(message)
  }

  /// Updates a dashboard based on current category state
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
  /// Requires guild_id to be passed since Category doesn't store it
  pub async fn queue_dash_update(&self, ctx: &Context, guild_id: GI) {
    //
    // Try to get queue from context data using the key from models module
    let data = ctx.data.read().await;
    if let Some(queue) = data.get::<DashboardQueueKey>() {
      queue.lock().await.request_update(guild_id, self.id);
      //
    } else {
      warn!("Dashboard queue not initialized in Context");
      // Note: Can't fallback to dash_update here because we'd need &mut self
      // The dashboard queue should always be initialized, so this is just a safety check
    }
  }

  /// Handles the join queue button
  async fn dash_join_queue(&mut self, cc: &ComponentContext<'_>, fmt_id: u8) -> Result<()> {
    cc.reply_acknowledge().await?;

    let user_id = cc.component.user.id;
    let user_tag = cc.component.user.tag();
    let _guild_id = cc.component.guild_id.unwrap();

    // Store channel IDs before any borrows
    let _dashboard_channel = self.channels.dashboard;

    // If player is already in this format, refresh their timeout and return
    if self.is_user_in_fmt(fmt_id, user_id) {
      debug!("{} already in format {}, refreshing timeout", user_tag, fmt_id);
      if let Some(sg) = self.format_mut(fmt_id) {
        for session in &mut sg.sessions {
          if let Some(sp) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
            sp.joined_at = std::time::SystemTime::now();
            break;
          }
        }
      }
      self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;
      return Ok(());
    }

    // Check if we have an idle or hot session to join in the target format
    let has_joinable_session = self.format(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.status == SessionStatus::Idle || s.status == SessionStatus::Hot)).unwrap_or(false);

    debug!("{} attempting to join {}: has_joinable_session={}", user_tag, fmt_id, has_joinable_session);

    if !has_joinable_session {
      debug!("{} blocked from joining {}: no joinable session (match in progress)", user_tag, fmt_id);
      use serenity::all::CreateInteractionResponseFollowup as CIRF;
      let followup = CIRF::new().content("Cannot join - match is in progress. Please wait.").ephemeral(true);
      cc.component.create_followup(&cc.ctx.http, followup).await?;
      return Ok(());
    }

    // Use resolve_player_for_queue for consistent player resolution
    use crate::handlers::player::resolve_player_for_queue;
    if let Some(guild_id) = cc.component.guild_id {
      // When dynamic ELO is enabled, check if this player needs skill selection first.
      let dynamic_elo_active = cc.db.config.get_active_elo(guild_id).await.unwrap_or(false);
      debug!("{} checking dynamic ELO: {}", user_tag, dynamic_elo_active);
      if dynamic_elo_active {
        let needs_selection = cc.db.elo.needs_skill_selection(user_id, guild_id).await.unwrap_or(false);
        debug!("{} needs skill selection: {}", user_tag, needs_selection);
        if needs_selection {
          let gamemode = cc.db.config.get_gamemode(guild_id).await.unwrap_or(None);
          let prompt = match &gamemode {
            Some(gm) => format!("For balancing reasons, please describe your experience with **{}**:", gm),
            None => "For balancing reasons, please describe your skill level:".to_string(),
          };

          use serenity::all::CreateInteractionResponseFollowup as CIRF;
          let buttons = vec![CAR::Buttons(vec![
            CB::new(format!("skill_select_beginner_{}_{}", self.id, fmt_id)).label("Beginner").style(BS::Secondary),
            CB::new(format!("skill_select_intermediate_{}_{}", self.id, fmt_id)).label("Intermediate").style(BS::Secondary),
            CB::new(format!("skill_select_expert_{}_{}", self.id, fmt_id)).label("Expert").style(BS::Secondary),
            CB::new(format!("skill_select_veteran_{}_{}", self.id, fmt_id)).label("Veteran").style(BS::Secondary),
          ])];
          let followup = CIRF::new().content(prompt).components(buttons).ephemeral(true);
          cc.component.create_followup(&cc.ctx.http, followup).await?;
          return Ok(());
        }
      }

      let (mut player, discord_rank, _rank_mismatch) = match resolve_player_for_queue(cc.ctx, &cc.db, guild_id, user_id).await {
        Ok(result) => result,
        Err(e) => {
          error!("Failed to resolve player {} for queue: {e}", user_tag);
          use serenity::all::CreateInteractionResponseFollowup as CIRF;
          let followup = CIRF::new().content("Failed to join queue. Please try again.").ephemeral(true);
          cc.component.create_followup(&cc.ctx.http, followup).await?;
          return Ok(());
        }
      };
      // Todo: player.tag is actually the player nickname, not discord tag
      debug!("{} resolved as player: tag={}, rank={}", user_tag, player.tag, discord_rank.name);

      // Fetch discord tag from component user for performance (avoid extra API call)
      player.tag = cc.component.user.tag();

      // Save rank for announcement (player will be moved)
      let player_rank = discord_rank.clone();

      // Log the queue join attempt BEFORE adding to queue to fix race condition
      let _server_name = guild_name(cc.ctx, guild_id);
      let _category_name = self.name.as_deref().unwrap_or("Unknown").to_string();
      let _username = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;

      // Get current pool length BEFORE adding player
      let (_pool_len_before, _fmt_quota) = self.format(fmt_id).map(|sg| (sg.sessions.iter().map(|s| s.pool.len()).sum::<usize>(), sg.quota as usize)).unwrap_or((0, 0));
      let fmt_name = self.format(fmt_id).map(|sg| sg.name.as_str());

      // Clone fmt_name to avoid borrowing issues
      let fmt_name_owned = fmt_name.map(|s| s.to_string());

      let queue_context = crate::QueueContext::new(cc.ctx, Some(guild_id), Some(&cc.db), Some(cc.manager.clone()));
      let is_user_in_vc = self.is_user_in_queue_vc(&cc.ctx.http, user_id).await;

      debug!("Attempting to queue {} with VC status: {}", user_tag, is_user_in_vc);
      if let Err(e) = self.queue_player_with_vc_status_fmt(fmt_id, player.clone(), discord_rank, queue_context, is_user_in_vc).await {
        error!("Failed to queue {}: {e}", user_tag);
      } else {
        debug!("Successfully queued {} in {}", user_tag, fmt_id);
        // Log AFTER queue operation so position and count are accurate
        if let Some(format) = self.format(fmt_id) {
          if let Err(e) = crate::log_queue_toggle(cc.ctx, &cc.db, guild_id, self.id, format, &player, "joined", None).await {
            warn!("Failed to log queue toggle: {e}");
          }
        }
        // Send join announcement (delayed + buffered)
        {
          use crate::models::alert_limiter::{schedule_alert, AlertType};

          schedule_alert(cc.ctx.clone(), self.channels.queue_chat, guild_id, user_id, cc.db.clone(), self.id, fmt_id, AlertType::Join, fmt_name_owned, player_rank.name.clone());
        }
      }
    } else {
      use serenity::all::CreateInteractionResponseFollowup as CIRF;
      let followup = CIRF::new().content("This command can only be used in a server.").ephemeral(true);
      cc.component.create_followup(&cc.ctx.http, followup).await?;
      return Ok(());
    }

    // Update dashboard to reflect changes
    //
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;
    //
    Ok(())
  }

  /// Handles the leave queue button
  async fn dash_leave_queue(&mut self, cc: &ComponentContext<'_>, format_id: u8) -> Result<()> {
    cc.reply_acknowledge().await?;

    let user_id = cc.component.user.id;

    // Check if player is in a live match - disallow leaving
    if let Ok(session) = self.get_user_sesh_fmt(format_id, user_id) {
      if session.status == SessionStatus::Live {
        use serenity::all::CreateInteractionResponseFollowup as CIRF;
        let followup = CIRF::new().content("You cannot leave during a live match. Please find a substitute if needed.").ephemeral(true);
        cc.component.create_followup(&cc.ctx.http, followup).await?;
        return Ok(());
      }
    }

    let quota = self.format(format_id).map(|sg| sg.quota as usize).unwrap_or(0);

    // Store fields before any borrows
    let _dashboard_channel = self.channels.dashboard;
    let queue_chat = self.channels.queue_chat;
    let _category_id = self.id;
    let _category_name = self.name.as_deref().unwrap_or("Unknown").to_string();

    // Get session index and format name before mutable borrow
    let fmt_name_owned = self.format(format_id).map(|sg| sg.name.clone());
    let session_idx = self.format(format_id).and_then(|sg| sg.sessions.iter().position(|s| s.pool.iter().any(|p| p.player.user_id == user_id)));

    // Check if player is in queue
    let format = self.format(format_id).cloned(); // Get format before mutable borrow
    let category_id = self.id; // Capture category_id before mutable borrow
    let ply_in_other_fmts = self.is_user_in_other_fmts(format_id, user_id);
    let should_regenerate_teams = if let Ok(session) = self.get_user_sesh_fmt(format_id, user_id) {
      // Check if player is physically in the queue VC
      let ply_in_vc = if let Some(player) = session.pool.iter().find(|p| p.player.user_id == user_id) { player.in_vc } else { false };
      // If player is in VC, check if they want to be disconnected
      if ply_in_vc && !ply_in_other_fmts {
        // Check user's VC disconnect preference
        let prefs = cc.db.players.get_prefs(user_id).await.unwrap_or_default();

        if prefs.vc_auto_leave {
          if let Some(guild_id) = cc.component.guild_id {
            use serenity::all::EditMember;
            let _ = cc.ctx.http.edit_member(guild_id, user_id, &EditMember::new().disconnect_member(), Some("Player left queue via dashboard button")).await;
          }
          // The voice_state_update handler will handle removing them from the queue
          return Ok(());
        }
        // User wants to stay in VC, manually remove them from queue
        // Fall through to remove them manually below
      }

      // Player is in queue but not in VC (or wants to stay in VC), remove them manually

      let was_hot = session.is_hot();
      let _username = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;

      // Capture position before removal for logging
      let _position_before_removal = session.pool.iter().position(|p| p.player.user_id == user_id).map(|p| p + 1);

      session.remove_player(user_id);
      let pool_len = session.pool.len();

      // Log with server and category context
      let guild_id = cc.component.guild_id.unwrap();
      let _server_name = guild_name(cc.ctx, guild_id);

      // Resolve player for logging
      if let Ok(player) = cc.db.get_player(user_id, cc.ctx).await {
        if let Some(ref format) = format {
          if let Err(e) = crate::log_queue_toggle(cc.ctx, &cc.db, guild_id, category_id, format, &player, "left", None).await {
            warn!("Failed to log queue toggle: {e}");
          }
        }
      }

      // Send leave announcement (delayed + buffered)
      {
        use crate::models::alert_limiter::{schedule_alert, AlertType};

        schedule_alert(cc.ctx.clone(), queue_chat, guild_id, user_id, cc.db.clone(), category_id, format_id, AlertType::Leave, fmt_name_owned.clone(), String::new());
      }

      // If session was hot, check what to do next
      if was_hot {
        if pool_len < quota {
          // Dropped below quota, transition back to idle
          if let Some(_idx) = session_idx {
          } else {
            info!("Session dropped below quota, transitioning from Hot to Idle");
          }
          session.idle();
          false
        } else {
          // Still at or above quota, need to regenerate teams
          if let Some(_idx) = session_idx {
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
      use serenity::all::CreateInteractionResponseFollowup as CIRF;
      let followup = CIRF::new().content("You are not in the queue!").ephemeral(true);
      cc.component.create_followup(&cc.ctx.http, followup).await?;
      return Ok(());
    };

    // Cancel the player's timeout task (outside the mutable borrow scope)
    if let Some(guild_id) = cc.component.guild_id {
      self.cancel_player_rejoin_expiration(cc.ctx, guild_id, format_id, user_id).await;
    }

    // Regenerate teams if needed (outside the session borrow scope)
    if should_regenerate_teams {
      self.generate_teams_fmt(format_id, cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;
    }

    // Check if team VCs should be cleaned up (OnLastLeave policy)
    self.check_team_vc_cleanup_on_leave(cc.ctx).await;

    // Update dashboard to reflect changes (queue count now only shown in dashboard)
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;
    Ok(())
  }

  /// Handles the shuffle teams button
  async fn dash_shuffle(&mut self, cc: &ComponentContext<'_>, format_id: u8) -> Result<()> {
    let quota = self.format(format_id).map(|sg| sg.quota as usize).unwrap_or(0);

    // Check if game is live
    let is_live = self.format(format_id).map(|sg| sg.sessions.iter().any(|s| s.status == SessionStatus::Live)).unwrap_or(false);
    if is_live {
      cc.reply_ephemeral("The game is live, can not shuffle").await?;
      return Ok(());
    }

    // Find the game to shuffle - can be Idle (if quota met) or Hot
    let has_shuffleable =
      self.format(format_id).map(|sg| sg.sessions.iter().any(|s| (s.status == SessionStatus::Idle || s.status == SessionStatus::Hot) && s.pool.len() >= quota)).unwrap_or(false);

    if !has_shuffleable {
      cc.reply_ephemeral(&format!("No game ready for shuffling. Need at least {quota} players in queue.")).await?;
      return Ok(());
    }

    // Defer update now that we know we have a game to shuffle
    cc.reply_acknowledge().await?;

    // Refresh player ranks from Discord roles before shuffling teams
    if let Some(guild_id) = cc.component.guild_id {
      self.reload_player_ranks(cc.ctx, guild_id, &cc.db).await;
    }

    // Call the same team generation logic used by generate_teams
    // This ensures balanced teams using the BCH algorithm
    self.generate_teams_fmt(format_id, cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;

    // Update dashboard to show new teams
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;

    Ok(())
  }

  /// Handles the start match button
  async fn dash_start(&mut self, cc: &ComponentContext<'_>, fmt_id: u8) -> Result<()> {
    use std::time::Duration;
    // Check if user has Runner role
    use crate::handlers::player::is_role_component;
    use crate::models::Role;

    // Try to acquire interaction lock to prevent duplicate processing
    let action_key = format!("start_match_{}_{}", self.id, fmt_id);
    if !cc.try_lock_interaction(&action_key).await? {
      cc.reply_acknowledge().await?;
      return Ok(());
    }

    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    match is_role_component(cc, &Role::Runner).await {
      Ok(true) => {
        // User has Runner role, proceed
      }
      Ok(false) => {
        cc.reply_ephemeral("Only runners can start matches.").await?;
        cc.unlock_interaction().await;
        return Ok(());
      }
      Err(e) => {
        warn!("Failed to check runner role: {e}");
        cc.reply_ephemeral("Failed to verify permissions.").await?;
        cc.unlock_interaction().await;
        return Ok(());
      }
    }

    // Check if there's a hot game to start in the target format
    let has_hot_game = self.format(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.is_hot())).unwrap_or(false);

    if !has_hot_game {
      cc.reply_ephemeral("No hot game ready to start.").await?;
      cc.unlock_interaction().await;
      return Ok(());
    }

    // Check if there's already a Push or Live session (prevent starting multiple games)
    let has_active_game = self.format(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.is_active())).unwrap_or(false);

    if has_active_game {
      cc.reply_ephemeral("A game is already in progress. Wait for it to finish.").await?;
      cc.unlock_interaction().await;
      return Ok(());
    }

    // Transition Hot → Push immediately to prevent race conditions
    if let Some(fmt) = self.format_mut(fmt_id) {
      if let Some(hot_session) = fmt.sessions.iter_mut().find(|s| s.is_hot()) {
        hot_session.last_action_at = Some(SystemTime::now());
        hot_session.push();
      }
    }

    cc.reply_update_message().await?;

    // Move players to team channels (Push → Live)
    let result = self.push_fmt(fmt_id, cc.ctx, guild_id, &cc.db, Some(cc.manager.clone())).await;

    // Release interaction lock
    cc.unlock_interaction().await;

    match result {
      Ok(_) => {
        info!("Players moved to team channels and game is now live");
        self.queue_dash_update(cc.ctx, guild_id).await;
        Ok(())
      }
      Err(e) => {
        error!("Failed to start match: {e}");
        // Revert Push → Hot so the session doesn't get stuck on "Moving players to team channels..."
        if let Some(fmt) = self.format_mut(fmt_id) {
          if let Some(push_session) = fmt.sessions.iter_mut().find(|s| s.status == crate::models::SessionStatus::Push) {
            push_session.hot();
          }
        }
        self.queue_dash_update(cc.ctx, guild_id).await;
        Ok(())
      }
    }
  }

  /// Handles the end match button - shows winner selection for consistency with runner menu
  async fn dash_end(&mut self, cc: &ComponentContext<'_>, fmt_id: u8) -> Result<()> {
    use crate::handlers::player::is_role_component;
    use crate::models::Role;

    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    let active_session =
      self.format(fmt_id).and_then(|sg| sg.sessions.iter().find(|s| s.status == SessionStatus::Live).or_else(|| sg.sessions.iter().find(|s| s.status == SessionStatus::Hot)));

    if active_session.is_none() {
      cc.reply_ephemeral("No active match to end.").await?;
      return Ok(());
    }

    // If require_score_report is enabled, show the score modal instead of ending directly
    if self.require_score_report {
      return self.dash_report_score(cc).await;
    }

    // Check if user is a runner (for consistency with runner menu)
    if !is_role_component(cc, &Role::Runner).await? {
      cc.reply_ephemeral("Only runners can end matches.").await?;
      return Ok(());
    }

    // Check if someone is already ending this match
    {
      let mgr = cc.manager.lock().await;
      if let Some(submitting_user_id) = mgr.get_active_score_submission(guild_id, self.id, fmt_id) {
        if submitting_user_id != cc.component.user.id {
          let submitting_user_tag = crate::log::get_user_tag(cc.ctx, submitting_user_id, &cc.db).await;
          cc.reply_ephemeral(&format!("{} started ending this match already", submitting_user_tag)).await?;
          return Ok(());
        }
      }
    }

    // Mark this user as ending
    {
      let mut mgr = cc.manager.lock().await;
      mgr.set_active_score_submission(guild_id, self.id, fmt_id, cc.component.user.id);
    }

    // Show winner selection buttons as ephemeral message
    let format_name = self.format(fmt_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Match".to_string());

    let embed = CE::new().title(format!("End {} - Select winner", format_name)).description("Choose the winning team to end the match:").color(0x00AAFF);

    let buttons = vec![CAR::Buttons(vec![
      CB::new(format!("dash_end_blu_{}_{}", self.id, fmt_id)).label("BLU WON").style(BS::Primary),
      CB::new(format!("dash_end_draw_{}_{}", self.id, fmt_id)).label("DRAW").style(BS::Secondary),
      CB::new(format!("dash_end_red_{}_{}", self.id, fmt_id)).label("RED WON").style(BS::Danger),
    ])];

    let response = CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true));
    cc.component.create_response(&cc.ctx.http, response).await?;

    Ok(())
  }

  /// Handles the cancel match button - reverts queue order and clears the match
  async fn dash_cancel(&mut self, cc: &ComponentContext<'_>, fmt_id: u8) -> Result<()> {
    use crate::handlers::player::is_role_component;
    use crate::models::Role;

    // Try to acquire interaction lock to prevent duplicate processing
    let action_key = format!("cancel_match_{}_{}", self.id, fmt_id);
    if !cc.try_lock_interaction(&action_key).await? {
      cc.reply_acknowledge().await?;
      return Ok(());
    }

    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Check if user is a runner
    if !is_role_component(cc, &Role::Runner).await? {
      cc.reply_ephemeral("Only runners can cancel matches.").await?;
      cc.unlock_interaction().await;
      return Ok(());
    }

    // Extract queue_vc before mutable borrow
    let _queue_vc = self.channels.queue_vc;

    // Find the active session
    let active_session = self.format_mut(fmt_id).and_then(|sg| sg.sessions.iter_mut().find(|s| s.status == SessionStatus::Live));

    if active_session.is_none() {
      cc.reply_ephemeral("No active match to cancel.").await?;
      cc.unlock_interaction().await;
      return Ok(());
    }

    let session = active_session.unwrap();

    // Check if match can still be cancelled
    if !session.can_cancel_match() {
      cc.reply_ephemeral("Match has been running for 5+ minutes and cannot be cancelled.").await?;
      cc.unlock_interaction().await;
      return Ok(());
    }

    // Restore the pre-match queue order
    // All players in pre_match_pool are still in this Live session, so restore them all
    if let Some(pre_match_pool) = session.pre_match_pool.take() {
      let player_count = pre_match_pool.len();
      session.pool = pre_match_pool;
      info!("Match cancelled - queue order restored for {} players", player_count);
    } else {
      warn!("No pre-match queue backup found, clearing session");
      session.pool.clear();
    }

    // Reset the session to idle
    session.idle();

    // Clean up any empty Idle sessions (e.g., overflow sessions created during start_match)
    // This prevents multiple Idle sessions from interfering with quota checks
    if let Some(fmt) = self.format_mut(fmt_id) {
      let removed = fmt.cleanup_empty_idle_sessions();
      if removed > 0 {
        info!("Cleaned up {} empty Idle session(s) after match cancellation", removed);
      }
    }

    cc.reply_ephemeral("Match cancelled. Queue order has been restored.").await?;
    self.queue_dash_update(cc.ctx, guild_id).await;

    // Release interaction lock
    cc.unlock_interaction().await;

    Ok(())
  }

  /// Handle end match result button click (dash_end_red/draw/blu_{category_id}_{format_id})
  async fn dash_handle_end_match_result(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
    use crate::handlers::player::is_role_component;
    use crate::models::Role;

    if !is_role_component(cc, &Role::Runner).await? {
      cc.reply_ephemeral("Only runners can end matches.").await?;
      return Ok(());
    }

    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Parse result and IDs from custom_id (format: dash_end_{result}_{category_id}_{format_id})
    let custom_id = &cc.component.data.custom_id;
    let parts: Vec<&str> = custom_id.split('_').collect();
    let result = parts.get(2).unwrap_or(&"");
    let category_id = parts.get(3).and_then(|s| s.parse::<u8>().ok());
    let format_id = parts.get(4).and_then(|s| s.parse::<u8>().ok());

    if !matches!(*result, "red" | "draw" | "blu") || category_id.is_none() || format_id.is_none() {
      cc.reply_ephemeral("Invalid action.").await?;
      return Ok(());
    }

    let category_id = category_id.unwrap();
    let format_id = format_id.unwrap();

    // Try to acquire interaction lock to prevent duplicate processing
    let action_key = format!("end_match_result_{}_{}_{}", category_id, format_id, result);
    if !cc.try_lock_interaction(&action_key).await? {
      cc.reply_acknowledge().await?;
      return Ok(());
    }

    // Guard against double-end: if the session already has score_reported set
    let already_reported = self.format(format_id).and_then(|sg| sg.sessions.iter().find(|s| s.is_active())).map(|s| s.score_reported).unwrap_or(false);

    if already_reported {
      cc.reply_acknowledge().await?;
      cc.unlock_interaction().await;
      return Ok(());
    }

    // Defer the interaction ephemerally to prevent Discord timeout and keep response private
    cc.reply_defer_ephemeral().await?;

    let guild_name_str = guild_name(cc.ctx, guild_id);
    let category_name = self.name.as_deref().unwrap_or("Unknown").to_string();
    let format_name = self.format(format_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Match".to_string());

    let result_text = match *result {
      "red" => "RED won",
      "draw" => "Draw",
      "blu" => "BLU won",
      _ => "Result",
    };

    info!("{} Runner {} ended match with result: {}", log_prefix_category(&guild_name_str, &category_name), cc.component.user.tag(), result_text);

    // Capture session player data for ELO processing before the session is ended
    let session_players: Vec<crate::models::session::SessionPlayer> =
      self.format(format_id).and_then(|sg| sg.sessions.iter().find(|s| s.is_active())).map(|s| s.pool.clone()).unwrap_or_default();

    // Mark score as reported
    if let Some(session) = self.format_mut(format_id).and_then(|sg| sg.sessions.iter_mut().find(|s| s.is_active())) {
      session.score_reported = true;
    }

    // Record match to database before ending
    let active_session =
      self.format(format_id).and_then(|sg| sg.sessions.iter().find(|s| s.status == SessionStatus::Live).or_else(|| sg.sessions.iter().find(|s| s.status == SessionStatus::Hot)));

    let match_id = if let Some(active_session) = active_session {
      let quota = self.format(format_id).map(|sg| sg.quota as usize).unwrap_or(0);
      let (team_red, team_blu) = get_sorted_teams(&active_session.pool, quota);
      let started_at = active_session.started_at;
      let session_id = active_session.team_channels.as_ref().and_then(|tc| tc.session_id.clone());
      Self::record_match_to_database(&cc.db, guild_id, self.id, format_id, started_at, session_id, &team_red, &team_blu).await.ok().flatten()
    } else {
      None
    };

    // Process match result with ELO using shared function
    let elo_changes = match crate::models::session::process_match_result_with_elo(cc.db.clone(), guild_id, category_id, &session_players, result, cc.ctx).await {
      Ok(changes) => changes,
      Err(e) => {
        error!("{} Failed to process match result with ELO: {e}", log_prefix_category(&guild_name_str, &category_name));
        None
      }
    };

    // Update session players' ELO values in memory if changes were applied
    if let Some(changes) = elo_changes {
      if let Some(session) = self.format_mut(format_id).and_then(|sg| sg.sessions.iter_mut().find(|s| s.is_active())) {
        for change in &changes {
          if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == change.user_id) {
            player.player.elo = change.new_elo;
          }
        }
        info!("{} Updated {} players' ELO in session memory", log_prefix_category(&guild_name_str, &category_name), changes.len());
      }
    }

    // Capture match data for chat embed before pull_fmt clears the session
    let queue_chat = self.channels.queue_chat;
    let reporter_tag = cc.component.user.tag();
    let (chat_embed_data, match_duration) = {
      let active_session = self
        .format(format_id)
        .and_then(|sg| sg.sessions.iter().find(|s| s.status == SessionStatus::Live).or_else(|| sg.sessions.iter().find(|s| s.status == SessionStatus::Hot)));
      if let Some(session) = active_session {
        let quota = self.format(format_id).map(|sg| sg.quota as usize).unwrap_or(0);
        let (team_red, team_blu) = get_sorted_teams(&session.pool, quota);
        let duration = session.started_at.and_then(|started| std::time::SystemTime::now().duration_since(started).ok()).map(|d| d.as_secs());
        (Some((team_red, team_blu)), duration)
      } else {
        (None, None)
      }
    };

    // End the match - pass None to avoid deadlock with manager lock held by caller
    match self.pull_fmt(format_id, cc.ctx, guild_id, &cc.db, None).await {
      Ok(_) => {
        info!("{} Match ended with {}", log_prefix_category(&guild_name_str, &category_name), result_text);
        self.queue_dash_update(cc.ctx, guild_id).await;

        let result_color = match *result {
          "red" => crate::RED,
          "blu" => crate::BLUE,
          _ => 0x888888,
        };

        // Post match result embed to queue chat
        if let Some((team_red, team_blu)) = chat_embed_data {
          let hide_elo = cc.db.config.get_bool(guild_id, "hide_elo", false).await.unwrap_or(false);
          let dynamic_elo_active = cc.db.config.get_active_elo(guild_id).await.unwrap_or(false);

          let mut chat_embed = CE::new().title(format!("{} - {}", format_name, result_text)).color(result_color);

          if let Some(secs) = match_duration {
            chat_embed = chat_embed.field("Duration", format!("{}m {}s", secs / 60, secs % 60), true);
          }

          chat_embed = TeamDisplay::new(team_red, team_blu, hide_elo, dynamic_elo_active).add_to_embed(chat_embed, &cc.db, guild_id).await;
          let footer_text = match match_id {
            Some(id) => format!("Logged by {} · Game #{}", reporter_tag, id),
            None => format!("Logged by {}", reporter_tag),
          };
          chat_embed = chat_embed.footer(serenity::all::CreateEmbedFooter::new(footer_text));

          let _ = queue_chat.send_message(&cc.ctx.http, CM::new().embed(chat_embed)).await;
        }

        let description = match match_id {
          Some(id) => format!("**{}** - {} (Game #{})", format_name, result_text, id),
          None => format!("**{}** - {}", format_name, result_text),
        };
        let embed = CE::new().title("Match ended").description(description).color(result_color);

        cc.component.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed).components(vec![])).await?;

        // Release interaction lock
        cc.unlock_interaction().await;
      }
      Err(e) => {
        error!("Failed to end match: {e}");

        let embed = CE::new().title("Failed to end match").description(format!("Error: {}", e)).color(0xFF0000);

        cc.component.edit_response(&cc.ctx.http, serenity::all::EditInteractionResponse::new().embed(embed).components(vec![])).await?;

        // Release interaction lock
        cc.unlock_interaction().await;
      }
    }

    Ok(())
  }

  /// Handles the report score button - shows modal for runners to input scores
  async fn dash_report_score(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
    use crate::handlers::player::is_role_component;
    use crate::models::Role;
    use serenity::all::CreateActionRow as CAR;
    use serenity::all::{CreateInputText, CreateModal, InputTextStyle};

    // Check if user is a runner
    if !is_role_component(cc, &Role::Runner).await? {
      cc.reply_ephemeral("Only runners can report scores.").await?;
      return Ok(());
    }

    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Parse category_id and format_id from button custom_id (format: report_score_CATID_FMTID)
    let custom_id = &cc.component.data.custom_id;
    let parts: Vec<&str> = custom_id.split('_').collect();
    let category_id = parts.get(2).and_then(|s| s.parse::<u8>().ok()).unwrap_or(self.id);
    let format_id = parts.get(3).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);

    // Check if someone is already submitting a score for this match
    {
      let mgr = cc.manager.lock().await;
      if let Some(submitting_user_id) = mgr.get_active_score_submission(guild_id, category_id, format_id) {
        if submitting_user_id != cc.component.user.id {
          let submitting_user_tag = crate::log::get_user_tag(cc.ctx, submitting_user_id, &cc.db).await;
          cc.reply_ephemeral(&format!("{} started reporting this match already", submitting_user_tag)).await?;
          return Ok(());
        }
      }
    }

    // Mark this user as submitting
    {
      let mut mgr = cc.manager.lock().await;
      mgr.set_active_score_submission(guild_id, category_id, format_id, cc.component.user.id);
    }

    // Create modal for score input with category_id and format_id embedded
    let modal = CreateModal::new(format!("report_score_modal_{}_{}", category_id, format_id), "Report match score").components(vec![
      CAR::InputText(
        CreateInputText::new(InputTextStyle::Short, "Blue team score", "blu_score")
          .placeholder(format!("0-{}", crate::models::constants::MAX_MATCH_SCORE))
          .required(true)
          .min_length(1)
          .max_length(1),
      ),
      CAR::InputText(
        CreateInputText::new(InputTextStyle::Short, "Red team score", "red_score")
          .placeholder(format!("0-{}", crate::models::constants::MAX_MATCH_SCORE))
          .required(true)
          .min_length(1)
          .max_length(1),
      ),
    ]);

    cc.component.create_response(&cc.ctx.http, CIR::Modal(modal)).await?;
    Ok(())
  }

  /// Handles button interaction events from the dashboard
  ///
  /// Processes all button interactions in a modular way
  ///
  /// * `cc` - The component context with button information
  ///
  /// Parse format ID from button custom_id suffix (format: action:sg_id).
  /// Returns 0 if no suffix or invalid.
  fn parse_fmt_id(parts: &[&str]) -> u8 {
    parts.get(1).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0)
  }

  pub async fn dash_handle_button_interaction(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
    let custom_id = &cc.component.data.custom_id;

    let parts: Vec<&str> = custom_id.split(':').collect();
    let action = parts[0];

    // Get server and category names for logging - store channel ID before any mut borrows
    let guild_id = cc.component.guild_id.unwrap();
    let _dashboard_channel = self.channels.dashboard;
    let guild_name = guild_name(cc.ctx, guild_id);
    let ctg_nm = self.name.as_deref().unwrap_or("Unknown").to_string();
    let fmt_id = Self::parse_fmt_id(&parts);
    let user_tag = get_user_tag(cc.ctx, cc.component.user.id, &cc.db).await;

    match action {
      "join_queue" => self.dash_join_queue(cc, fmt_id).await,
      "leave_queue" => self.dash_leave_queue(cc, fmt_id).await,
      "ping_players" => {
        let result = self.dash_ping(cc).await;
        match &result {
          Ok(_) => info!("{} {} used Ping", log_prefix_category(&guild_name, &ctg_nm), user_tag),
          Err(e) => warn!("{} {} failed to ping: {}", log_prefix_category(&guild_name, &ctg_nm), user_tag, e),
        }
        result
      }
      "change_expiry" => self.dash_change_expiry(cc, fmt_id).await,
      "set_expiry" => {
        let result = self.dash_set_expiry(cc, parts.get(1).copied()).await;
        match &result {
          Ok(Some(duration_str)) => info!("{} {} changed expiry time to {}", log_prefix_category(&guild_name, &ctg_nm), user_tag, duration_str),
          Ok(None) => {}
          Err(e) => warn!("{} {} failed to change expiry time: {}", log_prefix_category(&guild_name, &ctg_nm), user_tag, e),
        }
        result.map(|_| ())
      }
      "show_settings" => {
        let result = self.dash_show_settings(cc).await;
        match &result {
          Ok(_) => info!("{} {} requested settings", log_prefix_guild(&guild_name), user_tag),
          Err(e) => warn!("{} {} failed to show settings: {}", log_prefix_guild(&guild_name), user_tag, e),
        }
        result
      }
      "show_runner_menu" => {
        let result = crate::handlers::runner_menu::show_runner_menu(cc).await;
        match &result {
          Ok(_) => info!("{} {} requested runner menu", log_prefix_guild(&guild_name), user_tag),
          Err(e) => warn!("{} {} failed to show runner menu: {}", log_prefix_guild(&guild_name), user_tag, e),
        }
        result
      }
      "show_help" => {
        let result = crate::models::dashboard::show_help(cc).await;
        match &result {
          Ok(_) => info!("{} {} requested help", log_prefix_guild(&guild_name), user_tag),
          Err(e) => warn!("{} {} failed to show help: {}", log_prefix_guild(&guild_name), user_tag, e),
        }
        result
      }
      "shuffle_teams" => {
        let result = self.dash_shuffle(cc, fmt_id).await;
        match &result {
          Ok(_) => {
            info!("{} {} used Shuffle", log_prefix_category(&guild_name, &ctg_nm), user_tag);
            self.set_last_action(user_tag.clone(), "shuffled teams");
          }
          Err(e) => warn!("{} {} failed to shuffle teams: {}", log_prefix_category(&guild_name, &ctg_nm), user_tag, e),
        }
        result
      }
      "start_match" => {
        let fmt_name = self.format(fmt_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        let is_hot = self.format(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.is_hot())).unwrap_or(false);
        if !is_hot {
          return Ok(());
        }
        let result = self.dash_start(cc, fmt_id).await;
        match &result {
          Ok(_) => {
            info!("{} {} used Start", crate::log::log_prefix_format(&guild_name, &ctg_nm, &fmt_name), user_tag);
            self.set_last_action(user_tag.clone(), "started the game");
          }
          Err(e) => warn!("{} {} failed to start game: {}", crate::log::log_prefix_format(&guild_name, &ctg_nm, &fmt_name), user_tag, e),
        }
        result
      }
      "end_match" => {
        let fmt_name = self.format(fmt_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        let result = self.dash_end(cc, fmt_id).await;
        match &result {
          Ok(_) => {
            info!("{} {} used End", crate::log::log_prefix_format(&guild_name, &ctg_nm, &fmt_name), user_tag);
            self.set_last_action(user_tag.clone(), "ended the game");
          }
          Err(e) => warn!("{} {} failed to end game: {}", crate::log::log_prefix_format(&guild_name, &ctg_nm, &fmt_name), user_tag, e),
        }
        result
      }
      "cancel_match" => {
        let fmt_name = self.format(fmt_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        let result = self.dash_cancel(cc, fmt_id).await;
        match &result {
          Ok(_) => {
            info!("{} {} used Cancel", crate::log::log_prefix_format(&guild_name, &ctg_nm, &fmt_name), user_tag);
            self.set_last_action(user_tag.clone(), "cancelled the game");
          }
          Err(e) => warn!("{} {} failed to cancel game: {}", crate::log::log_prefix_format(&guild_name, &ctg_nm, &fmt_name), user_tag, e),
        }
        result
      }
      action if action.starts_with("dash_end_") => {
        let result = self.dash_handle_end_match_result(cc).await;
        match &result {
          Ok(_) => info!("{} {} ended match with result", log_prefix_category(&guild_name, &ctg_nm), user_tag),
          Err(e) => warn!("{} {} failed to end match with result: {}", log_prefix_category(&guild_name, &ctg_nm), user_tag, e),
        }
        result
      }
      action if action.starts_with("report_score") => {
        let result = self.dash_report_score(cc).await;
        match &result {
          Ok(_) => info!("{} {} used Report Score", log_prefix_category(&guild_name, &ctg_nm), user_tag),
          Err(e) => warn!("{} {} failed to report score: {}", log_prefix_category(&guild_name, &ctg_nm), user_tag, e),
        }
        result
      }
      // Handle user prefs navigation buttons
      action if action.starts_with("user_prefs_") => {
        use crate::handlers::settings::user_prefs_system::{get_user_prefs_menu_system, get_user_prefs_navigation_info, UserPrefsPage};
        let user_id = cc.component.user.id;

        if let Some(target_page) = get_user_prefs_navigation_info(action) {
          // Special handling for PingSettings page - needs guild context and database access
          if target_page == UserPrefsPage::PingSettings {
            use crate::handlers::settings::build_ping_settings_response;
            let response = build_ping_settings_response(user_id, cc.component.guild_id, &cc.db).await?;
            cc.component.create_response(&cc.ctx.http, response).await?;
            return Ok(());
          }
          
          let system = get_user_prefs_menu_system();
          let settings = match cc.db.players.get_prefs(user_id).await {
            Ok(s) => s,
            Err(e) => {
              cc.reply_ephemeral(&format!("Failed to load settings: {}", e)).await?;
              return Ok(());
            }
          };

          if let Some(response) = system.build_response(target_page, &settings) {
            cc.component.create_response(&cc.ctx.http, response).await?;
          }
          Ok(())
        } else {
          cc.reply_ephemeral(&format!("Unknown button action: {action}")).await?;
          Ok(())
        }
      }
      _ => {
        cc.reply_ephemeral(&format!("Unknown button action: {action}")).await?;
        Ok(())
      }
    }
  }

  /// Show expiry time options
  async fn dash_change_expiry(&mut self, cc: &ComponentContext<'_>, _fmt_id: u8) -> Result<()> {
    use serenity::all::{ButtonStyle as BS, CreateButton as CB};

    // Check if user is in queue (across all formats)
    let user_id = cc.component.user.id;
    let is_in_queue = self.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id)));

    if !is_in_queue {
      cc.reply_ephemeral("You must be in the queue to change your expiry time.").await?;
      return Ok(());
    }

    // Create buttons for time options: 30m, 1h, 2h, 3h, 4h
    let time_buttons = vec![
      CB::new("set_expiry:30m").label("30 minutes").style(BS::Secondary),
      CB::new("set_expiry:1h").label("1 hour").style(BS::Secondary),
      CB::new("set_expiry:2h").label("2 hours").style(BS::Secondary),
      CB::new("set_expiry:3h").label("3 hours").style(BS::Secondary),
      CB::new("set_expiry:4h").label("4 hours").style(BS::Secondary),
    ];

    let response = CIR::Message(CIRM::new().content("Select your expiry time for this queue instance:").components(vec![CAR::Buttons(time_buttons)]).ephemeral(true));

    cc.component.create_response(&cc.ctx.http, response).await?;
    Ok(())
  }

  /// Set expiry duration for user in current queue
  async fn dash_set_expiry(&mut self, cc: &ComponentContext<'_>, duration_str: Option<&str>) -> Result<Option<String>> {
    let user_id = cc.component.user.id;

    // Parse duration string
    let duration = match duration_str {
      Some("30m") => 30,
      Some("1h") => 60,
      Some("2h") => 120,
      Some("3h") => 180,
      Some("4h") => 240,
      _ => {
        cc.reply_ephemeral("Invalid expiry duration.").await?;
        return Ok(None);
      }
    };

    // Find and update the player's expiry duration in any session across all formats
    let mut found_format_id = None;
    let mut found_player = None;
    'outer: for sg in self.formats.iter_mut() {
      for session in sg.sessions.iter_mut() {
        if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
          player.queue_expiration = duration;
          found_format_id = Some(sg.id);
          found_player = Some(player.player.clone());
          break 'outer;
        }
      }
    }

    if found_format_id.is_none() {
      cc.reply_ephemeral("You are not in the queue.").await?;
      return Ok(None);
    }

    // Cancel old timeout and schedule new one with updated duration
    if let Some(guild_id) = cc.component.guild_id {
      if let (Some(fmt_id), Some(player)) = (found_format_id, found_player) {
        self.cancel_player_rejoin_expiration(cc.ctx, guild_id, fmt_id, user_id).await;
        self.set_player_rejoin_expiration(cc.ctx, guild_id, player, duration).await;
      }
    }

    // Delete the ephemeral message by updating it
    let label = duration_str.unwrap_or("unknown");
    let response = CIR::UpdateMessage(
      CIRM::new().content(format!("Expiry time set to {} for this queue instance.", label)).components(vec![]), // Remove buttons
    );
    cc.component.create_response(&cc.ctx.http, response).await?;

    // Update the dashboard
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;

    Ok(Some(label.to_string()))
  }

  /// Show user settings as ephemeral embed in dashboard channel
  async fn dash_show_settings(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
    // Defer with ephemeral response before async work
    let response = CIR::Defer(CIRM::new().ephemeral(true));
    cc.component.create_response(&cc.ctx.http, response).await?;

    use crate::handlers::settings::user_prefs_system::{get_user_prefs_menu_system, UserPrefsPage};

    let user_id = cc.component.user.id;

    // Get user's settings
    let settings = match cc.db.players.get_prefs(user_id).await {
      Ok(s) => s,
      Err(e) => {
        use serenity::all::CreateInteractionResponseFollowup as CIRF;
        let followup = CIRF::new().content(format!("Failed to load settings: {}", e)).ephemeral(true);
        cc.component.create_followup(&cc.ctx.http, followup).await?;
        return Ok(());
      }
    };

    // Build settings embed with interactive buttons using the new menu system
    let system = get_user_prefs_menu_system();
    let embed = system.build_embed(UserPrefsPage::Main, &settings);
    let components = system.build_components(UserPrefsPage::Main, &settings);

    // Use followup since we deferred
    use serenity::all::CreateInteractionResponseFollowup as CIRF;
    let followup = CIRF::new()
      .embed(embed.unwrap_or_default())
      .components(components.unwrap_or_default())
      .ephemeral(true);
    cc.component.create_followup(&cc.ctx.http, followup).await?;

    Ok(())
  }

  pub async fn lock_button(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
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

  pub async fn unlock_button(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
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

  /// Handle the Ping button - shows format selection ephemeral
  /// Runners: 15 minute cooldown, Regular players: 30 minute cooldown
  async fn dash_ping(&mut self, cc: &ComponentContext<'_>) -> Result<()> {
    use crate::handlers::player::is_role_component;
    use crate::models::Role;
    use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage};
    use std::time::{Duration, SystemTime};

    let _user_id = cc.component.user.id;
    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

    // Check if user is a runner (for cooldown duration)
    let is_runner = is_role_component(cc, &Role::Runner).await.unwrap_or(false);

    // Check if regular users are allowed to ping
    let ping_users_enabled = cc.db.config.get_ping_users_enabled(guild_id).await.unwrap_or(true);
    if !is_runner && !ping_users_enabled {
      cc.reply_ephemeral("Only runners can use the ping button.").await?;
      return Ok(());
    }

    // Get cooldown durations from config
    let user_cooldown_mins = cc.db.config.get_ping_user_cooldown(guild_id).await.unwrap_or(30);
    let runner_cooldown_mins = cc.db.config.get_ping_runner_cooldown(guild_id).await.unwrap_or(15);

    let cooldown_duration = if is_runner {
      Duration::from_secs(runner_cooldown_mins as u64 * 60)
    } else {
      Duration::from_secs(user_cooldown_mins as u64 * 60)
    };

    // Check cooldown
    let now = SystemTime::now();
    if let Some(last_ping) = self.last_ping_time {
      if let Ok(elapsed) = now.duration_since(last_ping) {
        if elapsed < cooldown_duration {
          let remaining = cooldown_duration - elapsed;
          let mins = remaining.as_secs() / 60;
          let secs = remaining.as_secs() % 60;
          let msg = format!("Ping on cooldown. Try again in {}m {}s", mins, secs);
          cc.reply_ephemeral(&msg).await?;
          return Ok(());
        }
      }
    }

    // Check bot permissions for @here
    let ping_channel = if self.channels.ping_channel.get() > 1 { self.channels.ping_channel } else { self.channels.dashboard };
    let can_mention = if let Ok(channel) = cc.ctx.http.get_channel(ping_channel).await {
      if let Some(guild_channel) = channel.guild() {
        let bot_id = cc.ctx.cache.current_user().id;
        if let Ok(permissions) = guild_channel.permissions_for_user(&cc.ctx.cache, bot_id) {
          permissions.mention_everyone()
        } else {
          false
        }
      } else {
        false
      }
    } else {
      false
    };

    if !can_mention {
      cc.reply_ephemeral("Bot doesn't have permission to ping @here in the ping channel.").await?;
      return Ok(());
    }

    // Build format selection buttons
    let mut buttons: Vec<CreateButton> = Vec::new();
    let mut all_full = true;

    for format in &self.formats {
      let players_in_queue: usize = format.sessions.iter().filter(|s| s.is_idle() || s.is_hot()).map(|s| s.pool.len()).sum();
      let players_needed = (format.quota as usize).saturating_sub(players_in_queue);

      if players_needed > 0 {
        all_full = false;
        let label = format!("{} (+{})", format.name, players_needed);
        buttons.push(CreateButton::new(format!("ping_format_{}_{}", self.id, format.id)).label(label).style(ButtonStyle::Primary));
      }
    }

    if all_full {
      cc.reply_ephemeral("All queues are already full!").await?;
      return Ok(());
    }

    let embed = CE::new().title("Ping for players").description("Select which format to ping for:").color(0x00AAFF);

    let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).components(vec![CreateActionRow::Buttons(buttons)]).ephemeral(true));

    cc.component.create_response(&cc.ctx.http, response).await?;

    Ok(())
  }

  /// Handle ping format selection button (ping_format_{category_id}_{format_id})
  pub async fn handle_ping_format(&mut self, cc: &ComponentContext<'_>, format_id: u8) -> Result<()> {
    use crate::handlers::player::is_role_component;
    use crate::models::Role;
    use serenity::all::{CreateInteractionResponse, CreateInteractionResponseMessage};
    use std::time::{Duration, SystemTime};

    let user_id = cc.component.user.id;
    let user_tag = cc.component.user.tag();
    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

    // Check if user is a runner (for cooldown duration)
    let is_runner = is_role_component(cc, &Role::Runner).await.unwrap_or(false);

    // Get cooldown durations from config
    let user_cooldown_mins = cc.db.config.get_ping_user_cooldown(guild_id).await.unwrap_or(30);
    let runner_cooldown_mins = cc.db.config.get_ping_runner_cooldown(guild_id).await.unwrap_or(15);

    let cooldown_duration = if is_runner {
      Duration::from_secs(runner_cooldown_mins as u64 * 60)
    } else {
      Duration::from_secs(user_cooldown_mins as u64 * 60)
    };

    // Check cooldown again (in case they waited)
    let now = SystemTime::now();
    if let Some(last_ping) = self.last_ping_time {
      if let Ok(elapsed) = now.duration_since(last_ping) {
        if elapsed < cooldown_duration {
          let remaining = cooldown_duration - elapsed;
          let mins = remaining.as_secs() / 60;
          let secs = remaining.as_secs() % 60;

          let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new().content(format!("Ping on cooldown. Try again in {}m {}s", mins, secs)).embeds(vec![]).components(vec![]),
          );
          cc.component.create_response(&cc.ctx.http, response).await?;
          return Ok(());
        }
      }
    }

    // Find the format and calculate players needed
    let format = self.formats.iter().find(|f| f.id == format_id).ok_or_else(|| anyhow::anyhow!("Format not found"))?;

    let players_in_queue: usize = format.sessions.iter().filter(|s| s.is_idle() || s.is_hot()).map(|s| s.pool.len()).sum();
    let players_needed = (format.quota as usize).saturating_sub(players_in_queue);

    if players_needed == 0 {
      let response = CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new().content("Queue is already full!").embeds(vec![]).components(vec![]));
      cc.component.create_response(&cc.ctx.http, response).await?;
      return Ok(());
    }

    // Update cooldown
    self.last_ping_time = Some(now);

    // Send the ping message to ping channel (or dashboard if not set)
    let ping_channel = if self.channels.ping_channel.get() > 1 { self.channels.ping_channel } else { self.channels.dashboard };

    // Use configured ping role from server config if set, otherwise use @here
    let ping_mention = if let Some(ref role_id) = cc.db.config.get_ping_role_id(guild_id).await? {
      if !role_id.trim().is_empty() {
        format!("<@&{}>", role_id.trim())
      } else {
        "@here".to_string()
      }
    } else {
      "@here".to_string()
    };

    let content = format!("{} +{} for {}\nPing by <@{}>", ping_mention, players_needed, format.name, user_id.get());

    if let Ok(sent) = ping_channel.send_message(&cc.ctx.http, CM::new().content(content)).await {
      let message_id = sent.id;
      let guild_name = guild_name(cc.ctx, guild_id);
      let category_name = self.name.as_deref().unwrap_or("Unknown").to_string();
      let log_prefix = log_prefix_category(&guild_name, &category_name);
      
      info!("{} Sent ping by {} for {} (msg_id: {})", log_prefix, user_tag, format.name, message_id);

      // Delete the message after 15 minutes
      let http = cc.ctx.http.clone();
      let channel_id = ping_channel;
      tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(15 * 60)).await;
        match channel_id.delete_message(&http, message_id).await {
          Ok(_) => {
            debug!("{} Ping message auto-deleted after 15 minutes (msg_id: {})", log_prefix, message_id);
          }
          Err(e) => {
            debug!("{} Ping message not found (msg_id: {}): {}", log_prefix, message_id, e);
          }
        }
      });

      // Confirm in ephemeral
      let response = CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new().content(format!("Pinged for {} (+{})", format.name, players_needed)).embeds(vec![]).components(vec![]),
      );
      cc.component.create_response(&cc.ctx.http, response).await?;
    } else {
      let response =
        CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new().content("Failed to ping. Check bot permissions.").embeds(vec![]).components(vec![]));
      cc.component.create_response(&cc.ctx.http, response).await?;
    }

    Ok(())
  }
}

//
// Dashboard update queue
//

/// Request to update a specific category's dashboard
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DashboardUpdateRequest {
  pub guild_id: GI,
  pub category_id: u8,
}

/// Dashboard update queue that batches updates to reduce API calls
pub struct DashboardUpdateQueue {
  sender: mpsc::UnboundedSender<DashboardUpdateRequest>,
}

impl Clone for DashboardUpdateQueue {
  fn clone(&self) -> Self {
    Self { sender: self.sender.clone() }
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

  /// Request a dashboard update for a specific category
  pub fn request_update(&self, guild_id: GI, category_id: u8) {
    let request = DashboardUpdateRequest { guild_id, category_id };
    if let Err(e) = self.sender.send(request) {
      warn!("Failed to queue dashboard update: {e}");
    }
  }

  /// Request dashboard updates for all categories across all servers
  pub fn request_update_all(&self, manager: &crate::models::Manager) {
    for srv in &manager.qguilds {
      for group in &srv.categories {
        self.request_update(srv.id, group.id);
      }
    }
  }

  /// Request dashboard updates for all categories in a specific guild
  pub fn request_update_guild(&self, manager: &crate::models::Manager, guild_id: GI) {
    if let Some(srv) = manager.qguilds.iter().find(|s| s.id == guild_id) {
      for group in &srv.categories {
        self.request_update(srv.id, group.id);
      }
    }
  }

  /// Request dashboard updates for all categories without needing the manager lock.
  /// Sends a sentinel that the batch processor expands once it acquires the lock.
  pub fn request_update_all_deferred(&self) {
    let request = DashboardUpdateRequest {
      guild_id: GI::new(1),
      category_id: u8::MAX, // sentinel
    };
    if let Err(e) = self.sender.send(request) {
      warn!("Failed to queue dashboard update-all: {e}");
    }
  }

  /// Background task that batches and processes dashboard updates
  ///
  /// This uses a HashSet to automatically deduplicate update requests for the same category.
  /// Since dashboards show current state, only the latest update matters - all previous
  /// requests for the same category are redundant and automatically discarded.
  async fn batch_processor(
    mut receiver: mpsc::UnboundedReceiver<DashboardUpdateRequest>,
    ctx: Context,
    manager: Arc<tokio::sync::Mutex<crate::models::Manager>>,
    database: Arc<crate::Database>,
  ) {
    let batch_window = Duration::from_millis(200); // Wait 200ms to batch updates
                                                   // HashSet automatically deduplicates - if 10 updates come in for the same category,
                                                   // we only keep one entry and process it once with the current state
    let mut pending_updates: HashSet<DashboardUpdateRequest> = HashSet::new();

    loop {
      // Wait for the first update request
      match receiver.recv().await {
        Some(request) => {
          let mut update_all = request.category_id == u8::MAX;
          if !update_all {
            pending_updates.insert(request);
          }

          // Now wait for the batch window, collecting more updates
          let deadline = tokio::time::Instant::now() + batch_window;

          loop {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
              Ok(Some(request)) => {
                if request.category_id == u8::MAX {
                  update_all = true;
                } else {
                  pending_updates.insert(request);
                }
              }
              Ok(None) => {
                // Channel closed, process remaining and exit
                if update_all {
                  Self::expand_update_all(&mut pending_updates, &manager).await;
                }
                Self::process_batch(&pending_updates, &ctx, manager.clone(), database.clone()).await;
                return;
              }
              Err(_) => {
                // Timeout - batch window expired, process the batch
                break;
              }
            }
          }

          // Expand update-all sentinel into concrete category requests
          if update_all {
            Self::expand_update_all(&mut pending_updates, &manager).await;
          }

          // Process the batched updates
          if !pending_updates.is_empty() {
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

  /// Expand an "update all" sentinel into concrete requests for every category.
  async fn expand_update_all(pending: &mut HashSet<DashboardUpdateRequest>, manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>) {
    let mgr = manager.lock().await;
    for srv in &mgr.qguilds {
      for grp in &srv.categories {
        pending.insert(DashboardUpdateRequest { guild_id: srv.id, category_id: grp.id });
      }
    }
  }

  /// Process a batch of dashboard updates
  async fn process_batch(updates: &HashSet<DashboardUpdateRequest>, ctx: &Context, manager: Arc<tokio::sync::Mutex<crate::models::Manager>>, database: Arc<crate::Database>) {
    // Process updates concurrently (Discord allows multiple requests in parallel)
    let mut tasks = Vec::new();

    for update in updates {
      let ctx = ctx.clone();
      let manager = manager.clone();
      let database = database.clone();
      let guild_id = update.guild_id;
      let category_id = update.category_id;

      // Spawn a task for each dashboard update
      let task = tokio::spawn(async move {
        //
        // Acquire lock briefly to get CURRENT dashboard data
        // This ensures we always show the latest state, regardless of how many
        // update requests were queued - they all get collapsed into this one update
        let (channel_id, dashboard_channel_id, message_id, embed, buttons, guild_name, _pool_size) = {
          let mut manager_lock = manager.lock().await;

          // Collect players in active sessions across all servers
          // Maps UserId -> (GuildId, format_name) for "in-game" status display
          let mut in_game_players: HashMap<UI, (GI, String)> = HashMap::new();
          for srv in &manager_lock.qguilds {
            for grp in &srv.categories {
              for sg in &grp.formats {
                let has_active = sg.sessions.iter().any(|s| s.is_active());
                if !has_active {
                  continue;
                }
                for session in &sg.sessions {
                  if !session.is_active() {
                    continue;
                  }
                  for sp in &session.pool {
                    in_game_players.insert(sp.player.user_id, (srv.id, sg.name.clone()));
                  }
                }
              }
            }
          }

          let server = match manager_lock.get_qguild(guild_id) {
            Ok(s) => s,
            Err(e) => {
              warn!("Failed to get server for dashboard update: {e}");
              return;
            }
          };

          let guild_name = server.name.clone();

          let category = match server.categories.iter_mut().find(|g| g.id == category_id) {
            Some(g) => g,
            None => {
              warn!("[{}] Failed to find category {} for dashboard update", guild_name, category_id);
              return;
            }
          };

          // Log current session state
          let pool_size = category.formats[0].sessions.first().map(|s| s.pool.len()).unwrap_or(0);
          //

          // NOTE: We don't reload_player_ranks here for performance.
          // That expensive operation is called only before team generation (hot/shuffle).
          // Dashboard updates happen frequently (every button press, VC change) and should be fast.

          // Sync VC status with actual Discord state when there are hot sessions,
          // so the missing players list is accurate even if voice state events were missed.
          let has_hot_session = category.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.is_hot()));
          if has_hot_session {
            category.verify_vc(&ctx, guild_id).await;
          }

          // Get dashboard message info
          let channel_id = category.channels.dashboard;
          let dashboard_channel_id = channel_id.get();
          let message_id = category.dashboard_msg;

          // Generate dashboard content
          let (embed, buttons) = match category.build_dashboard_content(&database, guild_id, &in_game_players).await {
            Ok(content) => content,
            Err(e) => {
              warn!("[{}] Failed to build dashboard content for category {}: {}", guild_name, category_id, e);
              return;
            }
          };

          (channel_id, dashboard_channel_id, message_id, embed, buttons, guild_name, pool_size)
        }; // Release lock here

        // Update the dashboard message WITHOUT holding any locks
        use serenity::all::EditMessage;
        let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{channel_id}"));
        match channel_id.edit_message(&ctx.http, message_id, EditMessage::new().embed(embed.clone()).components(buttons.clone())).await {
          Ok(_) => {
            // Dashboard updated successfully
          }
          Err(e) => {
            // Check if message was deleted (404 error)
            if e.to_string().contains("404") || e.to_string().contains("Unknown message") {
              warn!("[{}] Dashboard message was deleted in #{}, recreating...", guild_name, channel_name);

              // Recreate the dashboard message
              use serenity::all::CreateMessage;
              match channel_id.send_message(&ctx.http, CreateMessage::new().embed(embed).components(buttons)).await {
                Ok(new_msg) => {
                  info!("[{}] Recreated dashboard in #{}", guild_name, channel_name);

                  // Update the stored message ID in memory
                  let mut manager_lock = manager.lock().await;
                  if let Ok(server) = manager_lock.get_qguild(guild_id) {
                    if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
                      category.dashboard_msg = new_msg.id;
                    }
                  }
                  drop(manager_lock);

                  // Persist to database
                  if let Err(e) = database.categories.update_dashboard_msg(guild_id, dashboard_channel_id, new_msg.id.get()).await {
                    warn!("Failed to update dashboard message ID in database: {e}");
                  }
                }
                Err(create_err) => {
                  warn!("[{}] Failed to recreate dashboard in #{}: {}", guild_name, channel_name, create_err);
                }
              }
            } else {
              let hint = if e.to_string().contains("Missing access") { " (check that the bot has View Channel and Send Messages permissions on this channel)" } else { "" };
              warn!("[{}] Failed to update dashboard in #{}:{}{}", guild_name, channel_name, e, hint);
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

/// Show help information about how the queue system works
pub async fn show_help(cc: &ComponentContext<'_>) -> Result<()> {
  // Defer with ephemeral response before async work
  let response = CIR::Defer(CIRM::new().ephemeral(true));
  cc.component.create_response(&cc.ctx.http, response).await?;

  // Get user's VC auto-join setting to conditionally show that info
  let user_id = cc.component.user.id;
  let vc_auto_join_enabled = cc.db.players.get_prefs(user_id).await.map(|prefs| prefs.vc_auto_join).unwrap_or(false);

  let vc_auto_join_text = if vc_auto_join_enabled { " You can also just hop into the queue voice channel and you'll be added automatically." } else { "" };

  let description = format!(
    "## Joining and leaving the queue\n\
     When you want to play, just click the **Join** button on the dashboard and pick the format you want to play.{}\n\
     This will add you to a queue for a set period of time and you'll retain this spot in the queue until the match starts or you leave. \
     To leave the queue, simply click the **Leave** button on the dashboard.\n\
     ## How does the queue work?\n\
     Think of it as a line to go on a rollercoaster at a carnival. The quota is the number of seats on the cart - it must always be full before the ride can start. \
     Once enough people are in line to fill all the seats, those first people board the cart and the ride begins. \
     If there are more people in queue than the cart can fit, they stay in line and wait for the next ride.\n\
     We can also have multiple formats (like 6v6 and 4v4), each with its own queue running independently. \
     Even within a single format, if there are enough players, multiple matches can run at the same time - the bot will create more team channels and split players across them.\n\
     After a match ends, those players return to the queue and the next group of players gets selected for the next match. \
     Selection is mostly first-come-first-served, but the system ensures everyone gets a fair chance to play.\n\
     ## When do teams get made?\n\
     Once enough players join to fill a match, the bot generates balanced teams and shows a preview of it on the dashboard.\n\
     ## Where do we play?\n\
     The game starts when a runner presses the **Start** button. The bot then creates team voice channels and moves everyone to their team's channel. \
     After the game ends, the runner ends the match via **End** and you'll be moved back to the queue channel.\n\
     **## What happens during a match?\n\
     The dashboard updates live so you can always see who's in queue and what's happening. \
     If something goes wrong, runners (trusted users who can manage the queue), admins or xCape can step in to help fix issues.\n\n\
     **That's it!** The bot handles most things so you can focus on playing.\n\
     **Questions or feedback?** Contact <@257898548773912576>",
    vc_auto_join_text
  );

  let embed = CE::new().title("How does qBot work?").description(description).color(crate::CYAN);

  // Use followup since we deferred
  use serenity::all::CreateInteractionResponseFollowup as CIRF;
  let followup = CIRF::new().embed(embed).ephemeral(true);
  cc.component.create_followup(&cc.ctx.http, followup).await?;

  Ok(())
}
