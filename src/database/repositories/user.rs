use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{Context as Ctx, UserId as UI};
use sqlx::{Row, SqlitePool};
use tracing::{error, info, warn};

use crate::{Database, Elo, Rank, DEFAULT_RANK};
use crate::models::Player;
use super::Repository;

const ALERT_LINE_WIDTH:  usize = 70;
const ALERT_MAX_LINES:   usize = 6;
const FOOTER_LINE_WIDTH: usize = 100;
const FOOTER_MAX_LINES:  usize = 2;

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

/// Check if a string contains only allowed characters (ASCII printable + extended).
/// Returns true if valid, false if contains disallowed characters.
pub fn is_valid_user_text(s: &str) -> bool {
    s.chars().all(|c| {
        let code = c as u32;
        // ASCII printable (0x20-0x7E) + newline/tab + extended ASCII (0x80-0xFF)
        (0x20..=0x7E).contains(&code) || c == '\n' || c == '\t' || (0x80..=0xFF).contains(&code)
    })
}

/// Sanitize user text by removing disallowed characters.
/// Keeps ASCII printable, newline, tab, and extended ASCII (0x80-0xFF).
pub fn sanitize_user_text(s: &str) -> String {
    s.chars().filter(|&c| {
        let code = c as u32;
        (0x20..=0x7E).contains(&code) || c == '\n' || c == '\t' || (0x80..=0xFF).contains(&code)
    }).collect()
}

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

    /// Get player with rank determined from guild-specific ELO and Discord roles
    pub async fn get_with_guild_rank(&self, user_id: UI, ctx: &Ctx, guild_id: u64, db: &Database) -> Result<Player> {
        info!("DEBUG: get_player_with_guild_rank called for user {} in guild {}", user_id, guild_id);
        
        // Get base player data from users table
        let result = sqlx::query("SELECT user_id, steam_id, elo, tag FROM users WHERE user_id = ?")
            .bind(user_id.get() as i64)
            .fetch_one(&self.pool)
            .await?;

        let mut player = Self::get_player(result);
        
        // Get guild-specific ELO (this is the primary source now)
        let guild_elo = db.elos.get(user_id, guild_id).await?;
        player.elo = guild_elo.elo;
        player.rank = guild_elo.division;
        
        info!("DEBUG: Player guild ELO: {}, Rank: {}", player.elo, player.rank.name());

        // Check if player has Discord rank that should override for new players
        use crate::handlers::player::get_player_rank;
        if let Some(discord_rank) = get_player_rank(ctx, db, guild_id.into(), user_id).await {
            info!("DEBUG: Found Discord rank {} with ELO {}", discord_rank.name(), discord_rank.default_rank_elo());
            
            // Only override if player has no games (new to this guild)
            if guild_elo.games == 0 {
                let discord_default_elo = discord_rank.default_rank_elo();
                info!("DEBUG: New player in guild, using Discord rank {} with ELO {}", 
                      discord_rank.name(), discord_default_elo);
                player.rank = discord_rank;
                player.elo = discord_default_elo;
                
                // Initialize their guild ELO based on Discord rank
                if let Err(e) = db.elos.set(user_id, guild_id, player.elo, player.rank).await {
                    error!("Failed to initialize guild ELO for player {}: {}", user_id, e);
                } else {
                    info!("Initialized guild ELO for player {} to {} (rank: {})", 
                          user_id, player.elo, discord_rank.name());
                }
            }
        }

        Ok(player)
    }

    /// Update global ELO (legacy - prefer using EloRepository for guild-specific ELO)
    #[deprecated(note = "Use EloRepository::update_elo for guild-specific ELO")]
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
        self.get(*user_id).await
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
    pub async fn get_prefs(&self, user_id: UI) -> Result<UserSettings> {
        let result = sqlx::query(
            "SELECT timeout_length, join_announcement, vc_disconnect_on_leave, vc_auto_queue,
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
                let timeout_length: i64 = row.try_get("timeout_length").unwrap_or(30);
                let minutes = if timeout_length == 0 { 30 } else { timeout_length };

                // Load alert descriptions and truncate if too long
                let raw_alert_desc: Option<String> = row.try_get::<String, _>("alert_desc").ok().filter(|s| !s.is_empty());
                let raw_leave_alert_desc: Option<String> = row.try_get::<String, _>("leave_alert_desc").ok().filter(|s| !s.is_empty());
                let raw_alert_footer: Option<String> = row.try_get::<String, _>("alert_footer_text").ok().filter(|s| !s.is_empty());
                let raw_leave_alert_footer: Option<String> = row.try_get::<String, _>("leave_alert_footer_text").ok().filter(|s| !s.is_empty());

                let alert_desc = raw_alert_desc.as_ref().map(|s| {
                    let truncated = truncate_alert_message(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated alert_desc for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                let leave_alert_desc = raw_leave_alert_desc.as_ref().map(|s| {
                    let truncated = truncate_alert_message(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated leave_alert_desc for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                let alert_footer_text = raw_alert_footer.as_ref().map(|s| {
                    let truncated = truncate_footer_text(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated alert_footer_text for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                let leave_alert_footer_text = raw_leave_alert_footer.as_ref().map(|s| {
                    let truncated = truncate_footer_text(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated leave_alert_footer_text for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                // If truncation occurred, update the database
                let alert_truncated = raw_alert_desc.as_ref().map(|s| truncate_alert_message(s).len() < s.len()).unwrap_or(false);
                let leave_truncated = raw_leave_alert_desc.as_ref().map(|s| truncate_alert_message(s).len() < s.len()).unwrap_or(false);
                let footer_truncated = raw_alert_footer.as_ref().map(|s| truncate_footer_text(s).len() < s.len()).unwrap_or(false);
                let leave_footer_truncated = raw_leave_alert_footer.as_ref().map(|s| truncate_footer_text(s).len() < s.len()).unwrap_or(false);

                if alert_truncated {
                    let _ = sqlx::query("UPDATE users SET alert_desc = ? WHERE user_id = ?")
                        .bind(&alert_desc)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                if leave_truncated {
                    let _ = sqlx::query("UPDATE users SET leave_alert_desc = ? WHERE user_id = ?")
                        .bind(&leave_alert_desc)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                if footer_truncated {
                    let _ = sqlx::query("UPDATE users SET alert_footer_text = ? WHERE user_id = ?")
                        .bind(&alert_footer_text)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                if leave_footer_truncated {
                    let _ = sqlx::query("UPDATE users SET leave_alert_footer_text = ? WHERE user_id = ?")
                        .bind(&leave_alert_footer_text)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                Ok(UserSettings {
                    expiry_duration:              Duration::from_secs((minutes as u64) * 60),
                    join_announcement:            row.try_get::<i64, _>("join_announcement").unwrap_or(0) != 0,
                    vc_auto_leave:                      row.try_get::<i64, _>("vc_disconnect_on_leave").unwrap_or(1) != 0,
                    vc_auto_join:                row.try_get::<i64, _>("vc_auto_queue").unwrap_or(1) != 0,
                    announcement_color:           row.try_get("announcement_color").unwrap_or(3447003),
                    dm_alerts:                    row.try_get::<i64, _>("dm_enabled").unwrap_or(1) != 0,
                    notify_quota_threshold:       row.try_get::<i64, _>("notify_quota_threshold").ok().map(|v| v as u8),
                    alert_desc,
                    alert_footer_text,
                    alert_footer_icon:            row.try_get::<String, _>("alert_footer_icon").ok().filter(|s| !s.is_empty()),
                    alert_footer_thumbnail:       row.try_get::<String, _>("alert_footer_thumbnail").ok().filter(|s| !s.is_empty()),
                    leave_alert:                  row.try_get::<i64, _>("leave_alert").unwrap_or(0) != 0,
                    leave_alert_desc,
                    leave_alert_footer_text,
                    leave_alert_footer_icon:      row.try_get::<String, _>("leave_alert_footer_icon").ok().filter(|s| !s.is_empty()),
                    leave_alert_footer_thumbnail: row.try_get::<String, _>("leave_alert_footer_thumbnail").ok().filter(|s| !s.is_empty()),
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

        let timeout_length = (settings.expiry_duration.as_secs() / 60) as i64;
        
        sqlx::query(
            "UPDATE users SET
                timeout_length = ?,
                join_announcement = ?,
                vc_disconnect_on_leave = ?,
                vc_auto_queue = ?,
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
        .bind(timeout_length)
        .bind(if settings.join_announcement { 1 } else { 0 })
        .bind(if settings.vc_auto_leave { 1 } else { 0 })
        .bind(if settings.vc_auto_join { 1 } else { 0 })
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
        let allowed_fields = ["timeout_length", "join_announcement", "vc_disconnect_on_leave", "announcement_color", "dm_enabled"];
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
    pub vc_auto_leave:                        bool,
    pub vc_auto_join:                  bool,
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
            vc_auto_leave:                false,
            vc_auto_join:                 false,
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
