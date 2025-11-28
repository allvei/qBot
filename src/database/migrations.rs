use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::DEFAULT_TIMEOUT;

/// Adds a column to a table if it doesn't exist
/// 
/// # Arguments
/// 
/// * `table`   - The name of the table to add the column to
/// * `name`    - The name of the column to add
/// * `type`    - The type of the column to add
/// * `default` - The default value of the column to add
macro_rules! add_column {
    ($self:ident, $table:literal, $name:literal, $type:literal, $default:literal) => {
        if !$self.check_column($table, $name).await? {
            sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} DEFAULT {}", $table, $name, $type, $default))
                .execute(&$self.pool).await?;
        }
    };
}

/// Database migration system for managing schema changes
pub struct DatabaseMigrations {
    pool: SqlitePool,
}

impl DatabaseMigrations {
    pub fn new(pool: &SqlitePool) -> Self {
        Self {pool: pool.clone()}
    }

    // MASTERS
    
    /// Run all migrations in order
    pub async fn create_tables(&self) -> Result<()> {
        self.create_config_table().await?;
        self.create_users_table() .await?;
        self.create_groups_table().await?;
        self.create_teams_table() .await?;
        self.create_elo_table()   .await?;
        Ok(())
    }
    pub async fn verify_schemas(&self) -> Result<()> {
        self.verify_config().await?;
        self.verify_users() .await?;
        self.verify_groups().await?;
        self.verify_teams() .await?;
        self.verify_elos()   .await?;
        Ok(())
    }

    // CREATE TABLES

