use std::sync::Arc;

use anyhow::Result;
use serenity::all::{GuildId as GI, UserId as UI};
use tracing::{info, warn};

use crate::db::Database;
use crate::models::session::{SessionPlayer, Team};

/// Configuration for the PTB-Elo dynamic rating system.
///
/// Zero-Sum with Normalization engine that prioritizes rapid establishment
/// of new player ratings while maintaining long-term system stability.
pub struct DynamicEloConfig {
  /// Target center point of the bell curve
  pub anchor: f64,
  /// Maximum K-factor for brand new players (high volatility)
  pub k_ceiling: f64,
  /// Minimum K-factor for established players (low volatility)
  pub k_floor: f64,
  /// Exponential decay rate for K-factor
  pub decay_rate: f64,
  /// Trigger normalization correction if avg drifts by this many points
  pub offset_threshold: f64,
  /// The correction shift size when normalization triggers
  pub offset_amount: f64,
  /// ELO units per 1 legacy/manual point (for migration)
  pub scaling: f64,
}

impl Default for DynamicEloConfig {
  fn default() -> Self {
    Self {
      anchor: crate::DYNAMIC_ELO_ANCHOR,
      k_ceiling: crate::DYNAMIC_ELO_K_CEILING,
      k_floor: crate::DYNAMIC_ELO_K_FLOOR,
      decay_rate: crate::DYNAMIC_ELO_DECAY_RATE,
      offset_threshold: crate::DYNAMIC_ELO_OFFSET_THRESHOLD,
      offset_amount: crate::DYNAMIC_ELO_OFFSET_AMOUNT,
      scaling: crate::DYNAMIC_ELO_SCALING,
    }
  }
}

/// Player snapshot for a single match ELO calculation
#[derive(Debug, Clone)]
pub struct MatchParticipant {
  pub user_id: UI,
  pub elo: u16,
  pub games: u32,
  pub last_game_timestamp: Option<i64>,
}

/// Result of an ELO calculation for one player
#[derive(Debug, Clone)]
pub struct EloChange {
  pub user_id: UI,
  pub old_elo: u16,
  pub new_elo: u16,
  pub change: i32,
  pub won: bool,
}

