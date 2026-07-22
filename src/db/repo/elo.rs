use anyhow::Result;
use serenity::all::{GuildId as GI, RoleId, UserId as UI};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};

use crate::{db::helpers::RowHelpers, Rank};

/// Guild-specific ELO data for a player
#[derive(Debug, Clone)]
pub struct GuildElo {
  pub elo: u16,
  pub dynamic_elo: Option<u16>,
  pub rank: Rank,
  pub games: u32,
  pub wins: u32,
  pub last_game_timestamp: Option<i64>,
}

impl GuildElo {
  /// Create GuildElo from a SQL row with proper error handling
  pub fn from_row(row: &SqliteRow, guild_id: GI) -> Result<Option<Self>> {
    // Extract ELO stats
    let (elo, games, wins) = RowHelpers::extract_elo_stats(row)?;

    // Extract dynamic_elo (nullable column)
    let dynamic_elo: Option<u16> = row.try_get::<Option<i64>, _>("dynamic_elo").unwrap_or(None).map(|v| v as u16);

    // Extract last_game_timestamp (nullable column)
    let last_game_timestamp: Option<i64> = row.try_get::<Option<i64>, _>("last_game_timestamp").unwrap_or(None);

    // Extract rank data (use default if missing)
    let rank = match RowHelpers::extract_rank_data(row, guild_id)? {
      Some(rank) => rank,
      None => Rank { guild_id, role_id: RoleId::new(0), name: "Unranked".to_string(), elo: 0 },
    };

    Ok(Some(GuildElo { elo, dynamic_elo, rank, games, wins, last_game_timestamp }))
  }
}

/// Skill tier a new player self-selects on first queue join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTier {
  Beginner,
  Intermediate,
  Expert,
  Veteran,
}

impl SkillTier {
  pub fn label(self) -> &'static str {
    match self {
      Self::Beginner => "Beginner",
      Self::Intermediate => "Intermediate",
      Self::Expert => "Expert",
      Self::Veteran => "Veteran",
    }
  }

  pub fn from_str(s: &str) -> Option<Self> {
    match s {
      "beginner" => Some(Self::Beginner),
      "intermediate" => Some(Self::Intermediate),
      "expert" => Some(Self::Expert),
      "veteran" => Some(Self::Veteran),
      _ => None,
    }
  }

  /// Dynamic ELO value for this tier, anchored around the given center point.
  ///
  /// Tiers are evenly spaced at ±100 and ±300 from the anchor.
  pub fn initial_elo(self, anchor: f64) -> u16 {
    let offset: f64 = match self {
      Self::Beginner => -800.0,
      Self::Intermediate => -400.0,
      Self::Expert => 400.0,
      Self::Veteran => 800.0,
    };
    (anchor + offset).clamp(0.0, u16::MAX as f64) as u16
  }
}

#[derive(Clone)]
pub struct EloRepository {
  pool: SqlitePool,
}

