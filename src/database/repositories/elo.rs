use anyhow::Result;
use serenity::all::{UserId as UI, GuildId as GI};
use sqlx::{Row, SqlitePool};

use crate::Rank;

/// Guild-specific ELO data for a player
#[derive(Debug, Clone)]
pub struct GuildElo {
    pub elo:      u16,
    pub division: Rank,
    pub games:    u32,
    pub wins:     u32,
}

impl Default for GuildElo {
    fn default() -> Self {
        Self {
            elo:      50,
            division: Rank::Apprentice,
            games:    0,
            wins:     0,
        }
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

    /// Get a player's ELO for a specific guild (returns None if no record exists)
    pub async fn get_if_exists(&self, user_id: UI, guild_id: GI) -> Result<Option<GuildElo>> {
        let result = sqlx::query(
            "SELECT elo, division, games, wins FROM elos WHERE guild_id = ? AND user_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some(row) => {
                let elo:          i64    = row.get("elo");
                let division_str: String = row.get("division");
                let games:        i64    = row.get("games");
                let wins:         i64    = row.get("wins");

                Ok(Some(GuildElo {
                    elo:      elo   as u16,
                    division: Self::parse_division(&division_str),
                    games:    games as u32,
                    wins:     wins  as u32,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get a player's ELO for a specific guild (returns default if no record)
    pub async fn get(&self, user_id: UI, guild_id: GI) -> Result<GuildElo> {
        Ok(self.get_if_exists(user_id, guild_id).await?.unwrap_or_default())
    }

    /// Set a player's ELO for a specific guild (creates if not exists)
    pub async fn set(&self, user_id: UI, guild_id: GI, elo: u16, division: Rank) -> Result<()> {
        sqlx::query(
            "INSERT INTO elos (guild_id, user_id, elo, division)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET elo = excluded.elo, division = excluded.division"
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get() as i64)
        .bind(elo as i64)
        .bind(division.name())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update only the ELO value (division will be recalculated)
    pub async fn update_elo(&self, user_id: UI, guild_id: GI, elo: u16) -> Result<()> {
        let division = Rank::from_elo_default(elo);
        self.set(user_id, guild_id, elo, division).await
    }

    /// Record a game result and update ELO
    pub async fn record_game(&self, user_id: UI, guild_id: GI, won: bool, elo_change: i16) -> Result<GuildElo> {
        // Get current ELO or create default
        let current = self.get(user_id, guild_id).await?;
        
        // Calculate new ELO (clamped to 0-100)
        let new_elo      = (current.elo as i32 + elo_change as i32).clamp(0, 100) as u16;
        let new_division = Rank::from_elo_default(new_elo);
        let new_games    = current.games + 1;
        let new_wins     = if won { current.wins + 1 } else { current.wins };

        sqlx::query(
            "INSERT INTO elos (guild_id, user_id, elo, division, games, wins)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(guild_id, user_id) DO UPDATE SET 
                elo      = excluded.elo, 
                division = excluded.division,
                games    = excluded.games,
                wins     = excluded.wins"
        )
        .bind(guild_id.get() as i64)
        .bind(user_id.get()  as i64)
        .bind(new_elo        as i64)
        .bind(new_division.name())
        .bind(new_games      as i64)
        .bind(new_wins       as i64)
        .execute(&self.pool)
        .await?;

        Ok(GuildElo {
            elo:      new_elo,
            division: new_division,
            games:    new_games,
            wins:     new_wins,
        })
    }

    /// Get all ELO records for a user across all guilds
    pub async fn get_all_for_user(&self, user_id: UI) -> Result<Vec<(u64, GuildElo)>> {
        let rows = sqlx::query(
            "SELECT g.guild_id, e.elo, e.division, e.games, e.wins 
             FROM   elos e 
             JOIN   guilds g ON e.guild_id = g.id 
             WHERE  e.user_id = ?"
        )
        .bind(user_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let guild_id:     i64    = row.get("guild_id");
            let elo:          i64    = row.get("elo");
            let division_str: String = row.get("division");
            let games:        i64    = row.get("games");
            let wins:         i64    = row.get("wins");

            results.push((
                guild_id as u64,
                GuildElo {
                    elo:      elo as u16,
                    division: Self::parse_division(&division_str),
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
            "SELECT u.user_id, e.elo, e.division, e.games, e.wins 
             FROM elos e 
             JOIN users u ON e.user_id = u.id 
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
            let division_str: String = row.get("division");
            let games:        i64    = row.get("games");
            let wins:         i64    = row.get("wins");

            results.push((
                UI::new(user_id as u64),
                GuildElo {
                    elo:      elo as u16,
                    division: Self::parse_division(&division_str),
                    games:    games as u32,
                    wins:     wins as u32,
                },
            ));
        }

        Ok(results)
    }

    /// Parse division string to Rank enum
    fn parse_division(s: &str) -> Rank {
        match s {
            "Beginner"     => Rank::Beginner,
            "Newcomer"     => Rank::Newcomer,
            "Novice"       => Rank::Novice,
            "Apprentice"   => Rank::Apprentice,
            "Journeyman"   => Rank::Journeyman,
            "Expert"       => Rank::Expert,
            "Master"       => Rank::Master,
            "Master Elite" => Rank::MasterElite,
            "Grandmaster"  => Rank::Grandmaster,
            _              => Rank::Apprentice,
        }
    }
}
