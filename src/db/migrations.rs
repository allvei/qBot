use anyhow::Result;
use serenity::all::GuildId as GI;
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::db::helpers::MigrationHelpers;
use crate::{add_columns, DEFAULT_CONFIRM_TIME};

macro_rules! add_column {
  ($self:ident, $table:literal, $name:literal, $type:literal, $default:literal) => {
    if !$self.check_column($table, $name).await? {
      sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} DEFAULT {}", $table, $name, $type, $default)).execute(&$self.pool).await?;
    }
  };
}

macro_rules! add_column_not_null {
  ($self:ident, $table:literal, $name:literal, $type:literal, $default:literal) => {
    if !$self.check_column($table, $name).await? {
      sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} NOT NULL DEFAULT {}", $table, $name, $type, $default)).execute(&$self.pool).await?;
    }
  };
}

/// Database migration system for managing schema changes
pub struct DatabaseMigrations {
  pool: SqlitePool,
}

impl DatabaseMigrations {
  /// Runtime version of add_column that doesn't require literals
  async fn add_column_runtime(&self, table: &str, name: &str, sql_type: &str, default: &str) -> Result<()> {
    if !self.check_column(table, name).await? {
      let query = format!("ALTER TABLE {} ADD COLUMN {} {} DEFAULT {}", table, name, sql_type, default);
      sqlx::query(&query).execute(&self.pool).await?;
    }
    Ok(())
  }
}

impl MigrationHelpers for DatabaseMigrations {
  /// Add a column if it doesn't exist
  async fn add_column_if_missing(&self, table: &str, column: &str, column_type: &str, default: &str) -> Result<()> {
    if !self.check_column(table, column).await? {
      sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} DEFAULT {}", table, column, column_type, default)).execute(&self.pool).await?;
    }
    Ok(())
  }

  /// Add multiple columns from a list of (column, type, default) tuples
  async fn add_columns_if_missing(&self, table: &str, columns: &[(&str, &str, &str)]) -> Result<()> {
    for (column, column_type, default) in columns {
      self.add_column_if_missing(table, column, column_type, default).await?;
    }
    Ok(())
  }
}

impl DatabaseMigrations {
  pub fn new(pool: &SqlitePool) -> Self {
    Self { pool: pool.clone() }
  }

  // MASTERS

  /// Run all migrations in order
  pub async fn create_tables(&self) -> Result<()> {
    self.create_config_table().await?;
    self.create_users_table().await?;
    self.create_user_server_prefs_table().await?;
    self.create_categories_table().await?;
    self.create_teams_table().await?;
    self.create_formats_table().await?;
    self.create_elo_table().await?;
    self.create_ranks_table().await?;
    self.create_matches_table().await?;
    self.create_match_players_table().await?;
    self.create_fatkid_table().await?;
    self.create_guilds_table().await?;

    // Add foreign key constraint after both tables exist
    self.add_config_foreign_key().await?;

    Ok(())
  }
  pub async fn verify_schemas(&self) -> Result<()> {
    self.verify_config().await?;
    self.verify_users().await?;
    self.verify_categories().await?;
    self.verify_teams().await?;
    self.verify_formats().await?;
    self.verify_elos().await?;
    self.verify_ranks().await?;
    self.verify_matches().await?;
    self.verify_match_players().await?;
    self.verify_fatkids().await?;
    self.verify_guilds().await?;
    Ok(())
  }

  // CREATE TABLES

