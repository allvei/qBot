use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serenity::all::{Context as Ctx, UserId as UI, GuildId as GI};
use sqlx::{Row, SqlitePool};
use tracing::{error, info, warn};

use crate::{DEFAULT_ALERT_COLOR, Database, DEFAULT_TIMEOUT, Rank};
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
            || (0x200D..=0x200D).contains(&code)    // Zero-width joiner (for compound emojis)
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
        match sqlx::query("SELECT user_id, steam_id, discord_tag FROM users WHERE user_id = ?").bind(user_id.get() as i64).fetch_one(&self.pool).await {
            Ok(result) => Ok(Self::get_player(result)),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_with_tag(&self, user_id: UI, ctx: &Ctx) -> Result<Player> {
        let result = sqlx::query("SELECT user_id, steam_id, discord_tag FROM users WHERE user_id = ?")
        .bind(user_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        let mut player = Self::get_player(result);
        
        // If no tag in database, fetch from Discord API and cache it
        if player.tag.is_empty() {
            if let Ok(user) = ctx.http.get_user(user_id).await {
                player.tag = user.display_name().to_string();
                // Cache the tag for future use
                let _ = self.update_discord_tag(user_id, &player.tag).await;
            }
        }

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
            "INSERT INTO users (user_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING user_id, steam_id, discord_tag"
        )
        .bind(user_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn upsert(&self, user_id: UI, steam_id: Option<u64>) -> Result<Player> {
        let result = sqlx::query(
            "INSERT INTO users (user_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(user_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING user_id, steam_id, discord_tag"
        )
        .bind(user_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    /// 
    fn get_player(result: sqlx::sqlite::SqliteRow) -> Player {
        let user_id: i64          = result.get("user_id");
        let steam_id: Option<i64> = result.try_get("steam_id").ok();
        let discord_tag: Option<String> = result.try_get("discord_tag").ok().flatten();
        
        // Use stored discord_tag if available, otherwise empty string
        let tag = discord_tag.unwrap_or_default();
        
        // No rank available from basic user query - must be set later with guild-specific data
        Player::add(
            (user_id as u64).into(),
            tag,
            steam_id.map(|id| id as u64),
            None // No rank available without guild context
        )
    }

    /// Get player with rank determined from guild-specific ELO and Discord roles
    pub async fn get_with_guild_rank(&self, user_id: UI, _ctx: &Ctx, guild_id: GI, db: &Database) -> Result<Player> {
        info!("DEBUG: get_player_with_guild_rank called for user {} in guild {}", user_id, guild_id);
        
        // Get base player data from users table
        let result = sqlx::query("SELECT user_id, steam_id, discord_tag FROM users WHERE user_id = ?")
            .bind(user_id.get() as i64)
            .fetch_one(&self.pool)
            .await?;

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
                Some(role_id) => db.ranks.rank_from_role_id(guild_id, role_id).await?,
                None => return Err(anyhow!("No default rank configured for guild"))
            };
            
            // Convert the guild rank's ELO to the appropriate Rank struct
            let assigned_rank = Rank::from_elo(db, guild_id, default_guild_rank.elo).await?;
            let default_elo = default_guild_rank.elo;
            
            info!("DEBUG: New player in guild, using default rank {} (role {}) with ELO {}", 
                  assigned_rank.name, default_guild_rank.role_id, default_elo);
            player.rank = Some(assigned_rank.clone());
            player.elo  = default_elo;
            
            // Initialize their guild ELO
            if let Err(e) = db.elo.set(user_id, guild_id, player.elo, player.rank.clone().unwrap()).await {
                error!("Failed to initialize guild ELO for player {}: {}", user_id, e);
            } else {
                info!("Initialized guild ELO for player {} to {} (rank: {})", 
                      user_id, player.elo, assigned_rank.name);
            }
        }

        Ok(player)
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

    /// Update user's discord tag in database
    pub async fn update_discord_tag(&self, user_id: UI, discord_tag: &str) -> Result<()> {
        sqlx::query("UPDATE users SET discord_tag = ? WHERE user_id = ?")
            .bind(discord_tag)
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get user tag - retrieves from database or fetches from Discord API
    pub async fn get_tag(&self, user_id: UI, ctx: &Ctx) -> String {
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

    pub async fn get_pm_hot_alert(&self, user_id: UI) -> Result<bool> {
        let result = sqlx::query("SELECT pm_hot_alert FROM users WHERE user_id = ?")
            .bind(user_id.get() as i64)
            .fetch_optional(&self.pool)
            .await?;

        match result {
            Some(row) => {
                let pm_hot_alert: i64 = row.try_get("pm_hot_alert").unwrap_or(1);
                Ok(pm_hot_alert != 0)
            }
            None => {
                // User doesn't exist yet, return default (enabled)
                Ok(true)
            }
        }
    }

    pub async fn toggle_pm_hot_alert(&self, user_id: UI) -> Result<bool> {
        // Ensure user exists
        let _ = self.check_user(user_id, None).await?;

        // Get current value
        let current = self.get_pm_hot_alert(user_id).await?;
        let new_value = !current;

        // Update database
        sqlx::query("UPDATE users SET pm_hot_alert = ? WHERE user_id = ?")
            .bind(if new_value { 1 } else { 0 })
            .bind(user_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(new_value)
    }

    /// Get user settings
    pub async fn get_prefs(&self, user_id: UI) -> Result<UserSettings> {
        let result = sqlx::query(
            "SELECT timeout, vc_auto_leave, vc_auto_join,
                    join_alert_color, pm_hot_alert, pm_queue_alert_threshold,
                    join_alert_title, join_alert, join_alert_footer,
                    join_alert_footer_img, join_alert_img,
                    leave_alert_title, leave_alert,
                    leave_alert_footer, leave_alert_footer_img, leave_alert_img,
                    leave_alert_color
             FROM users WHERE user_id = ?"
        )
        .bind(user_id.get() as i64)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some(row) => {
                // Load alert descriptions and truncate if too long
                let raw_join_alert:         Option<String> = row.try_get::<String, _>("join_alert")        .ok().filter(|s| !s.is_empty());
                let raw_leave_alert:        Option<String> = row.try_get::<String, _>("leave_alert")       .ok().filter(|s| !s.is_empty());
                let raw_alert_footer:       Option<String> = row.try_get::<String, _>("join_alert_footer") .ok().filter(|s| !s.is_empty());
                let raw_leave_alert_footer: Option<String> = row.try_get::<String, _>("leave_alert_footer").ok().filter(|s| !s.is_empty());

                let join_alert = raw_join_alert.as_ref().map(|s| {
                    let truncated = truncate_alert_message(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated join_alert for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                let leave_alert = raw_leave_alert.as_ref().map(|s| {
                    let truncated = truncate_alert_message(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated leave_alert for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                let join_alert_footer = raw_alert_footer.as_ref().map(|s| {
                    let truncated = truncate_footer_text(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated join_alert_footer for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                let leave_alert_footer = raw_leave_alert_footer.as_ref().map(|s| {
                    let truncated = truncate_footer_text(s);
                    if truncated.len() < s.len() {
                        warn!("Truncated leave_alert_footer for user {}: {} -> {} chars", user_id, s.len(), truncated.len());
                    }
                    truncated
                }).filter(|s| !s.is_empty());

                // If truncation occurred, update the database  
                let alert_truncated        = raw_join_alert        .as_ref().map(|s| truncate_alert_message(s).len() < s.len()).unwrap_or(false);
                let leave_truncated        = raw_leave_alert  .as_ref().map(|s| truncate_alert_message(s).len() < s.len()).unwrap_or(false);
                let footer_truncated       = raw_alert_footer      .as_ref().map(|s| truncate_footer_text(s)  .len() < s.len()).unwrap_or(false);
                let leave_footer_truncated = raw_leave_alert_footer.as_ref().map(|s| truncate_footer_text(s)  .len() < s.len()).unwrap_or(false);

                if alert_truncated {
                    let _ = sqlx::query("UPDATE users SET join_alert = ? WHERE user_id = ?")
                        .bind(&join_alert)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                if leave_truncated {
                    let _ = sqlx::query("UPDATE users SET leave_alert = ? WHERE user_id = ?")
                        .bind(&leave_alert)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                if footer_truncated {
                    let _ = sqlx::query("UPDATE users SET join_alert_footer = ? WHERE user_id = ?")
                        .bind(&join_alert_footer)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                if leave_footer_truncated {
                    let _ = sqlx::query("UPDATE users SET leave_alert_footer = ? WHERE user_id = ?")
                        .bind(&leave_alert_footer)
                        .bind(user_id.get() as i64)
                        .execute(&self.pool)
                        .await;
                }

                Ok(UserSettings {
                    pm_hot_alert:             row.try_get::<i64, _>   ("pm_hot_alert")            .unwrap_or(1) != 0,
                    timeout:                  row.try_get::<u8, _>    ("timeout")                 .unwrap_or(DEFAULT_TIMEOUT),
                    vc_auto_join:             row.try_get::<i64, _>   ("vc_auto_join")            .unwrap_or(1) != 0,
                    join_alert_title:         row.try_get::<String, _>("join_alert_title")        .ok().filter(|s| !s.is_empty()),
                    join_alert_desc:          join_alert.clone(),
                    join_alert_color:         row.try_get::<u32, _>   ("join_alert_color")        .unwrap_or(DEFAULT_ALERT_COLOR),
                    join_alert_img:           row.try_get::<String, _>("join_alert_img")          .ok().filter(|s| !s.is_empty()),
                    join_alert_footer:        row.try_get::<String, _>("join_alert_footer")       .ok().filter(|s| !s.is_empty()),
                    join_alert_footer_img:    row.try_get::<String, _>("join_alert_footer_img")   .ok().filter(|s| !s.is_empty()),
                    vc_auto_leave:            row.try_get::<i64, _>   ("vc_auto_leave")           .unwrap_or(1) != 0,
                    leave_alert_title:        row.try_get::<String, _>("leave_alert_title")       .ok().filter(|s| !s.is_empty()),
                    leave_alert_desc:         row.try_get::<String, _>("leave_alert_desc")        .ok().filter(|s| !s.is_empty()),
                    leave_alert_color:        row.try_get::<u32, _>   ("leave_alert_color")       .unwrap_or(DEFAULT_ALERT_COLOR),
                    leave_alert_img:          row.try_get::<String, _>("leave_alert_img")         .ok().filter(|s| !s.is_empty()),
                    leave_alert_footer:       row.try_get::<String, _>("leave_alert_footer")      .ok().filter(|s| !s.is_empty()),
                    leave_alert_footer_img:   row.try_get::<String, _>("leave_alert_footer_img")  .ok().filter(|s| !s.is_empty()),
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

        sqlx::query(
            "UPDATE users SET
                pm_hot_alert           = ?,
                timeout                = ?,
                vc_auto_join           = ?,
                join_alert_title       = ?,
                join_alert             = ?,
                join_alert_color       = ?,
                join_alert_img         = ?,
                join_alert_footer      = ?,
                join_alert_footer_img  = ?,
                vc_auto_leave          = ?,
                leave_alert_title      = ?,
                leave_alert            = ?,
                leave_alert_color      = ?,
                leave_alert_img        = ?,
                leave_alert_footer     = ?,
                leave_alert_footer_img = ?
                WHERE user_id          = ?"
        )
        .bind(settings.pm_hot_alert)
        .bind(settings.timeout)
        .bind(settings.vc_auto_join)
        .bind(&settings.join_alert_title)
        .bind(&settings.join_alert_desc)
        .bind(settings.join_alert_color)
        .bind(&settings.join_alert_img)
        .bind(&settings.join_alert_footer)
        .bind(&settings.join_alert_footer_img)
        .bind(settings.vc_auto_leave)
        .bind(&settings.leave_alert_title)
        .bind(&settings.leave_alert_desc)
        .bind(settings.leave_alert_color)
        .bind(&settings.leave_alert_img)
        .bind(&settings.leave_alert_footer)
        .bind(&settings.leave_alert_footer_img)
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
        let allowed_fields = ["timeout", "join_alert", "vc_auto_leave", "join_alert_color", "pm_hot_alert"];
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
    pub pm_hot_alert:             bool,
    pub timeout:                  u8,
    pub vc_auto_join:             bool,
    pub join_alert_title:         Option<String>,
    pub join_alert_desc:          Option<String>,
    pub join_alert_color:         u32,
    pub join_alert_img:           Option<String>,
    pub join_alert_footer:        Option<String>,
    pub join_alert_footer_img:    Option<String>,
    pub vc_auto_leave:            bool,
    pub leave_alert_title:        Option<String>,
    pub leave_alert_desc:         Option<String>,
    pub leave_alert_color:        u32,
    pub leave_alert_img:          Option<String>,
    pub leave_alert_footer:       Option<String>,
    pub leave_alert_footer_img:   Option<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            pm_hot_alert:             false,
            timeout:                  DEFAULT_TIMEOUT,
            vc_auto_join:             false,
            join_alert_title:         None,
            join_alert_desc:          None,
            join_alert_color:         DEFAULT_ALERT_COLOR,
            join_alert_img:           None,
            join_alert_footer:        None,
            join_alert_footer_img:    None,
            vc_auto_leave:            false,
            leave_alert_title:        None,
            leave_alert_desc:         None,
            leave_alert_color:        DEFAULT_ALERT_COLOR,
            leave_alert_img:          None,
            leave_alert_footer:       None,
            leave_alert_footer_img:   None,
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
