use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serenity::all::{ChannelId as CI, MessageId as MI};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use super::Repository;
use crate::models::{Channels, Group, TeamChannel};

/// Configuration for creating or updating a group
pub struct GroupConfig {
    pub dashboard_channel_id: u64,
    pub chat_channel_id:      u64,
    pub queue_vc_id:          u64,
    pub red_vc_id:            u64,
    pub blu_vc_id:            u64,
    pub quota:                u8,
}

#[derive(Clone)]
pub struct GroupRepository {
    pool: SqlitePool,
}

impl GroupRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_group(&self, guild_id: u64, dashboard_msg: u64, config: GroupConfig) -> Result<Group> {
        info!("Creating new group with queue: {}", config.queue_vc_id);

        let result = sqlx::query(
            "INSERT INTO groups (guild_id, dashboard, chat, queue, dashboard_msg, red, blu, quota)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, group_id, name, timeout, guild_id, dashboard, chat, queue, dashboard_msg, red, blu, quota, connect_info"
        )
        .bind(guild_id                    as i64)
        .bind(config.dashboard_channel_id as i64)
        .bind(config.chat_channel_id      as i64)
        .bind(config.queue_vc_id          as i64)
        .bind(dashboard_msg               as i64)
        .bind(config.red_vc_id            as i64)
        .bind(config.blu_vc_id            as i64)
        .bind(config.quota                as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    pub async fn update_group(&self, guild_id: u64, config: GroupConfig) -> Result<Group> {
        info!("Updating group with queue_id: {}", config.queue_vc_id);

        let result = sqlx::query("UPDATE groups
                                  SET guild_id = ?, dashboard = ?, chat = ?, red = ?, blu = ?, quota = ?
                                  WHERE queue = ?
                                  RETURNING id, group_id, name, timeout, guild_id, dashboard, chat, queue, dashboard_msg, red, blu, game_increment, quota, connect_info"
        )
        .bind(guild_id                    as i64)
        .bind(config.dashboard_channel_id as i64)
        .bind(config.chat_channel_id      as i64)
        .bind(config.red_vc_id            as i64)
        .bind(config.blu_vc_id            as i64)
        .bind(config.quota                as i64)
        .bind(config.queue_vc_id          as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    async fn build_group_from_row_async(&self, result: &sqlx::sqlite::SqliteRow) -> Result<Group> {
        // Validate channel IDs before creating ChannelId objects
        let chat_id          = result.get::<i64, _>("chat")  as u64;
        let queue_id         = result.get::<i64, _>("queue") as u64;
        let red_id           = result.get::<i64, _>("red")   as u64;
        let blu_id           = result.get::<i64, _>("blu")   as u64;
        let dashboard_id     = result.get::<i64, _>("dashboard") as u64;
        let dashboard_msg_id = result.get::<i64, _>("dashboard_msg") as u64;

        let invalid_ids = [
            (chat_id          == 0, "chat"),
            (queue_id         == 0, "queue"),
            (red_id           == 0, "red"),
            (blu_id           == 0, "blu"),
            (dashboard_id     == 0, "dashboard"),
            (dashboard_msg_id == 0, "dashboard_msg")
        ];
        if let Some((true, id)) = invalid_ids.iter().find(|(is_zero, _)| *is_zero) {
            return Err(anyhow!("Group has invalid {} configuration (0 ID not allowed)", id));
        }

        let chat      = CI::new(chat_id);
        let queue     = CI::new(queue_id);
        let red       = CI::new(red_id);
        let blu       = CI::new(blu_id);
        let dashboard = CI::new(dashboard_id);

        let guild_id     = result.get::<i64, _>("guild_id") as u64;
        let group_id     = result.try_get::<i64, _>("group_id").unwrap_or(0) as u8;
        let name         = result.try_get::<Option<String>, _>("name").ok().flatten();
        let connect_info = result.try_get::<Option<String>, _>("connect_info").ok().flatten();

        // Load teams from teams table, fallback to single team from groups table
        let teams = match self.get_teams_for_group(guild_id, group_id).await {
            Ok(teams) if !teams.is_empty() => teams,
            _ => vec![TeamChannel::new(red, blu)], // Fallback to groups table red/blu
        };

        let mut group = Group::new(
            group_id,
            name,
            result.try_get::<i64, _>("quota")  .unwrap_or(12)  as u8,
            result.try_get::<i64, _>("timeout").unwrap_or(120) as u16,
            MI::new(dashboard_msg_id),
            Channels::new(chat, queue, teams, dashboard),
            Vec::new(),
        );
        group.connect_info = connect_info;

        Ok(group)
    }

    fn build_teams_from_row(&self, result: &sqlx::sqlite::SqliteRow) -> Result<TeamChannel> {
        let red  = CI::new(result.get::<i64, _>("red") as u64);
        let blu  = CI::new(result.get::<i64, _>("blu") as u64);
        let team = TeamChannel::new(red, blu);
        Ok(team)
    }

    pub async fn get_teams_for_group(&self, guild_id: u64, group_id: u8) -> Result<Vec<TeamChannel>> {
        let rows = sqlx::query("SELECT red, blu FROM teams WHERE guild_id = ? AND group_id = ?")
        .bind(guild_id as i64)
        .bind(group_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut teams = Vec::new();
        for row in rows {
            teams.push(self.build_teams_from_row(&row)?);
        }

        Ok(teams)
    }

    /// Check if a group exists for a guild
    pub async fn group_exists_for_guild(&self, guild_id: u64) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM groups WHERE guild_id = ?")
        .bind(guild_id as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }

    /// Get all groups for a guild
    pub async fn get_groups_for_guild(&self, guild_id: u64) -> Result<Vec<Group>> {
        let rows = sqlx::query("SELECT id, group_id, name, timeout, guild_id, dashboard, chat, queue, dashboard_msg, red, blu, game_increment, quota, connect_info
                                FROM groups
                                WHERE guild_id = ?"
        )
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut groups = Vec::new();
        for row in rows {
            match self.build_group_from_row_async(&row).await {
                Ok(group) => groups.push(group),
                Err(e) => {
                    let group_id: i64 = row.try_get("group_id").unwrap_or(0);
                    let queue_id: i64 = row.try_get("queue").unwrap_or(0);
                    warn!("Skipping invalid group {} (queue {}) for guild {}: {}",
                        group_id, queue_id, guild_id, e);
                }
            }
        }

        Ok(groups)
    }

    /// Update dashboard message ID for a group by its dashboard channel ID
    pub async fn update_dashboard_msg(&self, guild_id: u64, dashboard_channel_id: u64, dashboard_msg_id: u64) -> Result<()> {
        info!("Updating dashboard message ID for guild {} dashboard channel {}", guild_id, dashboard_channel_id);

        sqlx::query("UPDATE groups SET dashboard_msg = ? WHERE guild_id = ? AND dashboard = ?")
        .bind(dashboard_msg_id as i64)
        .bind(guild_id as i64)
        .bind(dashboard_channel_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group name
    pub async fn update_name(&self, guild_id: u64, group_id: u8, name: Option<&str>) -> Result<()> {
        info!("Updating name for guild {} group {}: {:?}", guild_id, group_id, name);

        sqlx::query("UPDATE groups SET name = ? WHERE guild_id = ? AND group_id = ?")
        .bind(name)
        .bind(guild_id as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group quota
    pub async fn update_quota(&self, guild_id: u64, group_id: u8, quota: u8) -> Result<()> {
        info!("Updating quota for guild {} group {}: {}", guild_id, group_id, quota);

        sqlx::query("UPDATE groups SET quota = ? WHERE guild_id = ? AND group_id = ?")
        .bind(quota as i64)
        .bind(guild_id as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group timeout
    pub async fn update_timeout(&self, guild_id: u64, group_id: u8, timeout: u16) -> Result<()> {
        info!("Updating timeout for guild {} group {}: {}", guild_id, group_id, timeout);

        sqlx::query("UPDATE groups SET timeout = ? WHERE guild_id = ? AND group_id = ?")
        .bind(timeout as i64)
        .bind(guild_id as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group connect info
    pub async fn update_connect_info(&self, guild_id: u64, group_id: u8, connect_info: Option<&str>) -> Result<()> {
        info!("Updating connect_info for guild {} group {}: {:?}", guild_id, group_id, connect_info);

        sqlx::query("UPDATE groups SET connect_info = ? WHERE guild_id = ? AND group_id = ?")
        .bind(connect_info)
        .bind(guild_id as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl Repository<Group, u8> for GroupRepository {
    async fn create(&self, group: &Group) -> Result<Group> {
        // Extract values from the group struct
        let dashboard_ch  = group.channels.dashboard .get();
        let dashboard_msg = group.dashboard_msg      .get();
        let chat          = group.channels.queue_chat.get();
        let queue         = group.channels.queue_vc  .get();
        let red           = group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0);
        let blu           = group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0);

        let config = GroupConfig {
            dashboard_channel_id: dashboard_ch,
            chat_channel_id:      chat,
            queue_vc_id:          queue,
            red_vc_id:            red,
            blu_vc_id:            blu,
            quota:                group.quota,
        };
        self.create_group(0, dashboard_msg, config).await
    }

    async fn get_by_id(&self, group_id: u8) -> Result<Group> {
        let result = sqlx::query("SELECT id, group_id, name, timeout, guild_id, dashboard, chat, queue, dashboard_msg, red, blu, game_increment, quota, connect_info
                                  FROM groups WHERE group_id = ?"
        )
        .bind(group_id as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    async fn update(&self, group: &Group) -> Result<Group> {
        let dashboard_ch = group.channels.dashboard .get();
        let chat         = group.channels.queue_chat.get();
        let queue        = group.channels.queue_vc  .get();
        let red          = group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0);
        let blu          = group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0);

        let config = GroupConfig {
            dashboard_channel_id: dashboard_ch,
            chat_channel_id:      chat,
            queue_vc_id:          queue,
            red_vc_id:            red,
            blu_vc_id:            blu,
            quota:                group.quota,
        };
        self.update_group(0, config).await
    }

    async fn delete(&self, group_id: u8) -> Result<()> {
        sqlx::query("DELETE FROM groups WHERE group_id = ?")
            .bind(group_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
