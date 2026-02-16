use anyhow::Result;
use serenity::all::{UserId as UI, GuildId as GI};
use sqlx::{Row, SqlitePool};

use crate::Rank;

/// Guild-specific ELO data for a player
#[derive(Debug, Clone)]
pub struct GuildElo {
    pub elo:      u16,
    pub rank:     Rank,
    pub games:    u32,
    pub wins:     u32,
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
            "SELECT e.elo, e.games, e.wins, r.name, r.elo as rank_elo, r.role_id
             FROM elo e
             LEFT JOIN ranks r ON e.rank = r.id
             WHERE e.guild_id = ? AND e.user_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some(row) => {
                let elo:     i64    = row.get("elo");
                let games:   i64    = row.get("games");
                let wins:    i64    = row.get("wins");
                let name:    Option<String> = row.get("name");
                let rank_elo: Option<i64> = row.get("rank_elo");
                let role_id: Option<i64> = row.get("role_id");

                let rank = if let (Some(name), Some(rank_elo), Some(role_id)) = (name, rank_elo, role_id) {
                    Rank {
                        guild_id,
                        role_id: serenity::all::RoleId::new(role_id as u64),
                        name,
                        elo: rank_elo as u16,
                    }
                } else {
                    // Return None if join failed - no valid rank data available
                    return Ok(None);
                };

                Ok(Some(GuildElo {
                    elo:   elo   as u16,
                    rank:  rank,
                    games: games as u32,
                    wins:  wins  as u32,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get a player's ELO for a specific guild (returns Discord role-based rank if no record)
    pub async fn get(&self, user_id: UI, guild_id: GI, db: &crate::Database) -> Result<GuildElo> {
        match self.get_if_exists(user_id, guild_id).await? {
            Some(elo) => Ok(elo),
            None => {
                // Player has no ELO record - try to determine rank from Discord roles
                // Note: This requires a Context, which we don't have here.
                // We'll need to modify this to accept a Context or use a different approach.
                
                // For now, fall back to guild default rank
                // TODO: This should be updated to check Discord roles when Context is available
                let default_rank = Rank::get_guild_default(db, guild_id).await
                    .map_err(|_| anyhow::anyhow!("Failed to get default rank for guild {}", guild_id))?;
                Ok(GuildElo {
                    elo: default_rank.elo,
                    rank: default_rank,
                    games: 0,
                    wins: 0,
                })
            }
        }
    }

    /// Resolve a Rank to its database primary key (ranks.id)
    async fn resolve_rank_id(&self, guild_id: GI, rank: &Rank) -> Result<i64> {
        sqlx::query_scalar(
            "SELECT id FROM ranks WHERE guild_id = ? AND name = ? AND role_id = ?"
        )
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
             ON CONFLICT(guild_id, user_id) DO UPDATE SET elo = excluded.elo, rank = excluded.rank"
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .bind(elo as i64)
        .bind(rank_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batch set ELO and rank for multiple users in a single transaction
    pub async fn batch_set(&self, guild_id: GI, elo: u16, rank: &Rank, user_ids: &[UI]) -> Result<u32> {
        if user_ids.is_empty() { return Ok(0); }

        let rank_id = self.resolve_rank_id(guild_id, rank).await?;

        let mut tx = self.pool.begin().await?;
        let mut success = 0u32;

        for chunk in user_ids.chunks(500) {
            let mut query = String::from(
                "INSERT INTO elo (guild_id, user_id, elo, rank) VALUES "
            );
            let placeholders: Vec<String> = chunk.iter()
                .map(|_| "(?, ?, ?, ?)".to_string())
                .collect();
            query.push_str(&placeholders.join(", "));
            query.push_str(" ON CONFLICT(guild_id, user_id) DO UPDATE SET elo = excluded.elo, rank = excluded.rank");

            let mut q = sqlx::query(&query);
            for uid in chunk {
                q = q.bind(guild_id.get() as i64)
                     .bind(uid.get() as i64)
                     .bind(elo as i64)
                     .bind(rank_id);
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
        
        // Calculate new ELO
        let new_elo   = (current.elo as i32 + elo_change as i32) as u16;
        let new_rank  = Rank::from_elo(db, guild_id, new_elo).await?;
        let new_games = current.games + 1;
        let new_wins  = if won { current.wins + 1 } else { current.wins };

        sqlx::query(
            "INSERT INTO elo (guild_id, user_id, elo, rank, games, wins)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET 
                elo      = excluded.elo, 
                rank = excluded.rank,
                games    = excluded.games,
                wins     = excluded.wins"
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get()  as i64)
        .bind(new_elo        as i64)
        .bind(self.resolve_rank_id(guild_id, &new_rank).await?)
        .bind(new_games      as i64)
        .bind(new_wins       as i64)
        .execute(&self.pool)
        .await?;

        Ok(GuildElo {
            elo:      new_elo,
            rank:     new_rank,
            games:    new_games,
            wins:     new_wins,
        })
    }

    /// Get all ELO records for a user across all guilds
    pub async fn get_all_for_user(&self, user_id: UI) -> Result<Vec<(u64, GuildElo)>> {
        let rows = sqlx::query(
            "SELECT e.guild_id, e.elo, e.games, e.wins, r.name, r.elo as rank_elo, r.role_id
             FROM elo e
             LEFT JOIN ranks r ON e.rank = r.id
             WHERE e.user_id = ?"
        )
        .bind(user_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let guild_id:     i64    = row.get("guild_id");
            let elo:          i64    = row.get("elo");
            let games:        i64    = row.get("games");
            let wins:         i64    = row.get("wins");
            let name:         Option<String> = row.get("name");
            let rank_elo:     Option<i64> = row.get("rank_elo");
            let role_id:      Option<i64> = row.get("role_id");

            let rank = if let (Some(name), Some(rank_elo), Some(role_id)) = (name, rank_elo, role_id) {
                Rank {
                    guild_id: GI::new(guild_id as u64),
                    role_id: serenity::all::RoleId::new(role_id as u64),
                    name,
                    elo: rank_elo as u16,
                }
            } else {
                // Skip records with invalid rank data
                continue;
            };

            results.push((
                guild_id as u64,
                GuildElo {
                    elo:      elo as u16,
                    rank,
                    games:    games as u32,
                    wins:     wins as u32,
                },
            ));
        }

        Ok(results)
    }

    /// Get leaderboard for a guild (top N players by ELO)
    pub async fn get_leaderboard(&self, guild_id: GI, limit: u32) -> Result<Vec<(UI, GuildElo)>> {
        let rows = sqlx::query(
            "SELECT e.user_id, e.elo, e.games, e.wins, r.name, r.elo as rank_elo, r.role_id
             FROM elo e
             LEFT JOIN ranks r ON e.rank = r.id
             WHERE e.guild_id = ? 
             ORDER BY e.elo DESC 
             LIMIT ?"
        )
        .bind(guild_id.get() as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let user_id:      i64    = row.get("user_id");
            let elo:          i64    = row.get("elo");
            let games:        i64    = row.get("games");
            let wins:         i64    = row.get("wins");
            let name:         Option<String> = row.get("name");
            let rank_elo:     Option<i64> = row.get("rank_elo");
            let role_id:      Option<i64> = row.get("role_id");

            let rank = if let (Some(name), Some(rank_elo), Some(role_id)) = (name, rank_elo, role_id) {
                Rank {
                    guild_id,
                    role_id: serenity::all::RoleId::new(role_id as u64),
                    name,
                    elo: rank_elo as u16,
                }
            } else {
                // Skip records with invalid rank data
                continue;
            };

            results.push((
                UI::new(user_id as u64),
                GuildElo {
                    elo:      elo as u16,
                    rank,
                    games:    games as u32,
                    wins:     wins as u32,
                },
            ));
        }

        Ok(results)
    }

    /// Validate and normalize player ELO based on their Discord rank
    /// 
    /// Discord roles define ELO ranges. Each rank has a base ELO (e.g., rank1=50, rank2=65).
    /// A player's ELO is valid if it's within [rank_base, next_rank_base).
    /// If ELO is unset or outside this range, it gets normalized to the rank's base ELO.
    /// 
    /// # Arguments
    /// * `user_id` - The player's user ID
    /// * `guild_id` - The guild ID
    /// * `discord_rank` - The player's current Discord rank (source of truth)
    /// * `db` - Database reference for querying ranks
    /// 
    /// # Returns
    /// The validated/normalized ELO and whether it was changed
    pub async fn validate_and_normalize_elo(
        &self,
        user_id: UI,
        guild_id: GI,
        discord_rank: &crate::Rank,
        db: &crate::Database,
    ) -> Result<(u16, bool)> {
        // Get all ranks to determine the valid ELO range
        let ranks = db.ranks.get_ranks(guild_id).await?;
        
        // Find the next rank's base ELO to determine upper bound
        let rank_min_elo = discord_rank.elo;
        let rank_max_elo = ranks.iter()
            .find(|r| r.elo > discord_rank.elo)
            .map(|r| r.elo)
            .unwrap_or(u16::MAX); // No upper bound if this is the highest rank
        
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
}