impl DynamicEloConfig {
  /// Expected score (probability of winning) for team_a vs team_b
  /// based on average team ELO ratings.
  ///
  /// E_a = 1 / (1 + 10^((R_b - R_a) / 400))
  pub fn expected_score(team_a_avg: f64, team_b_avg: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((team_b_avg - team_a_avg) / 400.0))
  }

  /// Dynamic K-factor for a player based on total matches played and inactivity.
  /// Decays exponentially from k_ceiling to k_floor, with a boost for returning players.
  ///
  /// K(n) = K_floor + (K_ceiling - K_floor) * e^(-decay_rate * n)
  /// Hiatus boost: increases by (K_ceiling - K_floor) / 200 per day inactive, capped at (K_ceiling - K_floor) / 2
  /// Diminishing returns: if base KF is already high, reduce the hiatus boost proportionally
  pub fn k_factor(&self, games: u32, last_game_timestamp: Option<i64>) -> f64 {
    let base_k = self.k_floor + (self.k_ceiling - self.k_floor) * (-self.decay_rate * games as f64).exp();

    // Check for hiatus boost
    if let Some(last_ts) = last_game_timestamp {
      let now = chrono::Utc::now().timestamp();
      let days_inactive = (now - last_ts) / 86400; // Convert seconds to days

      if days_inactive > 0 {
        let scope = self.k_ceiling - self.k_floor;
        let daily_increase = scope / 200.0;
        let max_boost = scope / 2.0;

        // Calculate raw hiatus boost
        let raw_boost = (days_inactive as f64 * daily_increase).min(max_boost);

        // Apply diminishing returns based on how close base_k is to ceiling
        // If base_k is close to ceiling, reduce the boost proportionally
        let base_k_progress = (base_k - self.k_floor) / scope; // 0 to 1
        let diminishing_factor = 1.0 - base_k_progress; // Reduce boost by this factor
        let effective_boost = raw_boost * diminishing_factor.max(0.0);

        return base_k + effective_boost;
      }
    }

    base_k
  }

  /// Convert a legacy/manual ELO value to the dynamic scale.
  ///
  /// new_elo = anchor + (old_elo - guild_average) * scaling
  ///
  /// `guild_average` is the current mean ELO of the guild being migrated.
  pub fn migrate_elo(&self, old_elo: u16, guild_average: f64) -> u16 {
    let migrated = self.anchor + (old_elo as f64 - guild_average) * self.scaling;
    migrated.round().clamp(0.0, u16::MAX as f64) as u16
  }

  /// Calculate ELO changes for all players in a team match.
  ///
  /// Uses K-Weighted Pool distribution:
  /// 1. Total Match K = sum of all individual K-factors
  /// 2. Match Pool = Total_K, scaled by the upset factor
  /// 3. Each player's change = (K_i / Total_K) * Pool * (S - E)
  ///
  /// Returns a vec of EloChange for every participant.
  pub fn calculate_match(&self, team_red: &[MatchParticipant], team_blu: &[MatchParticipant], result: MatchResult) -> Vec<EloChange> {
    if team_red.is_empty() || team_blu.is_empty() {
      return Vec::new();
    }

    let red_avg = team_red.iter().map(|p| p.elo as f64).sum::<f64>() / team_red.len() as f64;
    let blu_avg = team_blu.iter().map(|p| p.elo as f64).sum::<f64>() / team_blu.len() as f64;

    // Expected score from RED's perspective
    let e_red = Self::expected_score(red_avg, blu_avg);

    // Actual score from RED's perspective
    let s_red = match result {
      MatchResult::RedWin => 1.0,
      MatchResult::BluWin => 0.0,
      MatchResult::Draw => 0.5,
    };

    // Total Match K across all players
    let all_players: Vec<&MatchParticipant> = team_red.iter().chain(team_blu.iter()).collect();
    let total_k: f64 = all_players.iter().map(|p| self.k_factor(p.games, p.last_game_timestamp)).sum();

    // Upset factor: scales the pool based on how unexpected the result was
    // Larger pool for upsets, smaller for expected results
    let upset_factor = (s_red - e_red).abs() + 0.5;
    let match_pool = total_k * upset_factor;

    let mut changes = Vec::new();

    // RED team changes
    for p in team_red {
      let k = self.k_factor(p.games, p.last_game_timestamp);
      let share = k / total_k;
      let delta = (match_pool * share * (s_red - e_red)).round() as i32;
      let new_elo = (p.elo as i32 + delta).clamp(0, u16::MAX as i32) as u16;
      changes.push(EloChange { user_id: p.user_id, old_elo: p.elo, new_elo, change: delta, won: matches!(result, MatchResult::RedWin) });
    }

    // BLU team changes (mirror: S_blu = 1 - S_red, E_blu = 1 - E_red)
    let s_blu = 1.0 - s_red;
    let e_blu = 1.0 - e_red;
    for p in team_blu {
      let k = self.k_factor(p.games, p.last_game_timestamp);
      let share = k / total_k;
      let delta = (match_pool * share * (s_blu - e_blu)).round() as i32;
      let new_elo = (p.elo as i32 + delta).clamp(0, u16::MAX as i32) as u16;
      changes.push(EloChange { user_id: p.user_id, old_elo: p.elo, new_elo, change: delta, won: matches!(result, MatchResult::BluWin) });
    }

    changes
  }

  /// Calculate the normalization offset to apply system-wide.
  /// Returns 0 if no correction is needed.
  pub fn normalization_offset(&self, current_average: f64) -> i32 {
    let drift = current_average - self.anchor;
    if drift.abs() >= self.offset_threshold {
      if drift > 0.0 {
        -(self.offset_amount as i32)
      } else {
        self.offset_amount as i32
      }
    } else {
      0
    }
  }
}

