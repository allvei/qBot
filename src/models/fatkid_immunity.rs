use anyhow::Result;
use serenity::all::{GuildId as GI, UserId as UI};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Database;

const THREE_DAYS_SECONDS: i64 = 3 * 24 * 60 * 60;
const IMMUNITY_GAMES_THRESHOLD: u32 = 2;

/// Combined immunity information for a player
#[derive(Debug, Clone)]
pub struct PlayerImmunityInfo {
  pub has_immunity: bool,
  pub immunity_level: u32,
}

/// Get complete immunity information for a player in a single call
/// This is more efficient than calling has_immunity and get_immunity_level separately
pub async fn get_player_immunity_info(db: &Database, user_id: UI, guild_id: GI) -> Result<PlayerImmunityInfo> {
  // Check game count first
  let guild_elo = db.elo.get_if_exists(user_id, guild_id).await?;
  if let Some(elo_data) = guild_elo {
    if elo_data.games < IMMUNITY_GAMES_THRESHOLD {
      return Ok(PlayerImmunityInfo { has_immunity: true, immunity_level: 0 });
    }
  } else {
    // No record = new player = has immunity
    return Ok(PlayerImmunityInfo { has_immunity: true, immunity_level: 0 });
  }

  // Check fatkid immunity timestamp
  let (immunity_level, last_fatkidded) = db.fatkids.get_immunity(user_id, guild_id).await?;

  if let Some(last_fatkidded_timestamp) = last_fatkidded {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

    let time_since_fatkid = now - last_fatkidded_timestamp;

    // If fatkidded within last 3 days, has immunity
    if time_since_fatkid < THREE_DAYS_SECONDS {
      return Ok(PlayerImmunityInfo { has_immunity: true, immunity_level });
    } else if immunity_level > 0 {
      // Immunity expired, reset it
      db.fatkids.reset_immunity(user_id, guild_id).await?;
      return Ok(PlayerImmunityInfo { has_immunity: false, immunity_level: 0 });
    }
  }

  Ok(PlayerImmunityInfo { has_immunity: false, immunity_level })
}

/// Check if a player has fatkid immunity
///
/// Immunity rules:
/// 1. Players with < 2 games have immunity
/// 2. Players fatkidded within last 3 days have immunity
/// 3. Immunity level tracks how many times they've been fatkidded
pub async fn has_immunity(db: &Database, user_id: UI, guild_id: GI) -> Result<bool> {
  let info = get_player_immunity_info(db, user_id, guild_id).await?;
  Ok(info.has_immunity)
}

/// Get immunity level for sorting (higher = more recently fatkidded)
pub async fn get_immunity_level(db: &Database, user_id: UI, guild_id: GI) -> Result<u32> {
  let info = get_player_immunity_info(db, user_id, guild_id).await?;
  Ok(info.immunity_level)
}
