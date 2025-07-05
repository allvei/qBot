use anyhow::Result;
use serenity::all::RoleId;
use tracing::{info, warn};

use crate::models::command::CommandContext;
use crate::models::player::Role;

pub async fn check_role(cc: &CommandContext<'_>, role: &Role) -> Result<bool> {
    if let Some(guild_id) = cc.intax.guild_id {
        let member = guild_id.member(&cc.ctx.http, cc.intax.user.id).await;
        if let Ok(member) = member {
            info!(
                "[role] Checking if user has {} role with ID: {}",
                role.name(),
                role.id()
            );
            return Ok(member.roles.contains(&RoleId::new(role.id())));
        } else {
            warn!(
                "[role] Failed to fetch member for user {} in guild {}: {:?}",
                cc.intax.user.id,
                guild_id,
                member.as_ref().err()
            );
        }
    }
    Ok(false)
}
