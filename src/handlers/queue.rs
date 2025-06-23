use anyhow::Result;
use serenity::{
    all::{Context, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage},
    builder::CreateEmbedFooter,
    model::prelude::*,
};
use std::sync::Arc;
use crate::{database::Database, models::*};
use tracing::info;

/// Handles the `/queue` command, which allows players to join or leave the queue.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
pub async fn handle_queue_command<'a>(
    cc:           &CommandContext<'a>,
) -> Result<()> {
    let user_id: u64 = cc.intax.user.id.into();

    // Get ctx channel
    let channel = cc.intax.channel_id;
    
    // Get the current queue
    let mut queue = cc.db.get_queue(QueueRoleGroup::Journey).await?;
    
    // Check if user is already in queue
    let already_in_queue = queue.members.iter().any(|member| member.user.discord_id == user_id);
    
    if already_in_queue {
        // Remove the user from the queue
        queue.members.retain(|member| member.user.discord_id != user_id);
        
        // Update the queue in the database
        cc.db.update_queue(&queue).await?;
        
        let embed = CreateEmbed::new()
            .title("Left Queue")
            .description(format!("**{}** left the queue", username))
            .color(0xff6b6b)
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", queue.members.len())));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;

        info!("User {} ({}) left queue", username, user_id);
    } else {
        // Create a queue member with this player
        let queue_member = queue::QueueMember {
            user: player,
            is_buffered: false,
            buffered_by: player::Player::new(0),
        };
        
        // Add the member to the queue
        queue.members.push(queue_member);
        
        // Update the queue in the database
        cc.db.update_queue(&queue).await?;
        
        let new_count = queue.members.len();
        
        let embed = CreateEmbed::new()
            .title("Joined Queue")
            .description(format!("**{}** joined the queue", username))
            .color(0x51cf66)
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", new_count)));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;

        info!("User {} ({}) joined queue", username, user_id);        
        // Check if we have enough players for a match
        if new_count >= 8 {
            trigger_quota_notification(&cc.ctx, cc.db, cc.intax.guild_id.unwrap()).await?;
        }
    }
    
    Ok(())
}

/// Handles the `/queue status` command, which shows the current queue status.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
pub async fn handle_queue_status_command<'a>(
    cc:           &CommandContext<'a>,
) -> Result<()> {
    // Get the current queue
    let queue = cc.db.get_queue(QueueRoleGroup::Journey).await?;
    let count = queue.members.len();
    
    let description = if count == 0 {
        "Queue is empty. Use `/queue join` to join!".to_string()
    } else {
        let mut parts = vec![format!("**{} players in queue:**\n", count)];
        for (i, member) in queue.members.iter().enumerate() {
            // Use discord_id as display name if needed
            let name = format!("user_{}", member.user.discord_id);
            parts.push(format!("{}.{}", i + 1, name));
        }
        parts.join("\n")
    };
    
    let embed = CreateEmbed::new()
        .title("Queue Status")
        .description(description)
        .color(0x339af0)
        .footer(CreateEmbedFooter::new(format!("Queue: {}/8", count)));
    
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .ephemeral(true)
    );
    
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// Triggers a notification when the queue quota is reached.
/// 
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `_guild_id`  - The ID of the guild where the command was issued.
async fn trigger_quota_notification(
    ctx:       &Context,
    db:        Arc<Database>,
    _guild_id: GuildId,
) -> Result<()> {
    let config = db.get_config().await?;
    let queue = db.get_queue(QueueRoleGroup::Journey).await?;
    
    if queue.members.len() < 8 {
        return Ok(());
    }
    
    // Send notification to log channel
    if config.log_channel_id != 0 {
        let channel = ChannelId::new(config.log_channel_id);
        
        let mut player_mentions = Vec::new();
        for member in &queue.members[..8] {
            player_mentions.push(format!("<@{}>", member.user.discord_id));
        }
        
        let embed = CreateEmbed::new()
            .title("🔔 QUOTA REACHED!")
            .description(format!(
                "**8 players ready for pickup!**\n\n{}\n\nPlayers have 2 minutes to confirm. A runner will generate teams shortly.",
                player_mentions.join(" ")
            ))
            .color(0xffd43b)
            .footer(CreateEmbedFooter::new("Awaiting team generation..."));
        
        channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await?;
    }
    
    Ok(())
}
