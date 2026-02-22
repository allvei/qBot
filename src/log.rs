use serenity::all::{CommandInteraction, Context, UserId as UI};
use sqlx::Row;
use tracing::{info, warn};

use crate::models::constants::guild_name;

/// Get user tag for logging purposes - tries database first, then Discord API
pub async fn get_user_tag(ctx: &Context, user_id: UI, db: &crate::Database) -> String {
  // Try database first (most efficient)
  if let Ok(player) = db.get_user(user_id, ctx).await {
    if !player.tag.is_empty() {
      return player.tag;
    } else {
      warn!("User {} found in database but tag is empty, falling back to Discord API", user_id);
    }
  } else {
    warn!("User {} not found in database, falling back to Discord API", user_id);
  }

  // Fallback to Discord API
  ctx.http.get_user(user_id).await.map(|user| user.tag()).unwrap_or_else(|e| {
    warn!("Failed to get user {} from Discord API: {}, using user ID as fallback", user_id, e);
    user_id.to_string()
  })
}

/// Async log function that extrapolates all information from Format, Player, and Action
pub async fn log_queue_toggle(
  ctx: &Context,
  db: &crate::Database,
  guild_id: serenity::all::GuildId,
  category_id: u8,
  format: &crate::models::Format,
  player: &crate::models::Player,
  action: &str, // "joined" or "left"
) -> Result<(), anyhow::Error> {
  // Get format info from database using guild_id, category_id, and format_id
  let fmt_info = sqlx::query(
    "SELECT f.id, c.guild_name, c.name as category_name, f.name as format_name 
         FROM formats f 
         JOIN categories c ON f.guild_id = c.guild_id AND f.category_id = c.category_id 
         WHERE f.guild_id = ? AND f.category_id = ? AND f.format_id = ?",
  )
  .bind(guild_id.get() as i64)
  .bind(category_id as i64)
  .bind(format.id as i64)
  .fetch_one(&db.pool)
  .await?;

  let gld_nm: &str = fmt_info.get("guild_name");
  let ctg_nm: &str = fmt_info.get("category_name");
  let fmt_nm: &str = fmt_info.get("format_name");

  // Get pool size and position from format's sessions
  // Adjust count to show the state AFTER the action
  let pool_size =
    format.sessions.iter().find(|s| s.status == crate::models::SessionStatus::Idle || s.status == crate::models::SessionStatus::Hot)
      .map(|s| {
        let current = s.pool.len();
        let adjusted_count = if action == "joined" { 
          current + 1  // After joining, count increases
        } else { 
          current.saturating_sub(1)  // After leaving, count decreases
        };
        (adjusted_count, format.quota as usize)
      });

  // Calculate position based on actual player position in session
  let position = if action == "joined" {
    // For joins, find the player's actual position in the session after they would be added
    format.sessions.iter()
      .find(|s| s.status == crate::models::SessionStatus::Idle || s.status == crate::models::SessionStatus::Hot)
      .and_then(|s| s.pool.iter().position(|p| p.player.user_id == player.user_id))
      .map(|pos| pos + 1)
      .unwrap_or_else(|| {
        // If not found (race condition), use pool length + 1 as fallback
        format.sessions.iter()
          .find(|s| s.status == crate::models::SessionStatus::Idle || s.status == crate::models::SessionStatus::Hot)
          .map(|s| s.pool.len() + 1)
          .unwrap_or(1)
      })
  } else {
    // For leaves, look up their current position before removal
    format.sessions.iter()
      .find(|s| s.status == crate::models::SessionStatus::Idle || s.status == crate::models::SessionStatus::Hot)
      .and_then(|s| s.pool.iter().position(|p| p.player.user_id == player.user_id))
      .map(|pos| pos + 1)
      .unwrap_or(0)
  };

  // Determine queue type based on action
  let queue_type = match action {
    "joined" => QTT::BJ,
    "left" => QTT::BL,
    _ => QTT::BJ, // default
  };

  // Call the original function with extrapolated data
  log_queue_toggle_sync(gld_nm, ctg_nm, &player.tag, queue_type, pool_size, Some(fmt_nm), position);

  Ok(())
}

