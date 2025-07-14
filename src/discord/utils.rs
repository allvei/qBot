//! # Discord Utilities
//!
//! This module provides utility functions for working with the Discord API.
//! It includes helpers for formatting messages, managing channels, and other Discord-specific tasks.

use serenity::all::{ChannelId, Context, GuildId, UserId};
use tracing::{error, info};

use crate::discord::commands::CommandResponse;
use crate::error::{AppError, AppResult};

/// Send a command response to a channel
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `channel_id` - The channel to send the response to
/// * `response` - The command response to send
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
pub async fn send_response(
    ctx: &Context,
    channel_id: ChannelId,
    response: CommandResponse,
) -> AppResult<()> {
    match response {
        CommandResponse::Text(content) => {
            channel_id.say(&ctx.http, content).await.map_err(|e| {
                error!("Failed to send message: {}", e);
                AppError::DiscordError(format!("Failed to send message: {}", e))
            })?;
        }
        CommandResponse::Embed { title, description, color } => {
            let color_value = match color {
                Some((r, g, b)) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
                None => 0x3498db, // Default Discord blue
            };

            use serenity::builder::{CreateEmbed, CreateMessage};

            let embed = CreateEmbed::new().title(title).description(description).color(color_value);

            channel_id.send_message(&ctx.http, CreateMessage::new().embed(embed)).await.map_err(|e| {
                error!("Failed to send embed: {}", e);
                AppError::DiscordError(format!("Failed to send embed: {}", e))
            })?;
        }
        CommandResponse::None => {
            // No response needed
        }
    }

    Ok(())
}

/// Move a user to a voice channel
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `guild_id` - The guild ID
/// * `user_id` - The user to move
/// * `channel_id` - The channel to move the user to
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
pub async fn move_user_to_channel(
    ctx: &Context,
    guild_id: GuildId,
    user_id: UserId,
    channel_id: ChannelId,
) -> AppResult<()> {
    info!("Moving user {} to channel {}", user_id, channel_id);

    guild_id.move_member(&ctx.http, user_id, channel_id).await.map_err(|e| {
        error!("Failed to move user: {}", e);
        AppError::DiscordError(format!("Failed to move user: {}", e))
    })?;

    Ok(())
}

/// Get the name of a channel
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `channel_id` - The channel ID
///
/// # Returns
/// * `AppResult<String>` - The channel name or an error
pub async fn get_channel_name(
    ctx: &Context,
    channel_id: ChannelId,
) -> AppResult<String> {
    let channel = channel_id.to_channel(&ctx.http).await.map_err(|e| {
        error!("Failed to get channel: {}", e);
        AppError::DiscordError(format!("Failed to get channel: {}", e))
    })?;

    Ok(channel.guild().map_or_else(|| format!("DM Channel {}", channel_id), |c| c.name().to_string()))
}

/// Check if a user has a specific role
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `guild_id` - The guild ID
/// * `user_id` - The user to check
/// * `role_name` - The name of the role to check for
///
/// # Returns
/// * `AppResult<bool>` - Whether the user has the role
pub async fn user_has_role(
    ctx: &Context,
    guild_id: GuildId,
    user_id: UserId,
    role_name: &str,
) -> AppResult<bool> {
    let member = guild_id.member(&ctx.http, user_id).await.map_err(|e| {
        error!("Failed to get member: {}", e);
        AppError::DiscordError(format!("Failed to get member: {}", e))
    })?;

    let guild = guild_id.to_partial_guild(&ctx.http).await.map_err(|e| {
        error!("Failed to get guild: {}", e);
        AppError::DiscordError(format!("Failed to get guild: {}", e))
    })?;

    for role_id in &member.roles {
        if let Some(role) = guild.roles.get(role_id) {
            if role.name.eq_ignore_ascii_case(role_name) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}
