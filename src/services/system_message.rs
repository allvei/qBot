use anyhow::{anyhow, Result};
use serenity::all::{Context, CreateMessage, GuildId};
use tracing::{error, info, warn};

use crate::Database;

/// Send a system message to a specific guild's configured system message channel
pub async fn send_system_message(ctx: &Context, db: &Database, guild_id: GuildId, content: &str) -> Result<()> {
  info!("send_system_message called for guild {} with content length {}", guild_id, content.len());

  let channel_id = db.config.get_system_message_channel(guild_id).await?;
  info!("Got system message channel for guild {}: {:?}", guild_id, channel_id);

  match channel_id {
    Some(channel_id) => {
      // Verify channel exists and is a text channel
      info!("Checking if channel {} exists", channel_id);
      match ctx.http.get_channel(channel_id).await {
        Ok(channel) => {
          use serenity::all::{Channel, Permissions};

          // Check if it's a guild text channel
          let is_text_channel = matches!(channel, Channel::Guild(_));
          info!("Channel {} is text channel: {}", channel_id, is_text_channel);

          if !is_text_channel {
            let guild_name = crate::models::constants::guild_name(ctx, guild_id);
            error!("[{}] System message channel {} is not a guild text channel", guild_name, channel_id);
            return Err(anyhow!("System message channel {} is not a guild text channel", channel_id));
          }

          // Check bot permissions in the channel
          if let Channel::Guild(guild_channel) = channel {
            let bot_id = ctx.cache.current_user().id;
            info!("Checking permissions for bot {} in channel {}", bot_id, channel_id);
            let permissions = guild_channel.permissions_for_user(ctx, bot_id);

            if let Ok(perms) = permissions {
              info!("Bot permissions: {:?}", perms);
              if !perms.contains(Permissions::SEND_MESSAGES) {
                let guild_name = crate::models::constants::guild_name(ctx, guild_id);
                error!("[{}] Bot lacks SEND_MESSAGES permission in system message channel {}", guild_name, channel_id);
                return Err(anyhow!("Bot lacks SEND_MESSAGES permission in channel {}", channel_id));
              }
            } else {
              let guild_name = crate::models::constants::guild_name(ctx, guild_id);
              error!("[{}] Failed to check permissions in system message channel {}: {:?}", guild_name, channel_id, permissions);
              return Err(anyhow!("Failed to check permissions in channel {}", channel_id));
            }
          }

          // Send message
          info!("Attempting to send message to channel {}", channel_id);
          channel_id.send_message(&ctx.http, CreateMessage::new().content(content)).await.map_err(|e| {
            let guild_name = crate::models::constants::guild_name(ctx, guild_id);
            error!("[{}] Failed to send system message to channel {}: {}", guild_name, channel_id, e);
            anyhow!("Failed to send system message to channel {}: {}", channel_id, e)
          })?;
          info!("Sent system message to guild {} channel {}", guild_id, channel_id);
          Ok(())
        }
        Err(e) => {
          let guild_name = crate::models::constants::guild_name(ctx, guild_id);
          error!("[{}] System message channel {} not found or inaccessible: {}", guild_name, channel_id, e);
          Err(anyhow!("System message channel {} not found for guild {}", channel_id, guild_id))
        }
      }
    }
    None => {
      let guild_name = crate::models::constants::guild_name(ctx, guild_id);
      warn!("[{}] System messages channel is not set", guild_name);
      Err(anyhow!("System messages channel is not set for guild {}", guild_id))
    }
  }
}

/// Send a system message to all guilds that have a system message channel configured
pub async fn broadcast_system_message(ctx: &Context, db: &Database, content: &str) -> Result<Vec<(GuildId, Result<()>)>> {
  let guilds: Vec<GuildId> = ctx.cache.guilds().to_vec();
  let mut results = Vec::new();

  for guild_id in guilds {
    let result = send_system_message(ctx, db, guild_id, content).await;
    results.push((guild_id, result));
  }

  Ok(results)
}

/// Validate that all guilds have a valid system message channel configured
/// Returns a list of (guild_id, guild_name, error) for guilds with issues
pub async fn validate_system_message_channels(ctx: &Context, db: &Database) -> Vec<(String, String)> {
  let guilds: Vec<GuildId> = ctx.cache.guilds().to_vec();
  let mut errors = Vec::new();

  for guild_id in guilds {
    let guild_name = if let Some(name) = ctx.cache.guild(guild_id).map(|g| g.name.clone()) {
      name
    } else {
      db.guilds.get_display_name(guild_id).await.ok().flatten().unwrap_or_else(|| guild_id.to_string())
    };

    match db.config.get_system_message_channel(guild_id).await {
      Ok(Some(channel_id)) => {
        // Check if channel exists
        if let Err(e) = ctx.http.get_channel(channel_id).await {
          errors.push((guild_name, format!("Channel {} not found: {}", channel_id, e)));
        }
      }
      Ok(None) => {
        errors.push((guild_name, "System messages channel is unset".to_string()));
      }
      Err(e) => {
        errors.push((guild_name, format!("Database error: {}", e)));
      }
    }
  }

  errors
}