  async fn create_config_table(&self) -> Result<()> {
    if !self.check_table("config").await? {
      sqlx::query(
        "CREATE TABLE config (
          guild_id             INTEGER NOT NULL,
          runner_id            INTEGER,
          admin_id             INTEGER,
          active_elo           INTEGER DEFAULT 0,
          default_rank         INTEGER,
          elo_ranks_linked     INTEGER DEFAULT 1,
          post_game_auto_leave INTEGER DEFAULT 1,
          team_balance_method  TEXT DEFAULT 'bch',
          hide_elo             INTEGER DEFAULT 0,
          PRIMARY KEY(guild_id),
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;
    } else if !self.check_column("config", "guild_id").await? {
      add_column_not_null!(self, "config", "guild_id", "INTEGER", "0");
    }
    Ok(())
  }
  async fn verify_config(&self) -> Result<()> {
    // Base columns that are not in config_schema (roles, etc.)
    let mut required_columns = vec![
      "guild_id",
      "runner_id",
      "admin_id",
      "active_elo",
      "default_rank",
      "system_message_channel",
    ];
    
    // Add system_message_channel manually (not a boolean toggle)
    add_column!(self, "config", "system_message_channel", "INTEGER", "NULL");

    // Automatically add all columns from config_schema
    use crate::config_schema::{server_config, sql_type_for_rust_type, sql_default_for_value};
    for (column, rust_type, default_value) in server_config::COLUMNS {
      required_columns.push(column);
      let sql_type = sql_type_for_rust_type(rust_type);
      let sql_default = sql_default_for_value(default_value, rust_type);
      self.add_column_runtime("config", column, sql_type, &sql_default).await?;
    }
    
    self.verify_columns("config", &required_columns).await?;
    Ok(())
  }

  async fn create_users_table(&self) -> Result<()> {
    if !self.check_table("users").await? {
      sqlx::query(
        "CREATE TABLE users (
          user_id                  INTEGER PRIMARY KEY,
          steam_id                 INTEGER,
          discord_tag              TEXT    DEFAULT NULL,
          pm_hot_alert             INTEGER DEFAULT 0,
          pm_queue_alert_threshold INTEGER DEFAULT NULL,
          queue_expiration         INTEGER DEFAULT 30,
          vc_auto_join             INTEGER DEFAULT 0,
          join_alert_title         TEXT    DEFAULT NULL,
          join_alert               TEXT    DEFAULT NULL,
          join_alert_color         INTEGER DEFAULT 3447003,
          join_alert_img           TEXT    DEFAULT NULL,
          join_alert_footer        TEXT    DEFAULT NULL,
          join_alert_footer_img    TEXT    DEFAULT NULL,
          vc_auto_leave            INTEGER DEFAULT 0,
          vc_leave_queue           INTEGER DEFAULT 0,
          post_game_auto_leave     INTEGER DEFAULT 1,
          leave_alert_title        TEXT    DEFAULT NULL,
          leave_alert              TEXT    DEFAULT NULL,
          leave_alert_color        INTEGER DEFAULT 3447003,
          leave_alert_img          TEXT    DEFAULT NULL,
          leave_alert_footer       TEXT    DEFAULT NULL,
          leave_alert_footer_img   TEXT    DEFAULT NULL
        )",
      )
      .execute(&self.pool)
      .await?;
    } else {
      // Verify schema integrity
      let has_unique = self.check_unique("users", "user_id").await?;
      let has_user_id = self.check_column("users", "user_id").await?;
      let has_steam_id = self.check_column("users", "steam_id").await?;
      let has_pm_hot_alert = self.check_column("users", "pm_hot_alert").await?;

      // Add pm_hot_alert column if missing
      if has_user_id && !has_pm_hot_alert {
        add_column!(self, "users", "pm_hot_alert", "INTEGER", "0");
      }

      // Add new settings columns if missing
      if has_user_id {
        // Manually added columns (not in config_schema)
        add_column!(self, "users", "discord_tag", "TEXT", "NULL");
        add_column!(self, "users", "pm_queue_alert_threshold", "INTEGER", "NULL");
        add_column!(self, "users", "join_alert_title", "TEXT", "NULL");
        add_column!(self, "users", "join_alert", "TEXT", "NULL");
        add_column!(self, "users", "join_alert_color", "INTEGER", "3447003");
        add_column!(self, "users", "join_alert_img", "TEXT", "NULL");
        add_column!(self, "users", "join_alert_footer", "TEXT", "NULL");
        add_column!(self, "users", "join_alert_footer_img", "TEXT", "NULL");
        add_column!(self, "users", "post_game_auto_leave", "INTEGER", "1");
        add_column!(self, "users", "leave_alert_title", "TEXT", "NULL");
        add_column!(self, "users", "leave_alert", "TEXT", "NULL");
        add_column!(self, "users", "leave_alert_color", "INTEGER", "3447003");
        add_column!(self, "users", "leave_alert_img", "TEXT", "NULL");
        add_column!(self, "users", "leave_alert_footer", "TEXT", "NULL");
        add_column!(self, "users", "leave_alert_footer_img", "TEXT", "NULL");
        
        // Automatically add columns from user_preferences schema
        use crate::config_schema::{user_preferences, sql_type_for_rust_type, sql_default_for_value};
        for (column, rust_type, global_table, _override_table, default_value) in user_preferences::COLUMNS {
          if *global_table == "users" {
            let sql_type = sql_type_for_rust_type(rust_type);
            let sql_default = sql_default_for_value(default_value, rust_type);
            self.add_column_runtime("users", column, sql_type, &sql_default).await?;
          }
        }
      }

      // Drop old elo column if it exists (ELO is now in elo table)
      if has_user_id && self.check_column("users", "elo").await? {
        sqlx::query("ALTER TABLE users DROP COLUMN elo").execute(&self.pool).await.ok();
        // Ignore errors if column doesn't exist
      }

      if !has_user_id || !has_steam_id || !has_unique {
        // Backup existing data if any
        let backup_data = if has_user_id { sqlx::query("SELECT user_id, steam_id FROM users").fetch_all(&self.pool).await.unwrap_or_default() } else { Vec::new() };

        // Drop and recreate table
        sqlx::query("DROP TABLE users").execute(&self.pool).await?;
        sqlx::query(
          "CREATE TABLE users (
                        user_id                  INTEGER PRIMARY KEY,
                        steam_id                 INTEGER,
                        discord_tag              TEXT    DEFAULT NULL,
                        pm_hot_alert             INTEGER DEFAULT 1,
                        pm_queue_alert_threshold INTEGER DEFAULT NULL,
                        queue_expiration                  INTEGER DEFAULT 30,
                        vc_auto_join             INTEGER DEFAULT 0,
                        join_alert_title         TEXT    DEFAULT NULL,
                        join_alert               TEXT    DEFAULT NULL,
                        join_alert_color         INTEGER DEFAULT 3447003,
                        join_alert_img           TEXT    DEFAULT NULL,
                        join_alert_footer        TEXT    DEFAULT NULL,
                        join_alert_footer_img    TEXT    DEFAULT NULL,
                        vc_auto_leave            INTEGER DEFAULT 0,
                        vc_leave_queue           INTEGER DEFAULT 0,
                        post_game_auto_leave    INTEGER DEFAULT 1,
                        leave_alert_title        TEXT    DEFAULT NULL,
                        leave_alert              TEXT    DEFAULT NULL,
                        leave_alert_color        INTEGER DEFAULT 3447003,
                        leave_alert_img          TEXT    DEFAULT NULL,
                        leave_alert_footer       TEXT    DEFAULT NULL,
                        leave_alert_footer_img   TEXT    DEFAULT NULL
                    )",
        )
        .execute(&self.pool)
        .await?;

        // Restore data if we had any
        for row in backup_data {
          let user_id: i64 = row.get("user_id");
          let steam_id: Option<i64> = row.try_get("steam_id").ok();
          sqlx::query(
            "INSERT OR IGNORE INTO users (user_id, steam_id) VALUES (?, ?)
                                 ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id",
          )
          .bind(user_id)
          .bind(steam_id)
          .execute(&self.pool)
          .await?;
        }
      }
    }
    Ok(())
  }
  async fn verify_users(&self) -> Result<()> {
    let required_columns = vec![
      "user_id",
      "steam_id",
      "pm_hot_alert",
      "pm_queue_alert_threshold",
      "queue_expiration",
      "vc_auto_join",
      "join_alert_title",
      "join_alert",
      "join_alert_color",
      "join_alert_img",
      "join_alert_footer",
      "join_alert_footer_img",
      "vc_auto_leave",
      "vc_leave_queue",
      "leave_alert_title",
      "leave_alert",
      "leave_alert_color",
      "leave_alert_img",
      "leave_alert_footer",
      "leave_alert_footer_img",
    ];
    self.verify_columns("users", &required_columns).await?;
    Ok(())
  }

  async fn create_categories_table(&self) -> Result<()> {
    use crate::DEFAULT_QUOTA;

    if !self.check_table("categories").await? {
      sqlx::query(&format!(
        "CREATE TABLE categories (
          id             INTEGER PRIMARY KEY,
          guild_id       INTEGER NOT NULL,
          category_id    INTEGER DEFAULT 0,
          name           TEXT,
          confirm_time   INTEGER DEFAULT {DEFAULT_CONFIRM_TIME},
          category       INTEGER DEFAULT 0,
          dashboard      INTEGER NOT NULL UNIQUE,
          chat           INTEGER NOT NULL UNIQUE,
          queue          INTEGER NOT NULL UNIQUE,
          ping           INTEGER NOT NULL UNIQUE,
          dashboard_msg  INTEGER DEFAULT 0,
          game           INTEGER DEFAULT 0,
          game_increment INTEGER DEFAULT 0,
          quota          INTEGER DEFAULT {DEFAULT_QUOTA},
          connect_info   TEXT,
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
        )"
      ))
      .execute(&self.pool)
      .await?;
    } else {
      // Check if essential columns exist
      let has_id = self.check_column("categories", "id").await?;
      let has_guild_id = self.check_column("categories", "guild_id").await?;

      // If missing id or guild_id, need to recreate table (can't add PRIMARY KEY column)
      if !has_id || !has_guild_id {
        // Backup existing data before dropping table
        let backup_data = if has_guild_id {
          sqlx::query("SELECT guild_id, category_id, name, confirm_time, dashboard, chat, queue, dashboard_msg, game, game_increment, quota, connect_info FROM categories")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default()
        } else {
          Vec::new()
        };

        sqlx::query("DROP TABLE categories").execute(&self.pool).await?;
        sqlx::query(&format!(
          "CREATE TABLE categories (
            id             INTEGER PRIMARY KEY,
            guild_id       INTEGER NOT NULL,
            category_id    INTEGER DEFAULT 0,
            name           TEXT,
            confirm_time   INTEGER DEFAULT {DEFAULT_CONFIRM_TIME},
            dashboard      INTEGER NOT NULL UNIQUE,
            chat           INTEGER NOT NULL UNIQUE,
            queue          INTEGER NOT NULL UNIQUE,
            ping           INTEGER NOT NULL UNIQUE,
            dashboard_msg  INTEGER DEFAULT 0,
            game           INTEGER DEFAULT 0,
            game_increment INTEGER DEFAULT 0,
            quota          INTEGER DEFAULT {DEFAULT_QUOTA},
            connect_info   TEXT,
            FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
          )"
        ))
        .execute(&self.pool)
        .await?;

        // Restore backed up data
        for row in backup_data {
          let category_id: i64 = row.try_get("category_id").unwrap_or(0);
          let guild_id: i64 = row.get("guild_id");
          let name: Option<String> = row.try_get("name").ok();
          let confirm_time: i64 = row.try_get("confirm_time").unwrap_or(DEFAULT_CONFIRM_TIME as i64);
          let dashboard: i64 = row.get("dashboard");
          let chat: i64 = row.get("chat");
          let queue: i64 = row.get("queue");
          let dashboard_msg: i64 = row.try_get("dashboard_msg").unwrap_or(0);
          let game: i64 = row.try_get("game").unwrap_or(0);
          let game_increment: i64 = row.try_get("game_increment").unwrap_or(0);
          let quota: i64 = row.try_get("quota").unwrap_or(DEFAULT_QUOTA as i64);
          let connect_info: Option<String> = row.try_get("connect_info").ok();

          sqlx::query(
            "INSERT INTO categories (guild_id, category_id, name, confirm_time, dashboard, chat, queue, dashboard_msg, game, game_increment, quota, connect_info)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
          )
          .bind(category_id)
          .bind(guild_id)
          .bind(name)
          .bind(confirm_time)
          .bind(dashboard)
          .bind(chat)
          .bind(queue)
          .bind(dashboard_msg)
          .bind(game)
          .bind(game_increment)
          .bind(quota)
          .bind(connect_info)
          .execute(&self.pool)
          .await?;
        }
      }

      // Add multiple columns using helper macro
      add_columns!(self, "categories",
        "name":                   "TEXT"    => "NULL",
        "connect_info":           "TEXT"    => "NULL",
        "dm_alert_enabled":       "INTEGER" => "0",
        "dm_alert_threshold":     "INTEGER" => "0",
        "category":               "INTEGER" => "0",
        "dm_alert_users":         "TEXT"    => "'[]'",
        "team_vc_create_policy":  "TEXT"    => "'on_hot'",
        "team_vc_destroy_policy": "TEXT"    => "'after_pull'",
        "team_vc_keep_minimum":   "INTEGER" => "1",
        "require_score_report":   "INTEGER" => "0",
      );

      // Check if UNIQUE constraints exist on channel columns
      // SQLite doesn't have a direct way to check constraints, so we check if duplicate channels exist
      let has_duplicates: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
          SELECT dashboard FROM categories CATEGORY BY dashboard HAVING COUNT(*) > 1
          UNION ALL
          SELECT chat FROM categories CATEGORY BY chat HAVING COUNT(*) > 1
          UNION ALL
          SELECT queue FROM categories CATEGORY BY queue HAVING COUNT(*) > 1
        )",
      )
      .fetch_one(&self.pool)
      .await
      .unwrap_or(0);

      // If no duplicates exist, we can safely add UNIQUE constraints by recreating the table
      if has_duplicates == 0 {
        // Check if constraints already exist by trying to insert a duplicate
        // If it fails with UNIQUE constraint error, constraints exist
        let test_result = sqlx::query("SELECT dashboard FROM categories LIMIT 1").fetch_optional(&self.pool).await?;

        if let Some(row) = test_result {
          let test_dashboard: i64 = row.get("dashboard");

          // Try to insert a duplicate to test if UNIQUE constraint exists
          let constraint_exists = sqlx::query(
            "INSERT INTO categories (guild_id, dashboard, chat, queue, quota) 
              VALUES (999999, ?, 999998, 999997, 999996, 999995, 12)",
          )
          .bind(test_dashboard)
          .execute(&self.pool)
          .await
          .is_err();

          // Clean up test row if it was inserted
          let _ = sqlx::query("DELETE FROM categories WHERE guild_id = 999999").execute(&self.pool).await;

          // If constraint doesn't exist, recreate table with UNIQUE constraints
          if !constraint_exists {
            info!("Adding UNIQUE constraints to category channels...");

            // Backup all data
            let backup_data = sqlx::query(
              "SELECT id, guild_id, category_id, name, confirm_time, category, dashboard, chat, queue,
                dashboard_msg, game, game_increment, quota, connect_info,
                dm_alert_enabled, dm_alert_threshold, dm_alert_users
                FROM categories",
            )
            .fetch_all(&self.pool)
            .await?;

            // Drop and recreate table with UNIQUE constraints
            sqlx::query("DROP TABLE categories").execute(&self.pool).await?;
            sqlx::query(&format!(
              "CREATE TABLE categories (
                id                  INTEGER PRIMARY KEY,
                guild_id            INTEGER NOT NULL,
                category_id         INTEGER DEFAULT 0,
                name                TEXT,
                confirm_time        INTEGER DEFAULT {DEFAULT_CONFIRM_TIME},
                category            INTEGER DEFAULT 0,
                dashboard           INTEGER NOT NULL UNIQUE,
                chat                INTEGER NOT NULL UNIQUE,
                queue               INTEGER NOT NULL UNIQUE,
                ping                INTEGER NOT NULL UNIQUE,
                dashboard_msg       INTEGER DEFAULT 0,
                game                INTEGER DEFAULT 0,
                game_increment      INTEGER DEFAULT 0,
                quota               INTEGER DEFAULT {DEFAULT_QUOTA},
                connect_info        TEXT,
                dm_alert_enabled    INTEGER DEFAULT 0,
                dm_alert_threshold  INTEGER DEFAULT 0,
                dm_alert_users      TEXT DEFAULT '[]'
              )"
            ))
            .execute(&self.pool)
            .await?;

            // Restore data
            for row in backup_data {
              let id: i64 = row.get("id");
              let category_id: i64 = row.try_get("category_id").unwrap_or(0);
              let guild_id: i64 = row.get("guild_id");
              let name: Option<String> = row.try_get("name").ok();
              let confirm_time: i64 = row.try_get("confirm_time").unwrap_or(DEFAULT_CONFIRM_TIME as i64);
              let dashboard: i64 = row.get("dashboard");
              let chat: i64 = row.get("chat");
              let queue: i64 = row.get("queue");
              let dashboard_msg: i64 = row.try_get("dashboard_msg").unwrap_or(0);
              let game: i64 = row.try_get("game").unwrap_or(0);
              let game_increment: i64 = row.try_get("game_increment").unwrap_or(0);
              let quota: i64 = row.try_get("quota").unwrap_or(DEFAULT_QUOTA as i64);
              let connect_info: Option<String> = row.try_get("connect_info").ok();
              let category: i64 = row.try_get("category").unwrap_or(0);
              let dm_alert_enabled: i64 = row.try_get("dm_alert_enabled").unwrap_or(0);
              let dm_alert_threshold: i64 = row.try_get("dm_alert_threshold").unwrap_or(0);
              let dm_alert_users: String = row.try_get("dm_alert_users").unwrap_or_else(|_| "[]".to_string());

              sqlx::query(
                "INSERT INTO categories (id, guild_id, category_id, name, confirm_time, category, dashboard, chat, queue,
                  dashboard_msg, game, game_increment, quota, connect_info,
                  dm_alert_enabled, dm_alert_threshold, dm_alert_users)
                  VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
              )
              .bind(id)
              .bind(guild_id)
              .bind(category_id)
              .bind(name)
              .bind(confirm_time)
              .bind(category)
              .bind(dashboard)
              .bind(chat)
              .bind(queue)
              .bind(dashboard_msg)
              .bind(game)
              .bind(game_increment)
              .bind(quota)
              .bind(connect_info)
              .bind(dm_alert_enabled)
              .bind(dm_alert_threshold)
              .bind(dm_alert_users)
              .execute(&self.pool)
              .await?;
            }

            info!("Successfully added UNIQUE constraints to category channels");
          }
        }
      }
    }
    Ok(())
  }
  async fn verify_categories(&self) -> Result<()> {
    // Add name column if missing
    add_column!(self, "categories", "name", "TEXT", "NULL");
    // Add ping column if missing
    add_column!(self, "categories", "ping", "INTEGER", "1");

    // Base columns not in config_schema
    let mut required_columns = vec![
      "id",
      "guild_id",
      "category_id",
      "category",
      "dashboard",
      "chat",
      "queue",
      "dashboard_msg",
      "game",
      "game_increment",
      "connect_info",
      "dm_alert_users",
      "dm_alert_threshold",
      "team_vc_create_policy",
      "team_vc_destroy_policy",
    ];

    // Automatically add columns from category_config schema
    use crate::config_schema::{category_config, sql_type_for_rust_type, sql_default_for_value};
    for (column, rust_type, default_value) in category_config::COLUMNS {
      required_columns.push(column);
      let sql_type = sql_type_for_rust_type(rust_type);
      let sql_default = sql_default_for_value(default_value, rust_type);
      self.add_column_runtime("categories", column, sql_type, &sql_default).await?;
    }

    self.verify_columns("categories", &required_columns).await?;
    Ok(())
  }

  async fn create_teams_table(&self) -> Result<()> {
    if !self.check_table("teams").await? {
      sqlx::query(
        "CREATE TABLE teams (
          id           INTEGER PRIMARY KEY,
          guild_id     INTEGER NOT NULL,
          category_id  INTEGER NOT NULL,
          set_index    INTEGER NOT NULL,
          session_id   TEXT,
          red          INTEGER NOT NULL,
          blu          INTEGER NOT NULL,
          created_at   INTEGER DEFAULT (strftime('%s', 'now')),
          UNIQUE(guild_id, category_id, set_index)
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_teams(&self) -> Result<()> {
    // First, migrate existing data to add set_index and session_id if needed
    if !self.check_column("teams", "set_index").await? {
      info!("Migrating teams table to add set_index and session_id...");

      // Add new columns
      sqlx::query("ALTER TABLE teams ADD COLUMN set_index INTEGER DEFAULT 1").execute(&self.pool).await?;
      sqlx::query("ALTER TABLE teams ADD COLUMN session_id TEXT").execute(&self.pool).await?;
      sqlx::query("ALTER TABLE teams ADD COLUMN created_at INTEGER DEFAULT 0").execute(&self.pool).await?;

      // Update created_at for existing records to current timestamp
      sqlx::query("UPDATE teams SET created_at = strftime('%s', 'now') WHERE created_at = 0").execute(&self.pool).await?;

      // Update existing records to have sequential set_index within each guild/category
      let rows = sqlx::query("SELECT id, guild_id, category_id FROM teams ORDER BY id").fetch_all(&self.pool).await?;

      let mut current_set: std::collections::HashMap<(i64, i64), i32> = std::collections::HashMap::new();

      for row in rows {
        let guild_id: i64 = row.get("guild_id");
        let category_id: i64 = row.get("category_id");
        let id: i64 = row.get("id");

        let set_index = current_set.entry((guild_id, category_id)).or_insert(1);
        sqlx::query("UPDATE teams SET set_index = ? WHERE id = ?").bind(*set_index).bind(id).execute(&self.pool).await?;

        *set_index += 1;
      }

      info!("Teams table migration completed");
    }

    // Now verify all required columns exist
    let required_columns = vec!["id", "guild_id", "category_id", "set_index", "session_id", "red", "blu", "created_at"];
    self.verify_columns("teams", &required_columns).await?;

    Ok(())
  }

  async fn create_formats_table(&self) -> Result<()> {
    if !self.check_table("formats").await? {
      sqlx::query(
        "CREATE TABLE formats (
          id           INTEGER PRIMARY KEY,
          guild_id     INTEGER NOT NULL,
          category_id  INTEGER NOT NULL,
          format_id    INTEGER NOT NULL DEFAULT 0,
          name         TEXT    NOT NULL,
          quota        INTEGER NOT NULL DEFAULT 12,
          connect_info TEXT,
          UNIQUE(guild_id, category_id, format_id),
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_formats(&self) -> Result<()> {
    let required_columns = vec!["id", "guild_id", "category_id", "format_id", "name", "quota", "connect_info"];
    self.verify_columns("formats", &required_columns).await?;
    Ok(())
  }

  async fn create_elo_table(&self) -> Result<()> {
    if !self.check_table("elo").await? {
      sqlx::query(
        "CREATE TABLE elo (
          id        INTEGER PRIMARY KEY,
          guild_id  INTEGER NOT NULL,
          user_id   INTEGER NOT NULL,
          elo       INTEGER NOT NULL DEFAULT 50,
          rank      INTEGER NOT NULL,
          games     INTEGER NOT NULL DEFAULT 0,
          wins      INTEGER NOT NULL DEFAULT 0,
          UNIQUE(guild_id, user_id),
          FOREIGN KEY (rank)     REFERENCES ranks(id)        ON DELETE SET NULL,
          FOREIGN KEY (user_id)  REFERENCES users(user_id)   ON DELETE CASCADE
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_elos(&self) -> Result<()> {
    add_column!(self, "elo", "dynamic_elo", "INTEGER", "NULL");
    add_column!(self, "elo", "last_game_timestamp", "INTEGER", "NULL");

    let required_columns = vec!["id", "guild_id", "user_id", "elo", "rank", "games", "wins", "dynamic_elo", "last_game_timestamp"];
    self.verify_columns("elo", &required_columns).await?;

    // Check if we need to migrate the foreign key constraint
    self.migrate_elo_foreign_key().await?;

    // Update NULL dynamic_elo values to 1500 (default for unmigrated players)
    self.migrate_dynamic_elo_defaults().await?;

    Ok(())
  }
  /// Migrate elo table to fix foreign key constraint from ranks(role_id) to ranks(id)
  async fn migrate_elo_foreign_key(&self) -> Result<()> {
    // Check if the elo table exists and has data
    if !self.check_table("elo").await? {
      return Ok(());
    }

    // Check the current foreign key constraint
    // We need to check if it's referencing ranks(role_id) instead of ranks(id)
    let pragma_result: Option<String> = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='table' AND name='elo'").fetch_optional(&self.pool).await?;

    if let Some(sql) = pragma_result {
      // If the foreign key references role_id (with or without quotes), we need to migrate
      if sql.contains("REFERENCES \"ranks\"(\"role_id\")") || sql.contains("REFERENCES ranks(role_id)") {
        info!("Detected incorrect foreign key constraint, migrating elo table...");

        // Create backup
        sqlx::query("CREATE TABLE elo_backup AS SELECT * FROM elo").execute(&self.pool).await?;

        // Drop and recreate table with correct foreign key
        sqlx::query("DROP TABLE elo").execute(&self.pool).await?;

        sqlx::query(
          "CREATE TABLE elo (
            id        INTEGER PRIMARY KEY,
            guild_id  INTEGER NOT NULL,
            user_id   INTEGER NOT NULL,
            elo       INTEGER NOT NULL DEFAULT 50,
            rank      INTEGER NOT NULL,
            games     INTEGER NOT NULL DEFAULT 0,
            wins      INTEGER NOT NULL DEFAULT 0,
            UNIQUE(guild_id, user_id),
            FOREIGN KEY (rank)     REFERENCES ranks(id)        ON DELETE SET NULL,
            FOREIGN KEY (user_id)  REFERENCES users(user_id)   ON DELETE CASCADE
            FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
          )",
        )
        .execute(&self.pool)
        .await?;

        // Restore data, converting role_id to rank.id
        sqlx::query(
          "INSERT INTO elo (id, guild_id, user_id, elo, rank, games, wins)
            SELECT 
              eb.id,
              eb.guild_id,
              eb.user_id,
              eb.elo,
              COALESCE(r.id, 0),
              eb.games,
              eb.wins
            FROM elo_backup eb
            LEFT JOIN ranks r ON r.role_id = eb.rank AND r.guild_id = eb.guild_id",
        )
        .execute(&self.pool)
        .await?;

        // Clean up records that couldn't find matching ranks
        sqlx::query("DELETE FROM elo WHERE rank = 0").execute(&self.pool).await?;

        // Drop backup
        sqlx::query("DROP TABLE elo_backup").execute(&self.pool).await?;

        info!("Elo table migration completed successfully");
      }
    }

    Ok(())
  }

  /// Migrate NULL dynamic_elo values to 1500 (default for unmigrated players)
  async fn migrate_dynamic_elo_defaults(&self) -> Result<()> {
    // Check if the elo table exists
    if !self.check_table("elo").await? {
      return Ok(());
    }

    // Check if there are any NULL dynamic_elo values
    let null_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM elo WHERE dynamic_elo IS NULL").fetch_optional(&self.pool).await?.unwrap_or(0);

    if null_count > 0 {
      info!("Found {} players with NULL dynamic_elo, setting to 1500", null_count);
      sqlx::query("UPDATE elo SET dynamic_elo = 1500 WHERE dynamic_elo IS NULL").execute(&self.pool).await?;
      info!("Updated {} players to default dynamic_elo of 1500", null_count);
    }

    Ok(())
  }

  async fn create_guilds_table(&self) -> Result<()> {
    if !self.check_table("guilds").await? {
      sqlx::query(
        "CREATE TABLE guilds (
          id       INTEGER PRIMARY KEY,
          guild_id INTEGER NOT NULL,
          name     TEXT    NOT NULL,
          nick     TEXT,
          UNIQUE(guild_id)
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_guilds(&self) -> Result<()> {
    let required_columns = vec!["id", "guild_id", "name", "nick"];
    self.verify_columns("guilds", &required_columns).await?;

    Ok(())
  }

  async fn create_ranks_table(&self) -> Result<()> {
    // Check if old schema exists (has position or role_ids columns)
    let needs_migration = if self.check_table("ranks").await? { self.check_column("ranks", "position").await? || self.check_column("ranks", "role_ids").await? } else { false };

    if needs_migration {
      // Migrate old data to new schema
      let old_data: Vec<(i64, String, i64)> = sqlx::query_as("SELECT guild_id, name, elo FROM ranks").fetch_all(&self.pool).await.unwrap_or_default();

      sqlx::query("DROP TABLE ranks").execute(&self.pool).await?;

      sqlx::query(
        "CREATE TABLE ranks (
          id       INTEGER PRIMARY KEY,
          guild_id INTEGER NOT NULL,
          name     TEXT    NOT NULL,
          elo      INTEGER NOT NULL,
          role_id  INTEGER NOT NULL,
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;

      // Restore data
      for (guild_id, name, elo) in old_data {
        let _ = sqlx::query("INSERT OR IGNORE INTO ranks (guild_id, name, elo) VALUES (?, ?, ?)").bind(guild_id).bind(&name).bind(elo).execute(&self.pool).await;
      }
    } else if !self.check_table("ranks").await? {
      sqlx::query(
        "CREATE TABLE ranks (
          id       INTEGER PRIMARY KEY,
          guild_id INTEGER NOT NULL,
          name     TEXT    NOT NULL,
          elo      INTEGER NOT NULL,
          role_id  INTEGER NOT NULL,
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_id) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;
    }

    Ok(())
  }
  async fn verify_ranks(&self) -> Result<()> {
    let required_columns = vec!["id", "guild_id", "name", "elo", "role_id"];
    self.verify_columns("ranks", &required_columns).await?;
    Ok(())
  }

  async fn create_matches_table(&self) -> Result<()> {
    if !self.check_table("matches").await? {
      sqlx::query(
        "CREATE TABLE matches (
          id              INTEGER PRIMARY KEY,
          guild_id        INTEGER NOT NULL,
          category_id     INTEGER NOT NULL,
          format_id       INTEGER NOT NULL DEFAULT 0,
          session_id      TEXT,
          started_at      INTEGER NOT NULL,
          ended_at        INTEGER NOT NULL,
          duration_secs   INTEGER NOT NULL,
          result          TEXT,
          FOREIGN KEY (guild_id)    REFERENCES guilds(guild_id)        ON DELETE CASCADE,
          FOREIGN KEY (category_id) REFERENCES categories(category_id) ON DELETE SET NULL,
          FOREIGN KEY (format_id)   REFERENCES formats(format_id)      ON DELETE SET NULL,
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_matches(&self) -> Result<()> {
    // Migrate from red_score/blu_score to result column
    if !self.check_column("matches", "result").await? {
      // Add result column
      sqlx::query("ALTER TABLE matches ADD COLUMN result TEXT").execute(&self.pool).await?;

      // Convert existing scores to result
      if self.check_column("matches", "red_score").await? {
        sqlx::query(
          "UPDATE matches SET result = CASE 
            WHEN red_score > blu_score THEN 'red'
            WHEN blu_score > red_score THEN 'blu'
            WHEN red_score IS NOT NULL AND blu_score IS NOT NULL THEN 'draw'
            ELSE NULL
          END",
        )
        .execute(&self.pool)
        .await?;
      }
    }

    let required_columns = vec!["id", "guild_id", "category_id", "format_id", "session_id", "started_at", "ended_at", "duration_secs", "result"];
    self.verify_columns("matches", &required_columns).await?;
    Ok(())
  }

  async fn create_match_players_table(&self) -> Result<()> {
    if !self.check_table("match_players").await? {
      sqlx::query(
        "CREATE TABLE match_players (
          id          INTEGER PRIMARY KEY,
          match_id    INTEGER NOT NULL,
          user_id     INTEGER NOT NULL,
          team        TEXT NOT NULL CHECK(team IN ('red', 'blu')),
          elo_before  INTEGER NOT NULL,
          elo_after   INTEGER,
          FOREIGN KEY (match_id) REFERENCES matches(id) ON DELETE CASCADE,
          FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_match_players(&self) -> Result<()> {
    let required_columns = vec!["id", "match_id", "user_id", "team", "elo_before", "elo_after"];
    self.verify_columns("match_players", &required_columns).await?;
    Ok(())
  }

  async fn create_fatkid_table(&self) -> Result<()> {
    if !self.check_table("fatkids").await? {
      sqlx::query(
        "CREATE TABLE fatkids (
          id                INTEGER PRIMARY KEY AUTOINCREMENT,
          guild_id          INTEGER NOT NULL,
          user_id           INTEGER NOT NULL,
          immunity_level    INTEGER NOT NULL DEFAULT 0,
          last_fatkidded_at INTEGER,
          UNIQUE(guild_id, user_id),
          FOREIGN KEY (user_id)  REFERENCES users(user_id)  ON DELETE CASCADE,
          FOREIGN KEY (guild_id) REFERENCES guilds(guild_d) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }
  async fn verify_fatkids(&self) -> Result<()> {
    let required_columns = vec!["id", "guild_id", "user_id", "immunity_level", "last_fatkidded_at"];
    self.verify_columns("fatkids", &required_columns).await?;
    Ok(())
  }

  /// Add foreign key constraint to config table after both tables exist
  async fn add_config_foreign_key(&self) -> Result<()> {
    // Check if foreign key already exists by trying to query pragma_foreign_key_list
    let has_foreign_key = sqlx::query("PRAGMA foreign_key_list(config)").fetch_all(&self.pool).await?.into_iter().any(|row| {
      if let Ok(table) = row.try_get::<String, _>("table") {
        table == "ranks"
      } else {
        false
      }
    });

    if !has_foreign_key {
      // SQLite doesn't support adding foreign keys to existing tables directly
      // We need to recreate the table
      info!("Adding foreign key constraint to config table...");

      // Backup existing data
      let backup_data = sqlx::query("SELECT guild_id, runner_id, admin_id, active_elo, default_rank FROM config").fetch_all(&self.pool).await?;

      // Drop and recreate table with foreign key
      sqlx::query("DROP TABLE config").execute(&self.pool).await?;

      sqlx::query(
        "CREATE TABLE config (
          guild_id     INTEGER NOT NULL,
          runner_id    INTEGER,
          admin_id     INTEGER,
          active_elo   INTEGER,
          default_rank INTEGER,
          PRIMARY KEY(guild_id),
          FOREIGN KEY (default_rank) REFERENCES ranks(role_id)  ON DELETE SET NULL
          FOREIGN KEY (guild_id)     REFERENCES guilds(guild_d) ON DELETE CASCADE,
        )",
      )
      .execute(&self.pool)
      .await?;

      // Restore data
      for row in backup_data {
        let guild_id: i64 = row.get("guild_id");
        let runner_id: Option<i64> = row.try_get("runner_id").ok();
        let admin_id: Option<i64> = row.try_get("admin_id").ok();
        let active_elo: Option<i64> = row.try_get("active_elo").ok();
        let default_rank: Option<i64> = row.try_get("default_rank").ok();

        sqlx::query(
          "INSERT INTO config (guild_id, runner_id, admin_id, active_elo, default_rank)
                     VALUES (?, ?, ?, ?, ?)",
        )
        .bind(guild_id)
        .bind(runner_id)
        .bind(admin_id)
        .bind(active_elo)
        .bind(default_rank)
        .execute(&self.pool)
        .await?;
      }

      info!("Successfully added foreign key constraint to config table");
    }

    Ok(())
  }

  // HELPERS

  /// Verify that a table has all required columns
  async fn verify_columns(&self, table_name: &str, required_columns: &[&str]) -> Result<()> {
    let existing_cols: Vec<String> =
      sqlx::query(&format!("PRAGMA table_info({table_name})")).fetch_all(&self.pool).await?.into_iter().filter_map(|row| row.try_get::<String, _>("name").ok()).collect();

    for required_col in required_columns {
      if !existing_cols.contains(&required_col.to_string()) {
        return Err(anyhow::anyhow!("{} in {}", required_col, table_name));
      }
    }
    Ok(())
  }
  /// Create a default category entry for a guild if none exists
  pub async fn init_first_category(&self, guild_id: GI) -> Result<()> {
    let count: i64 = sqlx::query_scalar(
      "SELECT COUNT(*)
            FROM categories
            WHERE guild_id = ?",
    )
    .bind(guild_id.get() as i64)
    .fetch_one(&self.pool)
    .await?;

    if count == 0 {
      sqlx::query(
        "INSERT INTO categories (category_id, guild_id, dashboard, chat, queue)
          VALUES (1, ?, 1, 1, 1)",
      )
      .bind(guild_id.get() as i64)
      .execute(&self.pool)
      .await?;
    }

    Ok(())
  }
  /// Check if table exists
  async fn check_table(&self, table_name: &str) -> Result<bool> {
    let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name=?").bind(table_name).fetch_optional(&self.pool).await?;

    Ok(result.is_some())
  }
  /// Check if column exists in table
  async fn check_column(&self, table_name: &str, column_name: &str) -> Result<bool> {
    let existing_cols: Vec<String> =
      sqlx::query(&format!("PRAGMA table_info({table_name})")).fetch_all(&self.pool).await?.into_iter().filter_map(|row| row.try_get::<String, _>("name").ok()).collect();

    Ok(existing_cols.contains(&column_name.to_string()))
  }
  /// Create user_server_prefs table for per-server, per-user preferences
  async fn create_user_server_prefs_table(&self) -> Result<()> {
    if !self.check_table("user_server_prefs").await? {
      sqlx::query(
        "CREATE TABLE user_server_prefs (
          user_id          INTEGER NOT NULL,
          guild_id         INTEGER NOT NULL,
          vc_auto_join     INTEGER DEFAULT NULL,
          vc_auto_leave    INTEGER DEFAULT NULL,
          vc_leave_queue   INTEGER DEFAULT NULL,
          PRIMARY KEY (user_id, guild_id)
        )",
      )
      .execute(&self.pool)
      .await?;
      info!("Created user_server_prefs table");
    }
    Ok(())
  }

  /// Check if a unique constraint exists on a column
  async fn check_unique(&self, table: &str, column: &str) -> Result<bool> {
    // INTEGER PRIMARY KEY is the rowid alias in SQLite and won't appear in index_list,
    // so check table_info for the pk flag first.
    let table_info = sqlx::query(&format!("PRAGMA table_info({table})")).fetch_all(&self.pool).await?;

    let is_pk = table_info.iter().any(|row| {
      let name: String = row.try_get("name").unwrap_or_default();
      let pk: i64 = row.try_get("pk").unwrap_or(0);
      name == column && pk > 0
    });

    if is_pk {
      return Ok(true);
    }

    let index_info = sqlx::query(&format!("PRAGMA index_list({table})")).fetch_all(&self.pool).await?;

    let has_unique = index_info.iter().any(|row| if let Ok(unique) = row.try_get::<i64, _>("unique") { unique == 1 } else { false });
    Ok(has_unique)
  }
}
