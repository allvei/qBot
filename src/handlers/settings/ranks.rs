use std::sync::Arc;

use crate::{db::Database, handlers::settings::menu::RANK_CONFIG_TOGGLES};
use anyhow::Result;
use serenity::all::{ChannelId as CI, Context, GuildId as GI, PermissionOverwrite as PO, PermissionOverwriteType as POT, Permissions, RoleId as RI};
use tracing::info;

/// Get rank settings from database (for rank configuration menu)
pub async fn get_rank_settings(db: &Arc<Database>, guild_id: GI) -> Result<(Vec<bool>, Option<RI>)> {
  let mut toggle_states = Vec::with_capacity(RANK_CONFIG_TOGGLES.len());
  for toggle in RANK_CONFIG_TOGGLES {
    toggle_states.push(db.config.get_bool(guild_id, toggle.column, toggle.default).await?);
  }
  let default_rank_role = db.config.get_default_rank_role_id(guild_id).await?;
  Ok((toggle_states, default_rank_role))
}

/// Get all rank roles for display (name, elo, role_id)
pub async fn get_all_rank_roles(db: &Arc<Database>, guild_id: GI) -> Result<Vec<(String, u16, RI)>> {
  let guild_ranks = db.ranks.get_or_init_ranks(guild_id).await?;

  let result: Vec<(String, u16, RI)> = guild_ranks.into_iter().map(|gr| (gr.name, gr.elo, gr.role_id)).collect();

  Ok(result)
}

/// Apply ELO gate permissions on a category channel.
/// Denies VIEW_CHANNEL for @everyone, allows VIEW_CHANNEL for rank roles in [min_idx..=max_idx].
/// Also ensures the bot can still see the category.
pub async fn apply_elo_gate(ctx: &Context, guild_id: GI, category_id: CI, ranks: &[crate::db::repo::rank::GuildRank], min_idx: usize, max_idx: usize) -> Result<usize> {
  let guild = guild_id.to_partial_guild(&ctx.http).await?;
  let bot_user_id = ctx.cache.current_user().id;

  // Find bot's integration role
  let bot_role = guild.roles.values().find(|r| r.managed && r.tags.bot_id == Some(bot_user_id)).map(|r| r.id);

  // Grant bot permissions FIRST so it doesn't lose access after denying @everyone.
  // Must include MANAGE_CHANNELS and MANAGE_ROLES so the bot can still edit
  // dashboard messages, delete team VCs, and modify permissions on channels
  // under this category after @everyone is denied.
  let bot_perms = Permissions::VIEW_CHANNEL
    | Permissions::SEND_MESSAGES
    | Permissions::EMBED_LINKS
    | Permissions::CONNECT
    | Permissions::MOVE_MEMBERS
    | Permissions::MANAGE_CHANNELS
    | Permissions::MANAGE_ROLES;

  category_id.create_permission(&ctx.http, PO { allow: bot_perms, deny: Permissions::empty(), kind: POT::Member(bot_user_id) }).await?;

  // Allow bot integration role if present
  if let Some(role_id) = bot_role {
    category_id.create_permission(&ctx.http, PO { allow: bot_perms, deny: Permissions::empty(), kind: POT::Role(role_id) }).await?;
  }

  // Deny @everyone VIEW_CHANNEL on the category
  category_id.create_permission(&ctx.http, PO { allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL, kind: POT::Role(guild_id.everyone_role()) }).await?;

  // Collect all rank role IDs so we can deny those outside the range
  let mut allowed_count = 0usize;
  for (i, rank) in ranks.iter().enumerate() {
    if i >= min_idx && i <= max_idx {
      // Allow this rank to view
      category_id.create_permission(&ctx.http, PO { allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty(), kind: POT::Role(rank.role_id) }).await?;
      allowed_count += 1;
    } else {
      // Explicitly deny this rank
      category_id.create_permission(&ctx.http, PO { allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL, kind: POT::Role(rank.role_id) }).await?;
    }
  }

  info!("Applied rank gate on category {} in guild {}: ranks {}..={} ({} roles allowed)", category_id, guild_id, min_idx, max_idx, allowed_count);

  Ok(allowed_count)
}

/// Clear ELO gate permissions from a category channel.
/// Removes the VIEW_CHANNEL deny from @everyone and removes all rank role overwrites.
pub async fn clear_elo_gate(ctx: &Context, guild_id: GI, category_id: CI) -> Result<()> {
  // Remove @everyone VIEW_CHANNEL deny by deleting the overwrite
  category_id.delete_permission(&ctx.http, POT::Role(guild_id.everyone_role())).await?;

  // Get the current channel to find existing overwrites
  let channel = ctx.http.get_channel(category_id).await?;
  if let Some(guild_channel) = channel.guild() {
    for overwrite in &guild_channel.permission_overwrites {
      // Remove role overwrites (but keep member overwrites like the bot's)
      if let POT::Role(role_id) = overwrite.kind {
        if role_id != guild_id.everyone_role() {
          let _ = category_id.delete_permission(&ctx.http, POT::Role(role_id)).await;
        }
      }
    }
  }

  info!("Cleared rank gate on category {} in guild {}", category_id, guild_id);
  Ok(())
}
