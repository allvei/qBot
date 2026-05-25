use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serenity::all::{Context, GuildId, UserId};
use sqlx::{Row, SqlitePool};
use tracing::{error, info, warn};

use super::Repository;
use crate::models::Player;
use crate::{Database, Rank, DEFAULT_ALERT_COLOR};

const ALERT_LINE_WIDTH: usize = 70;
const ALERT_MAX_LINES: usize = 6;
const FOOTER_LINE_WIDTH: usize = 100;
const FOOTER_MAX_LINES: usize = 2;

/// Truncate text to fit within display constraints.
/// Newlines ceiling the current line to line_width chars.
fn truncate_text(s: &str, line_width: usize, max_lines: usize) -> String {
  let mut result = String::new();
  let mut line_count = 0;
  let mut line_chars = 0;

  for ch in s.chars() {
    if line_count >= max_lines {
      break;
    }

    if ch == '\n' {
      result.push(ch);
      line_count += 1;
      line_chars = 0;
    } else {
      if line_chars >= line_width {
        line_count += 1;
        if line_count >= max_lines {
          break;
        }
        line_chars = 0;
      }
      result.push(ch);
      line_chars += 1;
    }
  }

  result
}

/// Truncate alert message to fit within display constraints.
fn truncate_alert_message(s: &str) -> String {
  truncate_text(s, ALERT_LINE_WIDTH, ALERT_MAX_LINES)
}

/// Truncate footer text to fit within display constraints.
fn truncate_footer_text(s: &str) -> String {
  truncate_text(s, FOOTER_LINE_WIDTH, FOOTER_MAX_LINES)
}

/// Check if a string contains only allowed characters (ASCII printable + extended + emojis).
/// Returns true if valid, false if contains disallowed characters.
pub fn is_valid_user_text(s: &str) -> bool {
  s.chars().all(|c| {
    let code = c as u32;
    // ASCII printable (0x20-0x7E) + newline/tab + extended ASCII (0x80-0xFF) + Unicode emojis
    (0x20..=0x7E).contains(&code)
            || c == '\n'
            || c == '\t'
            || (0x80..=0xFF).contains(&code)
            || (0x1F300..=0x1FAFF).contains(&code)  // Emojis (Miscellaneous Symbols, Emoticons, etc.)
            || (0x2600..=0x27BF).contains(&code)    // Misc symbols, Dingbats
            || (0xFE00..=0xFE0F).contains(&code)    // Variation selectors
            || (0x200D..=0x200D).contains(&code) // Zero-width joiner (for compound emojis)
  })
}

/// Sanitize user text by removing disallowed characters.
/// Keeps ASCII printable, newline, tab, and extended ASCII (0x80-0xFF).
pub fn sanitize_user_text(s: &str) -> String {
  s.chars()
    .filter(|&c| {
      let code = c as u32;
      (0x20..=0x7E).contains(&code) || c == '\n' || c == '\t' || (0x80..=0xFF).contains(&code)
    })
    .collect()
}

#[derive(Clone)]
pub struct PlayerRepository {
  pool: SqlitePool,
}

