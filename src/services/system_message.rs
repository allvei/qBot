use anyhow::{anyhow, Result};
use serenity::all::{Context, CreateMessage, GuildId};
use tracing::{error, info, warn};

use crate::Database;

/// Send a system message to a specific guild's configured system message channel
pub async fn send_system_message(ctx: &Context, db: &Database, guild_id: GuildId, content: &str) -> Result<()> {
  let channel_id = db.config.get_system_message_channel(guild_id).await?;

  match channel_id {
    Some(channel_id) => {
      // Verify channel exists
      match ctx.http.get_channel(channel_id).await {
        Ok(_) => {
          // Send message
          channel_id.send_message(&ctx.http, CreateMessage::new().content(content)).await.map_err(|e| anyhow!("Failed to send system message: {}", e))?;
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
      warn!("[{}] No system message channel configured", guild_name);
      Err(anyhow!("No system message channel configured for guild {}", guild_id))
    }
  }
}

/// Send a system message to all guilds that have a system message channel configured
pub async fn broadcast_system_message(ctx: &Context, db: &Database, content: &str) -> Result<Vec<(GuildId, Result<()>)>> {
  let guilds: Vec<GuildId> = ctx.cache.guilds().iter().copied().collect();
  let mut results = Vec::new();

  for guild_id in guilds {
    let result = send_system_message(ctx, db, guild_id, content).await;
    results.push((guild_id, result));
  }

  Ok(results)
}

/// Validate that all guilds have a valid system message channel configured
/// Returns a list of (guild_id, guild_name, error) for guilds with issues
pub async fn validate_system_message_channels(ctx: &Context, db: &Database) -> Vec<(GuildId, String, String)> {
  let guilds: Vec<GuildId> = ctx.cache.guilds().iter().copied().collect();
  let mut errors = Vec::new();

  for guild_id in guilds {
    let guild_name = crate::models::constants::guild_name(ctx, guild_id);

    match db.config.get_system_message_channel(guild_id).await {
      Ok(Some(channel_id)) => {
        // Check if channel exists
        if let Err(e) = ctx.http.get_channel(channel_id).await {
          errors.push((guild_id, guild_name, format!("Channel {} not found: {}", channel_id, e)));
        }
      }
      Ok(None) => {
        errors.push((guild_id, guild_name, "No system message channel configured".to_string()));
      }
      Err(e) => {
        errors.push((guild_id, guild_name, format!("Database error: {}", e)));
      }
    }
  }

  errors
}
