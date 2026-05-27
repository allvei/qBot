use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serenity::all::{ChannelId as CI, GuildId as GI, MessageId as MI};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use super::Repository;
use crate::models::{Category, Channels, TeamChannel};
use crate::{log_prefix_category, log_prefix_guild};

/// Configuration for creating or updating a category
pub struct CategoryConfig {
  pub channel_category_id: u64,
  pub dashboard_channel_id: u64,
  pub chat_channel_id: u64,
  pub queue_vc_id: u64,
  pub ping_channel_id: u64,
  pub quota: u8,
}

#[derive(Clone)]
pub struct CategoryRepository {
  pool: SqlitePool,
}

impl CategoryRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  // Add a category to the database
  pub async fn add_category(&self, guild_id: GI, guild_name: &str, dashboard_msg: u64, config: CategoryConfig) -> Result<Category> {
    info!("Adding a new category with queue: {}", config.queue_vc_id);

    // Get the next available category_id for this guild
    let next_category_id: i64 =
      sqlx::query_scalar("SELECT COALESCE(MAX(category_id), -1) + 1 FROM categories WHERE guild_id = ?").bind(guild_id.get() as i64).fetch_one(&self.pool).await?;

    sqlx::query(
      "INSERT INTO categories (guild_id, category_id, category, dashboard, chat, queue, ping, dashboard_msg, quota)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(guild_id.get() as i64)
    .bind(next_category_id)
    .bind(config.channel_category_id as i64)
    .bind(config.dashboard_channel_id as i64)
    .bind(config.chat_channel_id as i64)
    .bind(config.queue_vc_id as i64)
    .bind(config.ping_channel_id as i64)
    .bind(dashboard_msg as i64)
    .bind(config.quota as i64)
    .execute(&self.pool)
    .await?;

    // Build the category directly from known values instead of parsing the
    // RETURNING row. A brand-new category has no formats, teams, or other
    // secondary data in the DB, so build_category_from_row_async would only
    // add risk of reading stale/wrong data (e.g. category_id defaulting to 0
    // via try_get fallback, which loads another category's formats).
    let category_id = next_category_id as u8;
    let category = if config.channel_category_id == 0 { CI::new(config.dashboard_channel_id) } else { CI::new(config.channel_category_id) };

    let category = Category::new(
      guild_id,
      Some(guild_name.to_string()),
      category_id,
      None,
      config.quota,
      crate::DEFAULT_CONFIRM_TIME, // default confirm_time
      MI::new(dashboard_msg),
      Channels::new(category, CI::new(config.chat_channel_id), CI::new(config.queue_vc_id), CI::new(config.ping_channel_id), vec![], CI::new(config.dashboard_channel_id)),
      Vec::new(),
    );

    info!("Created category {} with {} format(s)", category_id, category.formats.len());
    Ok(category)
  }
  // Update a category in the database
  pub async fn update_category(&self, guild_id: GI, config: CategoryConfig) -> Result<Category> {
    info!("Updating category with queue_id: {}", config.queue_vc_id);

    let result = sqlx::query("UPDATE categories
                                  SET guild_id = ?, category = ?, dashboard = ?, chat = ?, ping = ?, quota = ?
                                  WHERE queue = ?
                                  RETURNING id, category_id, name, confirm_time, guild_id, category, dashboard, chat, queue, ping, dashboard_msg, game_increment, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum, require_score_report"
        )
        .bind(guild_id.get()              as i64)
        .bind(config.channel_category_id          as i64)
        .bind(config.dashboard_channel_id as i64)
        .bind(config.chat_channel_id      as i64)
        .bind(config.ping_channel_id      as i64)
        .bind(config.quota                as i64)
        .bind(config.queue_vc_id          as i64)
        .fetch_one(&self.pool)
        .await?;

    self.build_category_from_row_async(&result).await
  }
  /// Delete a category from the database
  pub async fn remove_category(&self, guild_id: GI, category_id: u8) -> Result<()> {
    sqlx::query("DELETE FROM categories WHERE guild_id = ? AND category_id = ?").bind(guild_id.get() as i64).bind(category_id as i64).execute(&self.pool).await?;

    Ok(())
  }

  async fn build_category_from_row_async(&self, result: &sqlx::sqlite::SqliteRow) -> Result<Category> {
    // Validate channel IDs before creating ChannelId objects
    let chat_id = result.get::<i64, _>("chat") as u64;
    let queue_id = result.get::<i64, _>("queue") as u64;
    let ping_id = result.try_get::<i64, _>("ping").unwrap_or(1) as u64;
    let dashboard_id = result.get::<i64, _>("dashboard") as u64;
    let dashboard_msg_id = result.get::<i64, _>("dashboard_msg") as u64;

    let invalid_ids = [(chat_id == 0, "chat"), (queue_id == 0, "queue"), (ping_id == 0, "ping"), (dashboard_id == 0, "dashboard"), (dashboard_msg_id == 0, "dashboard_msg")];
    if let Some((true, id)) = invalid_ids.iter().find(|(is_zero, _)| *is_zero) {
      return Err(anyhow!("Category has invalid {} configuration (0 ID not allowed)", id));
    }

    let chat = CI::new(chat_id);
    let queue = CI::new(queue_id);
    let ping = CI::new(ping_id);
    let dashboard = CI::new(dashboard_id);
    let category_id = result.try_get::<i64, _>("category").unwrap_or(0) as u64;
    let category = if category_id == 0 { dashboard } else { CI::new(category_id) };

    let guild_id = GI::new(result.get::<i64, _>("guild_id") as u64);
    let category_id = result.get::<i64, _>("category_id") as u8;
    let name = result.try_get::<Option<String>, _>("name").ok().flatten();
    let connect_info = result.try_get::<Option<String>, _>("connect_info").ok().flatten();

    // TODO: teams are loaded in teams.rs not here
    // Load teams from teams table; fallback to legacy red/blu columns only if they hold real IDs
    let teams = self.get_teams_for_category(guild_id, category_id).await?;

    let mut category = Category::new(
      guild_id,
      None,
      category_id,
      name,
      result.try_get::<i64, _>("quota").unwrap_or(8) as u8,
      result.try_get::<i64, _>("confirm_time").unwrap_or(crate::DEFAULT_CONFIRM_TIME as i64) as u16,
      MI::new(dashboard_msg_id),
      Channels::new(category, chat, queue, ping, teams, dashboard),
      Vec::new(),
    );

    // Load team VC lifecycle settings
    {
      use crate::models::{TeamVcCreatePolicy, TeamVcDestroyPolicy, TeamVcSettings};
      let create_policy = result.try_get::<String, _>("team_vc_create_policy").ok().map(|s| TeamVcCreatePolicy::parse(&s)).unwrap_or_default();
      let destroy_policy = result.try_get::<String, _>("team_vc_destroy_policy").ok().map(|s| TeamVcDestroyPolicy::parse(&s)).unwrap_or_default();
      let keep_minimum = result.try_get::<i64, _>("team_vc_keep_minimum").unwrap_or(1) != 0;
      category.team_vc_settings = TeamVcSettings { create_policy, destroy_policy, keep_minimum };
    }

    // Load formats from DB; if present, replace the default format
    match self.get_formats(guild_id, category_id).await {
      Ok(sgs) if !sgs.is_empty() => {
        category.formats = sgs;
        // Apply category-level connect_info to all formats that don't have their own
        if let Some(ref cat_connect_info) = connect_info {
          for fmt in &mut category.formats {
            if fmt.connect_info.is_none() || fmt.connect_info.as_ref().is_none_or(|s| s.trim().is_empty()) {
              fmt.connect_info = Some(cat_connect_info.clone());
            }
          }
        }
      }
      _ => {
        // No DB formats yet - keep the default created by Category::new
        // and apply connect_info from the categories table to all formats
        let category_name = category.name.as_deref().unwrap_or("Unknown");
        info!("{} Using default formats", log_prefix_category("Unknown", category_name));
        if let Some(ref cat_connect_info) = connect_info {
          for fmt in &mut category.formats {
            fmt.connect_info = Some(cat_connect_info.clone());
          }
        }
      }
    }

    // Load DM alert settings
    category.dm_alert_enabled = result.try_get::<i64, _>("dm_alert_enabled").unwrap_or(0) != 0;
    category.dm_alert_threshold = result.try_get::<i64, _>("dm_alert_threshold").unwrap_or(0) as u8;

    // Parse dm_alert_users as JSON array
    if let Ok(users_json) = result.try_get::<String, _>("dm_alert_users") {
      if let Ok(users) = serde_json::from_str::<Vec<serenity::all::UserId>>(&users_json) {
        category.dm_alert_users = users;
      }
    }

    // Load require_score_report setting
    category.require_score_report = result.try_get::<i64, _>("require_score_report").unwrap_or(0) != 0;

    Ok(category)
  }

  fn build_teams_from_row(&self, result: &sqlx::sqlite::SqliteRow) -> Result<TeamChannel> {
    let red = CI::new(result.get::<i64, _>("red") as u64);
    let blu = CI::new(result.get::<i64, _>("blu") as u64);
    let set_index = result.get::<Option<i32>, _>("set_index").unwrap_or(1) as u32;
    let team = TeamChannel::new(red, blu, set_index);
    Ok(team)
  }

  pub async fn get_teams_for_category(&self, guild_id: GI, category_id: u8) -> Result<Vec<TeamChannel>> {
    let rows = sqlx::query("SELECT red, blu, set_index FROM teams WHERE guild_id = ? AND category_id = ?")
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .fetch_all(&self.pool)
      .await?;

    let mut teams = Vec::new();
    for row in rows {
      teams.push(self.build_teams_from_row(&row)?);
    }

    Ok(teams)
  }

  pub async fn insert_team(&self, guild_id: GI, category_id: u8, red: CI, blu: CI) -> Result<()> {
    sqlx::query("INSERT INTO teams (guild_id, category_id, red, blu) VALUES (?, ?, ?, ?)")
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .bind(red.get() as i64)
      .bind(blu.get() as i64)
      .execute(&self.pool)
      .await?;
    Ok(())
  }

  pub async fn delete_team(&self, guild_id: GI, category_id: u8, red: CI, blu: CI) -> Result<()> {
    sqlx::query("DELETE FROM teams WHERE guild_id = ? AND category_id = ? AND red = ? AND blu = ?")
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .bind(red.get() as i64)
      .bind(blu.get() as i64)
      .execute(&self.pool)
      .await?;
    Ok(())
  }

  pub async fn clear_teams(&self, guild_id: GI, category_id: u8) -> Result<()> {
    sqlx::query("DELETE FROM teams WHERE guild_id = ? AND category_id = ?").bind(guild_id.get() as i64).bind(category_id as i64).execute(&self.pool).await?;
    Ok(())
  }

  /// Check if a category exists for a guild
  pub async fn category_exists_for_guild(&self, guild_id: GI) -> Result<bool> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE guild_id = ?").bind(guild_id.get() as i64).fetch_one(&self.pool).await?;

    Ok(count > 0)
  }

  /// Get all categories for a guild
  pub async fn get_categories_for_guild(&self, guild_id: GI) -> Result<Vec<Category>> {
    let rows = sqlx::query("SELECT id, guild_id, category_id, name, confirm_time, category, dashboard, chat, queue, ping, dashboard_msg, game_increment, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum, require_score_report
                                FROM categories
                                WHERE guild_id = ?"
        )
        .bind(guild_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

    // Fetch guild-wide settings from their proper tables
    let guild_name: Option<String> = sqlx::query_scalar("SELECT name FROM guilds WHERE guild_id = ?").bind(guild_id.get() as i64).fetch_optional(&self.pool).await.ok().flatten();

    let team_balance_method: String = sqlx::query_scalar("SELECT COALESCE(team_balance_method, 'bch') FROM config WHERE guild_id = ?")
      .bind(guild_id.get() as i64)
      .fetch_optional(&self.pool)
      .await
      .ok()
      .flatten()
      .unwrap_or_else(|| "bch".to_string());

    let mut categories = Vec::new();
    for row in rows {
      match self.build_category_from_row_async(&row).await {
        Ok(mut category) => {
          category.guild_name = guild_name.clone();
          category.team_balance_method = crate::models::TeamBalanceMethod::parse(&team_balance_method);
          categories.push(category);
        }
        Err(e) => {
          let category_id: i64 = row.try_get("category_id").unwrap_or(0);
          let queue_id: i64 = row.try_get("queue").unwrap_or(0);
          warn!("Skipping invalid category {} (queue {}) for guild {}: {}", category_id, queue_id, guild_id, e);
        }
      }
    }

    Ok(categories)
  }

  /// Update dashboard message ID for a category by its dashboard channel ID
  pub async fn update_dashboard_msg(&self, guild_id: GI, dashboard_channel_id: u64, dashboard_msg_id: u64) -> Result<()> {
    // Get guild name for logging
    let guild_name: String = sqlx::query_scalar("SELECT name FROM guilds WHERE guild_id = ?")
      .bind(guild_id.get() as i64)
      .fetch_optional(&self.pool)
      .await
      .ok()
      .flatten()
      .unwrap_or_else(|| guild_id.to_string());

    info!("{} Updating dashboard message ID for dashboard channel {}", log_prefix_guild(&guild_name), dashboard_channel_id);

    sqlx::query("UPDATE categories SET dashboard_msg = ? WHERE guild_id = ? AND dashboard = ?")
      .bind(dashboard_msg_id as i64)
      .bind(guild_id.get() as i64)
      .bind(dashboard_channel_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Update dashboard message ID for a category by its category_id
  pub async fn update_dashboard_msg_by_category_id(&self, guild_id: GI, category_id: u8, dashboard_msg_id: u64) -> Result<()> {
    info!("Updating dashboard message ID for guild {} category {}", guild_id, category_id);

    sqlx::query("UPDATE categories SET dashboard_msg = ? WHERE guild_id = ? AND category_id = ?")
      .bind(dashboard_msg_id as i64)
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Update category name
  pub async fn update_name(&self, guild_id: GI, category_id: u8, name: Option<&str>) -> Result<()> {
    info!("Updating name for guild {} category {}: {:?}", guild_id, category_id, name);

    sqlx::query("UPDATE categories SET name = ? WHERE guild_id = ? AND category_id = ?")
      .bind(name)
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Update category quota
  pub async fn update_quota(&self, guild_id: GI, category_id: u8, quota: u8) -> Result<()> {
    info!("Updating quota for guild {} category {}: {}", guild_id, category_id, quota);

    sqlx::query("UPDATE categories SET quota = ? WHERE guild_id = ? AND category_id = ?")
      .bind(quota as i64)
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Update category confirm_time
  pub async fn update_confirm_time(&self, guild_id: GI, category_id: u8, confirm_time: u16) -> Result<()> {
    info!("Updating confirm_time for guild {} category {}: {}", guild_id, category_id, confirm_time);

    sqlx::query("UPDATE categories SET confirm_time = ? WHERE guild_id = ? AND category_id = ?")
      .bind(confirm_time as i64)
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Update category connect info
  pub async fn update_connect_info(&self, guild_id: GI, category_id: u8, connect_info: Option<&str>) -> Result<()> {
    info!("Updating connect_info for guild {} category {}: {:?}", guild_id, category_id, connect_info);

    sqlx::query("UPDATE categories SET connect_info = ? WHERE guild_id = ? AND category_id = ?")
      .bind(connect_info)
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  pub async fn update_team_vc_settings(&self, guild_id: GI, category_id: u8, settings: &crate::models::TeamVcSettings) -> Result<()> {
    sqlx::query("UPDATE categories SET team_vc_create_policy = ?, team_vc_destroy_policy = ?, team_vc_keep_minimum = ? WHERE guild_id = ? AND category_id = ?")
      .bind(settings.create_policy.to_db_str())
      .bind(settings.destroy_policy.to_db_str())
      .bind(if settings.keep_minimum { 1i64 } else { 0i64 })
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }
}

#[async_trait]
impl Repository<Category, u8> for CategoryRepository {
  async fn create(&self, category: &Category) -> Result<Category> {
    // Extract values from the category struct
    let guild_id = category.guild_id;
    let dashboard_channel_id = category.channels.dashboard.get();
    let dashboard_msg = category.dashboard_msg.get();
    let chat_channel_id = category.channels.queue_chat.get();
    let queue_vc_id = category.channels.queue_vc.get();
    let ping_channel_id = category.channels.ping_channel.get();
    let channel_category_id = category.channels.category.get();
    let config = CategoryConfig { channel_category_id, dashboard_channel_id, chat_channel_id, queue_vc_id, ping_channel_id, quota: category.quota() };
    self.add_category(guild_id, category.guild_name.as_deref().unwrap_or("Unknown"), dashboard_msg, config).await
  }

  async fn get_by_id(&self, category_id: u8) -> Result<Category> {
    let result = sqlx::query("SELECT id, category_id, name, confirm_time, guild_id, category, dashboard, chat, queue, ping, dashboard_msg, game_increment, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum, require_score_report
                                  FROM categories WHERE category_id = ?"
        )
        .bind(category_id as i64)
        .fetch_one(&self.pool)
        .await?;

    self.build_category_from_row_async(&result).await
  }

  async fn update(&self, category: &Category) -> Result<Category> {
    let guild_id = category.guild_id;
    let dashboard_channel_id = category.channels.dashboard.get();
    let chat_channel_id = category.channels.queue_chat.get();
    let queue_vc_id = category.channels.queue_vc.get();
    let ping_channel_id = category.channels.ping_channel.get();
    let channel_category_id = category.channels.category.get();
    let config = CategoryConfig { channel_category_id, dashboard_channel_id, chat_channel_id, queue_vc_id, ping_channel_id, quota: category.quota() };
    self.update_category(guild_id, config).await
  }

  async fn delete(&self, category_id: u8) -> Result<()> {
    sqlx::query("DELETE FROM categories WHERE category_id = ?").bind(category_id as i64).execute(&self.pool).await?;
    Ok(())
  }
}

impl CategoryRepository {
  /// Update DM alert settings for a category
  pub async fn update_dm_alert_settings(&self, guild_id: GI, category_id: u8, enabled: bool, threshold: u8, users: Vec<serenity::all::UserId>) -> Result<()> {
    let users_json = serde_json::to_string(&users)?;

    sqlx::query(
      "UPDATE categories SET dm_alert_enabled = ?, dm_alert_threshold = ?, dm_alert_users = ? 
             WHERE guild_id = ? AND category_id = ?",
    )
    .bind(enabled as i64)
    .bind(threshold as i64)
    .bind(users_json)
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  // ========================================================================
  // Format methods
  // ========================================================================

  /// Get all formats for a category
  pub async fn get_formats(&self, guild_id: GI, category_id: u8) -> Result<Vec<crate::models::Format>> {
    let rows = sqlx::query(
      "SELECT format_id, name, quota, connect_info FROM formats
             WHERE guild_id = ? AND category_id = ?
             ORDER BY format_id",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .fetch_all(&self.pool)
    .await?;

    let mut formats = Vec::new();
    for row in rows {
      let id: u8 = row.get::<i64, _>("format_id") as u8;
      let name: String = row.get("name");
      let quota: u8 = row.get::<i64, _>("quota") as u8;
      let connect_info: Option<String> = row.try_get("connect_info").ok().flatten();
      let mut sg = crate::models::Format::new(id, name, quota);
      sg.connect_info = connect_info;
      formats.push(sg);
    }

    Ok(formats)
  }

  /// Save a single format (upsert)
  pub async fn save_format(&self, guild_id: GI, category_id: u8, sg: &crate::models::Format) -> Result<()> {
    sqlx::query(
      "INSERT INTO formats (guild_id, category_id, format_id, name, quota, connect_info)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(guild_id, category_id, format_id) DO UPDATE SET
                name = excluded.name,
                quota = excluded.quota,
                connect_info = excluded.connect_info",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .bind(sg.id as i64)
    .bind(&sg.name)
    .bind(sg.quota as i64)
    .bind(&sg.connect_info)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Delete a format
  pub async fn delete_format(&self, guild_id: GI, category_id: u8, format_id: u8) -> Result<()> {
    sqlx::query("DELETE FROM formats WHERE guild_id = ? AND category_id = ? AND format_id = ?")
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .bind(format_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Save all formats for a category (replaces existing)
  pub async fn save_all_formats(&self, guild_id: GI, category_id: u8, formats: &[crate::models::Format]) -> Result<()> {
    // Delete all existing formats for this category
    sqlx::query("DELETE FROM formats WHERE guild_id = ? AND category_id = ?").bind(guild_id.get() as i64).bind(category_id as i64).execute(&self.pool).await?;

    // Insert all current formats
    for sg in formats {
      self.save_format(guild_id, category_id, sg).await?;
    }

    Ok(())
  }

  /// Get DM alert settings for a category
  pub async fn get_dm_alert_settings(&self, guild_id: GI, category_id: u8) -> Result<(bool, u8, Vec<serenity::all::UserId>)> {
    let result = sqlx::query(
      "SELECT dm_alert_enabled, dm_alert_threshold, dm_alert_users 
             FROM categories WHERE guild_id = ? AND category_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id as i64)
    .fetch_one(&self.pool)
    .await?;

    let enabled = result.get::<i64, _>("dm_alert_enabled") != 0;
    let threshold = result.get::<i64, _>("dm_alert_threshold") as u8;
    let users_json: String = result.get("dm_alert_users");
    let users = serde_json::from_str(&users_json).unwrap_or_default();

    Ok((enabled, threshold, users))
  }

  /// Update require_score_report setting for a category
  pub async fn update_require_score_report(&self, guild_id: GI, category_id: u8, require_score_report: bool) -> Result<()> {
    info!("Updating require_score_report for guild {} category {}: {}", guild_id, category_id, require_score_report);

    sqlx::query("UPDATE categories SET require_score_report = ? WHERE guild_id = ? AND category_id = ?")
      .bind(require_score_report as i64)
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .execute(&self.pool)
      .await?;

    Ok(())
  }

  /// Get require_score_report setting for a category
  pub async fn get_require_score_report(&self, guild_id: GI, category_id: u8) -> Result<bool> {
    let result: i64 = sqlx::query_scalar("SELECT require_score_report FROM categories WHERE guild_id = ? AND category_id = ?")
      .bind(guild_id.get() as i64)
      .bind(category_id as i64)
      .fetch_one(&self.pool)
      .await?;

    Ok(result != 0)
  }
}
