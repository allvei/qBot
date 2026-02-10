use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serenity::all::{ChannelId as CI, MessageId as MI, GuildId as GI};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};

use super::Repository;
use crate::models::{Channels, Group, TeamBalanceMethod, TeamChannel};

/// Configuration for creating or updating a group
pub struct GroupConfig {
    pub category_id:          u64,
    pub dashboard_channel_id: u64,
    pub chat_channel_id:      u64,
    pub queue_vc_id:          u64,
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

    pub async fn create_group(&self, guild_id: GI, dashboard_msg: u64, config: GroupConfig) -> Result<Group> {
        info!("Creating new group with queue: {}", config.queue_vc_id);

        // Get the next available group_id for this guild
        let next_group_id: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(group_id), -1) + 1 FROM groups WHERE guild_id = ?"
        )
        .bind(guild_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        let result = sqlx::query(
            "INSERT INTO groups (guild_id, group_id, category, dashboard, chat, queue, dashboard_msg, red, blu, quota)
             VALUES (?, ?, ?, ?, ?, ?, ?, 1, 1, ?)
             RETURNING id, group_id, name, timeout, guild_id, category, dashboard, chat, queue, dashboard_msg, red, blu, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum"
        )
        .bind(guild_id.get()              as i64)
        .bind(next_group_id)
        .bind(config.category_id          as i64)
        .bind(config.dashboard_channel_id as i64)
        .bind(config.chat_channel_id      as i64)
        .bind(config.queue_vc_id          as i64)
        .bind(dashboard_msg               as i64)
        .bind(config.quota                as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    pub async fn update_group(&self, guild_id: GI, config: GroupConfig) -> Result<Group> {
        info!("Updating group with queue_id: {}", config.queue_vc_id);

        let result = sqlx::query("UPDATE groups
                                  SET guild_id = ?, category = ?, dashboard = ?, chat = ?, quota = ?
                                  WHERE queue = ?
                                  RETURNING id, group_id, name, timeout, guild_id, category, dashboard, chat, queue, dashboard_msg, red, blu, game_increment, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum"
        )
        .bind(guild_id.get()              as i64)
        .bind(config.category_id          as i64)
        .bind(config.dashboard_channel_id as i64)
        .bind(config.chat_channel_id      as i64)
        .bind(config.quota                as i64)
        .bind(config.queue_vc_id          as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    /// Delete a group from the database
    pub async fn delete_group(&self, guild_id: GI, group_id: u8) -> Result<()> {
        sqlx::query("DELETE FROM groups WHERE guild_id = ? AND group_id = ?")
            .bind(guild_id.get() as i64)
            .bind(group_id as i64)
            .execute(&self.pool)
            .await?;
        
        Ok(())
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
            (dashboard_id     == 0, "dashboard"),
            (dashboard_msg_id == 0, "dashboard_msg")
        ];
        if let Some((true, id)) = invalid_ids.iter().find(|(is_zero, _)| *is_zero) {
            return Err(anyhow!("Group has invalid {} configuration (0 ID not allowed)", id));
        }

        let chat      = CI::new(chat_id);
        let queue     = CI::new(queue_id);
        let dashboard = CI::new(dashboard_id);
        let category_id = result.try_get::<i64, _>("category").unwrap_or(0) as u64;
        let category  = if category_id == 0 { dashboard } else { CI::new(category_id) };

        let guild_id     = GI::new(result.get::<i64, _>("guild_id") as u64);
        let group_id     = result.try_get::<i64, _>("group_id").unwrap_or(0) as u8;
        let name         = result.try_get::<Option<String>, _>("name").ok().flatten();
        let connect_info = result.try_get::<Option<String>, _>("connect_info").ok().flatten();
        let team_balance_method_str = result.try_get::<Option<String>, _>("team_balance_method").ok().flatten();
        let team_balance_method = team_balance_method_str
            .map(|s| TeamBalanceMethod::from_str(&s))
            .unwrap_or_default();

        // Load teams from teams table; fallback to legacy red/blu columns only if they hold real IDs
        let teams = match self.get_teams_for_group(guild_id, group_id).await {
            Ok(teams) if !teams.is_empty() => teams,
            _ => {
                if red_id > 1 && blu_id > 1 {
                    vec![TeamChannel::new(CI::new(red_id), CI::new(blu_id))]
                } else {
                    vec![]
                }
            }
        };

        let mut group = Group::new(
            guild_id,
            group_id,
            name,
            result.try_get::<i64, _>("quota")  .unwrap_or(12)  as u8,
            result.try_get::<i64, _>("timeout").unwrap_or(120) as u16,
            MI::new(dashboard_msg_id),
            Channels::new(category, chat, queue, teams, dashboard),
            Vec::new(),
        );
        group.team_balance_method = team_balance_method;

        // Load team VC lifecycle settings
        {
            use crate::models::{TeamVcCreatePolicy, TeamVcDestroyPolicy, TeamVcSettings};
            let create_policy = result.try_get::<String, _>("team_vc_create_policy")
                .ok()
                .map(|s| TeamVcCreatePolicy::from_str(&s))
                .unwrap_or_default();
            let destroy_policy = result.try_get::<String, _>("team_vc_destroy_policy")
                .ok()
                .map(|s| TeamVcDestroyPolicy::from_str(&s))
                .unwrap_or_default();
            let keep_minimum = result.try_get::<i64, _>("team_vc_keep_minimum").unwrap_or(1) != 0;
            group.team_vc_settings = TeamVcSettings {
                create_policy,
                destroy_policy,
                keep_minimum,
            };
        }

        // Load subgroups from DB; if present, replace the default subgroup
        match self.get_subgroups(guild_id, group_id).await {
            Ok(sgs) if !sgs.is_empty() => {
                group.subgroups = sgs;
            }
            _ => {
                // No DB subgroups yet - keep the default created by Group::new
                // and apply connect_info from the groups table
                group.set_connect_info(connect_info);
            }
        }

        // Load DM alert settings
        group.dm_alert_enabled = result.try_get::<i64, _>("dm_alert_enabled").unwrap_or(0) != 0;
        group.dm_alert_threshold = result.try_get::<i64, _>("dm_alert_threshold").unwrap_or(0) as u8;
        
        // Parse dm_alert_users as JSON array
        if let Ok(users_json) = result.try_get::<String, _>("dm_alert_users") {
            if let Ok(users) = serde_json::from_str::<Vec<serenity::all::UserId>>(&users_json) {
                group.dm_alert_users = users;
            }
        }

        Ok(group)
    }

    fn build_teams_from_row(&self, result: &sqlx::sqlite::SqliteRow) -> Result<TeamChannel> {
        let red  = CI::new(result.get::<i64, _>("red") as u64);
        let blu  = CI::new(result.get::<i64, _>("blu") as u64);
        let team = TeamChannel::new(red, blu);
        Ok(team)
    }

    pub async fn get_teams_for_group(&self, guild_id: GI, group_id: u8) -> Result<Vec<TeamChannel>> {
        let rows = sqlx::query("SELECT red, blu FROM teams WHERE guild_id = ? AND group_id = ?")
        .bind(guild_id.get() as i64)
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
    pub async fn group_exists_for_guild(&self, guild_id: GI) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM groups WHERE guild_id = ?")
        .bind(guild_id.get() as i64)
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }

    /// Get all groups for a guild
    pub async fn get_groups_for_guild(&self, guild_id: GI) -> Result<Vec<Group>> {
        let rows = sqlx::query("SELECT id, group_id, name, timeout, guild_id, category, dashboard, chat, queue, dashboard_msg, red, blu, game_increment, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum
                                FROM groups
                                WHERE guild_id = ?"
        )
        .bind(guild_id.get() as i64)
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
    pub async fn update_dashboard_msg(&self, guild_id: GI, dashboard_channel_id: u64, dashboard_msg_id: u64) -> Result<()> {
        info!("Updating dashboard message ID for guild {} dashboard channel {}", guild_id, dashboard_channel_id);

        sqlx::query("UPDATE groups SET dashboard_msg = ? WHERE guild_id = ? AND dashboard = ?")
        .bind(dashboard_msg_id as i64)
        .bind(guild_id.get() as i64)
        .bind(dashboard_channel_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update dashboard message ID for a group by its group_id
    pub async fn update_dashboard_msg_by_group_id(&self, guild_id: GI, group_id: u8, dashboard_msg_id: u64) -> Result<()> {
        info!("Updating dashboard message ID for guild {} group {}", guild_id, group_id);

        sqlx::query("UPDATE groups SET dashboard_msg = ? WHERE guild_id = ? AND group_id = ?")
        .bind(dashboard_msg_id as i64)
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group name
    pub async fn update_name(&self, guild_id: GI, group_id: u8, name: Option<&str>) -> Result<()> {
        info!("Updating name for guild {} group {}: {:?}", guild_id, group_id, name);

        sqlx::query("UPDATE groups SET name = ? WHERE guild_id = ? AND group_id = ?")
        .bind(name)
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group quota
    pub async fn update_quota(&self, guild_id: GI, group_id: u8, quota: u8) -> Result<()> {
        info!("Updating quota for guild {} group {}: {}", guild_id, group_id, quota);

        sqlx::query("UPDATE groups SET quota = ? WHERE guild_id = ? AND group_id = ?")
        .bind(quota as i64)
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group timeout
    pub async fn update_timeout(&self, guild_id: GI, group_id: u8, timeout: u16) -> Result<()> {
        info!("Updating timeout for guild {} group {}: {}", guild_id, group_id, timeout);

        sqlx::query("UPDATE groups SET timeout = ? WHERE guild_id = ? AND group_id = ?")
        .bind(timeout as i64)
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group connect info
    pub async fn update_connect_info(&self, guild_id: GI, group_id: u8, connect_info: Option<&str>) -> Result<()> {
        info!("Updating connect_info for guild {} group {}: {:?}", guild_id, group_id, connect_info);

        sqlx::query("UPDATE groups SET connect_info = ? WHERE guild_id = ? AND group_id = ?")
        .bind(connect_info)
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update group team balance method
    pub async fn update_team_balance_method(&self, guild_id: GI, group_id: u8, method: TeamBalanceMethod) -> Result<()> {
        info!("Updating team_balance_method for guild {} group {}: {}", guild_id, group_id, method);

        sqlx::query("UPDATE groups SET team_balance_method = ? WHERE guild_id = ? AND group_id = ?")
        .bind(method.as_str())
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_team_vc_settings(&self, guild_id: GI, group_id: u8, settings: &crate::models::TeamVcSettings) -> Result<()> {
        sqlx::query(
            "UPDATE groups SET team_vc_create_policy = ?, team_vc_destroy_policy = ?, team_vc_keep_minimum = ? WHERE guild_id = ? AND group_id = ?"
        )
        .bind(settings.create_policy.to_db_str())
        .bind(settings.destroy_policy.to_db_str())
        .bind(if settings.keep_minimum { 1i64 } else { 0i64 })
        .bind(guild_id.get() as i64)
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
        let guild_id      = group.guild_id;
        let dashboard_ch  = group.channels.dashboard .get();
        let dashboard_msg = group.dashboard_msg      .get();
        let chat          = group.channels.queue_chat.get();
        let queue         = group.channels.queue_vc  .get();
        let category      = group.channels.category  .get();
        let config = GroupConfig {
            category_id:          category,
            dashboard_channel_id: dashboard_ch,
            chat_channel_id:      chat,
            queue_vc_id:          queue,
            quota:                group.quota(),
        };
        self.create_group(guild_id, dashboard_msg, config).await
    }

    async fn get_by_id(&self, group_id: u8) -> Result<Group> {
        let result = sqlx::query("SELECT id, group_id, name, timeout, guild_id, category, dashboard, chat, queue, dashboard_msg, red, blu, game_increment, quota, connect_info, team_vc_create_policy, team_vc_destroy_policy, team_vc_keep_minimum
                                  FROM groups WHERE group_id = ?"
        )
        .bind(group_id as i64)
        .fetch_one(&self.pool)
        .await?;

        self.build_group_from_row_async(&result).await
    }

    async fn update(&self, group: &Group) -> Result<Group> {
        let guild_id     = group.guild_id;
        let dashboard_ch = group.channels.dashboard .get();
        let chat         = group.channels.queue_chat.get();
        let queue        = group.channels.queue_vc  .get();
        let category     = group.channels.category  .get();
        let config = GroupConfig {
            category_id:          category,
            dashboard_channel_id: dashboard_ch,
            chat_channel_id:      chat,
            queue_vc_id:          queue,
            quota:                group.quota(),
        };
        self.update_group(guild_id, config).await
    }

    async fn delete(&self, group_id: u8) -> Result<()> {
        sqlx::query("DELETE FROM groups WHERE group_id = ?")
            .bind(group_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

impl GroupRepository {
    /// Update DM alert settings for a group
    pub async fn update_dm_alert_settings(&self, guild_id: GI, group_id: u8, enabled: bool, threshold: u8, users: Vec<serenity::all::UserId>) -> Result<()> {
        let users_json = serde_json::to_string(&users)?;
        
        sqlx::query(
            "UPDATE groups SET dm_alert_enabled = ?, dm_alert_threshold = ?, dm_alert_users = ? 
             WHERE guild_id = ? AND group_id = ?"
        )
        .bind(enabled as i64)
        .bind(threshold as i64)
        .bind(users_json)
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    // ========================================================================
    // Subgroup methods
    // ========================================================================

    /// Get all subgroups for a group
    pub async fn get_subgroups(&self, guild_id: GI, group_id: u8) -> Result<Vec<crate::models::Subgroup>> {
        let rows = sqlx::query(
            "SELECT subgroup_id, name, quota, connect_info FROM subgroups
             WHERE guild_id = ? AND group_id = ?
             ORDER BY subgroup_id"
        )
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut subgroups = Vec::new();
        for row in rows {
            let id: u8 = row.get::<i64, _>("subgroup_id") as u8;
            let name: String = row.get("name");
            let quota: u8 = row.get::<i64, _>("quota") as u8;
            let connect_info: Option<String> = row.try_get("connect_info").ok().flatten();
            let mut sg = crate::models::Subgroup::new(id, name, quota);
            sg.connect_info = connect_info;
            subgroups.push(sg);
        }

        Ok(subgroups)
    }

    /// Save a single subgroup (upsert)
    pub async fn save_subgroup(&self, guild_id: GI, group_id: u8, sg: &crate::models::Subgroup) -> Result<()> {
        sqlx::query(
            "INSERT INTO subgroups (guild_id, group_id, subgroup_id, name, quota, connect_info)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(guild_id, group_id, subgroup_id) DO UPDATE SET
                name = excluded.name,
                quota = excluded.quota,
                connect_info = excluded.connect_info"
        )
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .bind(sg.id as i64)
        .bind(&sg.name)
        .bind(sg.quota as i64)
        .bind(&sg.connect_info)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete a subgroup
    pub async fn delete_subgroup(&self, guild_id: GI, group_id: u8, subgroup_id: u8) -> Result<()> {
        sqlx::query(
            "DELETE FROM subgroups WHERE guild_id = ? AND group_id = ? AND subgroup_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .bind(subgroup_id as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Save all subgroups for a group (replaces existing)
    pub async fn save_all_subgroups(&self, guild_id: GI, group_id: u8, subgroups: &[crate::models::Subgroup]) -> Result<()> {
        // Delete all existing subgroups for this group
        sqlx::query("DELETE FROM subgroups WHERE guild_id = ? AND group_id = ?")
            .bind(guild_id.get() as i64)
            .bind(group_id as i64)
            .execute(&self.pool)
            .await?;

        // Insert all current subgroups
        for sg in subgroups {
            self.save_subgroup(guild_id, group_id, sg).await?;
        }

        Ok(())
    }

    /// Get DM alert settings for a group
    pub async fn get_dm_alert_settings(&self, guild_id: GI, group_id: u8) -> Result<(bool, u8, Vec<serenity::all::UserId>)> {
        let result = sqlx::query(
            "SELECT dm_alert_enabled, dm_alert_threshold, dm_alert_users 
             FROM groups WHERE guild_id = ? AND group_id = ?"
        )
        .bind(guild_id.get() as i64)
        .bind(group_id as i64)
        .fetch_one(&self.pool)
        .await?;

        let enabled = result.get::<i64, _>("dm_alert_enabled") != 0;
        let threshold = result.get::<i64, _>("dm_alert_threshold") as u8;
        let users_json: String = result.get("dm_alert_users");
        let users = serde_json::from_str(&users_json).unwrap_or_default();

        Ok((enabled, threshold, users))
    }
}
