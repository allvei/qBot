use anyhow::Result;
use serenity::all::RoleId;
use sqlx::{Row, SqlitePool};
use tracing::info;

/// A configurable rank for a guild
#[derive(Debug, Clone)]
pub struct GuildRank {
    pub guild_id: u64,
    pub position: u8,
    pub name:     String,
    pub elo:      u16,
    pub role_ids: Vec<RoleId>,
}

impl GuildRank {
    pub fn new(guild_id: u64, position: u8, name: String, elo: u16) -> Self {
        Self {
            guild_id,
            position,
            name,
            elo,
            role_ids: Vec::new(),
        }
    }
}

/// Default ranks to seed new guilds with
pub fn default_ranks() -> Vec<(String, u16)> {
    vec![
        ("Beginner".to_string(),     10),
        ("Newcomer".to_string(),     30),
        ("Novice".to_string(),       40),
        ("Apprentice".to_string(),   50),
        ("Journeyman".to_string(),   65),
        ("Expert".to_string(),       75),
        ("Master".to_string(),       85),
        ("Master Elite".to_string(), 90),
        ("Grandmaster".to_string(),  95),
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

    /// Get all ranks for a guild, ordered by position
    pub async fn get_ranks(&self, guild_id: u64) -> Result<Vec<GuildRank>> {
        let rows = sqlx::query(
            "SELECT position, name, elo, role_ids FROM ranks WHERE guild_id = ? ORDER BY position ASC"
        )
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut ranks = Vec::new();
        for row in rows {
            let position: i64 = row.try_get("position")?;
            let name: String = row.try_get("name")?;
            let elo: i64 = row.try_get("elo")?;
            let role_ids_str: Option<String> = row.try_get("role_ids").ok();

            let role_ids = role_ids_str
                .map(|s| {
                    s.split(',')
                        .filter_map(|id| id.trim().parse::<u64>().ok())
                        .map(RoleId::new)
                        .collect()
                })
                .unwrap_or_default();

            ranks.push(GuildRank {
                guild_id,
                position: position as u8,
                name,
                elo: elo as u16,
                role_ids,
            });
        }

        Ok(ranks)
    }

    /// Get ranks for a guild, initializing with defaults if none exist
    pub async fn get_or_init_ranks(&self, guild_id: u64) -> Result<Vec<GuildRank>> {
        let ranks = self.get_ranks(guild_id).await?;
        if ranks.is_empty() {
            self.init_default_ranks(guild_id).await?;
            self.get_ranks(guild_id).await
        } else {
            Ok(ranks)
        }
    }

    /// Initialize default ranks for a guild
    pub async fn init_default_ranks(&self, guild_id: u64) -> Result<()> {
        let defaults = default_ranks();
        for (position, (name, elo)) in defaults.into_iter().enumerate() {
            sqlx::query(
                "INSERT OR IGNORE INTO ranks (guild_id, position, name, elo) VALUES (?, ?, ?, ?)"
            )
            .bind(guild_id as i64)
            .bind(position as i64)
            .bind(&name)
            .bind(elo as i64)
            .execute(&self.pool)
            .await?;
        }
        info!("Initialized default ranks for guild {}", guild_id);
        Ok(())
    }

    /// Get a rank by position
    pub async fn get_rank(&self, guild_id: u64, position: u8) -> Result<Option<GuildRank>> {
        let row = sqlx::query(
            "SELECT name, elo, role_ids FROM ranks WHERE guild_id = ? AND position = ?"
        )
        .bind(guild_id as i64)
        .bind(position as i64)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let name: String = row.try_get("name")?;
                let elo: i64 = row.try_get("elo")?;
                let role_ids_str: Option<String> = row.try_get("role_ids").ok();

                let role_ids = role_ids_str
                    .map(|s| {
                        s.split(',')
                            .filter_map(|id| id.trim().parse::<u64>().ok())
                            .map(RoleId::new)
                            .collect()
                    })
                    .unwrap_or_default();

                Ok(Some(GuildRank {
                    guild_id,
                    position,
                    name,
                    elo: elo as u16,
                    role_ids,
                }))
            }
            None => Ok(None),
        }
    }

    /// Update a rank's name
    pub async fn update_rank_name(&self, guild_id: u64, position: u8, name: &str) -> Result<()> {
        sqlx::query("UPDATE ranks SET name = ? WHERE guild_id = ? AND position = ?")
            .bind(name)
            .bind(guild_id as i64)
            .bind(position as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update a rank's ELO threshold
    pub async fn update_rank_elo(&self, guild_id: u64, position: u8, elo: u16) -> Result<()> {
        sqlx::query("UPDATE ranks SET elo = ? WHERE guild_id = ? AND position = ?")
            .bind(elo as i64)
            .bind(guild_id as i64)
            .bind(position as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update a rank's role IDs
    pub async fn update_rank_role_ids(&self, guild_id: u64, position: u8, role_ids: &[RoleId]) -> Result<()> {
        let role_ids_str = role_ids
            .iter()
            .map(|id| id.get().to_string())
            .collect::<Vec<_>>()
            .join(",");

        sqlx::query("UPDATE ranks SET role_ids = ? WHERE guild_id = ? AND position = ?")
            .bind(&role_ids_str)
            .bind(guild_id as i64)
            .bind(position as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Add a role ID to a rank
    pub async fn add_role_id(&self, guild_id: u64, position: u8, role_id: RoleId) -> Result<()> {
        if let Some(mut rank) = self.get_rank(guild_id, position).await? {
            if !rank.role_ids.contains(&role_id) {
                rank.role_ids.push(role_id);
                self.update_rank_role_ids(guild_id, position, &rank.role_ids).await?;
            }
        }
        Ok(())
    }

    /// Remove a role ID from a rank
    pub async fn remove_role_id(&self, guild_id: u64, position: u8, role_id: RoleId) -> Result<()> {
        if let Some(mut rank) = self.get_rank(guild_id, position).await? {
            rank.role_ids.retain(|id| *id != role_id);
            self.update_rank_role_ids(guild_id, position, &rank.role_ids).await?;
        }
        Ok(())
    }

    /// Add a new rank at a position (shifts existing ranks up)
    pub async fn add_rank(&self, guild_id: u64, position: u8, name: &str, elo: u16) -> Result<()> {
        // Get all positions that need to shift (in descending order)
        let positions: Vec<i64> = sqlx::query_scalar(
            "SELECT position FROM ranks WHERE guild_id = ? AND position >= ? ORDER BY position DESC"
        )
            .bind(guild_id as i64)
            .bind(position as i64)
            .fetch_all(&self.pool)
            .await?;

        // Shift each position up by 1, starting from highest to avoid UNIQUE conflicts
        for pos in positions {
            sqlx::query("UPDATE ranks SET position = ? WHERE guild_id = ? AND position = ?")
                .bind(pos + 1)
                .bind(guild_id as i64)
                .bind(pos)
                .execute(&self.pool)
                .await?;
        }

        // Insert new rank
        sqlx::query("INSERT INTO ranks (guild_id, position, name, elo) VALUES (?, ?, ?, ?)")
            .bind(guild_id as i64)
            .bind(position as i64)
            .bind(name)
            .bind(elo as i64)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Delete a rank at a position (shifts existing ranks down)
    pub async fn delete_rank(&self, guild_id: u64, position: u8) -> Result<()> {
        // Delete the rank
        sqlx::query("DELETE FROM ranks WHERE guild_id = ? AND position = ?")
            .bind(guild_id as i64)
            .bind(position as i64)
            .execute(&self.pool)
            .await?;

        // Shift ranks above this position down by 1
        sqlx::query("UPDATE ranks SET position = position - 1 WHERE guild_id = ? AND position > ?")
            .bind(guild_id as i64)
            .bind(position as i64)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Find rank from ELO value
    pub async fn rank_from_elo(&self, guild_id: u64, elo: u16) -> Result<Option<GuildRank>> {
        let ranks = self.get_or_init_ranks(guild_id).await?;
        
        // Find the highest rank where elo >= rank.elo
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

    /// Find rank from role ID
    pub async fn rank_from_role_id(&self, guild_id: u64, role_id: RoleId) -> Result<Option<GuildRank>> {
        let ranks = self.get_or_init_ranks(guild_id).await?;
        
        for rank in ranks {
            if rank.role_ids.contains(&role_id) {
                return Ok(Some(rank));
            }
        }

        Ok(None)
    }

    /// Get all role IDs for all ranks in a guild
    pub async fn all_role_ids(&self, guild_id: u64) -> Result<Vec<RoleId>> {
        let ranks = self.get_or_init_ranks(guild_id).await?;
        Ok(ranks.into_iter().flat_map(|r| r.role_ids).collect())
    }

    /// Load rank mappings for efficient cached lookups
    pub async fn load_rank_mappings(&self, guild_id: u64) -> Result<Vec<(GuildRank, Vec<RoleId>)>> {
        let ranks = self.get_or_init_ranks(guild_id).await?;
        Ok(ranks.into_iter().map(|r| {
            let role_ids = r.role_ids.clone();
            (r, role_ids)
        }).collect())
    }
}