/// Match result from a team perspective
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchResult {
  RedWin,
  BluWin,
  Draw,
}

impl MatchResult {
  pub fn from_str(s: &str) -> Option<Self> {
    match s {
      "red" => Some(Self::RedWin),
      "blu" => Some(Self::BluWin),
      "draw" => Some(Self::Draw),
      _ => None,
    }
  }
}

/// Process ELO changes for a completed match when dynamic ELO is enabled.
///
/// 1. Reads current ELO/games for each player
/// 2. Calculates changes via the dynamic engine
/// 3. Applies changes to the elo table
/// 4. Updates elo_after in match_players
/// 5. Runs normalization check
///
/// Returns the list of changes applied, or None if dynamic ELO is disabled.
pub async fn process_match_elo(
  db: &Arc<Database>,
  guild_id: GI,
  match_id: i64,
  session_players: &[SessionPlayer],
  result_str: &str,
  ctx: &serenity::prelude::Context,
) -> Result<Option<Vec<EloChange>>> {
  // Check if dynamic ELO is enabled
  let is_enabled = db.config.get_active_elo(guild_id).await.unwrap_or(false);
  if !is_enabled {
    return Ok(None);
  }

  let result = match MatchResult::from_str(result_str) {
    Some(r) => r,
    None => {
      warn!("Invalid match result string for ELO processing: {}", result_str);
      return Ok(None);
    }
  };

  let config = DynamicEloConfig::default();

  // Build participant lists from session players
  let mut team_red = Vec::new();
  let mut team_blu = Vec::new();

  for sp in session_players {
    let team = match sp.team {
      Some(Team::Red) => &mut team_red,
      Some(Team::Blu) => &mut team_blu,
      _ => continue,
    };

    // Get current ELO data from database (or default for new players)
    let guild_elo = db.elo.get(sp.player.user_id, guild_id, db).await?;

    // Use dynamic_elo if set, otherwise initialize at anchor
    let elo = guild_elo.dynamic_elo.unwrap_or(config.anchor as u16);

    team.push(MatchParticipant { user_id: sp.player.user_id, elo, games: guild_elo.games, last_game_timestamp: guild_elo.last_game_timestamp });
  }

  if team_red.is_empty() || team_blu.is_empty() {
    warn!("Cannot process ELO: one or both teams are empty");
    return Ok(None);
  }

  // Calculate ELO changes
  let changes = config.calculate_match(&team_red, &team_blu, result);

  // Apply changes to database (writes dynamic_elo only, legacy elo untouched)
  for change in &changes {
    if let Err(e) = db.elo.record_dynamic_game(change.user_id, guild_id, change.won, change.new_elo, db).await {
      warn!("Failed to record dynamic ELO change for {}: {}", change.user_id, e);
    }

    // Update elo_after in match_players
    if let Err(e) = db.matches.update_player_elo_after(match_id, change.user_id, change.new_elo as i64).await {
      warn!("Failed to update elo_after for {}: {}", change.user_id, e);
    }
  }

  // Log summary
  let total_change: i32 = changes.iter().map(|c| c.change).sum();
  info!("Dynamic ELO applied for match {}: {} players, net drift: {}", match_id, changes.len(), total_change);

  for c in &changes {
    let user_tag = crate::log::get_user_tag(ctx, c.user_id, db).await;
    let participant = team_red.iter().chain(team_blu.iter()).find(|p| p.user_id == c.user_id);
    let games = participant.map(|p| p.games).unwrap_or(0);
    let last_game_timestamp = participant.and_then(|p| p.last_game_timestamp);
    info!("  {} {}→{} ({:+}) K={:.1} games={}", user_tag, c.old_elo, c.new_elo, c.change, config.k_factor(games, last_game_timestamp), games);
  }

  Ok(Some(changes))
}
