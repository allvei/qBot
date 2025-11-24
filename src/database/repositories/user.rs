use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{Context, UserId};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::models::Player;
use super::Repository;

#[derive(Clone)]
pub struct UserRepository {
    pool: SqlitePool,
}

impl UserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_by_discord_id(&self, discord_id: UserId) -> Result<Player> {
        let result = sqlx::query(
            "SELECT id, discord_id, steam_id FROM users WHERE discord_id = ?"
        )
        .bind(discord_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn get_by_discord_id_with_tag(&self, discord_id: UserId, ctx: &Context) -> Result<Player> {
        let result = sqlx::query(
            "SELECT id, discord_id, steam_id FROM users WHERE discord_id = ?"
        )
        .bind(discord_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        let mut player = Self::get_player(result);

        // Fetch discord tag from Discord API
        if let Ok(user) = ctx.http.get_user(discord_id).await {
            player.discord_tag = Some(user.tag());
        }

        Ok(player)
    }

    pub async fn create_or_update(&self, discord_id: UserId, steam_id: Option<u64>) -> Result<Player> {
        // info!("Creating or updating user with discord_id: {}", discord_id);

        let result = sqlx::query(
            "INSERT INTO users (discord_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(discord_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING id, discord_id, steam_id"
        )
        .bind(discord_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn create_or_update_with_tag(&self, discord_id: UserId, steam_id: Option<u64>, ctx: &Context) -> Result<Player> {
        // info!("Creating or updating user with discord_id: {}", discord_id);

        let result = sqlx::query(
            "INSERT INTO users (discord_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(discord_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING id, discord_id, steam_id"
        )
        .bind(discord_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;

        let mut player = Self::get_player(result);

        // Fetch discord tag from Discord API
        if let Ok(user) = ctx.http.get_user(discord_id).await {
            player.discord_tag = Some(user.tag());
        }

        Ok(player)
    }

    fn get_player(result: sqlx::sqlite::SqliteRow) -> Player {
        Player::add(result.get::<u64, _>        ("discord_id").into(),
                    None,
                    result.get::<Option<i64>, _>("steam_id")  .map(|id| id as u64)
                   )
    }

    pub async fn update_steam_id(&self, discord_id: UserId, steam_id: Option<u64>) -> Result<Player> {
        info!("Updating user steam_id for discord_id: {}", discord_id);

        sqlx::query("UPDATE users SET steam_id = ? WHERE discord_id = ?")
            .bind(steam_id.map(|id| id as i64))
            .bind(discord_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(Player::add(discord_id, None, steam_id))
    }

    pub async fn get_dm_enabled(&self, discord_id: UserId) -> Result<bool> {
        let result = sqlx::query("SELECT dm_enabled FROM users WHERE discord_id = ?")
            .bind(discord_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        match result {
            Some(row) => {
                let dm_enabled: i64 = row.try_get("dm_enabled").unwrap_or(1);
                Ok(dm_enabled != 0)
            }
            None => {
                // User doesn't exist yet, return default (enabled)
                Ok(true)
            }
        }
    }

    pub async fn toggle_dm_enabled(&self, discord_id: UserId) -> Result<bool> {
        // Ensure user exists
        let _ = self.create_or_update(discord_id, None).await?;

        // Get current value
        let current = self.get_dm_enabled(discord_id).await?;
        let new_value = !current;

        // Update database
        sqlx::query("UPDATE users SET dm_enabled = ? WHERE discord_id = ?")
            .bind(if new_value { 1 } else { 0 })
            .bind(discord_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(new_value)
    }

    /// Get user settings
    pub async fn get_settings(&self, discord_id: UserId) -> Result<UserSettings> {
        let result = sqlx::query(
            "SELECT auto_remove_minutes, join_announcement, vc_disconnect_on_leave,
                    announcement_color, dm_enabled, notify_quota_threshold,
                    announcement_description, announcement_footer_text,
                    announcement_footer_icon, announcement_thumbnail,
                    leave_announcement, leave_announcement_description,
                    leave_announcement_footer_text, leave_announcement_footer_icon, leave_announcement_thumbnail
             FROM users WHERE discord_id = ?"
        )
        .bind(discord_id.get() as i64)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some(row) => {
                let auto_remove = row.try_get("auto_remove_minutes").unwrap_or(30);
                Ok(UserSettings {
                auto_remove_time: if auto_remove == 0 { 30 } else { auto_remove },
                join_announcement: row.try_get::<i64, _>("join_announcement").unwrap_or(0) != 0,
                vc_kick: row.try_get::<i64, _>("vc_disconnect_on_leave").unwrap_or(1) != 0,
                announcement_color: row.try_get("announcement_color").unwrap_or(3447003),
                dm_alerts: row.try_get::<i64, _>("dm_enabled").unwrap_or(1) != 0,
                notify_quota_threshold: row.try_get::<i64, _>("notify_quota_threshold").ok().map(|v| v as u8),
                announcement_description: row.try_get("announcement_description").ok(),
                announcement_footer_text: row.try_get("announcement_footer_text").ok(),
                announcement_footer_icon: row.try_get("announcement_footer_icon").ok(),
                announcement_thumbnail: row.try_get("announcement_thumbnail").ok(),
                leave_announcement: row.try_get::<i64, _>("leave_announcement").unwrap_or(0) != 0,
                leave_announcement_description: row.try_get("leave_announcement_description").ok(),
                leave_announcement_footer_text: row.try_get("leave_announcement_footer_text").ok(),
                leave_announcement_footer_icon: row.try_get("leave_announcement_footer_icon").ok(),
                leave_announcement_thumbnail: row.try_get("leave_announcement_thumbnail").ok(),
            })
            }
            None => {
                // User doesn't exist, return defaults
                Ok(UserSettings::default())
            }
        }
    }

    /// Update user settings
    pub async fn update_settings(&self, discord_id: UserId, settings: &UserSettings) -> Result<()> {
        // Ensure user exists
        let _ = self.create_or_update(discord_id, None).await?;

        sqlx::query(
            "UPDATE users SET
                auto_remove_minutes = ?,
                join_announcement = ?,
                vc_disconnect_on_leave = ?,
                announcement_color = ?,
                dm_enabled = ?,
                notify_quota_threshold = ?,
                announcement_description = ?,
                announcement_footer_text = ?,
                announcement_footer_icon = ?,
                announcement_thumbnail = ?,
                leave_announcement = ?,
                leave_announcement_description = ?,
                leave_announcement_footer_text = ?,
                leave_announcement_footer_icon = ?,
                leave_announcement_thumbnail = ?
             WHERE discord_id = ?"
        )
        .bind(settings.auto_remove_time)
        .bind(if settings.join_announcement { 1 } else { 0 })
        .bind(if settings.vc_kick { 1 } else { 0 })
        .bind(settings.announcement_color)
        .bind(if settings.dm_alerts { 1 } else { 0 })
        .bind(settings.notify_quota_threshold.map(|v| v as i64))
        .bind(&settings.announcement_description)
        .bind(&settings.announcement_footer_text)
        .bind(&settings.announcement_footer_icon)
        .bind(&settings.announcement_thumbnail)
        .bind(if settings.leave_announcement { 1 } else { 0 })
        .bind(&settings.leave_announcement_description)
        .bind(&settings.leave_announcement_footer_text)
        .bind(&settings.leave_announcement_footer_icon)
        .bind(&settings.leave_announcement_thumbnail)
        .bind(discord_id.get() as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update a single setting field
    pub async fn update_setting_field(&self, discord_id: UserId, field: &str, value: i64) -> Result<()> {
        // Ensure user exists
        let _ = self.create_or_update(discord_id, None).await?;

        // Validate field name to prevent SQL injection
        let allowed_fields = ["auto_remove_minutes", "join_announcement", "vc_disconnect_on_leave",
                               "announcement_color", "dm_enabled"];
        if !allowed_fields.contains(&field) {
            return Err(anyhow::anyhow!("Invalid setting field: {}", field));
        }

        let query_str = format!("UPDATE users SET {} = ? WHERE discord_id = ?", field);
        sqlx::query(&query_str)
            .bind(value)
            .bind(discord_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

/// User settings structure
#[derive(Debug, Clone)]
pub struct UserSettings {
    pub auto_remove_time:               i64,
    pub join_announcement:              bool,
    pub vc_kick:                        bool,
    pub announcement_color:             i64,
    pub dm_alerts:                      bool,
    pub notify_quota_threshold:         Option<u8>,
    pub announcement_description:       Option<String>,
    pub announcement_footer_text:       Option<String>,
    pub announcement_footer_icon:       Option<String>,
    pub announcement_thumbnail:         Option<String>,
    pub leave_announcement:             bool,
    pub leave_announcement_description: Option<String>,
    pub leave_announcement_footer_text: Option<String>,
    pub leave_announcement_footer_icon: Option<String>,
    pub leave_announcement_thumbnail:   Option<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            auto_remove_time:               30, // Default 30 minutes, enforced 1-60 range
            join_announcement:              false,
            vc_kick:                        true,
            announcement_color:             3447003, // Discord blurple
            dm_alerts:                      true,
            notify_quota_threshold:         None,
            announcement_description:       None,
            announcement_footer_text:       None,
            announcement_footer_icon:       None,
            announcement_thumbnail:         None,
            leave_announcement:             false,
            leave_announcement_description: None,
            leave_announcement_footer_text: None,
            leave_announcement_footer_icon: None,
            leave_announcement_thumbnail:   None,
        }
    }
}

#[async_trait]
impl Repository<Player, UserId> for UserRepository {
    async fn create(&self, player: &Player) -> Result<Player> {
        self.create_or_update(player.discord_id, player.steam_id).await
    }

    async fn get_by_id(&self, discord_id: UserId) -> Result<Player> {
        self.get_by_discord_id(discord_id).await
    }

    async fn update(&self, player: &Player) -> Result<Player> {
        self.update_steam_id(player.discord_id, player.steam_id).await
    }

    async fn delete(&self, discord_id: UserId) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE discord_id = ?")
            .bind(discord_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