impl EloRepository {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  /// Get a player's ELO for a specific guild (returns None if no record)
  pub async fn get_if_exists(&self, user_id: UI, guild_id: GI) -> Result<Option<GuildElo>> {
    let result = sqlx::query(
      "SELECT e.elo, e.dynamic_elo, e.games, e.wins, e.last_game_timestamp, r.name, r.elo as rank_elo, r.role_id
             FROM elo e
             LEFT JOIN ranks r ON e.rank = r.id
             WHERE e.guild_id = ? AND e.user_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .fetch_optional(&self.pool)
    .await?;

    match result {
      Some(row) => GuildElo::from_row(&row, guild_id),
      None => Ok(None),
    }
  }

  /// Get a player's ELO for a specific guild (returns Discord role-based rank if no record)
  pub async fn get(&self, user_id: UI, guild_id: GI, db: &crate::Database) -> Result<GuildElo> {
    match self.get_if_exists(user_id, guild_id).await? {
      Some(elo) => Ok(elo),
      None => {
        // Player has no ELO record - fall back to guild default rank
        let default_rank = Rank::get_guild_default(db, guild_id).await.map_err(|_| anyhow::anyhow!("Failed to get default rank for guild {}", guild_id))?;
        Ok(GuildElo { elo: default_rank.elo, dynamic_elo: None, rank: default_rank, games: 0, wins: 0, last_game_timestamp: None })
      }
    }
  }

  /// Resolve a Rank to its database primary key (ranks.id)
  async fn resolve_rank_id(&self, guild_id: GI, rank: &Rank) -> Result<i64> {
    sqlx::query_scalar("SELECT id FROM ranks WHERE guild_id = ? AND name = ? AND role_id = ?")
      .bind(guild_id.get() as i64)
      .bind(&rank.name)
      .bind(rank.role_id.get() as i64)
      .fetch_one(&self.pool)
      .await
      .map_err(|e| anyhow::anyhow!("Failed to find rank ID for rank '{}': {}", rank.name, e))
  }

  /// Set or update a player's ELO and rank
  pub async fn set(&self, user_id: UI, guild_id: GI, elo: u16, rank: Rank) -> Result<()> {
    let rank_id = self.resolve_rank_id(guild_id, &rank).await?;

    sqlx::query(
      "INSERT INTO elo (guild_id, user_id, elo, rank)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET elo = excluded.elo, rank = excluded.rank",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .bind(elo as i64)
    .bind(rank_id)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Set the initial dynamic ELO for a new player based on their chosen skill tier.
  ///
  /// Only writes `dynamic_elo` when the player has no existing record or their
  /// `dynamic_elo` is still NULL — never overwrites an already-established rating.
  pub async fn set_initial_dynamic_elo(&self, user_id: UI, guild_id: GI, tier: SkillTier) -> Result<()> {
    let anchor = crate::DYNAMIC_ELO_ANCHOR;
    let initial_elo = tier.initial_elo(anchor);

    sqlx::query(
      "UPDATE elo SET dynamic_elo = ?
             WHERE guild_id = ? AND user_id = ? AND dynamic_elo IS NULL",
    )
    .bind(initial_elo as i64)
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Returns true when the player has no dynamic_elo value yet (NULL).
  pub async fn needs_skill_selection(&self, user_id: UI, guild_id: GI) -> Result<bool> {
    let result: Option<Option<i64>> = sqlx::query_scalar("SELECT dynamic_elo FROM elo WHERE guild_id = ? AND user_id = ?")
      .bind(guild_id.get() as i64)
      .bind(user_id.get() as i64)
      .fetch_optional(&self.pool)
      .await?;

    Ok(match result {
      Some(None) => true, // Row exists but dynamic_elo is NULL
      None => true,       // No row at all — also needs selection
      Some(Some(_)) => false,
    })
  }

  /// Batch set ELO and rank for multiple users in a single transaction
  pub async fn batch_set(&self, guild_id: GI, elo: u16, rank: &Rank, user_ids: &[UI]) -> Result<u32> {
    if user_ids.is_empty() {
      return Ok(0);
    }

    let rank_id = self.resolve_rank_id(guild_id, rank).await?;

    let mut tx = self.pool.begin().await?;
    let mut success = 0u32;

    for chunk in user_ids.chunks(500) {
      let placeholders: Vec<String> = chunk.iter().map(|_| "(?, ?, ?, ?)".to_string()).collect();
      let query = format!(
        "INSERT INTO elo (guild_id, user_id, elo, rank) VALUES {} ON CONFLICT(guild_id, user_id) DO UPDATE SET elo = excluded.elo, rank = excluded.rank",
        placeholders.join(", ")
      );

      let mut q = sqlx::query(&query);
      for uid in chunk {
        q = q.bind(guild_id.get() as i64).bind(uid.get() as i64).bind(elo as i64).bind(rank_id);
      }
      let result = q.execute(&mut *tx).await?;
      success += result.rows_affected() as u32;
    }

    tx.commit().await?;
    Ok(success)
  }

  /// Update only the ELO value (rank will be recalculated)
  pub async fn update_elo(&self, user_id: UI, guild_id: GI, elo: u16, db: &crate::Database) -> Result<()> {
    let rank = Rank::from_elo(db, guild_id, elo).await?;
    self.set(user_id, guild_id, elo, rank).await
  }

  /// Record a game result and update ELO
  pub async fn record_game(&self, user_id: UI, guild_id: GI, won: bool, elo_change: i16, db: &crate::Database) -> Result<GuildElo> {
    // Get current ELO or create default
    let current = self.get(user_id, guild_id, db).await?;

    // Calculate new ELO (clamp to valid u16 range)
    let new_elo = (current.elo as i32 + elo_change as i32).clamp(0, u16::MAX as i32) as u16;
    let new_rank = Rank::from_elo(db, guild_id, new_elo).await?;
    let new_games = current.games + 1;
    let new_wins = if won { current.wins + 1 } else { current.wins };

    sqlx::query(
      "INSERT INTO elo (guild_id, user_id, elo, rank, games, wins)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET 
                elo      = excluded.elo, 
                rank = excluded.rank,
                games    = excluded.games,
                wins     = excluded.wins",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .bind(new_elo as i64)
    .bind(self.resolve_rank_id(guild_id, &new_rank).await?)
    .bind(new_games as i64)
    .bind(new_wins as i64)
    .execute(&self.pool)
    .await?;

    Ok(GuildElo { elo: new_elo, dynamic_elo: current.dynamic_elo, rank: new_rank, games: new_games, wins: new_wins, last_game_timestamp: current.last_game_timestamp })
  }

  /// Record a dynamic ELO game result.
  /// Only updates `dynamic_elo`, `games`, and `wins`. Leaves legacy `elo` untouched.
  pub async fn record_dynamic_game(&self, user_id: UI, guild_id: GI, won: bool, new_dynamic_elo: u16, db: &crate::Database) -> Result<GuildElo> {
    let current = self.get(user_id, guild_id, db).await?;

    let new_games = current.games + 1;
    let new_wins = if won { current.wins + 1 } else { current.wins };
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
      "INSERT INTO elo (guild_id, user_id, elo, rank, dynamic_elo, games, wins, last_game_timestamp)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET
                dynamic_elo = excluded.dynamic_elo,
                games       = excluded.games,
                wins        = excluded.wins,
                last_game_timestamp = excluded.last_game_timestamp",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .bind(current.elo as i64)
    .bind(self.resolve_rank_id(guild_id, &current.rank).await?)
    .bind(new_dynamic_elo as i64)
    .bind(new_games as i64)
    .bind(new_wins as i64)
    .bind(now)
    .execute(&self.pool)
    .await?;

    Ok(GuildElo { elo: current.elo, dynamic_elo: Some(new_dynamic_elo), rank: current.rank, games: new_games, wins: new_wins, last_game_timestamp: Some(now) })
  }

  /// Get all ELO records for a user across all guilds
  pub async fn get_all_for_user(&self, user_id: UI) -> Result<Vec<(u64, GuildElo)>> {
    let rows = sqlx::query(
      "SELECT e.guild_id, e.elo, e.dynamic_elo, e.games, e.wins, e.last_game_timestamp, r.name, r.elo as rank_elo, r.role_id
             FROM elo e
             LEFT JOIN ranks r ON e.rank = r.id
             WHERE e.user_id = ?",
    )
    .bind(user_id.get() as i64)
    .fetch_all(&self.pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
      let guild_id: i64 = row.get("guild_id");
      let guild_gi = GI::new(guild_id as u64);

      if let Some(guild_elo) = GuildElo::from_row(&row, guild_gi)? {
        results.push((guild_id as u64, guild_elo));
      }
    }

    Ok(results)
  }

  /// Revert ELO changes for a match (set dynamic_elo back to elo_before, decrement games/wins)
  pub async fn revert_match_elo(&self, user_id: UI, guild_id: GI, elo_before: u16, won: bool) -> Result<()> {
    let current = match self.get_if_exists(user_id, guild_id).await? {
      Some(elo) => elo,
      None => return Ok(()), // No ELO record to revert
    };

    let new_dynamic_elo = Some(elo_before);
    let new_games = current.games.saturating_sub(1);
    let new_wins = if won { current.wins.saturating_sub(1) } else { current.wins };

    sqlx::query(
      "UPDATE elo
             SET dynamic_elo = ?, games = ?, wins = ?
             WHERE guild_id = ? AND user_id = ?",
    )
    .bind(new_dynamic_elo.map(|e| e as i64))
    .bind(new_games as i64)
    .bind(new_wins as i64)
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Get leaderboard for a guild (top N players by ELO)
  pub async fn get_leaderboard(&self, guild_id: GI, limit: u32) -> Result<Vec<(UI, GuildElo)>> {
    let rows = sqlx::query(
      "SELECT e.user_id, e.elo, e.dynamic_elo, e.games, e.wins, e.last_game_timestamp, r.name, r.elo as rank_elo, r.role_id
             FROM elo e
             LEFT JOIN ranks r ON e.rank = r.id
             WHERE e.guild_id = ? 
             ORDER BY e.elo DESC 
             LIMIT ?",
    )
    .bind(guild_id.get() as i64)
    .bind(limit as i64)
    .fetch_all(&self.pool)
    .await?;

    let mut results = Vec::new();
    for row in rows {
      let user_id: i64 = row.get("user_id");

      if let Some(guild_elo) = GuildElo::from_row(&row, guild_id)? {
        results.push((UI::new(user_id as u64), guild_elo));
      }
    }

    Ok(results)
  }

  /// Get the average legacy ELO of all players in a guild
  pub async fn get_guild_average_elo(&self, guild_id: GI) -> Result<f64> {
    let avg: f64 = sqlx::query_scalar("SELECT COALESCE(AVG(CAST(elo AS REAL)), 0.0) FROM elo WHERE guild_id = ?").bind(guild_id.get() as i64).fetch_one(&self.pool).await?;

    Ok(avg)
  }

  /// Get the average dynamic ELO of all players who have one (for normalization)
  pub async fn get_guild_average_dynamic_elo(&self, guild_id: GI) -> Result<f64> {
    let avg: f64 = sqlx::query_scalar("SELECT COALESCE(AVG(CAST(dynamic_elo AS REAL)), 0.0) FROM elo WHERE guild_id = ? AND dynamic_elo IS NOT NULL")
      .bind(guild_id.get() as i64)
      .fetch_one(&self.pool)
      .await?;

    Ok(avg)
  }

  /// Apply a linear offset to all dynamic ELO values in a guild (normalization).
  /// Preserves relative distances while correcting system mean drift.
  /// Only affects players who have a dynamic_elo value.
  pub async fn apply_normalization_offset(&self, guild_id: GI, offset: i32) -> Result<u32> {
    let result = if offset >= 0 {
      sqlx::query("UPDATE elo SET dynamic_elo = dynamic_elo + ? WHERE guild_id = ? AND dynamic_elo IS NOT NULL")
        .bind(offset as i64)
        .bind(guild_id.get() as i64)
        .execute(&self.pool)
        .await?
    } else {
      sqlx::query("UPDATE elo SET dynamic_elo = MAX(0, dynamic_elo + ?) WHERE guild_id = ? AND dynamic_elo IS NOT NULL")
        .bind(offset as i64)
        .bind(guild_id.get() as i64)
        .execute(&self.pool)
        .await?
    };

    Ok(result.rows_affected() as u32)
  }

  /// Migrate all guild players without a dynamic_elo to the dynamic scale.
  ///
  /// For each player where `dynamic_elo IS NULL`:
  ///   `dynamic_elo = anchor + (elo - guild_average) * scaling`
  ///
  /// Returns the number of players migrated.
  pub async fn migrate_guild_to_dynamic_elo(&self, guild_id: GI, anchor: f64, scaling: f64) -> Result<u32> {
    let avg = self.get_guild_average_elo(guild_id).await?;

    // Fetch all players without dynamic_elo
    let rows = sqlx::query("SELECT user_id, elo FROM elo WHERE guild_id = ? AND dynamic_elo IS NULL").bind(guild_id.get() as i64).fetch_all(&self.pool).await?;

    let config = crate::models::dynamic_elo::DynamicEloConfig { anchor, scaling, ..Default::default() };

    let mut count = 0u32;
    for row in rows {
      let user_id: i64 = row.get("user_id");
      let elo: i64 = row.get("elo");

      let migrated = config.migrate_elo(elo as u16, avg);

      sqlx::query("UPDATE elo SET dynamic_elo = ? WHERE guild_id = ? AND user_id = ?").bind(migrated as i64).bind(guild_id.get() as i64).bind(user_id).execute(&self.pool).await?;

      count += 1;
    }

    Ok(count)
  }

  /// Validate and normalize player ELO based on their Discord rank
  ///
  /// Discord roles define ELO ranges. Each rank has a base ELO (e.g., rank1=50, rank2=65).
  /// A player's ELO is valid if it's within [rank_base, next_rank_base).
  /// If ELO is unset or outside this range, it gets normalized to the rank's base ELO.
  pub async fn validate_and_normalize_elo(&self, user_id: UI, guild_id: GI, discord_rank: &crate::Rank, db: &crate::Database) -> Result<(u16, bool)> {
    // Get all ranks to determine the valid ELO range
    let ranks = db.ranks.get_ranks(guild_id).await?;

    // Find the next rank's base ELO to determine upper bound
    let rank_min_elo = discord_rank.elo;
    let rank_max_elo = ranks.iter().find(|r| r.elo > discord_rank.elo).map(|r| r.elo).unwrap_or(u16::MAX); // No upper bound if this is the highest rank

    // Get current ELO from database if it exists
    let existing_elo = self.get_if_exists(user_id, guild_id).await?;

    match existing_elo {
      Some(guild_elo) => {
        if guild_elo.elo >= rank_min_elo && guild_elo.elo < rank_max_elo {
          // ELO is within valid range for this rank - keep it
          Ok((guild_elo.elo, false))
        } else {
          // ELO is outside valid range - normalize to rank base
          self.set(user_id, guild_id, rank_min_elo, discord_rank.clone()).await?;
          Ok((rank_min_elo, true))
        }
      }
      None => {
        // No ELO record - set to rank base
        self.set(user_id, guild_id, rank_min_elo, discord_rank.clone()).await?;
        Ok((rank_min_elo, true))
      }
    }
  }

  /// Delete a player's ELO record for a specific guild
  pub async fn delete_for_guild(&self, user_id: UI, guild_id: GI) -> Result<()> {
    sqlx::query("DELETE FROM elo WHERE guild_id = ? AND user_id = ?").bind(guild_id.get() as i64).bind(user_id.get() as i64).execute(&self.pool).await?;
    Ok(())
  }

  /// Set or update only the dynamic ELO value
  pub async fn set_dynamic_elo(&self, user_id: UI, guild_id: GI, dynamic_elo: Option<u16>) -> Result<()> {
    sqlx::query(
      "UPDATE elo SET dynamic_elo = ?
             WHERE guild_id = ? AND user_id = ?",
    )
    .bind(dynamic_elo.map(|e| e as i64))
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .execute(&self.pool)
    .await?;

    Ok(())
  }
}