/// Send a community update to a specific guild's configured community updates channel
pub async fn send_community_update(ctx: &Context, db: &Database, guild_id: GuildId, content: &str) -> Result<()> {
  info!("send_community_update called for guild {} with content length {}", guild_id, content.len());

  let channel_id = db.config.get_community_updates_channel(guild_id).await?;
  info!("Got community updates channel for guild {}: {:?}", guild_id, channel_id);

  match channel_id {
    Some(channel_id) => {
      // Verify channel exists and is a text channel
      info!("Checking if channel {} exists", channel_id);
      match ctx.http.get_channel(channel_id).await {
        Ok(channel) => {
          use serenity::all::{Channel, Permissions};

          // Check if it's a guild text channel
          let is_text_channel = matches!(channel, Channel::Guild(_));
          info!("Channel {} is text channel: {}", channel_id, is_text_channel);

          if !is_text_channel {
            let guild_name = crate::models::constants::guild_name(ctx, guild_id);
            error!("[{}] Community updates channel {} is not a guild text channel", guild_name, channel_id);
            return Err(anyhow!("Community updates channel {} is not a guild text channel", channel_id));
          }

          // Check bot permissions in the channel
          if let Channel::Guild(guild_channel) = channel {
            let bot_id = ctx.cache.current_user().id;
            info!("Checking permissions for bot {} in channel {}", bot_id, channel_id);
            let permissions = guild_channel.permissions_for_user(ctx, bot_id);

            if let Ok(perms) = permissions {
              info!("Bot permissions: {:?}", perms);
              if !perms.contains(Permissions::SEND_MESSAGES) {
                let guild_name = crate::models::constants::guild_name(ctx, guild_id);
                error!("[{}] Bot lacks SEND_MESSAGES permission in community updates channel {}", guild_name, channel_id);
                return Err(anyhow!("Bot lacks SEND_MESSAGES permission in channel {}", channel_id));
              }
            } else {
              let guild_name = crate::models::constants::guild_name(ctx, guild_id);
              error!("[{}] Failed to check permissions in community updates channel {}: {:?}", guild_name, channel_id, permissions);
              return Err(anyhow!("Failed to check permissions in channel {}", channel_id));
            }
          }

          // Send message
          info!("Attempting to send message to channel {}", channel_id);
          channel_id.send_message(&ctx.http, CreateMessage::new().content(content)).await.map_err(|e| {
            let guild_name = crate::models::constants::guild_name(ctx, guild_id);
            error!("[{}] Failed to send community update to channel {}: {}", guild_name, channel_id, e);
            anyhow!("Failed to send community update to channel {}: {}", channel_id, e)
          })?;
          info!("Sent community update to guild {} channel {}", guild_id, channel_id);
          Ok(())
        }
        Err(e) => {
          let guild_name = crate::models::constants::guild_name(ctx, guild_id);
          error!("[{}] Community updates channel {} not found or inaccessible: {}", guild_name, channel_id, e);
          Err(anyhow!("Community updates channel {} not found for guild {}", channel_id, guild_id))
        }
      }
    }
    None => {
      let guild_name = crate::models::constants::guild_name(ctx, guild_id);
      warn!("[{}] No community updates channel configured", guild_name);
      Err(anyhow!("No community updates channel configured for guild {}", guild_id))
    }
  }
}

/// Send a community update to all guilds that have a community updates channel configured
pub async fn broadcast_community_update(ctx: &Context, db: &Database, content: &str) -> Result<Vec<(GuildId, Result<()>)>> {
  let guilds: Vec<GuildId> = ctx.cache.guilds().to_vec();
  let mut results = Vec::new();

  for guild_id in guilds {
    let result = send_community_update(ctx, db, guild_id, content).await;
    results.push((guild_id, result));
  }

  Ok(results)
}

/// Validate that all guilds have a valid community updates channel configured
/// Returns a list of (guild_id, guild_name, error) for guilds with issues
pub async fn validate_community_updates_channels(ctx: &Context, db: &Database) -> Vec<(GuildId, String, String)> {
  let guilds: Vec<GuildId> = ctx.cache.guilds().to_vec();
  let mut errors = Vec::new();

  for guild_id in guilds {
    let guild_name = crate::models::constants::guild_name(ctx, guild_id);

    match db.config.get_community_updates_channel(guild_id).await {
      Ok(Some(channel_id)) => {
        // Check if channel exists
        if let Err(e) = ctx.http.get_channel(channel_id).await {
          errors.push((guild_id, guild_name, format!("Channel {} not found: {}", channel_id, e)));
        }
      }
      Ok(None) => {
        errors.push((guild_id, guild_name, "No community updates channel configured".to_string()));
      }
      Err(e) => {
        errors.push((guild_id, guild_name, format!("Database error: {}", e)));
      }
    }
  }

  errors
}
