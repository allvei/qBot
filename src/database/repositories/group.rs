use anyhow::Result;
use async_trait::async_trait;
use serenity::all::ChannelId;
use sqlx::{Row, SqlitePool};
use tracing::{info, warn, error};

use crate::models::data::{Group, Dashboard, Channels, Teams};
use super::Repository;

#[derive(Clone)]
pub struct GroupRepository {
    pool: SqlitePool,
}

impl GroupRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_group(
        &self,
        guild_id: u64,
        dashboard: u64,
        chat: u64,
        queue: u64,
        dashboard_msg_id: u64,
        red: u64,
        blu: u64,
        session_quota: u8,
    ) -> Result<Group> {
        info!("Creating new group with queue: {}", queue);
        
        let result = sqlx::query(
            "INSERT INTO groups (guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, session_quota)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, group_id, timeout, dashboard, chat, queue, dashboard_msg_id, red, blu, session_quota"
        )
        .bind(guild_id as i64)
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(queue as i64)
        .bind(dashboard_msg_id as i64)
        .bind(red as i64)
        .bind(blu as i64)
        .bind(session_quota as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(self.build_group_from_row(&result).unwrap())
    }

    pub async fn get_by_channel(&self, channel_id: ChannelId) -> Result<Group> {
        info!("Looking for group with channel_id: {}", channel_id);
        
        let result = sqlx::query(
            "SELECT id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, session_increment, session_quota
             FROM groups
             WHERE dashboard = ? OR chat = ? OR queue = ? OR red = ? OR blu = ? OR dashboard_msg_id = ?"
        )
        .bind(channel_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(channel_id.get() as i64)
        .bind(channel_id.get() as i64)
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(row) => {
                let group = self.build_group_from_row(&row)?;
                self.validate_group_data(&group).await?;
                Ok(group)
            },
            Err(sqlx::Error::RowNotFound) => {
                error!("No group found for channel_id: {}. Available groups:", channel_id);
                self.log_available_groups().await;
                Err(anyhow::anyhow!(
                    "No group configuration found for channel {}. Please configure a group for this channel.", 
                    channel_id
                ))
            },
            Err(e) => Err(anyhow::anyhow!("Database error: {}", e))
        }
    }

    pub async fn update_group(
        &self,
        guild_id: u64,
        queue_id: u64,
        dashboard: u64,
        chat: u64,
        red: u64,
        blu: u64,
        session_quota: u8,
    ) -> Result<Group> {
        info!("Updating group with queue_id: {}", queue_id);
        
        let result = sqlx::query(
            "UPDATE groups
             SET guild_id = ?, dashboard = ?, chat = ?, red = ?, blu = ?, session_quota = ?
             WHERE queue = ?
             RETURNING id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, session_increment, session_quota"
        )
        .bind(guild_id as i64)
        .bind(dashboard as i64)
        .bind(chat as i64)
        .bind(red as i64)
        .bind(blu as i64)
        .bind(session_quota as i64)
        .bind(queue_id as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(self.build_group_from_row(&result).unwrap())
    }

    fn build_group_from_row(&self, result: &sqlx::sqlite::SqliteRow) -> Result<Group> {
        let dashboard_ch = result.get::<i64, _>("dashboard") as u64;
        let dashboard_msg_raw = result.try_get::<i64, _>("dashboard_msg_id")
            .unwrap_or(0) as u64;
        
        let chat = result.get::<i64, _>("chat") as u64;
        let queue = result.get::<i64, _>("queue") as u64;
        let red = result.get::<i64, _>("red") as u64;
        let blu = result.get::<i64, _>("blu") as u64;

        // Handle invalid MessageId (0 or NULL) by using a placeholder
        let dashboard_msg = if dashboard_msg_raw == 0 {
            serenity::all::MessageId::new(1) // Use 1 as minimum valid MessageId
        } else {
            serenity::all::MessageId::new(dashboard_msg_raw)
        };

        let group = Group {
            group_id: result.try_get::<i64, _>("group_id").unwrap_or(0) as u8,
            timeout: result.try_get::<i64, _>("timeout").unwrap_or(120) as u16,
            dashboard: Dashboard::new(ChannelId::new(dashboard_ch), dashboard_msg),
            channels: Channels::new(
                ChannelId::new(queue),
                ChannelId::new(queue), // Use queue for both text and voice for now
                vec![Teams::new(ChannelId::new(red), ChannelId::new(blu))]
            ),
            quota: result.get::<i64, _>("session_quota") as u8,
            sessions: Vec::new(),
        };

        Ok(group)
    }

    /// Validate group data integrity
    async fn validate_group_data(&self, group: &Group) -> Result<()> {
        // Check for placeholder values that indicate incomplete configuration
        if group.dashboard.ch.get() == 1 || group.dashboard.msg.get() == 1 {
            warn!("Group {} has placeholder dashboard configuration", group.group_id);
        }
        
        if group.channels.queue.get() == 1 || group.channels.queue_vc.get() == 1 {
            warn!("Group {} has placeholder channel configuration", group.group_id);
        }
        
        if group.channels.teams.iter().any(|t| t.red_vc.get() == 1 || t.blu_vc.get() == 1) {
            warn!("Group {} has placeholder team channel configuration", group.group_id);
        }
        
        Ok(())
    }

    /// Log available groups for debugging
    async fn log_available_groups(&self) {
        match sqlx::query("SELECT group_id, guild_id, chat, queue FROM groups")
            .fetch_all(&self.pool)
            .await 
        {
            Ok(rows) => {
                for row in rows {
                    let group_id: i64 = row.get("group_id");
                    let guild_id: i64 = row.get("guild_id");
                    let chat: i64 = row.get("chat");
                    let queue: i64 = row.get("queue");
                    info!("Available group: id={}, guild={}, chat={}, queue={}", 
                          group_id, guild_id, chat, queue);
                }
            },
            Err(e) => error!("Failed to fetch available groups: {}", e)
        }
    }

    /// Check if a group exists for a guild
    pub async fn group_exists_for_guild(&self, guild_id: u64) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM groups WHERE guild_id = ?"
        )
        .bind(guild_id as i64)
        .fetch_one(&self.pool)
        .await?;
        
        Ok(count > 0)
    }

    /// Get all groups for a guild
    pub async fn get_groups_for_guild(&self, guild_id: u64) -> Result<Vec<Group>> {
        let rows = sqlx::query(
            "SELECT id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, session_increment, session_quota
             FROM groups WHERE guild_id = ?"
        )
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut groups = Vec::new();
        for row in rows {
            groups.push(self.build_group_from_row(&row)?);
        }
        
        Ok(groups)
    }
}

