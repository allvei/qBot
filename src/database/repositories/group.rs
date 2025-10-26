use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serenity::all::{ChannelId, MessageId};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn, error};

use crate::models::{server::*, game::TeamChannel};
use super::Repository;

#[derive(Clone)]
pub struct GroupRepository {
    pool: SqlitePool,
}

impl GroupRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool
        }
    }

    pub async fn create_group(
        &self,
        guild_id:             u64,
        dashboard_channel_id: u64,
        chat_channel_id:      u64,
        queue_vc_id:          u64,
        dashboard_msg_id:     u64,
        red_vc_id:            u64,
        blu_vc_id:            u64,
        game_quota:        u8,
    ) -> Result<Group> {
        info!("Creating new group with queue: {}", queue_vc_id);
        
        let result = sqlx::query(
            "INSERT INTO groups (guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, game_quota)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, group_id, timeout, dashboard, chat, queue, dashboard_msg_id, red, blu, game_quota"
        )
        .bind(guild_id             as i64)
        .bind(dashboard_channel_id as i64)
        .bind(chat_channel_id      as i64)
        .bind(queue_vc_id          as i64)
        .bind(dashboard_msg_id     as i64)
        .bind(red_vc_id            as i64)
        .bind(blu_vc_id            as i64)
        .bind(game_quota        as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(self.build_group_from_row_async(&result).await.unwrap())
    }

    pub async fn update_group(
        &self,
        guild_id:             u64,
        queue_vc_id:          u64,
        dashboard_channel_id: u64,
        chat_channel_id:      u64,
        red_vc_id:            u64,
        blu_vc_id:            u64,
        game_quota:        u8,
    ) -> Result<Group> {
        info!("Updating group with queue_id: {}", queue_vc_id);
        
        let result = sqlx::query(
            "UPDATE groups
             SET guild_id = ?, dashboard = ?, chat = ?, red = ?, blu = ?, game_quota = ?
             WHERE queue = ?
             RETURNING id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, game_increment, game_quota"
        )
        .bind(guild_id             as i64)
        .bind(dashboard_channel_id as i64)
        .bind(chat_channel_id      as i64)
        .bind(red_vc_id            as i64)
        .bind(blu_vc_id            as i64)
        .bind(game_quota        as i64)
        .bind(queue_vc_id          as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(self.build_group_from_row_async(&result).await.unwrap())
    }

    async fn build_group_from_row_async(&self, result: &sqlx::sqlite::SqliteRow) -> Result<Group> {
        // Validate channel IDs before creating ChannelId objects
        let chat_id      = result.get::<i64, _>("chat")  as u64;
        let queue_id     = result.get::<i64, _>("queue") as u64;
        let red_id       = result.get::<i64, _>("red")   as u64;
        let blu_id       = result.get::<i64, _>("blu")   as u64;
        let dashboard_id = result.get::<i64, _>("dashboard") as u64;
        
        // Reject groups with invalid (0) channel IDs - no undefined data allowed
        let invalid_ids = [
            (chat_id      == 0, "chat"),
            (queue_id     == 0, "queue"),
            (red_id       == 0, "red"),
            (blu_id       == 0, "blu"),
            (dashboard_id == 0, "dashboard")
        ];
        if let Some((true, id)) = invalid_ids.iter().find(|(is_zero, _)| *is_zero) {
            return Err(anyhow!("Group has invalid {} channel configuration (0 ID not allowed)", id));
        }
        
        let chat      = ChannelId::new(chat_id);
        let queue     = ChannelId::new(queue_id);
        let red       = ChannelId::new(red_id);
        let blu       = ChannelId::new(blu_id);
        let dashboard = ChannelId::new(dashboard_id);
        
        let guild_id = result.get::<i64, _>("guild_id") as u64;
        let group_id = result.try_get::<i64, _>("group_id").unwrap_or(0) as u8;

        // Load teams from teams table, fallback to single team from groups table
        let teams = match self.get_teams_for_group(guild_id, group_id).await {
            Ok(teams) if !teams.is_empty() => teams,
            _ => vec![TeamChannel::new(red, blu)], // Fallback to groups table red/blu
        };

        let group = Group::new(
            group_id,
            result.try_get::<i64, _>("game_quota").unwrap_or(12)     as u8,
            result.try_get::<i64, _>("timeout")      .unwrap_or(120)    as u16,
            MessageId::new(result.get::<i64, _>("dashboard_msg_id")     as u64),
            Channels::new(
                chat,
                queue,
                teams,
                dashboard,
            ),
            Vec::new(),
        );

        Ok(group)
    }

    fn build_teams_from_row(&self, result: &sqlx::sqlite::SqliteRow) -> Result<TeamChannel> {
        let red  = ChannelId::new(result.get::<i64, _>("red") as u64);
        let blu  = ChannelId::new(result.get::<i64, _>("blu") as u64);
        let team = TeamChannel::new(red, blu);
        Ok(team)
    }

    pub async fn get_teams_for_group(&self, guild_id: u64, group_id: u8) -> Result<Vec<TeamChannel>> {
        let rows = sqlx::query(
            "SELECT red, blu
             FROM teams
             WHERE guild_id = ? AND group_id = ?"
        )
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

    /// Validate group data integrity
    async fn validate_group_data(&self, group: &Group) -> Result<()> {
        // Check for placeholder values that indicate incomplete configuration
        if group.channels.dashboard.get() == 1 || group.dashboard_msg.get() == 1 {
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
            "SELECT id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, game_increment, game_quota
             FROM groups WHERE guild_id = ?"
        )
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut groups = Vec::new();
        for row in rows {
            groups.push(self.build_group_from_row_async(&row).await?);
        }
        
        Ok(groups)
    }
}

#[async_trait]
impl Repository<Group, u8> for GroupRepository {
    async fn create(&self, group: &Group) -> Result<Group> {
        // Extract values from the group struct
        let dashboard_ch  = group.channels.dashboard.get();
        let dashboard_msg = group.dashboard_msg     .get();
        let chat          = group.channels.queue    .get();
        let queue         = group.channels.queue_vc .get();
        let red           = group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0);
        let blu           = group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0);

        self.create_group(0, dashboard_ch, chat, queue, dashboard_msg, red, blu, group.quota).await
    }

    async fn get_by_id(&self, group_id: u8) -> Result<Group> {
        let result = sqlx::query(
            "SELECT id, group_id, timeout, guild_id, dashboard, chat, queue, dashboard_msg_id, red, blu, game_increment, game_quota
             FROM groups WHERE group_id = ?"
        )
        .bind(group_id as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    async fn update(&self, group: &Group) -> Result<Group> {
        let dashboard_ch = group.channels.dashboard.get();
        let chat         = group.channels.queue    .get();
        let queue        = group.channels.queue_vc .get();
        let red          = group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0);
        let blu          = group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0);

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
