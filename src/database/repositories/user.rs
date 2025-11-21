use anyhow::Result;
use async_trait::async_trait;
use serenity::all::{Context, UserId};
use sqlx::{Row, SqlitePool};
use tracing::info;

use crate::models::Player;
use super::Repository;

#[derive(Clone)]
pub struct UserRepository {
    pool: SqlitePool,
}

impl UserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_by_discord_id(&self, discord_id: UserId) -> Result<Player> {
        let result = sqlx::query(
            "SELECT id, discord_id, steam_id FROM users WHERE discord_id = ?"
        )
        .bind(discord_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn get_by_discord_id_with_tag(&self, discord_id: UserId, ctx: &Context) -> Result<Player> {
        let result = sqlx::query(
            "SELECT id, discord_id, steam_id FROM users WHERE discord_id = ?"
        )
        .bind(discord_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        let mut player = Self::get_player(result);
        
        // Fetch discord tag from Discord API
        if let Ok(user) = ctx.http.get_user(discord_id).await {
            player.discord_tag = Some(user.tag());
        }
        
        Ok(player)
    }

    pub async fn create_or_update(&self, discord_id: UserId, steam_id: Option<u64>) -> Result<Player> {
        // info!("Creating or updating user with discord_id: {}", discord_id);

        let result = sqlx::query(
            "INSERT INTO users (discord_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(discord_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING id, discord_id, steam_id"
        )
        .bind(discord_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::get_player(result))
    }

    pub async fn create_or_update_with_tag(&self, discord_id: UserId, steam_id: Option<u64>, ctx: &Context) -> Result<Player> {
        // info!("Creating or updating user with discord_id: {}", discord_id);

        let result = sqlx::query(
            "INSERT INTO users (discord_id, steam_id)
             VALUES (?, ?)
             ON CONFLICT(discord_id) DO UPDATE SET steam_id=excluded.steam_id
             RETURNING id, discord_id, steam_id"
        )
        .bind(discord_id.get() as i64)
        .bind(steam_id.map(|id| id as i64).unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;

        let mut player = Self::get_player(result);
        
        // Fetch discord tag from Discord API
        if let Ok(user) = ctx.http.get_user(discord_id).await {
            player.discord_tag = Some(user.tag());
        }
        
        Ok(player)
    }

    fn get_player(result: sqlx::sqlite::SqliteRow) -> Player {
        Player::add(result.get::<u64, _>        ("discord_id").into(),
                    None,
                    result.get::<Option<i64>, _>("steam_id")  .map(|id| id as u64)
                   )
    }

    pub async fn update_steam_id(&self, discord_id: UserId, steam_id: Option<u64>) -> Result<Player> {
        info!("Updating user steam_id for discord_id: {}", discord_id);

        sqlx::query("UPDATE users SET steam_id = ? WHERE discord_id = ?")
            .bind(steam_id.map(|id| id as i64))
            .bind(discord_id.get() as i64)
            .execute(&self.pool)
            .await?;

        Ok(Player::add(discord_id, None, steam_id))
    }
}

#[async_trait]
impl Repository<Player, UserId> for UserRepository {
    async fn create(&self, player: &Player) -> Result<Player> {
        self.create_or_update(player.discord_id, player.steam_id).await
    }

    async fn get_by_id(&self, discord_id: UserId) -> Result<Player> {
        self.get_by_discord_id(discord_id).await
    }

    async fn update(&self, player: &Player) -> Result<Player> {
        self.update_steam_id(player.discord_id, player.steam_id).await
    }

    async fn delete(&self, discord_id: UserId) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE discord_id = ?")
            .bind(discord_id.get() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