#[async_trait]
impl Repository<Group, u8> for GroupRepository {
    async fn create(&self, group: &Group) -> Result<Group> {
        // Extract values from the group struct
        let dashboard_ch = group.dashboard.ch.get();
        let dashboard_msg = group.dashboard.msg.get();
        let chat = group.channels.queue.get();
        let queue = group.channels.queue_vc.get();
        let red = group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0);
        let blu = group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0);

        self.create_group(0, dashboard_ch, chat, queue, dashboard_msg, red, blu, group.quota).await
    }

    async fn get_by_id(&self, group_id: u8) -> Result<Group> {
        let result = sqlx::query(
            "SELECT id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, session_increment, session_quota
             FROM groups WHERE group_id = ?"
        )
        .bind(group_id as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row(&result)
    }

    async fn update(&self, group: &Group) -> Result<Group> {
        let dashboard_ch = group.dashboard.ch.get();
        let chat = group.channels.queue.get();
        let queue = group.channels.queue_vc.get();
        let red = group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0);
        let blu = group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0);

        self.update_group(0, queue, dashboard_ch, chat, red, blu, group.quota).await
    }

    async fn delete(&self, group_id: u8) -> Result<()> {
        sqlx::query("DELETE FROM groups WHERE group_id = ?")
            .bind(group_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
