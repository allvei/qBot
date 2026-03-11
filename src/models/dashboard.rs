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
use tracing::{error, info, warn};

use crate::models::{Category, ComponentContext as CC, DashboardQueueKey, Session, SessionPlayer, SessionStatus};

// Helper methods to reduce code duplication

/// Format team display for embed fields
async fn format_team_display(embed: serenity::all::CreateEmbed, pool: &[crate::models::SessionPlayer], label: &str) -> serenity::all::CreateEmbed {
  if pool.is_empty() {
    return embed;
  }

  let formatted_players: Vec<_> = pool.iter().map(|p| format!("‹**{}**› <@{}>", p.player.elo, p.player.user_id)).collect();

  embed.field(format!("{} ({})", label, pool.len()), formatted_players.join("\n"), false)
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
}

impl TeamDisplay {
  /// Create new team display from sorted teams
  fn new(red: Vec<crate::models::SessionPlayer>, blu: Vec<crate::models::SessionPlayer>) -> Self {
    Self { red, blu }
  }

  /// Build team field headers with average ELO
  fn build_headers(&self) -> (String, String) {
    let red_header = format!("‹**{}**› 🔴 RED", get_avg_elo(&self.red));
    let blu_header = format!("‹**{}**› 🔵 BLU", get_avg_elo(&self.blu));
    (red_header, blu_header)
  }

  /// Add both team fields to an embed in a single call
  /// Blue team is shown first, then red team
  async fn add_to_embed(self, embed: CE, db: &crate::Database, guild_id: GI) -> CE {
    let (red_header, blu_header) = self.build_headers();
    embed.field(blu_header, format_team_field(&self.blu, db, guild_id).await, true).field(red_header, format_team_field(&self.red, db, guild_id).await, true)
  }
}

/// Helper function to calculate average ELO for a team
fn get_avg_elo(team: &[crate::models::SessionPlayer]) -> f64 {
  (team.iter().map(|p| p.player.elo as f64).sum::<f64>() / team.len() as f64 * 10.0).round() / 10.0
}

