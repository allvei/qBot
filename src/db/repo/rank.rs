use anyhow::{Error, Result, anyhow};
use sqlx::{Row, SqlitePool};
use tracing::info;
use serenity::all::{RoleId, GuildId as GI};

/// A configurable rank for a guild (sorted by ELO)
#[derive(Debug, Clone)]
pub struct GuildRank {
    pub guild_id: GI,
    pub name:     String,
    pub elo:      u16,
    pub role_id:  RoleId,
}

impl GuildRank {
    pub fn new(guild_id: GI, name: String, elo: u16, role_id: RoleId) -> Self {
        Self { guild_id, name, elo, role_id }
    }
}

/// Default ranks to seed new guilds with (name, elo)
pub fn default_ranks() -> Vec<(String, u16)> {
    vec![
        ("Beginner"    .to_string(), 10),
        ("Newcomer"    .to_string(), 30),
        ("Novice"      .to_string(), 40),
        ("Apprentice"  .to_string(), 50),
        ("Journeyman"  .to_string(), 65),
        ("Expert"      .to_string(), 75),
        ("Master"      .to_string(), 85),
        ("Master Elite".to_string(), 90),
        ("Grandmaster" .to_string(), 95),
    ]
}

#[derive(Clone)]
pub struct RankRepository {
    pool: SqlitePool,
}

impl RankRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get all ranks for a guild, ordered by ELO ascending
    pub async fn get_ranks(&self, guild_id: GI) -> Result<Vec<GuildRank>> {
        let rows = sqlx::query(
            "SELECT name, elo, role_id FROM ranks WHERE guild_id = ? ORDER BY elo ASC"
        )
        .bind(guild_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut ranks = Vec::new();
        for row in rows {
            let name:    String = row.try_get("name")?;
            let elo:     i64    = row.try_get("elo")?;
            let role_id: i64    = row.try_get("role_id")?;
            
            // Skip ranks with invalid role_id (0 means NULL in database)
            if role_id == 0 {
                continue;
            }
            
            let role_id = RoleId::new(role_id as u64);

            ranks.push(GuildRank {
                guild_id,
                name,
                elo: elo as u16,
                role_id,
            });
        }

        Ok(ranks)
    }

    /// Get ranks for a guild
    pub async fn get_or_init_ranks(&self, guild_id: GI) -> Result<Vec<GuildRank>> {
        self.get_ranks(guild_id).await
    }

    /// Initialize default ranks for a guild
    pub async fn init_default_ranks(&self, guild_id: GI) -> Result<()> {
        let defaults = default_ranks();
        for (name, elo) in defaults {
            sqlx::query(
                "INSERT OR IGNORE INTO ranks (guild_id, name, elo) VALUES (?, ?, ?)"
            )
            .bind(guild_id.get() as i64)
            .bind(&name)
            .bind(elo as i64)
            .execute(&self.pool)
            .await?;
        }
        info!("Initialized default ranks for guild {}", guild_id);
        Ok(())
    }

    /// Get a rank by name
    pub async fn get_rank_by_name(&self, guild_id: GI, name: &str) -> Result<Option<GuildRank>> {
        let row = sqlx::query(
            "SELECT name, elo, role_id FROM ranks WHERE guild_id = ? AND name = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let name:    String = row.try_get("name")?;
                let elo:     i64    = row.try_get("elo")?;
                let role_id: i64    = row.try_get("role_id")?;
                
                // Skip ranks with invalid role_id (0 means NULL in database)
                if role_id == 0 {
                    return Ok(None);
                }
                
                let role_id = RoleId::new(role_id as u64);

                Ok(Some(GuildRank {
                    guild_id,
                    name,
                    elo: elo as u16,
                    role_id,
                }))
            }
            None => Ok(None),
        }
    }

    /// Update a rank's name
    pub async fn update_rank_name(&self, guild_id: GI, old_name: &str, new_name: &str) -> Result<()> {
        sqlx::query("UPDATE ranks SET name = ? WHERE guild_id = ? AND name = ?")
            .bind(new_name)
            .bind(guild_id.get() as i64)
            .bind(old_name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update a rank's ELO threshold
    pub async fn update_rank_elo(&self, guild_id: GI, name: &str, elo: u16) -> Result<()> {
        sqlx::query("UPDATE ranks SET elo = ? WHERE guild_id = ? AND name = ?")
            .bind(elo as i64)
            .bind(guild_id.get() as i64)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Add a new rank
    pub async fn add_rank(&self, guild_id: GI, name: &str, elo: u16, role_id: RoleId) -> Result<()> {
        sqlx::query("INSERT INTO ranks (guild_id, name, elo, role_id) VALUES (?, ?, ?, ?)")
            .bind(guild_id.get() as i64)
            .bind(name)
            .bind(elo as i64)
            .bind(role_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update a rank's linked Discord role
    pub async fn update_rank_role(&self, guild_id: GI, name: &str, role_id: RoleId) -> Result<()> {
        sqlx::query("UPDATE ranks SET role_id = ? WHERE guild_id = ? AND name = ?")
            .bind(role_id.get() as i64)
            .bind(guild_id.get() as i64)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a rank by name
    pub async fn delete_rank(&self, guild_id: GI, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM ranks WHERE guild_id = ? AND name = ?")
            .bind(guild_id.get() as i64)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Find rank from ELO value (returns highest rank where elo >= rank.elo)
    pub async fn rank_from_elo(&self, guild_id: GI, elo: u16) -> Result<Option<GuildRank>> {
        let ranks = self.get_or_init_ranks(guild_id).await?;
        
        // Find the highest rank where elo >= rank.elo (ranks are sorted by ELO ascending)
        let mut best_rank: Option<&GuildRank> = None;
        for rank in &ranks {
            if elo >= rank.elo {
                best_rank = Some(rank);
            } else {
                break;
            }
        }

        Ok(best_rank.cloned())
    }

    /// Find rank name from ELO value
    pub async fn rank_name_from_elo(&self, guild_id: GI, elo: u16) -> Result<String> {
        match self.rank_from_elo(guild_id, elo).await? {
            Some(rank) => Ok(rank.name),
            None => Ok("Beginner".to_string()),
        }
    }

    /// Get rank struct from RoleId
    pub async fn rank_from_role_id(&self, guild_id: GI, role_id: RoleId) -> Result<GuildRank, Error> {
        let ranks = self.get_or_init_ranks(guild_id).await?;
        
        // Find the rank with the matching role_id
        for rank in &ranks {
            if rank.role_id == role_id {
                return Ok(rank.clone());
            }
        }

        Err(anyhow!("Rank not found for role ID {}", role_id))
    }
}
