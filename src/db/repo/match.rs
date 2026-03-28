use anyhow::Result;
use serenity::all::{GuildId as GI, UserId as UI};
use sqlx::SqlitePool;
use std::time::SystemTime;

#[derive(Clone)]
pub struct MatchRepo {
  pool: SqlitePool,
}

impl MatchRepo {
  pub fn new(pool: &SqlitePool) -> Self {
    Self { pool: pool.clone() }
  }

  /// Insert a new match record and return the match ID
  pub async fn insert_match(
    &self,
    guild_id: GI,
    category_id: i64,
    format_id: i64,
    session_id: Option<String>,
    started_at: SystemTime,
    ended_at: SystemTime,
    duration_secs: u64,
  ) -> Result<i64> {
    let started_timestamp = started_at.duration_since(SystemTime::UNIX_EPOCH)?.as_secs() as i64;
    let ended_timestamp = ended_at.duration_since(SystemTime::UNIX_EPOCH)?.as_secs() as i64;

    let result = sqlx::query(
      "INSERT INTO matches (guild_id, category_id, format_id, session_id, started_at, ended_at, duration_secs)
       VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id)
    .bind(format_id)
    .bind(session_id)
    .bind(started_timestamp)
    .bind(ended_timestamp)
    .bind(duration_secs as i64)
    .execute(&self.pool)
    .await?;

    Ok(result.last_insert_rowid())
  }

  /// Insert match players for a given match
  pub async fn insert_match_players(&self, match_id: i64, players: Vec<MatchPlayerInsert>) -> Result<()> {
    for player in players {
      sqlx::query(
        "INSERT INTO match_players (match_id, user_id, team, elo_before)
         VALUES (?, ?, ?, ?)",
      )
      .bind(match_id)
      .bind(player.user_id.get() as i64)
      .bind(player.team)
      .bind(player.elo_before)
      .execute(&self.pool)
      .await?;
    }
    Ok(())
  }

  /// Update match result ('red', 'blu', or 'draw')
  pub async fn update_match_result(&self, match_id: i64, result: &str) -> Result<()> {
    sqlx::query("UPDATE matches SET result = ? WHERE id = ?")
      .bind(result)
      .bind(match_id)
      .execute(&self.pool)
      .await?;
    Ok(())
  }

  /// Get the most recent match ID for a guild/category (used to find match to update scores)
  pub async fn get_latest_match_id(&self, guild_id: GI, category_id: i64) -> Result<Option<i64>> {
    let result: Option<i64> = sqlx::query_scalar(
      "SELECT id FROM matches 
       WHERE guild_id = ? AND category_id = ?
       ORDER BY ended_at DESC
       LIMIT 1",
    )
    .bind(guild_id.get() as i64)
    .bind(category_id)
    .fetch_optional(&self.pool)
    .await?;

    Ok(result)
  }

  /// Get player match history
  pub async fn get_player_matches(&self, guild_id: GI, user_id: UI, limit: i64) -> Result<Vec<MatchRecord>> {
    let matches = sqlx::query_as::<_, MatchRecord>(
      "SELECT m.id, m.guild_id, m.category_id, m.format_id, m.session_id, 
              m.started_at, m.ended_at, m.duration_secs, m.result,
              mp.team, mp.elo_before, mp.elo_after
       FROM matches m
       JOIN match_players mp ON m.id = mp.match_id
       WHERE m.guild_id = ? AND mp.user_id = ?
       ORDER BY m.ended_at DESC
       LIMIT ?",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .bind(limit)
    .fetch_all(&self.pool)
    .await?;

    Ok(matches)
  }

  /// Get player statistics for a guild
  pub async fn get_player_stats(&self, guild_id: GI, user_id: UI) -> Result<PlayerStats> {
    let total_matches: i64 = sqlx::query_scalar(
      "SELECT COUNT(*) FROM match_players mp
       JOIN matches m ON mp.match_id = m.id
       WHERE m.guild_id = ? AND mp.user_id = ?",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .fetch_one(&self.pool)
    .await?;

    let wins: i64 = sqlx::query_scalar(
      "SELECT COUNT(*) FROM match_players mp
       JOIN matches m ON mp.match_id = m.id
       WHERE m.guild_id = ? AND mp.user_id = ?
       AND m.result = mp.team
       AND m.result IS NOT NULL",
    )
    .bind(guild_id.get() as i64)
    .bind(user_id.get() as i64)
    .fetch_one(&self.pool)
    .await?;

    Ok(PlayerStats {
      total_matches,
      wins,
      losses: total_matches - wins,
    })
  }
}

#[derive(Debug)]
pub struct MatchPlayerInsert {
  pub user_id: UI,
  pub team: String,
  pub elo_before: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct MatchRecord {
  pub id: i64,
  pub guild_id: i64,
  pub category_id: u8,
  pub format_id: i64,
  pub session_id: Option<String>,
  pub started_at: i64,
  pub ended_at: i64,
  pub duration_secs: i64,
  pub result: Option<String>,
  pub team: String,
  pub elo_before: i64,
  pub elo_after: Option<i64>,
}

#[derive(Debug)]
pub struct PlayerStats {
  pub total_matches: i64,
  pub wins: i64,
  pub losses: i64,
}