/// Helper function to format team players as a string for embed fields
async fn format_team_field(team: &[crate::models::SessionPlayer], _db: &crate::Database, _guild_id: GI) -> String {
  let mut lines = Vec::new();
  for player in team {
    lines.push(format!("‹**{}**› <@{}>", player.player.elo, player.player.user_id));
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
  pub fn parse(custom_id: &str) -> Self {
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
    matches!(self, Self::DashboardJoin | Self::DashboardLeave | Self::DashboardShuffle | Self::DashboardStart | Self::DashboardEnd)
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
      let mut row = vec![CB::new(format!("join_queue{fmt_suffix}")).label(&join_label).style(BS::Success)];
      if has_queued_players {
        row.push(CB::new(format!("leave_queue{fmt_suffix}")).label("Leave").style(BS::Danger));
        row.push(CB::new(format!("change_expiry{fmt_suffix}")).label("Edit timeout").style(BS::Secondary));
      }
      if is_hot {
        row.push(CB::new(format!("start_match{fmt_suffix}")).label("Start").style(BS::Success));
        row.push(CB::new(format!("shuffle_teams{fmt_suffix}")).label("Shuffle").style(BS::Secondary));
      } else if is_live {
        row.push(CB::new(format!("start_match{fmt_suffix}")).label("End").style(BS::Danger));
      }
      buttons.push(CAR::Buttons(row));
    }

    // Last row: Preferences, Runner Menu, and Help
    buttons.push(Self::create_dashboard_footer_buttons());

    Ok(buttons)
  }

  fn gen_button(config: (&'static str, &'static str, BS, bool)) -> CB {
    let (action, label, style, enabled) = config;
    CB::new(action).label(label).style(style).disabled(!enabled)
  }

  /// Create the standard footer buttons for the dashboard
  /// Includes Preferences, Runner Menu, and Help buttons
  fn create_dashboard_footer_buttons() -> CAR {
    CAR::Buttons(vec![
      Self::gen_button(("show_settings", "Preferences", BS::Secondary, true)),
      Self::gen_button(("show_runner_menu", "Runner menu", BS::Secondary, true)),
      Self::gen_button(("show_help", "Help", BS::Secondary, true)),
    ])
  }

  /// Record a match to the database with all player information
  async fn record_match_to_database(
      db: &crate::Database,
      guild_id: GI,
      category_id: u8,
      format_id: u8,
      session: &Session,
      team_red: &[SessionPlayer],
      team_blu: &[SessionPlayer],
  ) -> Result<()> {
      use crate::db::repo::MatchPlayerInsert;
      use std::time::SystemTime;

      // Only record if match was actually started (has started_at timestamp)
      if let Some(started_at) = session.started_at {
          let ended_at = SystemTime::now();
          let duration_secs = ended_at
              .duration_since(started_at)
              .map(|d| d.as_secs())
              .unwrap_or(0);
          
          // Insert match record
          match db.matches.insert_match(
              guild_id,
              category_id as i64,
              format_id as i64,
              session.team_channels.as_ref().and_then(|tc| tc.session_id.clone()),
              started_at,
              ended_at,
              duration_secs,
          ).await {
              Ok(match_id) => {
                  // Insert match players
                  let mut players = Vec::new();
                  
                  for player in team_red {
                      players.push(MatchPlayerInsert {
                          user_id: player.player.user_id,
                          team: "red".to_string(),
                          elo_before: player.player.elo as i64,
                      });
                  }
                  
                  for player in team_blu {
                      players.push(MatchPlayerInsert {
                          user_id: player.player.user_id,
                          team: "blu".to_string(),
                          elo_before: player.player.elo as i64,
                      });
                  }
                  
                  if let Err(e) = db.matches.insert_match_players(match_id, players).await {
                      error!("Failed to insert match players: {e}");
                  }
              }
              Err(e) => {
                  error!("Failed to insert match record: {e}");
              }
          }
      }
      
      Ok(())
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
    let timeout_seconds = self.timeout as u64;
    let post_game_timeout = db.config.get_post_game_timeout(guild_id).await.unwrap_or(120) as u64;
    let has_multiple = self.formats.len() > 1;

    let mut embed = CE::new().title(self.display_name());

    // Single loop: each format renders all its content together
    let fmt_count = self.formats.len();
    for (fmt_i, sg) in self.formats.iter().enumerate() {
      let quota = sg.quota as usize;
      let inactives: Vec<_> = sg.sessions.iter().filter(|s| !s.is_active()).collect();
      let actives: Vec<_> = sg.sessions.iter().filter(|s| s.is_active()).collect();

      let fmt_label = if has_multiple { format!("{} queue", sg.name) } else { "Queue".to_string() };

      // --- Active games (Hot/Push/Live/Pull) ---
      if !actives.is_empty() {
        // Get timestamp from first active session
        let started_time = actives
          .first()
          .and_then(|session| session.started_at)
          .and_then(|started_at| crate::timestamp_from_system_time(&started_at, crate::Style::Relative))
          .unwrap_or_else(|| "recently".to_string());

        // Determine status for field title
        let status_text = match actives.first().unwrap().status {
          SessionStatus::Hot => "Ready to start",
          SessionStatus::Push => "Moving players to team channels...",
          SessionStatus::Live => &format!("Started {}", started_time),
          SessionStatus::Pull => "Moving players back to queue...",
          _ => "",
        };

        let mut match_info = String::new();
        for session in &actives {
          if session.is_hot() {
            let players_never_joined: Vec<_> = session.pool.iter().take(quota).filter(|p| !p.in_queue_vc).collect();

            if !players_never_joined.is_empty() {
              // Use match_ended_at if available (post-game scenario), otherwise ready_at
              let base_time = session.match_ended_at.or(session.ready_at);
              if let Some(base_time) = base_time {
                // Use appropriate timeout based on scenario
                let deadline_timeout = if session.match_ended_at.is_some() { post_game_timeout } else { timeout_seconds };

                if let Ok(d) = base_time.duration_since(SystemTime::UNIX_EPOCH) {
                  let deadline = d.as_secs() + deadline_timeout;
                  match_info.push_str(&format!("Join deadline: {}\n", crate::timestamp_from_unix(deadline as i64, crate::Style::Relative)));
                  match_info.push_str("Missing players will be removed.\n\n");
                }
              }
              match_info.push_str("**Missing players:**\n");
              for player in &players_never_joined {
                match_info.push_str(&format!("  • ‹**{}**› <@{}>\n", player.player.elo, player.player.user_id));
              }
            } else {
              match_info.push_str("All players ready");
            }
          }
          // Note: Push/Live/Pull status is now in the field title, no need to add here
        }

        embed = embed.field(format!("{fmt_label} - {}", status_text), match_info, false);

        // Team fields for active sessions
        for session in &actives {
          if session.pool.len() >= quota {
            let (team_red, team_blu) = get_sorted_teams(&session.pool, quota);
            embed = TeamDisplay::new(team_red, team_blu).add_to_embed(embed, db, guild_id).await;
          }
        }

        // Show overflow players for idle session (when there's an active game)
        if let Some(next_session) = inactives.first() {
          if !next_session.pool.is_empty() {
            embed = format_team_display(embed, &next_session.pool, "Waiting for next game").await;
          }
        }
      }
      // --- Idle/Hot session ---
      else if let Some(current_session) = inactives.first() {
        let queue_players = current_session.pool.len();

        if current_session.is_hot() {
          // Validate Hot session has correct player count
          if queue_players < quota {
            warn!(
              "Dashboard validation: Hot session has {} players but quota is {}. This indicates a bug - session should have been transitioned back to Idle.",
              queue_players, quota
            );
          }
          
          // Hot session - show missing players and teams
          let mut hot_info = String::new();
          let players_never_joined: Vec<_> = current_session.pool.iter().take(quota).filter(|p| !p.in_queue_vc).collect();

          if !players_never_joined.is_empty() {
            // Use match_ended_at if available (post-game scenario), otherwise ready_at
            let base_time = current_session.match_ended_at.or(current_session.ready_at);
            if let Some(base_time) = base_time {
              // Use appropriate timeout based on scenario
              let deadline_timeout = if current_session.match_ended_at.is_some() { post_game_timeout } else { timeout_seconds };

              if let Ok(d) = base_time.duration_since(SystemTime::UNIX_EPOCH) {
                let deadline = d.as_secs() + deadline_timeout;
                hot_info.push_str(&format!("Join deadline: {}\n", crate::timestamp_from_unix(deadline as i64, crate::Style::Relative)));
                hot_info.push_str("Missing players will be removed.\n\n");
              }
            }
            hot_info.push_str("**Missing players:**\n");
            for player in &players_never_joined {
              hot_info.push_str(&format!("  • ‹**{}**› <@{}>\n", player.player.elo, player.player.user_id));
            }
          }
          embed = embed.field(format!("{fmt_label} - Ready to start"), hot_info, false);

          // Team fields
          if queue_players >= quota {
            let (team_red, team_blu) = get_sorted_teams(&current_session.pool, quota);
            embed = TeamDisplay::new(team_red, team_blu).add_to_embed(embed, db, guild_id).await;

            // Overflow players
            if queue_players > quota {
              let overflow_count = queue_players - quota;
              let fatkid: Vec<_> = current_session.pool.iter().skip(quota).map(|p| format!("‹**{}**› <@{}>", p.player.elo, p.player.user_id)).collect();
              embed = embed.field(format!("Waiting for next game ({overflow_count}/{quota})"), fatkid.join("\n"), false);
            }
          }
        } else if queue_players == 0 {
          // Empty queue
          embed = add_waiting_field(embed, &fmt_label, 0, quota, "*Join to get started!*");
        } else {
          // Idle with players - show player list and timers
          // Queue header as a field so it stays categoryed with player fields
          let mut players_field = String::new();
          let mut timers_field = String::new();

          for player in current_session.pool.iter() {
            let elo_str = format!("‹**{}**› ", player.player.elo);
            players_field.push_str(&format!("{elo_str}<@{}>\n", player.player.user_id));

            if let Some((game_guild_id, fmt_name)) = in_game_players.get(&player.player.user_id) {
              if *game_guild_id == guild_id {
                timers_field.push_str(&format!("In {fmt_name} game\n"));
              } else {
                timers_field.push_str("In-game\n");
              }
            } else if player.in_queue_vc {
              timers_field.push_str("VC\n");
            } else {
              let timeout = if let Ok(settings) = db.users.get_prefs(player.player.user_id).await { settings.timeout } else { player.timeout };

              if timeout > 0 {
                if let Ok(join_time) = player.joined_at.duration_since(std::time::SystemTime::UNIX_EPOCH) {
                  let expiry_timestamp = join_time.as_secs() + (timeout as u64 * 60);
                  timers_field.push_str(&format!("Timeout {}\n", crate::timestamp_from_unix(expiry_timestamp as i64, crate::Style::Relative)));
                } else {
                  timers_field.push_str("-\n");
                }
              } else {
                timers_field.push_str("-\n");
              }
            }
          }

          // Use format name in the field title
          embed = embed.field(format!("{fmt_label} - Idle ({queue_players}/{quota})"), players_field, true);
          embed = embed.field("Status", timers_field, true);
        }
      } else {
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

  /// Queue dashboard updates for all categories across all servers (non-blocking, batched)
  /// Used when game state changes (live/end) affect in-game status on other dashboards
  pub async fn queue_dash_update_all(&self, ctx: &Context) {
    let data = ctx.data.read().await;
    if let Some(queue) = data.get::<DashboardQueueKey>() {
      queue.lock().await.request_update_all_deferred();
    } else {
      warn!("Dashboard queue not initialized in Context");
    }
  }

  /// Queue a dashboard update (non-blocking, batched)
  /// Requires guild_id to be passed since Category doesn't store it
  pub async fn queue_dash_update(&self, ctx: &Context, guild_id: GI) {
    //
    // Try to get queue from context data using the key from models module
    let data = ctx.data.read().await;
    if let Some(queue) = data.get::<DashboardQueueKey>() {
      queue.lock().await.request_update(guild_id, self.ctg_id as u64);
      //
    } else {
      warn!("Dashboard queue not initialized in Context");
      // Note: Can't fallback to dash_update here because we'd need &mut self
      // The dashboard queue should always be initialized, so this is just a safety check
    }
  }

  /// Handles the join queue button
  async fn dash_join_queue(&mut self, cc: &CC<'_>, fmt_id: u8) -> Result<()> {
    let user_id = cc.component.user.id;

    // Get player tag from database (primary source)
    let tag = match cc.db.get_user(user_id, cc.ctx).await {
      Ok(player) => player.tag,
      Err(_) => cc.component.user.display_name().to_string(),
    };

    // Store channel IDs before any borrows
    let _dashboard_channel = self.channels.dashboard;

    // If player is already in this format, refresh their timeout and return
    if self.is_user_in_fmt(fmt_id, user_id) {
      if let Some(sg) = self.fmt_mut(fmt_id) {
        for session in &mut sg.sessions {
          if let Some(sp) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
            sp.joined_at = std::time::SystemTime::now();
            break;
          }
        }
      }
      cc.defer_update().await?;
      self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;
      return Ok(());
    }

    // Check if we have an idle or hot session to join in the target format
    let has_joinable_session = self.fmt(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.status == SessionStatus::Idle || s.status == SessionStatus::Hot)).unwrap_or(false);

    if !has_joinable_session {
      cc.reply("Cannot join - match is in progress. Please wait.").await?;
      return Ok(());
    }

    // Defer update now that we know we'll succeed
    //
    cc.defer_update().await?;
    //

    // Get player rank: DB for speed, Discord roles for truth
    use crate::handlers::player::{get_or_assign_player_rank, get_player_rank, get_user_rank_from_discord_roles};
    use crate::Rank;
    if let Some(guild_id) = cc.component.guild_id {
      // First, get Discord role (source of truth) - returns GuildRank with ELO
      let role_based_guild_rank = get_user_rank_from_discord_roles(cc.ctx, &cc.db, guild_id, user_id).await;

      // Convert to Rank struct and get ELO
      let (discord_rank, _rank_min_elo) = if let Some(db_rank) = get_player_rank(&cc.db, guild_id, user_id).await {
        // Discord role is source of truth if it exists
        if let Some(guild_rank) = &role_based_guild_rank {
          let role_rank = Rank::from_name(&cc.db, guild_id, &guild_rank.name).await.unwrap_or(db_rank.clone());
          if role_rank != db_rank {
            info!("Rank mismatch for {}: Discord='{}' DB='{}', using Discord", &tag, guild_rank.name, db_rank.name);
          }
          (role_rank, guild_rank.elo)
        } else {
          // No Discord role, keep DB rank (they may have lost role but keep earned rank)
          let elo = db_rank.elo;
          (db_rank, elo)
        }
      } else {
        // No DB rank - check Discord roles before defaulting
        if let Some(guild_rank) = &role_based_guild_rank {
          let role_rank = Rank::from_name(&cc.db, guild_id, &guild_rank.name).await.unwrap_or_else(|_| Rank {
            guild_id,
            role_id: guild_rank.role_id,
            name: guild_rank.name.clone(),
            elo: guild_rank.elo,
          });
          (role_rank, guild_rank.elo)
        } else {
          // No DB rank or Discord roles, assign default
          match get_or_assign_player_rank(&cc.db, guild_id, user_id).await {
            Ok(rank) => {
              info!("Assigned default rank '{}' to {}", rank.name, user_id);
              let elo = rank.elo;
              (rank, elo)
            }
            Err(e) => {
              error!("Failed to get or assign rank for user {}: {}", user_id, e);
              return Ok(());
            }
          }
        }
      };

      // Get base player info
      let mut player = match cc.db.get_user(user_id, cc.ctx).await {
        Ok(p) => p,
        Err(_) => cc.db.new_user(user_id, cc.ctx).await?,
      };

      // Check if ELO and ranks are linked before normalizing
      let elo_ranks_linked = cc.db.config.get_elo_ranks_linked(guild_id).await.unwrap_or(true);

      let (validated_elo, was_normalized) = if elo_ranks_linked {
        // Linked: validate and normalize ELO based on Discord rank
        match cc.db.elo.validate_and_normalize_elo(user_id, guild_id, &discord_rank, &cc.db).await {
          Ok(result) => result,
          Err(e) => {
            warn!("Failed to validate ELO for user {}: {}", user_id, e);
            (discord_rank.elo, false)
          }
        }
      } else {
        // Independent: keep existing ELO, don't normalize
        let existing_elo = cc.db.elo.get_if_exists(user_id, guild_id).await.ok().flatten().map(|elo| elo.elo).unwrap_or(discord_rank.elo);
        (existing_elo, false)
      };

      if was_normalized {
        info!("ELO normalized for {}: {} -> {} (rank: {})", user_id, discord_rank.elo, validated_elo, discord_rank.name);
      }

      player.elo = validated_elo;
      player.rank = Some(discord_rank.clone());

      // Fetch discord tag from component user for performance (avoid extra API call)
      player.tag = cc.component.user.tag();

      // Save rank for announcement (player will be moved)
      let player_rank = discord_rank.clone();

      // Log the queue join attempt BEFORE adding to queue to fix race condition
      let _server_name = guild_name(cc.ctx, guild_id);
      let _category_name = self.name.as_deref().unwrap_or("Unknown").to_string();
      let _username = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;

      // Get current pool length BEFORE adding player
      let (_pool_len_before, _fmt_quota) = self.fmt(fmt_id).map(|sg| (sg.sessions.iter().map(|s| s.pool.len()).sum::<usize>(), sg.quota as usize)).unwrap_or((0, 0));
      let fmt_name = self.fmt(fmt_id).map(|sg| sg.name.as_str());

      // Clone fmt_name to avoid borrowing issues
      let fmt_name_owned = fmt_name.map(|s| s.to_string());

      // Log BEFORE queue operation to fix race condition with quota notifications
      if let Some(format) = self.fmt(fmt_id) {
        if let Err(e) = crate::log_queue_toggle(cc.ctx, &cc.db, guild_id, self.ctg_id, format, &player, "joined", None).await {
          warn!("Failed to log queue toggle: {e}");
        }
      }

      if let Err(e) = self.queue_player_fmt(fmt_id, player, discord_rank, cc.ctx, Some(guild_id), Some(&cc.db), Some(cc.manager.clone())).await {
        warn!("Failed to queue player: {e}");
      } else {
        // Send join announcement (delayed + buffered)
        {
          use crate::models::alert_limiter::{schedule_alert, AlertType};

          schedule_alert(
            cc.ctx.clone(),
            self.channels.queue_chat,
            guild_id,
            user_id,
            cc.db.clone(),
            self.ctg_id,
            fmt_id,
            AlertType::Join,
            fmt_name_owned,
            player_rank.name.clone(),
          );
        }
      }
    } else {
      cc.reply("This command can only be used in a server.").await?;
      return Ok(());
    }

    // Update dashboard to reflect changes
    //
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;
    //

    Ok(())
  }

  /// Handles the leave queue button
  async fn dash_leave_queue(&mut self, cc: &CC<'_>, fmt_id: u8) -> Result<()> {
    let user_id = cc.component.user.id;

    // Check if player is in a live match - disallow leaving
    if let Ok(session) = self.get_user_sesh_fmt(fmt_id, user_id) {
      if session.status == SessionStatus::Live {
        cc.reply("You cannot leave during a live match. Please find a substitute if needed.").await?;
        return Ok(());
      }
    }

    let quota = self.fmt(fmt_id).map(|sg| sg.quota as usize).unwrap_or(0);

    // Store fields before any borrows
    let _dashboard_channel = self.channels.dashboard;
    let queue_chat = self.channels.queue_chat;
    let _category_id = self.ctg_id;
    let _category_name = self.name.as_deref().unwrap_or("Unknown").to_string();

    // Get session index and format name before mutable borrow
    let fmt_name_owned = self.fmt(fmt_id).map(|sg| sg.name.clone());
    let session_idx = self.fmt(fmt_id).and_then(|sg| sg.sessions.iter().position(|s| s.pool.iter().any(|p| p.player.user_id == user_id)));

    // Check if player is in queue
    let format = self.fmt(fmt_id).cloned(); // Get format before mutable borrow
    let category_id = self.ctg_id; // Capture category_id before mutable borrow
    let should_regenerate_teams = if let Ok(session) = self.get_user_sesh_fmt(fmt_id, user_id) {
      // Check if player is physically in the queue VC
      let player_in_vc = if let Some(player) = session.pool.iter().find(|p| p.player.user_id == user_id) { player.in_queue_vc } else { false };

      // If player is in VC, check if they want to be disconnected
      if player_in_vc {
        // Check user's VC disconnect preference
        let settings = cc.db.users.get_prefs(user_id).await.unwrap_or_default();

        if settings.vc_auto_leave {
          // Acknowledge button press before disconnecting
          cc.defer_update().await?;

          // User wants to be disconnected from VC
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
      // Defer update immediately

      cc.defer_update().await?;

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
      if let Ok(player) = cc.db.get_user(user_id, cc.ctx).await {
        if let Some(ref format) = format {
          if let Err(e) = crate::log_queue_toggle(cc.ctx, &cc.db, guild_id, category_id, format, &player, "left", None).await {
            warn!("Failed to log queue toggle: {e}");
          }
        }
      }

      // Send leave announcement (delayed + buffered)
      {
        use crate::models::alert_limiter::{schedule_alert, AlertType};

        schedule_alert(cc.ctx.clone(), queue_chat, guild_id, user_id, cc.db.clone(), category_id, fmt_id, AlertType::Leave, fmt_name_owned.clone(), String::new());
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

      cc.reply("You are not in the queue!").await?;
      return Ok(());
    };

    // Regenerate teams if needed (outside the session borrow scope)
    if should_regenerate_teams {
      self.generate_teams_fmt(fmt_id, cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;
    }

    // Check if team VCs should be cleaned up (OnLastLeave policy)
    self.check_team_vc_cleanup_on_leave(cc.ctx).await;

    // Update dashboard to reflect changes (queue count now only shown in dashboard)
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;

    Ok(())
  }

  /// Handles the shuffle teams button
  async fn dash_shuffle(&mut self, cc: &CC<'_>, fmt_id: u8) -> Result<()> {
    let quota = self.fmt(fmt_id).map(|sg| sg.quota as usize).unwrap_or(0);

    // Check if game is live
    let is_live = self.fmt(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.status == SessionStatus::Live)).unwrap_or(false);
    if is_live {
      cc.reply("The game is live, can not shuffle").await?;
      return Ok(());
    }

    // Find the game to shuffle - can be Idle (if quota met) or Hot
    let has_shuffleable =
      self.fmt(fmt_id).map(|sg| sg.sessions.iter().any(|s| (s.status == SessionStatus::Idle || s.status == SessionStatus::Hot) && s.pool.len() >= quota)).unwrap_or(false);

    if !has_shuffleable {
      cc.reply(&format!("No game ready for shuffling. Need at least {quota} players in queue.")).await?;
      return Ok(());
    }

    // Defer update now that we know we have a game to shuffle
    cc.defer_update().await?;

    // Refresh player ranks from Discord roles before shuffling teams
    if let Some(guild_id) = cc.component.guild_id {
      self.reload_player_ranks(cc.ctx, guild_id, &cc.db).await;
    }

    // Call the same team generation logic used by generate_teams
    // This ensures balanced teams using the BCH algorithm
    self.generate_teams_fmt(fmt_id, cc.ctx, cc.component.guild_id.unwrap(), Some(&cc.db)).await;

    // Update dashboard to show new teams
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;

    Ok(())
  }

  /// Handles the start match button
  async fn dash_start(&mut self, cc: &CC<'_>, fmt_id: u8) -> Result<()> {
    use std::time::Duration;
    // Check if user has Runner role
    use crate::handlers::player::check_component_role;
    use crate::models::Role;

    match check_component_role(cc, &Role::Runner).await {
      Ok(true) => {
        // User has Runner role, proceed
      }
      Ok(false) => {
        cc.reply("Only runners can start matches.").await?;
        return Ok(());
      }
      Err(e) => {
        warn!("Failed to check runner role: {e}");
        cc.reply("Failed to verify permissions.").await?;
        return Ok(());
      }
    }

    // Check if there's a hot game to start in the target format
    let has_hot_game = self.fmt(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.is_hot())).unwrap_or(false);

    if !has_hot_game {
      cc.reply("No hot game ready to start.").await?;
      return Ok(());
    }

    // Check for duplicate action within 2 seconds on the same hot session
    if let Some(fmt) = self.fmt_mut(fmt_id) {
      if let Some(hot_session) = fmt.sessions.iter_mut().find(|s| s.is_hot()) {
        if let Some(last_action) = hot_session.last_action_at {
          if let Ok(elapsed) = SystemTime::now().duration_since(last_action) {
            if elapsed < Duration::from_secs(2) {
              // Duplicate action within 2 seconds - acknowledge silently
              cc.defer_update().await?;
              return Ok(());
            }
          }
        }
        // Update last action timestamp
        hot_session.last_action_at = Some(SystemTime::now());
      }
    }

    // Defer update now that we're going to start the match
    cc.defer_update().await?;

    // Move players to team channels (Hot → Push → Live)
    match self.push_fmt(fmt_id, cc.ctx, cc.component.guild_id.unwrap(), &cc.db).await {
      Ok(_) => {
        info!("Players moved to team channels and game is now live");
        // Update all dashboards to reflect in-game status for match players
        self.queue_dash_update_all(cc.ctx).await;
        Ok(())
      }
      Err(e) => {
        error!("Failed to start match: {e}");
        Ok(())
      }
    }
  }

  /// Handles the end match button - directly ends the match
  async fn dash_end(&mut self, cc: &CC<'_>, fmt_id: u8) -> Result<()> {
    use serenity::all::CreateMessage;
    use std::time::SystemTime;

    // Check if there's an active game to end in the target format
    let active_session = self.fmt(fmt_id).and_then(|sg| sg.sessions.iter().find(|s| s.status == SessionStatus::Hot || s.status == SessionStatus::Live));

    if active_session.is_none() {
      cc.reply("No active match to end.").await?;
      return Ok(());
    }

    // Capture match info before pulling
    let active_session = active_session.unwrap();
    let match_time = active_session.started_at.and_then(|started| SystemTime::now().duration_since(started).ok()).map(|d| d.as_secs());
    let quota = self.fmt(fmt_id).map(|sg| sg.quota as usize).unwrap_or(0);
    let (team_red, team_blu) = get_sorted_teams(&active_session.pool, quota);
    let guild_id = cc.component.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Record match to database
    Self::record_match_to_database(
        &cc.db,
        guild_id,
        self.ctg_id,
        fmt_id,
        active_session,
        &team_red,
        &team_blu,
    ).await?;

    // Build match summary embed
    let mut embed = CE::new().title("Match ended").color(0x5865F2);

    // Format duration
    if let Some(secs) = match_time {
      let mins = secs / 60;
      let remaining_secs = secs % 60;
      embed = embed.field("Time", format!("{}m {}s", mins, remaining_secs), true);
    }

    embed = TeamDisplay::new(team_red, team_blu).add_to_embed(embed, &cc.db, guild_id).await;

    // Defer update now that we're going to end the match
    cc.defer_update().await?;

    // Post match summary to queue chat only if match was 5+ minutes
    if let Some(secs) = match_time {
      if secs >= 300 {
        // 5 minutes = 300 seconds
        let queue_chat = self.channels.queue_chat;
        
        // Add Report Score button for runners with category_id and format_id embedded
        use serenity::all::CreateActionRow;
        let button = CB::new(format!("report_score_{}_{}", self.ctg_id, fmt_id)).label("Report Score").style(BS::Primary);
        let components = vec![CreateActionRow::Buttons(vec![button])];
        
        let _ = queue_chat.send_message(&cc.ctx.http, CreateMessage::new().embed(embed).components(components)).await;
      }
    }

    // Move players back to queue channel (Hot/Live → Pull → Idle)
    match self.pull_fmt(fmt_id, cc.ctx, guild_id, &cc.db, Some(cc.manager.clone())).await {
      Ok(_) => {
        info!("Match ended, players moved back to queue");
        // Update all dashboards to clear in-game status for match players
        self.queue_dash_update_all(cc.ctx).await;
        Ok(())
      }
      Err(e) => {
        error!("Failed to end match: {e}");
        Ok(())
      }
    }
  }

  /// Handles the report score button - shows modal for runners to input scores
  async fn dash_report_score(&mut self, cc: &CC<'_>) -> Result<()> {
    use crate::handlers::player::check_component_role;
    use crate::models::Role;
    use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
    use serenity::all::CreateActionRow as CAR;

    // Check if user is a runner
    if !check_component_role(cc, &Role::Runner).await? {
      cc.reply("Only runners can report scores.").await?;
      return Ok(());
    }

    // Parse category_id and format_id from button custom_id (format: report_score_CATID_FMTID)
    let custom_id = &cc.component.data.custom_id;
    let parts: Vec<&str> = custom_id.split('_').collect();
    let category_id = parts.get(2).and_then(|s| s.parse::<i64>().ok()).unwrap_or(self.ctg_id as i64);
    let format_id = parts.get(3).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);

    // Create modal for score input with category_id and format_id embedded
    let modal = CreateModal::new(format!("report_score_modal_{}_{}", category_id, format_id), "Report Match Score")
      .components(vec![
        CAR::InputText(CreateInputText::new(InputTextStyle::Short, "Blue team score", "blu_score").placeholder(format!("0-{}", crate::models::constants::MAX_MATCH_SCORE)).required(true).min_length(1).max_length(1)),
        CAR::InputText(CreateInputText::new(InputTextStyle::Short, "Red team score", "red_score").placeholder(format!("0-{}", crate::models::constants::MAX_MATCH_SCORE)).required(true).min_length(1).max_length(1)),
      ]);

    cc.component.create_response(&cc.ctx.http, CIR::Modal(modal)).await?;
    Ok(())
  }

  /// Handles button interaction events from the dashboard
  ///
  /// Processes all button interactions in a modular way
  ///
  /// * `cc` - The component context with button information
  /// Parse format ID from button custom_id suffix (format: action:sg_id).
  /// Returns 0 if no suffix or invalid.
  fn parse_fmt_id(parts: &[&str]) -> u8 {
    parts.get(1).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0)
  }

  pub async fn dash_handle_button_interaction(&mut self, cc: &CC<'_>) -> Result<()> {
    let custom_id = &cc.component.data.custom_id;

    let parts: Vec<&str> = custom_id.split(':').collect();
    let action = parts[0];

    // Get server and category names for logging - store channel ID before any mut borrows
    let gld_id = cc.component.guild_id.unwrap();
    let _dashboard_channel = self.channels.dashboard;
    let gld_nm = guild_name(cc.ctx, gld_id);
    let ctg_nm = self.name.as_deref().unwrap_or("Unknown").to_string();
    let fmt_id = Self::parse_fmt_id(&parts);
    let usr_tg = get_user_tag(cc.ctx, cc.component.user.id, &cc.db).await;

    match action {
      "join_queue" => self.dash_join_queue(cc, fmt_id).await,
      "leave_queue" => self.dash_leave_queue(cc, fmt_id).await,
      "change_expiry" => {
        info!("{} {} requested expiry time change", log_prefix_category(&gld_nm, &ctg_nm), usr_tg);
        self.dash_change_expiry(cc, fmt_id).await
      }
      "set_expiry" => {
        info!("{} {} changed expiry time", log_prefix_category(&gld_nm, &ctg_nm), usr_tg);
        self.dash_set_expiry(cc, parts.get(1).copied()).await
      }
      "show_settings" => {
        info!("{} {} requested settings", log_prefix_guild(&gld_nm), usr_tg);
        self.dash_show_settings(cc).await
      }
      "show_runner_menu" => {
        info!("{} {} requested runner menu", log_prefix_guild(&gld_nm), usr_tg);
        crate::handlers::runner_menu::show_runner_menu(cc).await
      }
      "show_help" => {
        info!("{} {} requested help", log_prefix_guild(&gld_nm), usr_tg);
        crate::models::dashboard::show_help(cc).await
      }
      "shuffle_teams" => {
        info!("{} {} used Shuffle", log_prefix_category(&gld_nm, &ctg_nm), usr_tg);
        self.dash_shuffle(cc, fmt_id).await
      }
      "start_match" => {
        // Combined Start/End button: dispatch based on current format state
        let fmt_name = self.fmt(fmt_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        let is_live = self.fmt(fmt_id).map(|sg| sg.sessions.iter().any(|s| s.is_active())).unwrap_or(false);
        if is_live {
          info!("{} {} used End", crate::log::log_prefix_format(&gld_nm, &ctg_nm, &fmt_name), usr_tg);
          self.dash_end(cc, fmt_id).await
        } else {
          info!("{} {} used Start", crate::log::log_prefix_format(&gld_nm, &ctg_nm, &fmt_name), usr_tg);
          self.dash_start(cc, fmt_id).await
        }
      }
      "end_match" => {
        let fmt_name = self.fmt(fmt_id).map(|sg| sg.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        info!("{} {} used End", crate::log::log_prefix_format(&gld_nm, &ctg_nm, &fmt_name), usr_tg);
        self.dash_end(cc, fmt_id).await
      }
      action if action.starts_with("report_score") => {
        info!("{} {} used Report Score", log_prefix_category(&gld_nm, &ctg_nm), usr_tg);
        self.dash_report_score(cc).await
      }
      _ => {
        cc.reply(&format!("Unknown button action: {action}")).await?;
        Ok(())
      }
    }
  }

  /// Show expiry time options
  async fn dash_change_expiry(&mut self, cc: &CC<'_>, _fmt_id: u8) -> Result<()> {
    use serenity::all::{ButtonStyle as BS, CreateButton as CB};

    // Check if user is in queue (across all formats)
    let user_id = cc.component.user.id;
    let is_in_queue = self.formats.iter().any(|sg| sg.sessions.iter().any(|s| s.pool.iter().any(|p| p.player.user_id == user_id)));

    if !is_in_queue {
      cc.reply("You must be in the queue to change your expiry time.").await?;
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
  async fn dash_set_expiry(&mut self, cc: &CC<'_>, duration_str: Option<&str>) -> Result<()> {
    let user_id = cc.component.user.id;

    // Parse duration string
    let duration = match duration_str {
      Some("30m") => 30,
      Some("1h") => 60,
      Some("2h") => 120,
      Some("3h") => 180,
      Some("4h") => 240,
      _ => {
        cc.reply("Invalid expiry duration.").await?;
        return Ok(());
      }
    };

    // Find and update the player's expiry duration in any session across all formats
    let mut found = false;
    'outer: for sg in self.formats.iter_mut() {
      for session in sg.sessions.iter_mut() {
        if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
          player.timeout = duration;
          found = true;
          break 'outer;
        }
      }
    }

    if !found {
      cc.reply("You are not in the queue.").await?;
      return Ok(());
    }

    // Delete the ephemeral message by updating it
    let response = CIR::UpdateMessage(
      CIRM::new().content(format!("Expiry time set to {} for this queue instance.", duration_str.unwrap_or("unknown"))).components(vec![]), // Remove buttons
    );
    cc.component.create_response(&cc.ctx.http, response).await?;

    // Update the dashboard
    self.queue_dash_update(cc.ctx, cc.component.guild_id.unwrap()).await;

    Ok(())
  }

  /// Show user settings as ephemeral embed in dashboard channel
  async fn dash_show_settings(&mut self, cc: &CC<'_>) -> Result<()> {
    use crate::handlers::settings::{build_settings_buttons, build_settings_embed};

    let user_id = cc.component.user.id;

    // Get user's settings
    let settings = match cc.db.users.get_prefs(user_id).await {
      Ok(s) => s,
      Err(e) => {
        cc.reply(&format!("Failed to load settings: {}", e)).await?;
        return Ok(());
      }
    };

    // Build settings embed with interactive buttons
    let embed = build_settings_embed(&settings);
    let buttons = build_settings_buttons(&settings);

    // Send ephemeral message with settings embed and buttons
    let response = CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true));

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

/// Request to update a specific category's dashboard
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DashboardUpdateRequest {
  pub guild_id: GI,
  pub category_id: u64,
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
  pub fn request_update(&self, guild_id: GI, category_id: u64) {
    let request = DashboardUpdateRequest { guild_id, category_id };
    if let Err(e) = self.sender.send(request) {
      warn!("Failed to queue dashboard update: {e}");
    }
  }

  /// Request dashboard updates for all categories across all servers
  pub fn request_update_all(&self, manager: &crate::models::Manager) {
    for srv in &manager.servers {
      for grp in &srv.categories {
        self.request_update(srv.guild_id, grp.ctg_id as u64);
      }
    }
  }

  /// Request dashboard updates for all categories without needing the manager lock.
  /// Sends a sentinel that the batch processor expands once it acquires the lock.
  pub fn request_update_all_deferred(&self) {
    let request = DashboardUpdateRequest {
      guild_id: GI::new(1),
      category_id: u64::MAX, // sentinel
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
          let mut update_all = request.category_id == u64::MAX;
          if !update_all {
            pending_updates.insert(request);
          }

          // Now wait for the batch window, collecting more updates
          let deadline = tokio::time::Instant::now() + batch_window;

          loop {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
              Ok(Some(request)) => {
                if request.category_id == u64::MAX {
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
    for srv in &mgr.servers {
      for grp in &srv.categories {
        pending.insert(DashboardUpdateRequest { guild_id: srv.guild_id, category_id: grp.ctg_id as u64 });
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
          for srv in &manager_lock.servers {
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
                    in_game_players.insert(sp.player.user_id, (srv.guild_id, sg.name.clone()));
                  }
                }
              }
            }
          }

          let server = match manager_lock.get_server(guild_id) {
            Ok(s) => s,
            Err(e) => {
              warn!("Failed to get server for dashboard update: {e}");
              return;
            }
          };

          let guild_name = server.guild_name.clone();

          let category = match server.categories.iter_mut().find(|g| g.ctg_id == category_id as u8) {
            Some(g) => g,
            None => {
              warn!("[{}] Failed to find category {} for dashboard update", guild_name, category_id);
              return;
            }
          };

          // Log current session state
          let pool_size = category.formats[0].sessions.first().map(|s| s.pool.len()).unwrap_or(0);
          //

          // Refresh player ranks from Discord to ensure dashboard shows current ranks
          // This prevents desync when players are promoted while sitting in queue
          category.reload_player_ranks(&ctx, guild_id, &database).await;

          // Validate VC status to ensure accurate display of who is in voice chat
          // This prevents desync where flags don't match Discord's actual voice states
          category.validate_vc_status(&ctx, guild_id).await;

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
                  if let Ok(server) = manager_lock.get_server(guild_id) {
                    if let Some(category) = server.categories.iter_mut().find(|g| g.ctg_id == category_id as u8) {
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
              let hint = if e.to_string().contains("Missing Access") { " (check that the bot has View Channel and Send Messages permissions on this channel)" } else { "" };
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
pub async fn show_help(cc: &CC<'_>) -> Result<()> {
  // Get user's VC auto-join setting to conditionally show that info
  let user_id = cc.component.user.id;
  let vc_auto_join_enabled = cc.db.users.get_prefs(user_id).await
    .map(|prefs| prefs.vc_auto_join)
    .unwrap_or(false);

  let vc_auto_join_text = if vc_auto_join_enabled {
    " You can also just hop into the queue voice channel and you'll be added automatically."
  } else {
    ""
  };

  let description = format!(
    "**Joining and leaving the queue**\n\
     When you want to play, just click the **Join** button on the dashboard and pick the format you want to play.{}\n\
     This will add you to a queue for a set period of time and you'll retain this spot in the queue until the match starts or you leave. \
     To leave the queue, simply click the **Leave** button on the dashboard.\n\n\
     **How does the queue work?**\n\
     Think of it as a line to go on a rollercoaster at a carnival. The quota is the number of seats on the cart - it must always be full before the ride can start. \
     Once enough people are in line to fill all the seats, those first people board the cart and the ride begins. \
     If there are more people in queue than the cart can fit, they stay in line and wait for the next ride.\n\n\
     We can also have multiple formats (like 6v6 and 4v4), each with its own queue running independently. \
     Even within a single format, if there are enough players, multiple matches can run at the same time - the bot will create more team channels and split players across them.\n\
     After a match ends, those players return to the queue and the next group of players gets selected for the next match. \
     Selection is mostly first-come-first-served, but the system ensures everyone gets a fair chance to play.\n\n\
     **When do teams get made?**\n\
     Once enough players join to fill a match, the bot generates balanced teams and shows a preview of it on the dashboard.\n\
     **Where do we play?**\n\
     The game starts when a runner presses the **Start** button. The bot then creates team voice channels and moves everyone to their team's channel. \
     After the game ends, the runner ends the match via **End** and you'll be moved back to the queue channel.\n\n\
     **What happens during a match?**\n\
     The dashboard updates live so you can always see who's in queue and what's happening. \
     If something goes wrong, runners (trusted users who can manage the queue), admins or xCape can step in to help fix issues.\n\n\
     **That's it!** The bot handles most things so you can focus on playing.\n\n\
     **Questions or feedback?** Contact <@257898548773912576>",
    vc_auto_join_text
  );

  let embed = CE::new()
    .title("How does qBot work?")
    .description(description)
    .color(crate::CYAN);

  let response = CIR::Message(
    CIRM::new()
      .embed(embed)
      .ephemeral(true)
  );
  cc.component.create_response(&cc.ctx.http, response).await?;

  Ok(())
}
