use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{Context as Ctx, UserId as UI};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn, error};

use crate::{Database, Elo, Rank, DEFAULT_RANK};
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

    pub async fn get(&self, user_id: UI) -> Result<Player> {
        match sqlx::query("SELECT id, user_id, tag, steam_id, elo FROM users WHERE user_id = ?").bind(user_id.get() as i64).fetch_one(&self.pool).await {
            Ok(result) => Ok(Self::get_player(result)),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_with_tag(&self, user_id: UI, ctx: &Ctx) -> Result<Player> {
        let result = sqlx::query("SELECT id, user_id, tag, steam_id, elo FROM users WHERE user_id = ?")
        .bind(user_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        let player = Self::get_player(result);

        Ok(player)
    }

    /// Get player with tag and try to get display name from guild context
    pub async fn get_with_nick(&self, user_id: UI, ctx: &Ctx, guild_id: Option<serenity::all::GuildId>) -> Result<Player> {
        let mut player = self.get_with_tag(user_id, ctx).await?;

        // Try to get display name (nickname) if guild context is available
        if let Some(guild_id) = guild_id {
            player.tag = ctx.cache.member(guild_id, user_id).unwrap().display_name().to_string();
        }

        Ok(player)
    }

    /// Ensure user exists without fetching tag (for internal operations)
    pub async fn check_user(&self, user_id: UI, steam_id: Option<u64>) -> Result<Player> {
        let result = sqlx::query(
            "INSERT INTO users (user_id, steam_id, elo)
             VALUES (?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING id, user_id, tag, steam_id, elo"
        )
        .bind(user_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .bind(30) // default ELO only for new users
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn upsert(&self, user_id: UI, steam_id: Option<u64>, ctx: &Ctx) -> Result<Player> {
        // Fetch discord tag from API first
        let tag = if let Ok(user) = ctx.http.get_user(user_id).await {
            Some(user.tag())
        } else {
            None
        };

        let result = sqlx::query(
            "INSERT INTO users (user_id, steam_id, elo, tag)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id, tag=excluded.tag
             RETURNING id, user_id, tag, steam_id, elo"
        )
        .bind(user_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .bind(30) // default ELO only for new users
        .bind(&tag)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn upsert_tag(&self, user_id: UI, steam_id: Option<u64>, ctx: &Ctx) -> Result<Player> {
        let tag = if let Ok(user) = ctx.http.get_user(user_id).await {
            Some(user.tag())
        } else {
            None
        };

        let result = sqlx::query(
            "INSERT INTO users (user_id, steam_id, elo, tag)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id, tag=excluded.tag
             RETURNING id, user_id, tag, steam_id, elo"
        )
        .bind(user_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .bind(30) // default ELO only for new users
        .bind(&tag)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    fn get_player(result: sqlx::sqlite::SqliteRow) -> Player {
        let mut player = Player::default(result.get::<u64, _>           ("user_id").into(),
                                         result.get::<String, _>        ("tag"),
                                         result.get::<Option<i64>, _>   ("steam_id").map(|id| id as u64));
        
        // Set ELO if present in database, otherwise leave at default (will be updated later)
        if let Ok(Some(elo)) = result.try_get::<Option<u16>, _>("elo") {
            player.set_elo(elo);
        }
        player
    }

    fn get_rank(rank: Option<String>) -> Rank {
        match rank {
            Some(val) if val == "Beginner"    => Rank::Beginner,
            Some(val) if val == "Newcomer"    => Rank::Newcomer,
            Some(val) if val == "Novice"      => Rank::Novice,
            Some(val) if val == "Apprentice"  => Rank::Apprentice,
            Some(val) if val == "Journeyman"  => Rank::Journeyman,
            Some(val) if val == "Expert"      => Rank::Expert,
            Some(val) if val == "Master"      => Rank::Master,
            Some(val) if val == "MasterElite" => Rank::MasterElite,
            Some(val) if val == "Grandmaster" => Rank::Grandmaster,
            _                                 => DEFAULT_RANK,
        }
    }

    /// Get player with rank determined from Discord roles (for display purposes)
    pub async fn get_with_guild_rank(&self, user_id: UI, ctx: &Ctx, guild_id: u64, db: &Database) -> Result<Player> {
        info!("DEBUG: get_player_with_guild_rank called for user {} in guild {}", user_id, guild_id);
        
        let result = sqlx::query("SELECT user_id, steam_id, elo, tag FROM users WHERE user_id = ?")
            .bind(user_id.get() as i64)
            .fetch_one(&self.pool)
            .await?;

        let mut player = Self::get_player(result);
        info!("DEBUG: Player from database - ELO: {}, Rank: {}", player.elo, player.rank.name());

        // Check if player has Discord rank that should override database ELO
        use crate::handlers::player::get_player_rank;
        if let Some(discord_rank) = get_player_rank(ctx, db, guild_id.into(), user_id).await {
            info!("DEBUG: Found Discord rank {} with ELO {}", discord_rank.name(), discord_rank.default_rank_elo());
            
            // Check for ELO mismatch - if player has low ELO but high Discord rank, fix it
            let discord_default_elo = discord_rank.default_rank_elo();
            let elo_mismatch = player.elo <= 30 && discord_default_elo > 30;
            
            if elo_mismatch {
                warn!("ELO MISMATCH DETECTED: Player {} has ELO {} but Discord rank {} (default ELO {}). Auto-correcting...", 
                      user_id, player.elo, discord_rank.name(), discord_default_elo);
                
                player.rank = discord_rank;
                player.elo = discord_default_elo;
                
                // Update the database with the corrected ELO
                if let Err(e) = self.update_elo(user_id, Some(player.elo)).await {
                    error!("Failed to auto-correct ELO for player {}: {}", user_id, e);
                } else {
                    info!("Successfully auto-corrected ELO for player {} to {} (rank: {})", 
                          user_id, player.elo, discord_rank.name());
                }
            } else if player.elo <= 30 {
                info!("DEBUG: Player has low/default ELO ({}), using Discord rank {} with ELO {}", 
                      player.elo, discord_rank.name(), discord_rank.default_rank_elo());
                player.rank = discord_rank;
                player.elo = discord_default_elo;
            } else {
                info!("DEBUG: Player has meaningful ELO ({}), keeping database ELO", player.elo);
                player.update_rank_from_elo(db, guild_id).await;
                info!("DEBUG: After update_rank_from_elo - Player ELO: {}, Rank: {}", player.elo, player.rank.name());
            }
        } else {
            info!("DEBUG: No Discord rank found, using database ELO-to-rank conversion");
            player.update_rank_from_elo(db, guild_id).await;
            info!("DEBUG: After update_rank_from_elo - Player ELO: {}, Rank: {}", player.elo, player.rank.name());
        }

        Ok(player)
    }

    pub async fn update_elo(&self, user_id: UI, elo: Option<Elo>) -> Result<()> {
        // Ensure user exists
        let _ = self.check_user(user_id, None).await?;

        sqlx::query("UPDATE users SET elo = ? WHERE user_id = ?")
            .bind(elo.map(|e| e as i64))
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn update_steam_id(&self, user_id: &UI, steam_id: Option<u64>) -> Result<Player> {
        info!("Updating user steam_id for user_id: {}", user_id);

        sqlx::query("UPDATE users SET steam_id = ? WHERE user_id = ?")
            .bind(steam_id.map(|id| id as i64))
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(self.get(*user_id).await?)
    }

    pub async fn get_dm_enabled(&self, user_id: UI) -> Result<bool> {
        let result = sqlx::query("SELECT dm_enabled FROM users WHERE user_id = ?")
            .bind(user_id.get() as i64)
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

    pub async fn toggle_dm_enabled(&self, user_id: UI) -> Result<bool> {
        // Ensure user exists
        let _ = self.check_user(user_id, None).await?;

        // Get current value
        let current = self.get_dm_enabled(user_id).await?;
        let new_value = !current;

        // Update database
        sqlx::query("UPDATE users SET dm_enabled = ? WHERE user_id = ?")
            .bind(if new_value { 1 } else { 0 })
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(new_value)
    }

    /// Get user settings
    pub async fn get_settings(&self, user_id: UI) -> Result<UserSettings> {
        let result = sqlx::query(
            "SELECT auto_remove_minutes, join_announcement, vc_disconnect_on_leave,
                    announcement_color, dm_enabled, notify_quota_threshold,
                    alert_desc, alert_footer_text,
                    alert_footer_icon, alert_footer_thumbnail,
                    leave_alert, leave_alert_desc,
                    leave_alert_footer_text, leave_alert_footer_icon, leave_alert_footer_thumbnail
             FROM users WHERE user_id = ?"
        )
        .bind(user_id.get() as i64)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some(row) => {
                let auto_remove_minutes: i64 = row.try_get("auto_remove_minutes").unwrap_or(30);
                let minutes = if auto_remove_minutes == 0 { 30 } else { auto_remove_minutes };
                Ok(UserSettings {
                    expiry_duration:              Duration::from_secs((minutes as u64) * 60),
                    join_announcement:            row.try_get::<i64, _>("join_announcement").unwrap_or(0) != 0,
                    vc_kick:                      row.try_get::<i64, _>("vc_disconnect_on_leave").unwrap_or(1) != 0,
                    announcement_color:           row.try_get("announcement_color").unwrap_or(3447003),
                    dm_alerts:                    row.try_get::<i64, _>("dm_enabled").unwrap_or(1) != 0,
                    notify_quota_threshold:       row.try_get::<i64, _>("notify_quota_threshold").ok().map(|v| v as u8),
                    alert_desc:                   row.try_get("alert_desc").ok(),
                    alert_footer_text:            row.try_get("alert_footer_text").ok(),
                    alert_footer_icon:            row.try_get("alert_footer_icon").ok(),
                    alert_footer_thumbnail:       row.try_get("alert_footer_thumbnail").ok(),
                    leave_alert:                  row.try_get::<i64, _>("leave_alert").unwrap_or(0) != 0,
                    leave_alert_desc:             row.try_get("leave_alert_desc").ok(),
                    leave_alert_footer_text:      row.try_get("leave_alert_footer_text").ok(),
                    leave_alert_footer_icon:      row.try_get("leave_alert_footer_icon").ok(),
                    leave_alert_footer_thumbnail: row.try_get("leave_alert_footer_thumbnail").ok(),
                })
            }
            None => {
                // User doesn't exist, return defaults
                Ok(UserSettings::default())
            }
        }
    }

    /// Update user settings
    pub async fn update_settings(&self, user_id: UI, settings: &UserSettings) -> Result<()> {
        // Ensure user exists
        let _ = self.check_user(user_id, None).await?;

        let auto_remove_minutes = (settings.expiry_duration.as_secs() / 60) as i64;
        
        sqlx::query(
            "UPDATE users SET
                auto_remove_minutes = ?,
                join_announcement = ?,
                vc_disconnect_on_leave = ?,
                announcement_color = ?,
                dm_enabled = ?,
                notify_quota_threshold = ?,
                alert_desc = ?,
                alert_footer_text = ?,
                alert_footer_icon = ?,
                alert_footer_thumbnail = ?,
                leave_alert = ?,
                leave_alert_desc = ?,
                leave_alert_footer_text = ?,
                leave_alert_footer_icon = ?,
                leave_alert_footer_thumbnail = ?
             WHERE user_id = ?"
        )
        .bind(auto_remove_minutes)
        .bind(if settings.join_announcement { 1 } else { 0 })
        .bind(if settings.vc_kick { 1 } else { 0 })
        .bind(settings.announcement_color)
        .bind(if settings.dm_alerts { 1 } else { 0 })
        .bind(settings.notify_quota_threshold.map(|v| v as i64))
        .bind(&settings.alert_desc)
        .bind(&settings.alert_footer_text)
        .bind(&settings.alert_footer_icon)
        .bind(&settings.alert_footer_thumbnail)
        .bind(if settings.leave_alert { 1 } else { 0 })
        .bind(&settings.leave_alert_desc)
        .bind(&settings.leave_alert_footer_text)
        .bind(&settings.leave_alert_footer_icon)
        .bind(&settings.leave_alert_footer_thumbnail)
        .bind(user_id.get() as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update a single setting field
    pub async fn update_setting_field(&self, user_id: UI, field: &str, value: i64) -> Result<()> {
        // Ensure user exists
        let _ = self.check_user(user_id, None).await?;

        // Validate field name to prevent SQL injection
        let allowed_fields = ["auto_remove_minutes", "join_announcement", "vc_disconnect_on_leave", "announcement_color", "dm_enabled"];
        if !allowed_fields.contains(&field) {
            return Err(anyhow::anyhow!("Invalid setting field: {}", field));
        }

        let query_str = format!("UPDATE users SET {} = ? WHERE user_id = ?", field);
        sqlx::query(&query_str)
            .bind(value)
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

/// User settings structure
#[derive(Debug, Clone)]
pub struct UserSettings {
    pub expiry_duration:                Duration,
    pub join_announcement:              bool,
    pub vc_kick:                        bool,
    pub announcement_color:             i64,
    pub dm_alerts:                      bool,
    pub notify_quota_threshold:         Option<u8>,
    pub alert_desc:                     Option<String>,
    pub alert_footer_text:              Option<String>,
    pub alert_footer_icon:              Option<String>,
    pub alert_footer_thumbnail:         Option<String>,
    pub leave_alert:                    bool,
    pub leave_alert_desc:               Option<String>,
    pub leave_alert_footer_text:        Option<String>,
    pub leave_alert_footer_icon:        Option<String>,
    pub leave_alert_footer_thumbnail:   Option<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            expiry_duration:              Duration::from_secs(30 * 60), // Default 30 minutes
            join_announcement:            false,
            vc_kick:                      true,
            announcement_color:           3447003, // Discord blurple
            dm_alerts:                    true,
            notify_quota_threshold:       None,
            alert_desc:                   None,
            alert_footer_text:            None,
            alert_footer_icon:            None,
            alert_footer_thumbnail:       None,
            leave_alert:                  false,
            leave_alert_desc:             None,
            leave_alert_footer_text:      None,
            leave_alert_footer_icon:      None,
            leave_alert_footer_thumbnail: None,
        }
    }
}

#[async_trait]
impl Repository<Player, UI> for UserRepository {
    async fn create(&self, player: &Player) -> Result<Player> {
        self.check_user(player.user_id, player.steam_id).await
    }

    async fn get_by_id(&self, user_id: UI) -> Result<Player> {
        self.get(user_id).await
    }

    async fn update(&self, player: &Player) -> Result<Player> {
        self.update_steam_id(&player.user_id, player.steam_id).await?;
        Ok(player.clone())
    }

    async fn delete(&self, user_id: UI) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE user_id = ?")
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