impl PlayerRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  // Return user purely with DB data
  pub async fn get(&self, user_id: UserId) -> Result<Player> {
    match sqlx::query("SELECT user_id, steam_id, discord_tag, queue_expiration FROM users WHERE user_id = ?").bind(user_id.get() as i64).fetch_one(&self.pool).await {
      Ok(result) => Ok(Self::get_player(result)),
      Err(e) => Err(e.into()),
    }
  }

  // With context, we can use API as a backup
  pub async fn get_with_tag(&self, user_id: UserId, ctx: &Context) -> Result<Player> {
    let result = sqlx::query("SELECT user_id, steam_id, discord_tag, queue_expiration FROM users WHERE user_id = ?").bind(user_id.get() as i64).fetch_one(&self.pool).await?;

    let mut player = Self::get_player(result);

    // If no tag in database, fetch from Discord API and cache it
    if player.tag.is_empty() {
      match ctx.http.get_user(user_id).await {
        Ok(user) => {
          player.tag = user.tag(); // Use tag() instead of display_name()
                                   // Cache the tag for future use
          let _ = self.update_discord_tag(user_id, &player.tag).await;
        }
        Err(e) => {
          warn!("Failed to fetch user {}: {}", user_id, e);
          player.tag = "unknown".to_string();
        }
      }
    }

    Ok(player)
  }

  /// Get player with tag and try to get display name from guild context
  pub async fn get_with_nick(&self, user_id: UserId, ctx: &Context, guild_id: Option<serenity::all::GuildId>) -> Result<Player> {
    let mut player = self.get_with_tag(user_id, ctx).await?;

    // Try to get display name (nickname) if guild context is available
    if let Some(guild_id) = guild_id {
      player.tag = ctx.cache.member(guild_id, user_id).unwrap().display_name().to_string();
    }

    Ok(player)
  }

  /// Ensure user exists without fetching tag (for internal operations)
  pub async fn check_user(&self, user_id: UserId, steam_id: Option<u64>) -> Result<Player> {
    let result = sqlx::query(
      "INSERT INTO users (user_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING user_id, steam_id, discord_tag, queue_expiration",
    )
    .bind(user_id.get() as i64)
    .bind(steam_id.map(|id| id as i64).unwrap_or(0))
    .fetch_one(&self.pool)
    .await?;

    Ok(Self::get_player(result))
  }

  /// Batch ensure multiple users exist in the database (single transaction)
  pub async fn batch_ensure(&self, user_ids: &[UserId]) -> Result<()> {
    if user_ids.is_empty() {
      return Ok(());
    }

    let mut tx = self.pool.begin().await?;
    for chunk in user_ids.chunks(500) {
      let mut query = String::from("INSERT INTO users (user_id, steam_id) VALUES ");
      let placeholders: Vec<String> = chunk.iter().map(|_| "(?, 0)".to_string()).collect();
      query.push_str(&placeholders.join(", "));
      query.push_str(" ON CONFLICT(user_id) DO NOTHING");

      let mut q = sqlx::query(&query);
      for uid in chunk {
        q = q.bind(uid.get() as i64);
      }
      q.execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
  }

  pub async fn upsert(&self, user_id: UserId, steam_id: Option<u64>) -> Result<Player> {
    let result = sqlx::query(
      "INSERT INTO users (user_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING user_id, steam_id, discord_tag, queue_expiration",
    )
    .bind(user_id.get() as i64)
    .bind(steam_id.map(|id| id as i64).unwrap_or(0))
    .fetch_one(&self.pool)
    .await?;

    Ok(Self::get_player(result))
  }

  /// Extract player data from a database row
  fn get_player(result: sqlx::sqlite::SqliteRow) -> Player {
    let user_id: i64 = result.get("user_id");
    let steam_id: Option<i64> = result.try_get("steam_id").ok();
    let discord_tag: Option<String> = result.try_get("discord_tag").ok().flatten();
    let queue_expiration: i64 = result.get("queue_expiration");

    // Use stored discord_tag if available, otherwise empty string
    let tag = discord_tag.unwrap_or_default();

    // No rank available from basic user query - must be set later with guild-specific data
    Player::add(
      (user_id as u64).into(),
      tag,
      queue_expiration as u8,
      steam_id.map(|id| id as u64),
      None, // No rank available without guild context
    )
  }

  /// Get player with rank determined from guild-specific ELO and Discord roles
  pub async fn get_with_guild_rank(&self, user_id: UserId, _ctx: &Context, guild_id: GuildId, db: &Database) -> Result<Player> {
    info!("DEBUG: get_player_with_guild_rank called for user {} in guild {}", user_id, guild_id);

    // Get base player data from users table
    let result = sqlx::query("SELECT user_id, steam_id, discord_tag, queue_expiration FROM users WHERE user_id = ?").bind(user_id.get() as i64).fetch_one(&self.pool).await?;

    let mut player = Self::get_player(result);

    // Get guild-specific ELO (this is the primary source now)
    let guild_elo = db.elo.get(user_id, guild_id, db).await?;
    player.elo = guild_elo.elo;
    player.rank = Some(guild_elo.rank);

    info!("DEBUG: Player guild ELO: {}, Rank: {}", player.elo, player.rank.as_ref().unwrap().name);

    // For new players (no games), initialize with default rank
    if guild_elo.games == 0 && player.elo == 0 {
      // Get default rank role ID from config
      let default_rank_role_id = db.config.get_default_rank_role_id(guild_id).await?;

      // Find the configured default rank in the database by role ID
      let default_guild_rank = match default_rank_role_id {
        Some(role_id) => match db.ranks.rank_from_role_id(guild_id, role_id).await {
          Ok(rank) => rank,
          Err(_) => {
            let rank = Rank::lowest(db, guild_id).await.map_err(|_| anyhow!("No ranks configured for guild"))?;
            crate::db::repo::rank::GuildRank::new(guild_id, rank.name.clone(), rank.elo, rank.role_id)
          }
        },
        None => {
          let rank = Rank::lowest(db, guild_id).await.map_err(|_| anyhow!("No ranks configured for guild"))?;
          crate::db::repo::rank::GuildRank::new(guild_id, rank.name.clone(), rank.elo, rank.role_id)
        }
      };

      // Convert the guild rank's ELO to the appropriate Rank struct
      let assigned_rank = Rank::from_elo(db, guild_id, default_guild_rank.elo).await?;
      let default_elo = default_guild_rank.elo;

      info!("DEBUG: New player in guild, using default rank {} (role {}) with ELO {}", assigned_rank.name, default_guild_rank.role_id, default_elo);
      player.rank = Some(assigned_rank.clone());
      player.elo = default_elo;

      // Initialize their guild ELO
      if let Err(e) = db.elo.set(user_id, guild_id, player.elo, player.rank.clone().unwrap()).await {
        error!("Failed to initialize guild ELO for player {}: {}", user_id, e);
      } else {
        info!("Initialized guild ELO for player {} to {} (rank: {})", user_id, player.elo, assigned_rank.name);
      }
    }

    Ok(player)
  }

  pub async fn update_steam_id(&self, user_id: &UserId, steam_id: Option<u64>) -> Result<Player> {
    info!("Updating user steam_id for user_id: {}", user_id);

    sqlx::query("UPDATE users SET steam_id = ? WHERE user_id = ?").bind(steam_id.map(|id| id as i64)).bind(user_id.get() as i64).execute(&self.pool).await?;
    self.get(*user_id).await
  }

  /// Update user's discord tag in database
  /// Search users by tag or user_id substring (case-insensitive)
  pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<Player>> {
    let rows = sqlx::query(
      "SELECT user_id, steam_id, discord_tag, queue_expiration
             FROM users
             WHERE discord_tag LIKE ? OR CAST(user_id AS TEXT) LIKE ?
             LIMIT ?",
    )
    .bind(format!("%{}%", query))
    .bind(format!("%{}%", query))
    .bind(limit)
    .fetch_all(&self.pool)
    .await?;

    Ok(rows.into_iter().map(Self::get_player).collect())
  }

  pub async fn update_discord_tag(&self, user_id: UserId, discord_tag: &str) -> Result<()> {
    sqlx::query("UPDATE users SET discord_tag = ? WHERE user_id = ?").bind(discord_tag).bind(user_id.get() as i64).execute(&self.pool).await?;
    Ok(())
  }

  /// Get user tag - retrieves from database or fetches from Discord API
  pub async fn get_tag(&self, user_id: UserId, ctx: &Context) -> String {
    // Try to get from database first
    if let Ok(player) = self.get(user_id).await {
      if !player.tag.is_empty() {
        return player.tag;
      }
    }

    // Fallback to Discord API and cache it
    if let Ok(user) = ctx.http.get_user(user_id).await {
      let tag = user.tag();
      // Store it for future use
      let _ = self.update_discord_tag(user_id, &tag).await;
      return tag;
    }

    // Last resort: return user ID as string
    user_id.to_string()
  }

  pub async fn get_pm_hot_alert(&self, user_id: UserId) -> Result<bool> {
    let result = sqlx::query("SELECT pm_hot_alert FROM users WHERE user_id = ?").bind(user_id.get() as i64).fetch_optional(&self.pool).await?;

    match result {
      Some(row) => {
        let pm_hot_alert: i64 = row.try_get("pm_hot_alert").unwrap_or(0);
        Ok(pm_hot_alert != 0)
      }
      None => {
        // User doesn't exist yet, return default (disabled - opt-in)
        Ok(false)
      }
    }
  }

  pub async fn set_pm_hot_alert(&self, user_id: UserId, enabled: bool) -> Result<()> {
    // Ensure user exists
    let _ = self.check_user(user_id, None).await?;

    // Update database
    sqlx::query("UPDATE users SET pm_hot_alert = ? WHERE user_id = ?").bind(if enabled { 1 } else { 0 }).bind(user_id.get() as i64).execute(&self.pool).await?;

    Ok(())
  }

  pub async fn toggle_pm_hot_alert(&self, user_id: UserId) -> Result<bool> {
    // Ensure user exists
    let _ = self.check_user(user_id, None).await?;

    // Get current value
    let current = self.get_pm_hot_alert(user_id).await?;
    let new_value = !current;

    // Set new value
    self.set_pm_hot_alert(user_id, new_value).await?;

    Ok(new_value)
  }

  /// Get user settings
  pub async fn get_prefs(&self, user_id: UserId) -> Result<UserPreferences> {
    let result = sqlx::query(
      "SELECT queue_expiration, vc_auto_leave, vc_leave_queue, vc_auto_join,
                    join_alert_color, pm_hot_alert, pm_queue_alert_threshold,
                    join_alert_title, join_alert, join_alert_footer,
                    join_alert_footer_img, join_alert_img,
                    leave_alert_title, leave_alert,
                    leave_alert_footer, leave_alert_footer_img, leave_alert_img,
                    leave_alert_color
             FROM users WHERE user_id = ?",
    )
    .bind(user_id.get() as i64)
    .fetch_optional(&self.pool)
    .await?;

    match result {
      Some(row) => {
        // Load alert descriptions and truncate if too long
        let raw_join_alert: Option<String> = row.try_get::<String, _>("join_alert").ok().filter(|s| !s.is_empty());
        let raw_leave_alert: Option<String> = row.try_get::<String, _>("leave_alert").ok().filter(|s| !s.is_empty());
        let raw_alert_footer: Option<String> = row.try_get::<String, _>("join_alert_footer").ok().filter(|s| !s.is_empty());
        let raw_leave_alert_footer: Option<String> = row.try_get::<String, _>("leave_alert_footer").ok().filter(|s| !s.is_empty());

        let join_alert = raw_join_alert
          .as_ref()
          .map(|s| {
            let truncated = truncate_alert_message(s);
            if truncated.len() < s.len() {
              warn!("Truncated join_alert for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
            }
            truncated
          })
          .filter(|s| !s.is_empty());

        let leave_alert = raw_leave_alert
          .as_ref()
          .map(|s| {
            let truncated = truncate_alert_message(s);
            if truncated.len() < s.len() {
              warn!("Truncated leave_alert for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
            }
            truncated
          })
          .filter(|s| !s.is_empty());

        let join_alert_footer = raw_alert_footer
          .as_ref()
          .map(|s| {
            let truncated = truncate_footer_text(s);
            if truncated.len() < s.len() {
              warn!("Truncated join_alert_footer for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
            }
            truncated
          })
          .filter(|s| !s.is_empty());

        let leave_alert_footer = raw_leave_alert_footer
          .as_ref()
          .map(|s| {
            let truncated = truncate_footer_text(s);
            if truncated.len() < s.len() {
              warn!("Truncated leave_alert_footer for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
            }
            truncated
          })
          .filter(|s| !s.is_empty());

        // If truncation occurred, update the database
        let alert_truncated = raw_join_alert.as_ref().map(|s| truncate_alert_message(s).len() < s.len()).unwrap_or(false);
        let leave_truncated = raw_leave_alert.as_ref().map(|s| truncate_alert_message(s).len() < s.len()).unwrap_or(false);
        let footer_truncated = raw_alert_footer.as_ref().map(|s| truncate_footer_text(s).len() < s.len()).unwrap_or(false);
        let leave_footer_truncated = raw_leave_alert_footer.as_ref().map(|s| truncate_footer_text(s).len() < s.len()).unwrap_or(false);

        if alert_truncated {
          let _ = sqlx::query("UPDATE users SET join_alert = ? WHERE user_id = ?").bind(&join_alert).bind(user_id.get() as i64).execute(&self.pool).await;
        }

        if leave_truncated {
          let _ = sqlx::query("UPDATE users SET leave_alert = ? WHERE user_id = ?").bind(&leave_alert).bind(user_id.get() as i64).execute(&self.pool).await;
        }

        if footer_truncated {
          let _ = sqlx::query("UPDATE users SET join_alert_footer = ? WHERE user_id = ?").bind(&join_alert_footer).bind(user_id.get() as i64).execute(&self.pool).await;
        }

        if leave_footer_truncated {
          let _ = sqlx::query("UPDATE users SET leave_alert_footer = ? WHERE user_id = ?").bind(&leave_alert_footer).bind(user_id.get() as i64).execute(&self.pool).await;
        }

        Ok(UserPreferences {
          pm_hot_alert: row.try_get::<i64, _>("pm_hot_alert").unwrap_or(0) != 0,
          queue_expiration: row.try_get::<u8, _>("queue_expiration").unwrap_or(crate::DEFAULT_QUEUE_EXPIRATION),
          vc_auto_join: row.try_get::<i64, _>("vc_auto_join").unwrap_or(0) != 0,
          join_alert_title: row.try_get::<String, _>("join_alert_title").ok().filter(|s| !s.is_empty()),
          join_alert_desc: join_alert.clone(),
          join_alert_color: row.try_get::<u32, _>("join_alert_color").unwrap_or(DEFAULT_ALERT_COLOR),
          join_alert_img: row.try_get::<String, _>("join_alert_img").ok().filter(|s| !s.is_empty()),
          join_alert_footer: row.try_get::<String, _>("join_alert_footer").ok().filter(|s| !s.is_empty()),
          join_alert_footer_img: row.try_get::<String, _>("join_alert_footer_img").ok().filter(|s| !s.is_empty()),
          vc_auto_leave: row.try_get::<i64, _>("vc_auto_leave").unwrap_or(0) != 0,
          vc_leave_queue: row.try_get::<i64, _>("vc_leave_queue").unwrap_or(0) != 0,
          leave_alert_title: row.try_get::<String, _>("leave_alert_title").ok().filter(|s| !s.is_empty()),
          leave_alert_desc: leave_alert.clone(),
          leave_alert_color: row.try_get::<u32, _>("leave_alert_color").unwrap_or(DEFAULT_ALERT_COLOR),
          leave_alert_img: row.try_get::<String, _>("leave_alert_img").ok().filter(|s| !s.is_empty()),
          leave_alert_footer: row.try_get::<String, _>("leave_alert_footer").ok().filter(|s| !s.is_empty()),
          leave_alert_footer_img: row.try_get::<String, _>("leave_alert_footer_img").ok().filter(|s| !s.is_empty()),
        })
      }
      None => {
        // User doesn't exist, return defaults
        warn!("Can't find user_id {} in DB, using default preferences", user_id.get());
        Ok(UserPreferences::default())
      }
    }
  }

  pub async fn get_pref(&self, column: &str, user_id: UserId) -> Result<String> {
    let result = sqlx::query(
      "SELECT ?
            FROM users WHERE user_id = ?",
    )
    .bind(column)
    .bind(user_id.get() as i64)
    .fetch_optional(&self.pool)
    .await?;

    match result {
      Some(row) => Ok(row.try_get::<String, _>(column).ok().filter(|s| !s.is_empty()).unwrap()),
      None => {
        warn!("Can't find user_id {} in DB, using default preferences", user_id.get());
        Ok("".to_string())
      }
    }
  }

  /// Update user settings
  pub async fn update_prefs(&self, user_id: UserId, prefs: &UserPreferences) -> Result<()> {
    // Ensure user exists
    let _ = self.check_user(user_id, None).await?;

    sqlx::query(
      "UPDATE users SET
                pm_hot_alert           = ?,
                queue_expiration                = ?,
                vc_auto_join           = ?,
                join_alert_title       = ?,
                join_alert             = ?,
                join_alert_color       = ?,
                join_alert_img         = ?,
                join_alert_footer      = ?,
                join_alert_footer_img  = ?,
                vc_auto_leave          = ?,
                vc_leave_queue         = ?,
                leave_alert_title      = ?,
                leave_alert            = ?,
                leave_alert_color      = ?,
                leave_alert_img        = ?,
                leave_alert_footer     = ?,
                leave_alert_footer_img = ?
                WHERE user_id          = ?",
    )
    .bind(prefs.pm_hot_alert)
    .bind(prefs.queue_expiration)
    .bind(prefs.vc_auto_join)
    .bind(&prefs.join_alert_title)
    .bind(&prefs.join_alert_desc)
    .bind(prefs.join_alert_color)
    .bind(&prefs.join_alert_img)
    .bind(&prefs.join_alert_footer)
    .bind(&prefs.join_alert_footer_img)
    .bind(prefs.vc_auto_leave)
    .bind(prefs.vc_leave_queue)
    .bind(&prefs.leave_alert_title)
    .bind(&prefs.leave_alert_desc)
    .bind(prefs.leave_alert_color)
    .bind(&prefs.leave_alert_img)
    .bind(&prefs.leave_alert_footer)
    .bind(&prefs.leave_alert_footer_img)
    .bind(user_id.get() as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Update a single setting field
  pub async fn update_prefs_field(&self, user_id: UserId, field: &str, value: i64) -> Result<()> {
    // Ensure user exists
    let _ = self.check_user(user_id, None).await?;

    // Validate field name to prevent SQL injection
    let allowed_fields = ["queue_expiration", "join_alert", "vc_auto_leave", "join_alert_color", "pm_hot_alert"];
    if !allowed_fields.contains(&field) {
      return Err(anyhow::anyhow!("Invalid setting field: {}", field));
    }

    let query_str = format!("UPDATE users SET {} = ? WHERE user_id = ?", field);
    sqlx::query(&query_str).bind(value).bind(user_id.get() as i64).execute(&self.pool).await?;

    Ok(())
  }

  /// Get resolved VC preferences for a user in a specific server
  /// Resolution order: per-server user override > server default > global user pref > hardcoded default
  /// Returns (vc_auto_join, vc_auto_leave, vc_leave_queue)
  pub async fn get_resolved_vc_prefs(
    &self,
    user_id: UserId,
    guild_id: serenity::all::GuildId,
    user_server_prefs_repo: &crate::db::repo::UserServerPrefsRepository,
    config_repo: &crate::db::repo::ConfigRepository,
  ) -> Result<(bool, bool, bool)> {
    // Get per-server user overrides
    let (server_auto_join, server_auto_leave, server_leave_queue) = user_server_prefs_repo.get_all_vc_prefs(user_id, guild_id).await?;

    // Get server defaults
    let server_default_auto_join = config_repo.get_default_vc_auto_join(guild_id).await.unwrap_or(false);
    let server_default_auto_leave = config_repo.get_default_vc_auto_leave(guild_id).await.unwrap_or(false);
    let server_default_leave_queue = config_repo.get_default_vc_leave_queue(guild_id).await.unwrap_or(false);

    // Get global user preferences as fallback
    let global_prefs = self.get_prefs(user_id).await.unwrap_or_default();

    // Resolve each preference
    let vc_auto_join = server_auto_join.unwrap_or_else(|| server_default_auto_join);
    let vc_auto_leave = server_auto_leave.unwrap_or_else(|| server_default_auto_leave);
    let vc_leave_queue = server_leave_queue.unwrap_or_else(|| server_default_leave_queue);

    Ok((vc_auto_join, vc_auto_leave, vc_leave_queue))
  }
}

/// User settings structure
#[derive(Debug, Clone)]
pub struct UserPreferences {
  pub pm_hot_alert: bool,
  pub queue_expiration: u8,
  pub vc_auto_join: bool,
  pub join_alert_title: Option<String>,
  pub join_alert_desc: Option<String>,
  pub join_alert_color: u32,
  pub join_alert_img: Option<String>,
  pub join_alert_footer: Option<String>,
  pub join_alert_footer_img: Option<String>,
  pub vc_auto_leave: bool,
  pub vc_leave_queue: bool,
  pub leave_alert_title: Option<String>,
  pub leave_alert_desc: Option<String>,
  pub leave_alert_color: u32,
  pub leave_alert_img: Option<String>,
  pub leave_alert_footer: Option<String>,
  pub leave_alert_footer_img: Option<String>,
}

impl Default for UserPreferences {
  fn default() -> Self {
    Self {
      pm_hot_alert: false,
      queue_expiration: crate::DEFAULT_QUEUE_EXPIRATION,
      vc_auto_join: false,
      join_alert_title: None,
      join_alert_desc: None,
      join_alert_color: DEFAULT_ALERT_COLOR,
      join_alert_img: None,
      join_alert_footer: None,
      join_alert_footer_img: None,
      vc_auto_leave: false,
      vc_leave_queue: false,
      leave_alert_title: None,
      leave_alert_desc: None,
      leave_alert_color: DEFAULT_ALERT_COLOR,
      leave_alert_img: None,
      leave_alert_footer: None,
      leave_alert_footer_img: None,
    }
  }
}

#[async_trait]
impl Repository<Player, UserId> for PlayerRepository {
  async fn create(&self, player: &Player) -> Result<Player> {
    self.check_user(player.user_id, player.steam_id).await
  }

  async fn get_by_id(&self, user_id: UserId) -> Result<Player> {
    self.get(user_id).await
  }

  async fn update(&self, player: &Player) -> Result<Player> {
    self.update_steam_id(&player.user_id, player.steam_id).await?;
    Ok(player.clone())
  }

  async fn delete(&self, user_id: UserId) -> Result<()> {
    sqlx::query("DELETE FROM users WHERE user_id = ?").bind(user_id.get() as i64).execute(&self.pool).await?;
    Ok(())
  }
}