pub fn log_queue_toggle_sync(
  guild_name: &str,
  category_name: &str,
  tag: &str,
  queue_type: QTT,
  pool_size: Option<(usize, usize)>,
  fmt_name: Option<&str>,
  position: usize,
) {
  let (action, source) = match queue_type {
    QTT::BJ => ("joined", None),
    QTT::BL => ("left", None),
    QTT::VJ => ("joined", Some("VC")),
    QTT::VL => ("left", Some("VC")),
  };

  let pos_part = if action != "left" { format!("#{} ", position) } else { String::new() };
  let prefix = log_prefix_format(guild_name, category_name, fmt_name.unwrap_or(""));

  match (pool_size, source) {
    (Some((current, quota)), Some(src)) => info!("{} {}{} {} ({}) [{}/{}]", prefix, pos_part, tag, action, src, current, quota),
    (Some((current, quota)), None) =>      info!("{} {}{} {} [{}/{}]",      prefix, pos_part, tag, action, current, quota),
    (None, Some(src)) =>                   info!("{} {}{} {} ({})",         prefix, pos_part, tag, action, src),
    (None, None) =>                        info!("{} {}{} {}",              prefix, pos_part, tag, action),
  }
}

/// Generate log prefix in format [GUILD_NAME]
pub fn log_prefix_guild(guild_name: &str) -> String {
  format!("[{}]", guild_name)
}

/// Generate log prefix in format [GUILD_NAME/CATEGORY_NAME/FORMAT_NAME]
pub fn log_prefix_category(guild_name: &str, category_name: &str) -> String {
  format!("[{}/{}]", guild_name, category_name)
}

/// Generate log prefix in format [GUILD_NAME/CATEGORY_NAME/FORMAT_NAME]
pub fn log_prefix_format(guild_name: &str, category_name: &str, format_name: &str) -> String {
  let fmt_suffix = if format_name.is_empty() { "".to_string() } else { format!("/{}", format_name) };
  format!("[{}/{}{}]", guild_name, category_name, fmt_suffix)
}

/// Generate log prefix from Context and IDs
pub async fn log_prefix_from_context(ctx: &Context, guild_id: serenity::all::GuildId, category_name: &str, format_name: &str) -> String {
  let guild_name = guild_name(ctx, guild_id);
  log_prefix_format(&guild_name, category_name, format_name)
}

pub enum QueueToggleType {
  BJ, // Button Join
  BL, // Button Leave
  VJ, // Voice Join
  VL, // Voice Leave
}

type QTT = QueueToggleType;

/// Log command usage with optional parameters
pub async fn log_command_usage(
  ctx: &Context,
  interaction: &CommandInteraction,
  db: &crate::Database,
  command_name: &str,
  target_user: Option<UI>,
  additional_params: Option<&str>,
) {
  let guild_name = guild_name(ctx, interaction.guild_id.unwrap());
  let user_tag = get_user_tag(ctx, interaction.user.id, db).await;

  let mut message = format!("[{}] {} used /{}", guild_name, user_tag, command_name);

  // Add target user if specified
  if let Some(target) = target_user {
    let target_tag = get_user_tag(ctx, target, db).await;
    message.push_str(&format!(" on {}", target_tag));
  }

  // Add additional parameters if specified
  if let Some(params) = additional_params {
    message.push_str(&format!(" {}", params));
  }

  info!("{}", message);
}

/// Simplified command logging without database (for commands that don't need user tags)
pub fn log_command_usage_simple(ctx: &Context, interaction: &CommandInteraction, command_name: &str, additional_params: Option<&str>) {
  let guild_name = guild_name(ctx, interaction.guild_id.unwrap());
  let user_tag = interaction.user.tag();

  let mut message = format!("[{}] {} used /{}", guild_name, user_tag, command_name);

  // Add additional parameters if specified
  if let Some(params) = additional_params {
    message.push_str(&format!(" {}", params));
  }

  info!("{}", message);
}
