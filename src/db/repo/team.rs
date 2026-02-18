use anyhow::Result;
use serenity::all::{ChannelId as CI, GuildId as GI};
use sqlx::{SqlitePool, Row};
use tracing::info;

/// Repository for managing team voice channels
#[derive(Clone)]
pub struct TeamRepository {
    pool: SqlitePool,
}

impl TeamRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Add a team channel pair to the database
    pub async fn add_team(&self, guild_id: GI, category_id: u8, red_vc: CI, blu_vc: CI) -> Result<()> {
        sqlx::query(
            "INSERT INTO teams (guild_id, category_id, red, blu) VALUES (?, ?, ?, ?)"
        )
        .bind(guild_id.get() as i64)
        .bind(category_id as i64)
        .bind(red_vc.get() as i64)
        .bind(blu_vc.get() as i64)
        .execute(&self.pool)
        .await?;

        info!("Added team channel pair to database: RED={} BLU={} for guild {} category {}", 
              red_vc.get(), blu_vc.get(), guild_id.get(), category_id);
        Ok(())
    }

    /// Get all team channels for a guild and category
    pub async fn get_teams_for_category(&self, guild_id: GI, category_id: u8) -> Result<Vec<(CI, CI)>> {
        let rows = sqlx::query(
            "SELECT red, blu FROM teams WHERE guild_id = ? AND category_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(category_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let teams = rows.iter().map(|row| {
            let red = CI::new(row.get::<i64, _>("red") as u64);
            let blu = CI::new(row.get::<i64, _>("blu") as u64);
            (red, blu)
        }).collect();

        Ok(teams)
    }

    /// Get all team channels for a guild
    pub async fn get_teams_for_guild(&self, guild_id: GI) -> Result<Vec<(u8, CI, CI)>> {
        let rows = sqlx::query(
            "SELECT category_id, red, blu FROM teams WHERE guild_id = ?"
        )
        .bind(guild_id.get() as i64)
        .fetch_all(&self.pool)
        .await?;

        let teams = rows.iter().map(|row| {
            let category_id = row.get::<i64, _>("category_id") as u8;
            let red = CI::new(row.get::<i64, _>("red") as u64);
            let blu = CI::new(row.get::<i64, _>("blu") as u64);
            (category_id, red, blu)
        }).collect();

        Ok(teams)
    }

    /// Remove a specific team channel pair
    pub async fn remove_team(&self, guild_id: GI, red_vc: CI, blu_vc: CI) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM teams WHERE guild_id = ? AND red = ? AND blu = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(red_vc.get() as i64)
        .bind(blu_vc.get() as i64)
        .execute(&self.pool)
        .await?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!("Removed team channel pair from database: RED={} BLU={} for guild {}", 
                  red_vc.get(), blu_vc.get(), guild_id.get());
        }
        Ok(deleted)
    }

    /// Remove all team channels for a specific category
    pub async fn remove_teams_for_category(&self, guild_id: GI, category_id: u8) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM teams WHERE guild_id = ? AND category_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(category_id as i64)
        .execute(&self.pool)
        .await?;

        let deleted_count = result.rows_affected();
        if deleted_count > 0 {
            info!("Removed {} team channel pairs from database for guild {} category {}", 
                  deleted_count, guild_id.get(), category_id);
        }
        Ok(deleted_count)
    }

    /// Remove all team channels for a guild
    pub async fn remove_teams_for_guild(&self, guild_id: GI) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM teams WHERE guild_id = ?"
        )
        .bind(guild_id.get() as i64)
        .execute(&self.pool)
        .await?;

        let deleted_count = result.rows_affected();
        if deleted_count > 0 {
            info!("Removed {} team channel pairs from database for guild {}", 
                  deleted_count, guild_id.get());
        }
        Ok(deleted_count)
    }

    /// Check if a channel is a tracked team channel
    pub async fn is_team_channel(&self, guild_id: GI, channel_id: CI) -> Result<bool> {
        let result = sqlx::query(
            "SELECT COUNT(*) as count FROM teams WHERE guild_id = ? AND (red = ? OR blu = ?)"
        )
        .bind(guild_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(channel_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(result.get::<i64, _>("count") > 0)
    }

    /// Get team channels that no longer exist in Discord (orphaned database entries)
    pub async fn get_orphaned_teams(&self, guild_id: GI, existing_channel_ids: &[CI]) -> Result<Vec<(CI, CI)>> {
        if existing_channel_ids.is_empty() {
            // If no channels exist, all teams are orphaned
            let rows = sqlx::query(
                "SELECT red, blu FROM teams WHERE guild_id = ?"
            )
            .bind(guild_id.get() as i64)
            .fetch_all(&self.pool)
            .await?;

            return Ok(rows.iter().map(|row| {
                let red = CI::new(row.get::<i64, _>("red") as u64);
                let blu = CI::new(row.get::<i64, _>("blu") as u64);
                (red, blu)
            }).collect());
        }

        // Create placeholders for the IN clause
        let placeholders: Vec<String> = existing_channel_ids.iter()
            .map(|_| "?".to_string())
            .collect();
        let in_clause = placeholders.join(",");

        let query = format!(
            "SELECT red, blu FROM teams WHERE guild_id = ? AND red NOT IN ({}) AND blu NOT IN ({})",
            in_clause, in_clause
        );

        let mut query_builder = sqlx::query(&query).bind(guild_id.get() as i64);
        
        for channel_id in existing_channel_ids {
            query_builder = query_builder.bind(channel_id.get() as i64);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        Ok(rows.iter().map(|row| {
            let red = CI::new(row.get::<i64, _>("red") as u64);
            let blu = CI::new(row.get::<i64, _>("blu") as u64);
            (red, blu)
        }).collect())
    }
}