    async fn create_config_table(&self) -> Result<()> {
        if !self.check_table("config").await? {
            sqlx::query(
                "CREATE TABLE config (
                    guild       INTEGER NOT NULL,
                    key         TEXT NOT NULL,
                    value       TEXT,
                    description TEXT,
                    PRIMARY KEY(guild, key)
                )"
            )
            .execute(&self.pool)
            .await?;
        } else if !self.check_column("config", "guild").await? {
            sqlx::query("ALTER TABLE config
                         ADD COLUMN guild INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
    async fn create_users_table(&self) ->  Result<()> {
        if !self.check_table("users").await? {
            sqlx::query(
                "CREATE TABLE users (
                    id                              INTEGER PRIMARY KEY,
                    user_id                      INTEGER NOT NULL UNIQUE,
                    steam_id                        INTEGER,
                    dm_enabled                      INTEGER DEFAULT 1,
                    auto_remove_minutes             INTEGER DEFAULT 30,
                    join_announcement               INTEGER DEFAULT 0,
                    vc_disconnect_on_leave          INTEGER DEFAULT 1,
                    announcement_color              INTEGER DEFAULT 3447003,
                    show_stats_in_announcement      INTEGER DEFAULT 0,
                    notify_quota_threshold          INTEGER DEFAULT NULL,
                    alert_desc                      TEXT    DEFAULT NULL,
                    alert_footer_text               TEXT    DEFAULT NULL,
                    alert_footer_icon               TEXT    DEFAULT NULL,
                    alert_footer_thumbnail          TEXT    DEFAULT NULL,
                    leave_alert                     INTEGER DEFAULT 0,
                    leave_alert_desc                TEXT    DEFAULT NULL,
                    leave_alert_footer_text         TEXT    DEFAULT NULL,
                    leave_alert_footer_icon         TEXT    DEFAULT NULL,
                    leave_alert_footer_thumbnail    TEXT    DEFAULT NULL,
                    elo                             INTEGER DEFAULT 30
                )"
            )
            .execute(&self.pool)
            .await?;
        } else {
            // Verify schema integrity
            let has_unique     = self.check_unique("users", "user_id").await?;
            let has_user_id    = self.check_column("users", "user_id").await?;
            let has_steam_id   = self.check_column("users", "steam_id")  .await?;
            let has_dm_enabled = self.check_column("users", "dm_enabled").await?;

            // Add dm_enabled column if missing
            if has_user_id && !has_dm_enabled {
                sqlx::query("ALTER TABLE users ADD COLUMN dm_enabled INTEGER DEFAULT 1")
                    .execute(&self.pool)
                    .await?;
            }

            // Add new settings columns if missing
            if has_user_id {
                add_column!(self, "users", "auto_remove_minutes",          "INTEGER", "30");
                add_column!(self, "users", "join_announcement",            "INTEGER", "0");
                add_column!(self, "users", "vc_disconnect_on_leave",       "INTEGER", "1");
                add_column!(self, "users", "announcement_color",           "INTEGER", "3447003");
                add_column!(self, "users", "show_stats_in_announcement",   "INTEGER", "0");
                add_column!(self, "users", "notify_quota_threshold",       "INTEGER", "NULL");
                add_column!(self, "users", "alert_desc",                   "TEXT",    "NULL");
                add_column!(self, "users", "alert_footer_text",            "TEXT",    "NULL");
                add_column!(self, "users", "alert_footer_icon",            "TEXT",    "NULL");
                add_column!(self, "users", "alert_footer_thumbnail",       "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert",                  "INTEGER", "0");
                add_column!(self, "users", "leave_alert_desc",             "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert_footer_text",      "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert_footer_icon",      "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert_footer_thumbnail", "TEXT",    "NULL");
                add_column!(self, "users", "elos",                         "INTEGER", "30");
            }

            if !has_user_id || !has_steam_id || !has_unique {

                // Backup existing data if any
                let backup_data = if has_user_id {
                    sqlx::query("SELECT user_id, steam_id FROM users")
                        .fetch_all(&self.pool)
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                // Drop and recreate table
                sqlx::query("DROP TABLE users").execute(&self.pool).await?;
                sqlx::query(
                    "CREATE TABLE users (
                        id                           INTEGER PRIMARY KEY,
                        user_id                      INTEGER NOT NULL UNIQUE,
                        steam_id                     INTEGER,
                        dm_enabled                   INTEGER DEFAULT 1,
                        auto_remove_minutes          INTEGER DEFAULT 0,
                        join_announcement            INTEGER DEFAULT 0,
                        vc_disconnect_on_leave       INTEGER DEFAULT 1,
                        announcement_color           INTEGER DEFAULT 3447003,
                        show_stats_in_announcement   INTEGER DEFAULT 0,
                        notify_quota_threshold       INTEGER DEFAULT NULL,
                        alert_desc                   TEXT    DEFAULT NULL,
                        alert_footer_text            TEXT    DEFAULT NULL,
                        alert_footer_icon            TEXT    DEFAULT NULL,
                        alert_footer_thumbnail       TEXT    DEFAULT NULL,
                        leave_alert                  INTEGER DEFAULT 0,
                        leave_alert_desc             TEXT    DEFAULT NULL,
                        leave_alert_footer_text      TEXT    DEFAULT NULL,
                        leave_alert_footer_icon      TEXT    DEFAULT NULL,
                        leave_alert_footer_thumbnail TEXT    DEFAULT NULL,
                        elo                          INTEGER DEFAULT 30
                    )"
                )
                .execute(&self.pool)
                .await?;

                // Restore data if we had any
                for row in backup_data {
                    let user_id: i64 = row.get("user_id");
                    let steam_id: Option<i64> = row.try_get("steam_id").ok();
                    sqlx::query("INSERT OR IGNORE
                                 INTO users (user_id, steam_id, dm_enabled, auto_remove_minutes, join_announcement, vc_disconnect_on_leave, announcement_color, show_stats_in_announcement, notify_quota_threshold, alert_desc, alert_footer_text, alert_footer_icon, alert_footer_thumbnail, leave_alert, leave_alert_desc, leave_alert_footer_text, leave_alert_footer_icon, leave_alert_footer_thumbnail, elo)
                                 VALUES (?, ?, 1, 0, 0, 1, 3447003, 0, NULL, NULL, NULL, NULL, 0, NULL, NULL, NULL, NULL, NULL, 30)")
                        .bind(user_id)
                        .bind(steam_id)
                        .execute(&self.pool)
                        .await?;
                }
            }
        }
        Ok(())
    }
    async fn create_groups_table(&self) -> Result<()> {
        use crate::DEFAULT_QUOTA;

        if !self.check_table("groups").await? {
            sqlx::query(&format!("CREATE TABLE groups (
                    id                INTEGER PRIMARY KEY,
                    group_id          INTEGER DEFAULT 0,
                    timeout           INTEGER DEFAULT {DEFAULT_TIMEOUT},
                    guild_id          INTEGER NOT NULL,
                    dashboard         INTEGER NOT NULL,
                    chat              INTEGER NOT NULL,
                    queue             INTEGER NOT NULL,
                    dashboard_msg     INTEGER DEFAULT 0,
                    red               INTEGER NOT NULL,
                    blu               INTEGER NOT NULL,
                    game              INTEGER DEFAULT 0,
                    game_increment    INTEGER DEFAULT 0,
                    quota             INTEGER DEFAULT {DEFAULT_QUOTA}
                )"
            ))
            .execute(&self.pool)
            .await?;
        } else {
            // Check if essential columns exist
            let has_guild_id = self.check_column("groups", "guild_id").await?;

            if !has_guild_id {
                sqlx::query("DROP TABLE groups").execute(&self.pool).await?;
                sqlx::query(&format!(
                    "CREATE TABLE groups (
                        id                INTEGER PRIMARY KEY,
                        group_id          INTEGER DEFAULT 0,
                        timeout           INTEGER DEFAULT {DEFAULT_TIMEOUT},
                        guild_id          INTEGER NOT NULL,
                        dashboard         INTEGER NOT NULL,
                        chat              INTEGER NOT NULL,
                        queue             INTEGER NOT NULL,
                        dashboard_msg     INTEGER DEFAULT 0,
                        red               INTEGER NOT NULL,
                        blu               INTEGER NOT NULL,
                        game              INTEGER DEFAULT 0,
                        game_increment    INTEGER DEFAULT 0,
                        quota             INTEGER DEFAULT {DEFAULT_QUOTA}
                    )"
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }
    async fn create_teams_table(&self) ->  Result<()> {
        if !self.check_table("teams").await? {
            sqlx::query(
                "CREATE TABLE teams (
                    id       INTEGER PRIMARY KEY,
                    guild_id INTEGER NOT NULL,
                    group_id INTEGER NOT NULL,
                    red      INTEGER NOT NULL,
                    blu      INTEGER NOT NULL
                )"
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
    async fn create_elo_table(&self) ->    Result<()> {
        if !self.check_table("elos").await? {
            sqlx::query(
                "CREATE TABLE elos (
                    id       INTEGER PRIMARY KEY,
                    guild_id INTEGER NOT NULL,
                    user_id  INTEGER NOT NULL,
                    elo      INTEGER NOT NULL
                )"
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    // SCHEMA VALIDATIONS

    async fn verify_config(&self) -> Result<()> {
        let required_columns = vec!["guild", "key", "value", "description"];
        self.verify_columns("config", &required_columns).await?;
        Ok(())
    }
    async fn verify_users(&self)  -> Result<()> {
        let required_columns = vec![
            "id",
            "user_id",
            "tag",
            "steam_id",
            "dm_enabled",
            "auto_remove_minutes",
            "join_announcement",
            "vc_disconnect_on_leave",
            "announcement_color",
            "show_stats_in_announcement",
            "notify_quota_threshold",
            "alert_desc",
            "alert_footer_text",
            "alert_footer_icon",
            "alert_footer_thumbnail",
            "leave_alert",
            "leave_alert_desc",
            "leave_alert_footer_text",
            "leave_alert_footer_icon",
            "leave_alert_footer_thumbnail",
            "elo",
        ];
        self.verify_columns("users", &required_columns).await?;
        Ok(())
    }
    async fn verify_groups(&self) -> Result<()> {
        let required_columns = vec![
            "id", "group_id", "timeout", "guild_id", "dashboard",
            "chat", "queue", "dashboard_msg", "red", "blu",
            "game", "game_increment", "quota"
        ];
        self.verify_columns("groups", &required_columns).await?;
        Ok(())
    }
    async fn verify_teams(&self)  -> Result<()> {
        let required_columns = vec!["id", "guild_id", "group_id", "red", "blu"];
        self.verify_columns("teams", &required_columns).await?;
        Ok(())
    }
    async fn verify_elos(&self)   -> Result<()> {
        let required_columns = vec!["id", "guild_id", "user_id", "elo"];
        self.verify_columns("elos", &required_columns).await?;
        Ok(())
    }

    // HELPERS

    /// Verify that a table has all required columns
    async fn verify_columns(&self, table_name: &str, required_columns: &[&str]) -> Result<()> {
        let existing_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({table_name})"))
            .fetch_all(&self.pool).await?.into_iter().filter_map(|row| row.try_get::<String, _>("name").ok()).collect();

        for required_col in required_columns {
            if !existing_cols.contains(&required_col.to_string()) {
                return Err(anyhow::anyhow!("🔴 {} in {}", required_col, table_name));
            }
        }
        Ok(())
    }

    /// Create a default group entry for a guild if none exists
    pub async fn init_first_group(&self, guild_id: u64) -> Result<()> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
            FROM groups
            WHERE guild_id = ?"
        )
        .bind(guild_id as i64)
        .fetch_one(&self.pool)
        .await?;

        if count == 0 {
            sqlx::query("INSERT INTO groups (group_id, guild_id, dashboard, chat, queue, red, blu)
                        VALUES (1, ?, 1, 1, 1, 1, 1)"
            )
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Check if table exists
    async fn check_table(&self, table_name: &str) -> Result<bool> {
        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name=?"
        )
        .bind(table_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(!result.is_empty())
    }

    /// Check if column exists in table
    async fn check_column(&self, table_name: &str, column_name: &str) -> Result<bool> {
        let existing_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({table_name})"))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();

        Ok(existing_cols.contains(&column_name.to_string()))
    }

    /// Check if a unique constraint exists on a column
    async fn check_unique(&self, table: &str, _column: &str) -> Result<bool> {
        let index_info = sqlx::query(&format!("PRAGMA index_list({table})"))
            .fetch_all(&self.pool)
            .await?;

        let has_unique = index_info.iter().any(|row| {
            if let Ok(unique) = row.try_get::<i64, _>("unique") {
                unique == 1
            } else {
                false
            }
        });
        Ok(has_unique)
    }

}
