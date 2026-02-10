use anyhow::Result;
use serenity::{all::GuildId as GI};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::DEFAULT_HOT_JOIN_TIMEOUT;

macro_rules! add_column {
    ($self:ident, $table:literal, $name:literal, $type:literal, $default:literal) => {
        if !$self.check_column($table, $name).await? {
            sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} DEFAULT {}", $table, $name, $type, $default))
                .execute(&$self.pool).await?;
        }
    };
}

macro_rules! add_column_not_null {
    ($self:ident, $table:literal, $name:literal, $type:literal, $default:literal) => {
        if !$self.check_column($table, $name).await? {
            sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} NOT NULL DEFAULT {}", $table, $name, $type, $default))
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
        self.create_config_table()   .await?;
        self.create_users_table()    .await?;
        self.create_groups_table()   .await?;
        self.create_teams_table()    .await?;
        self.create_subgroups_table().await?;
        self.create_elo_table()      .await?;
        self.create_ranks_table()    .await?;
        
        // Add foreign key constraint after both tables exist
        self.add_config_foreign_key().await?;
        
        Ok(())
    }
    pub async fn verify_schemas(&self) -> Result<()> {
        self.verify_config()   .await?;
        self.verify_users()    .await?;
        self.verify_groups()   .await?;
        self.verify_teams()    .await?;
        self.verify_subgroups().await?;
        self.verify_elos()     .await?;
        self.verify_ranks()    .await?;
        Ok(())
    }

    // CREATE TABLES

    async fn create_config_table(&self) -> Result<()> {
        if !self.check_table("config").await? {
            sqlx::query(
                "CREATE TABLE config (
                    guild_id     INTEGER NOT NULL,
                    runner_id    INTEGER,
                    admin_id     INTEGER,
                    active_elo   INTEGER,
                    default_rank INTEGER,
                    PRIMARY KEY(guild_id)
                )"
            )
            .execute(&self.pool)
            .await?;
        } else if !self.check_column("config", "guild_id").await? {
            add_column_not_null!(self, "config", "guild_id", "INTEGER", "0");
        }
        Ok(())
    }
    async fn verify_config(&self) -> Result<()> {
        let required_columns = vec!["guild_id", "runner_id", "admin_id", "active_elo", "default_rank"];
        self.verify_columns("config", &required_columns).await?;
        Ok(())
    }
    async fn create_users_table(&self) ->  Result<()> {
        if !self.check_table("users").await? {
            sqlx::query(
                "CREATE TABLE users (
                    user_id                  INTEGER PRIMARY KEY,
                    steam_id                 INTEGER,
                    discord_tag              TEXT    DEFAULT NULL,
                    pm_hot_alert             INTEGER DEFAULT 1,
                    pm_queue_alert_threshold INTEGER DEFAULT NULL,
                    timeout                  INTEGER DEFAULT 30,
                    vc_auto_join             INTEGER DEFAULT 0,
                    join_alert_title         TEXT    DEFAULT NULL,
                    join_alert               TEXT    DEFAULT NULL,
                    join_alert_color         INTEGER DEFAULT 3447003,
                    join_alert_img           TEXT    DEFAULT NULL,
                    join_alert_footer        TEXT    DEFAULT NULL,
                    join_alert_footer_img    TEXT    DEFAULT NULL,
                    vc_auto_leave            INTEGER DEFAULT 0,
                    leave_alert_title        TEXT    DEFAULT NULL,
                    leave_alert              TEXT    DEFAULT NULL,
                    leave_alert_color        INTEGER DEFAULT 3447003,
                    leave_alert_img          TEXT    DEFAULT NULL,
                    leave_alert_footer       TEXT    DEFAULT NULL,
                    leave_alert_footer_img   TEXT    DEFAULT NULL
                )"
            )
            .execute(&self.pool)
            .await?;
        } else {
            // Verify schema integrity
            let has_unique       = self.check_unique("users", "user_id").await?;
            let has_user_id      = self.check_column("users", "user_id").await?;
            let has_steam_id     = self.check_column("users", "steam_id")  .await?;
            let has_pm_hot_alert = self.check_column("users", "pm_hot_alert").await?;

            // Add pm_hot_alert column if missing
            if has_user_id && !has_pm_hot_alert {
                add_column!(self, "users", "pm_hot_alert", "INTEGER", "1");
            }

            // Add new settings columns if missing
            if has_user_id {
                add_column!(self, "users", "discord_tag",              "TEXT",    "NULL");
                add_column!(self, "users", "pm_hot_alert",             "INTEGER", "1");
                add_column!(self, "users", "pm_queue_alert_threshold", "INTEGER", "NULL");
                add_column!(self, "users", "timeout",                  "INTEGER", "30");
                add_column!(self, "users", "vc_auto_join",             "INTEGER", "0");
                add_column!(self, "users", "join_alert_title",         "TEXT",    "NULL");
                add_column!(self, "users", "join_alert",               "TEXT",    "NULL");
                add_column!(self, "users", "join_alert_color",         "INTEGER", "3447003");
                add_column!(self, "users", "join_alert_img",           "TEXT",    "NULL");
                add_column!(self, "users", "join_alert_footer",        "TEXT",    "NULL");
                add_column!(self, "users", "join_alert_footer_img",    "TEXT",    "NULL");
                add_column!(self, "users", "vc_auto_leave",            "INTEGER", "0");
                add_column!(self, "users", "leave_alert_title",        "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert",              "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert_color",        "INTEGER", "3447003");
                add_column!(self, "users", "leave_alert_img",          "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert_footer",       "TEXT",    "NULL");
                add_column!(self, "users", "leave_alert_footer_img",   "TEXT",    "NULL");
            }

            // Drop old elo column if it exists (ELO is now in elo table)
            if has_user_id && self.check_column("users", "elo").await? {
                sqlx::query("ALTER TABLE users DROP COLUMN elo")
                    .execute(&self.pool)
                    .await
                    .ok(); // Ignore errors if column doesn't exist
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
                        user_id                  INTEGER PRIMARY KEY,
                        steam_id                 INTEGER,
                        discord_tag              TEXT    DEFAULT NULL,
                        pm_hot_alert             INTEGER DEFAULT 1,
                        pm_queue_alert_threshold INTEGER DEFAULT NULL,
                        timeout                  INTEGER DEFAULT 30,
                        vc_auto_join             INTEGER DEFAULT 0,
                        join_alert_title         TEXT    DEFAULT NULL,
                        join_alert               TEXT    DEFAULT NULL,
                        join_alert_color         INTEGER DEFAULT 3447003,
                        join_alert_img           TEXT    DEFAULT NULL,
                        join_alert_footer        TEXT    DEFAULT NULL,
                        join_alert_footer_img    TEXT    DEFAULT NULL,
                        vc_auto_leave            INTEGER DEFAULT 0,
                        leave_alert_title        TEXT    DEFAULT NULL,
                        leave_alert              TEXT    DEFAULT NULL,
                        leave_alert_color        INTEGER DEFAULT 3447003,
                        leave_alert_img          TEXT    DEFAULT NULL,
                        leave_alert_footer       TEXT    DEFAULT NULL,
                        leave_alert_footer_img   TEXT    DEFAULT NULL
                    )"
                )
                .execute(&self.pool)
                .await?;

                // Restore data if we had any
                for row in backup_data {
                    let user_id: i64 = row.get("user_id");
                    let steam_id: Option<i64> = row.try_get("steam_id").ok();
                    sqlx::query("INSERT OR IGNORE INTO users (user_id, steam_id) VALUES (?, ?)
                                 ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id")
                        .bind(user_id)
                        .bind(steam_id)
                        .execute(&self.pool)
                        .await?;
                }
            }
        }
        Ok(())
    }
    async fn verify_users(&self)  -> Result<()> {
        let required_columns = vec![
            "user_id",
            "steam_id",
            "pm_hot_alert",
            "pm_queue_alert_threshold",
            "timeout",
            "vc_auto_join",
            "join_alert_title",
            "join_alert",
            "join_alert_color",
            "join_alert_img",
            "join_alert_footer",
            "join_alert_footer_img",
            "vc_auto_leave",
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
    async fn create_groups_table(&self) -> Result<()> {
        use crate::DEFAULT_QUOTA;

        if !self.check_table("groups").await? {
            sqlx::query(&format!("CREATE TABLE groups (
                    id                INTEGER PRIMARY KEY,
                    group_id          INTEGER DEFAULT 0,
                    name              TEXT,
                    timeout           INTEGER DEFAULT {DEFAULT_HOT_JOIN_TIMEOUT},
                    guild_id          INTEGER NOT NULL,
                    category          INTEGER DEFAULT 0,
                    dashboard         INTEGER NOT NULL UNIQUE,
                    chat              INTEGER NOT NULL UNIQUE,
                    queue             INTEGER NOT NULL UNIQUE,
                    dashboard_msg     INTEGER DEFAULT 0,
                    red               INTEGER NOT NULL UNIQUE,
                    blu               INTEGER NOT NULL UNIQUE,
                    game              INTEGER DEFAULT 0,
                    game_increment    INTEGER DEFAULT 0,
                    quota             INTEGER DEFAULT {DEFAULT_QUOTA},
                    connect_info      TEXT
                )"
            ))
            .execute(&self.pool)
            .await?;
        } else {
            // Check if essential columns exist
            let has_id = self.check_column("groups", "id").await?;
            let has_guild_id = self.check_column("groups", "guild_id").await?;

            // If missing id or guild_id, need to recreate table (can't add PRIMARY KEY column)
            if !has_id || !has_guild_id {
                // Backup existing data before dropping table
                let backup_data = if has_guild_id {
                    sqlx::query("SELECT group_id, name, timeout, guild_id, dashboard, chat, queue, dashboard_msg, red, blu, game, game_increment, quota, connect_info FROM groups")
                        .fetch_all(&self.pool)
                        .await
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };

                sqlx::query("DROP TABLE groups").execute(&self.pool).await?;
                sqlx::query(&format!(
                    "CREATE TABLE groups (
                        id                INTEGER PRIMARY KEY,
                        group_id          INTEGER DEFAULT 0,
                        name              TEXT,
                        timeout           INTEGER DEFAULT {DEFAULT_HOT_JOIN_TIMEOUT},
                        guild_id          INTEGER NOT NULL,
                        dashboard         INTEGER NOT NULL UNIQUE,
                        chat              INTEGER NOT NULL UNIQUE,
                        queue             INTEGER NOT NULL UNIQUE,
                        dashboard_msg     INTEGER DEFAULT 0,
                        red               INTEGER NOT NULL UNIQUE,
                        blu               INTEGER NOT NULL UNIQUE,
                        game              INTEGER DEFAULT 0,
                        game_increment    INTEGER DEFAULT 0,
                        quota             INTEGER DEFAULT {DEFAULT_QUOTA},
                        connect_info      TEXT
                    )"
                ))
                .execute(&self.pool)
                .await?;

                // Restore backed up data
                for row in backup_data {
                    let group_id: i64 = row.try_get("group_id").unwrap_or(0);
                    let name: Option<String> = row.try_get("name").ok();
                    let timeout: i64 = row.try_get("timeout").unwrap_or(DEFAULT_HOT_JOIN_TIMEOUT as i64);
                    let guild_id: i64 = row.get("guild_id");
                    let dashboard: i64 = row.get("dashboard");
                    let chat: i64 = row.get("chat");
                    let queue: i64 = row.get("queue");
                    let dashboard_msg: i64 = row.try_get("dashboard_msg").unwrap_or(0);
                    let red: i64 = row.get("red");
                    let blu: i64 = row.get("blu");
                    let game: i64 = row.try_get("game").unwrap_or(0);
                    let game_increment: i64 = row.try_get("game_increment").unwrap_or(0);
                    let quota: i64 = row.try_get("quota").unwrap_or(DEFAULT_QUOTA as i64);
                    let connect_info: Option<String> = row.try_get("connect_info").ok();

                    sqlx::query(
                        "INSERT INTO groups (group_id, name, timeout, guild_id, dashboard, chat, queue, dashboard_msg, red, blu, game, game_increment, quota, connect_info)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                    )
                    .bind(group_id)
                    .bind(name)
                    .bind(timeout)
                    .bind(guild_id)
                    .bind(dashboard)
                    .bind(chat)
                    .bind(queue)
                    .bind(dashboard_msg)
                    .bind(red)
                    .bind(blu)
                    .bind(game)
                    .bind(game_increment)
                    .bind(quota)
                    .bind(connect_info)
                    .execute(&self.pool)
                    .await?;
                }
            }

            // Add name column if missing
            if !self.check_column("groups", "name").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN name TEXT")
                    .execute(&self.pool)
                    .await?;
            }

            // Add connect_info column if missing
            if !self.check_column("groups", "connect_info").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN connect_info TEXT")
                    .execute(&self.pool)
                    .await?;
            }

            // Add team_balance_method column if missing
            if !self.check_column("groups", "team_balance_method").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN team_balance_method TEXT DEFAULT 'BCH'")
                    .execute(&self.pool)
                    .await?;
            }

            // Add dm_alert_enabled column if missing
            if !self.check_column("groups", "dm_alert_enabled").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN dm_alert_enabled INTEGER DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
            }

            // Add dm_alert_threshold column if missing
            if !self.check_column("groups", "dm_alert_threshold").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN dm_alert_threshold INTEGER DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
            }

            // Add category column if missing
            if !self.check_column("groups", "category").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN category INTEGER DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
            }

            // Add dm_alert_users column if missing (JSON array of user IDs)
            if !self.check_column("groups", "dm_alert_users").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN dm_alert_users TEXT DEFAULT '[]'")
                    .execute(&self.pool)
                    .await?;
            }

            // Add team VC lifecycle settings columns if missing
            if !self.check_column("groups", "team_vc_create_policy").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN team_vc_create_policy TEXT DEFAULT 'on_hot'")
                    .execute(&self.pool)
                    .await?;
            }
            if !self.check_column("groups", "team_vc_destroy_policy").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN team_vc_destroy_policy TEXT DEFAULT 'after_pull'")
                    .execute(&self.pool)
                    .await?;
            }
            if !self.check_column("groups", "team_vc_keep_minimum").await? {
                sqlx::query("ALTER TABLE groups ADD COLUMN team_vc_keep_minimum INTEGER DEFAULT 1")
                    .execute(&self.pool)
                    .await?;
            }

            // Check if UNIQUE constraints exist on channel columns
            // SQLite doesn't have a direct way to check constraints, so we check if duplicate channels exist
            let has_duplicates: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM (
                    SELECT dashboard FROM groups GROUP BY dashboard HAVING COUNT(*) > 1
                    UNION ALL
                    SELECT chat FROM groups GROUP BY chat HAVING COUNT(*) > 1
                    UNION ALL
                    SELECT queue FROM groups GROUP BY queue HAVING COUNT(*) > 1
                    UNION ALL
                    SELECT red FROM groups GROUP BY red HAVING COUNT(*) > 1
                    UNION ALL
                    SELECT blu FROM groups GROUP BY blu HAVING COUNT(*) > 1
                )"
            )
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            // If no duplicates exist, we can safely add UNIQUE constraints by recreating the table
            if has_duplicates == 0 {
                // Check if constraints already exist by trying to insert a duplicate
                // If it fails with UNIQUE constraint error, constraints exist
                let test_result = sqlx::query("SELECT dashboard FROM groups LIMIT 1")
                    .fetch_optional(&self.pool)
                    .await?;

                if let Some(row) = test_result {
                    let test_dashboard: i64 = row.get("dashboard");
                    
                    // Try to insert a duplicate to test if UNIQUE constraint exists
                    let constraint_exists = sqlx::query(
                        "INSERT INTO groups (guild_id, dashboard, chat, queue, red, blu, quota) 
                         VALUES (999999, ?, 999998, 999997, 999996, 999995, 12)"
                    )
                    .bind(test_dashboard)
                    .execute(&self.pool)
                    .await
                    .is_err();

                    // Clean up test row if it was inserted
                    let _ = sqlx::query("DELETE FROM groups WHERE guild_id = 999999")
                        .execute(&self.pool)
                        .await;

                    // If constraint doesn't exist, recreate table with UNIQUE constraints
                    if !constraint_exists {
                        info!("Adding UNIQUE constraints to group channels...");
                        
                        // Backup all data
                        let backup_data = sqlx::query(
                            "SELECT id, group_id, name, timeout, guild_id, category, dashboard, chat, queue, 
                             dashboard_msg, red, blu, game, game_increment, quota, connect_info,
                             team_balance_method, dm_alert_enabled, dm_alert_threshold, dm_alert_users
                             FROM groups"
                        )
                        .fetch_all(&self.pool)
                        .await?;

                        // Drop and recreate table with UNIQUE constraints
                        sqlx::query("DROP TABLE groups").execute(&self.pool).await?;
                        sqlx::query(&format!(
                            "CREATE TABLE groups (
                                id                INTEGER PRIMARY KEY,
                                group_id          INTEGER DEFAULT 0,
                                name              TEXT,
                                timeout           INTEGER DEFAULT {DEFAULT_HOT_JOIN_TIMEOUT},
                                guild_id          INTEGER NOT NULL,
                                category          INTEGER DEFAULT 0,
                                dashboard         INTEGER NOT NULL UNIQUE,
                                chat              INTEGER NOT NULL UNIQUE,
                                queue             INTEGER NOT NULL UNIQUE,
                                dashboard_msg     INTEGER DEFAULT 0,
                                red               INTEGER NOT NULL UNIQUE,
                                blu               INTEGER NOT NULL UNIQUE,
                                game              INTEGER DEFAULT 0,
                                game_increment    INTEGER DEFAULT 0,
                                quota             INTEGER DEFAULT {DEFAULT_QUOTA},
                                connect_info      TEXT,
                                team_balance_method TEXT DEFAULT 'BCH',
                                dm_alert_enabled  INTEGER DEFAULT 0,
                                dm_alert_threshold INTEGER DEFAULT 0,
                                dm_alert_users    TEXT DEFAULT '[]'
                            )"
                        ))
                        .execute(&self.pool)
                        .await?;

                        // Restore data
                        for row in backup_data {
                            let id: i64 = row.get("id");
                            let group_id: i64 = row.try_get("group_id").unwrap_or(0);
                            let name: Option<String> = row.try_get("name").ok();
                            let timeout: i64 = row.try_get("timeout").unwrap_or(DEFAULT_HOT_JOIN_TIMEOUT as i64);
                            let guild_id: i64 = row.get("guild_id");
                            let dashboard: i64 = row.get("dashboard");
                            let chat: i64 = row.get("chat");
                            let queue: i64 = row.get("queue");
                            let dashboard_msg: i64 = row.try_get("dashboard_msg").unwrap_or(0);
                            let red: i64 = row.get("red");
                            let blu: i64 = row.get("blu");
                            let game: i64 = row.try_get("game").unwrap_or(0);
                            let game_increment: i64 = row.try_get("game_increment").unwrap_or(0);
                            let quota: i64 = row.try_get("quota").unwrap_or(DEFAULT_QUOTA as i64);
                            let connect_info: Option<String> = row.try_get("connect_info").ok();
                            let category: i64 = row.try_get("category").unwrap_or(0);
                            let team_balance_method: Option<String> = row.try_get("team_balance_method").ok();
                            let dm_alert_enabled: i64 = row.try_get("dm_alert_enabled").unwrap_or(0);
                            let dm_alert_threshold: i64 = row.try_get("dm_alert_threshold").unwrap_or(0);
                            let dm_alert_users: String = row.try_get("dm_alert_users").unwrap_or_else(|_| "[]".to_string());

                            sqlx::query(
                                "INSERT INTO groups (id, group_id, name, timeout, guild_id, category, dashboard, chat, queue, 
                                 dashboard_msg, red, blu, game, game_increment, quota, connect_info,
                                 team_balance_method, dm_alert_enabled, dm_alert_threshold, dm_alert_users)
                                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                            )
                            .bind(id)
                            .bind(group_id)
                            .bind(name)
                            .bind(timeout)
                            .bind(guild_id)
                            .bind(category)
                            .bind(dashboard)
                            .bind(chat)
                            .bind(queue)
                            .bind(dashboard_msg)
                            .bind(red)
                            .bind(blu)
                            .bind(game)
                            .bind(game_increment)
                            .bind(quota)
                            .bind(connect_info)
                            .bind(team_balance_method)
                            .bind(dm_alert_enabled)
                            .bind(dm_alert_threshold)
                            .bind(dm_alert_users)
                            .execute(&self.pool)
                            .await?;
                        }

                        info!("Successfully added UNIQUE constraints to group channels");
                    }
                }
            }
        }
        Ok(())
    }
    async fn verify_groups(&self) -> Result<()> {
        // Add name column if missing
        add_column!(self, "groups", "name", "TEXT", "NULL");
        
        let required_columns = vec![
            "id", "group_id", "timeout", "guild_id", "category", "dashboard",
            "chat", "queue", "dashboard_msg", "red", "blu",
            "game", "game_increment", "quota", "connect_info",
            "team_balance_method", "dm_alert_enabled", "dm_alert_threshold",
            "dm_alert_users"
        ];
        self.verify_columns("groups", &required_columns).await?;
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
    async fn verify_teams(&self)  -> Result<()> {
        let required_columns = vec!["id", "guild_id", "group_id", "red", "blu"];
        self.verify_columns("teams", &required_columns).await?;
        Ok(())
    }
    async fn create_subgroups_table(&self) -> Result<()> {
        if !self.check_table("subgroups").await? {
            sqlx::query(
                "CREATE TABLE subgroups (
                    id            INTEGER PRIMARY KEY,
                    guild_id      INTEGER NOT NULL,
                    group_id      INTEGER NOT NULL,
                    subgroup_id   INTEGER NOT NULL DEFAULT 0,
                    name          TEXT NOT NULL,
                    quota         INTEGER NOT NULL DEFAULT 12,
                    connect_info  TEXT,
                    UNIQUE(guild_id, group_id, subgroup_id)
                )"
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
    async fn verify_subgroups(&self) -> Result<()> {
        let required_columns = vec!["id", "guild_id", "group_id", "subgroup_id", "name", "quota", "connect_info"];
        self.verify_columns("subgroups", &required_columns).await?;
        Ok(())
    }
    async fn create_elo_table(&self) ->    Result<()> {
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
                    FOREIGN KEY (rank)    REFERENCES ranks(id) ON DELETE SET NULL,
                    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
                )"
            )
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
    async fn verify_elos(&self)   -> Result<()> {
        let required_columns = vec!["id", "guild_id", "user_id", "elo", "rank", "games", "wins"];
        self.verify_columns("elo", &required_columns).await?;
        
        // Check if we need to migrate the foreign key constraint
        self.migrate_elo_foreign_key().await?;
        
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
        let pragma_result: Option<String> = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='elo'"
        )
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(sql) = pragma_result {
            // If the foreign key references role_id (with or without quotes), we need to migrate
            if sql.contains("REFERENCES \"ranks\"(\"role_id\")") || sql.contains("REFERENCES ranks(role_id)") {
                info!("Detected incorrect foreign key constraint, migrating elo table...");
                
                // Create backup
                sqlx::query("CREATE TABLE elo_backup AS SELECT * FROM elo")
                    .execute(&self.pool)
                    .await?;
                
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
                        FOREIGN KEY (rank)    REFERENCES ranks(id) ON DELETE SET NULL,
                        FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
                    )"
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
                     LEFT JOIN ranks r ON r.role_id = eb.rank AND r.guild_id = eb.guild_id"
                )
                .execute(&self.pool)
                .await?;
                
                // Clean up records that couldn't find matching ranks
                sqlx::query("DELETE FROM elo WHERE rank = 0")
                    .execute(&self.pool)
                    .await?;
                
                // Drop backup
                sqlx::query("DROP TABLE elo_backup")
                    .execute(&self.pool)
                    .await?;
                
                info!("Elo table migration completed successfully");
            }
        }
        
        Ok(())
    }
    async fn create_ranks_table(&self) ->  Result<()> {
        // Check if old schema exists (has position or role_ids columns)
        let needs_migration = if self.check_table("ranks").await? {
            self.check_column("ranks", "position").await? || self.check_column("ranks", "role_ids").await?
        } else {
            false
        };

        if needs_migration {
            // Migrate old data to new schema
            let old_data: Vec<(i64, String, i64)> = sqlx::query_as(
                "SELECT guild_id, name, elo FROM ranks"
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

            sqlx::query("DROP TABLE ranks").execute(&self.pool).await?;

            sqlx::query(
                "CREATE TABLE ranks (
                    id       INTEGER PRIMARY KEY,
                    guild_id INTEGER NOT NULL,
                    name     TEXT    NOT NULL,
                    elo      INTEGER NOT NULL,
                    role_id  INTEGER NOT NULL
                )"
            )
            .execute(&self.pool)
            .await?;

            // Restore data
            for (guild_id, name, elo) in old_data {
                let _ = sqlx::query(
                    "INSERT OR IGNORE INTO ranks (guild_id, name, elo) VALUES (?, ?, ?)"
                )
                .bind(guild_id)
                .bind(&name)
                .bind(elo)
                .execute(&self.pool)
                .await;
            }
        } else if !self.check_table("ranks").await? {
            sqlx::query(
                "CREATE TABLE ranks (
                    id       INTEGER PRIMARY KEY,
                    guild_id INTEGER NOT NULL,
                    name     TEXT    NOT NULL,
                    elo      INTEGER NOT NULL,
                    role_id  INTEGER NOT NULL
                )"
            )
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
    async fn verify_ranks(&self)  -> Result<()> {
        let required_columns = vec!["id", "guild_id", "name", "elo", "role_id"];
        self.verify_columns("ranks", &required_columns).await?;
        Ok(())
    }
    
    /// Add foreign key constraint to config table after both tables exist
    async fn add_config_foreign_key(&self) -> Result<()> {
        // Check if foreign key already exists by trying to query pragma_foreign_key_list
        let has_foreign_key = sqlx::query("PRAGMA foreign_key_list(config)")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .any(|row| {
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
            let backup_data = sqlx::query(
                "SELECT guild_id, runner_id, admin_id, active_elo, default_rank FROM config"
            )
            .fetch_all(&self.pool)
            .await?;
            
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
                    FOREIGN KEY (default_rank) REFERENCES ranks(role_id) ON DELETE SET NULL
                )"
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
                     VALUES (?, ?, ?, ?, ?)"
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
    pub async fn init_first_group(&self, guild_id: GI) -> Result<()> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
            FROM groups
            WHERE guild_id = ?"
        )
        .bind(guild_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        if count == 0 {
            sqlx::query("INSERT INTO groups (group_id, guild_id, dashboard, chat, queue, red, blu)
                        VALUES (1, ?, 1, 1, 1, 1, 1)"
            )
            .bind(guild_id.get() as i64)
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
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
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
