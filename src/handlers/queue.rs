use anyhow::Result;
use serenity::{
    all::{CommandInteraction, Context, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage},
    builder::CreateEmbedFooter,
    model::prelude::*,
};
use std::sync::Arc;
use crate::{database::Database, models::*};

pub async fn handle_queue_command(
    ctx: &Context,
    interaction: &CommandInteraction,
    db: Arc<Database>,
) -> Result<()> {
    let user_id = interaction.user.id.to_string();
    let username = interaction.user.display_name();
    
    // Get or create user
    let user = db.get_or_create_user(&user_id, &username).await?;
    
    // Check if user is already in queue
    let current_queue_count = db.get_queue_count(QueueType::Default).await?;
    let queue_players = db.get_queue_waiting(QueueType::Default).await?;
    
    let already_in_queue = queue_players.iter().any(|(_, u)| u.discord_id == user_id);
    
    if already_in_queue {
        // Leave queue
        db.leave_queue_by_user_id(user.id).await?;
        
        let embed = CreateEmbed::new()
            .title("Left Queue")
            .description(format!("**{}** left the queue", username))
            .color(0xff6b6b)
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", current_queue_count - 1)));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true)
        );
        
        interaction.create_response(&ctx.http, response).await?;
    } else {
        // Join queue
        db.join_queue(user.id, QueueType::Default).await?;
        let new_count = current_queue_count + 1;
        
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
        
        interaction.create_response(&ctx.http, response).await?;
        
        // Check if we have enough players for a match
        if new_count >= 8 {
            trigger_quota_notification(ctx, db, interaction.guild_id.unwrap()).await?;
        }
    }
    
    Ok(())
}

pub async fn handle_queue_status_command(
    ctx: &Context,
    interaction: &CommandInteraction,
    db: Arc<Database>,
) -> Result<()> {
    let queue_players = db.get_queue_waiting(QueueType::Default).await?;
    let count = queue_players.len();
    
    let description = if count == 0 {
        "Queue is empty. Use `/queue join` to join!".to_string()
    } else {
        let mut parts = vec![format!("**{} players in queue:**\n", count)];
        for (i, (_, user)) in queue_players.iter().enumerate() {
            parts.push(format!("{}. {}", i + 1, user.username));
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
    
    interaction.create_response(&ctx.http, response).await?;
    Ok(())
}

async fn trigger_quota_notification(
    ctx: &Context,
    db: Arc<Database>,
    _guild_id: GuildId,
) -> Result<()> {
    let config = db.get_config().await?;
    let queue_players = db.get_queue_waiting(QueueType::Default).await?;
    
    if queue_players.len() < 8 {
        return Ok(());
    }
    
    // Send notification to log channel
    if !config.log_channel_id.is_empty() {
        let channel_id: u64 = config.log_channel_id.parse()?;
        let channel = ChannelId::new(channel_id);
        
        let mut player_mentions = Vec::new();
        for (_, user) in &queue_players[..8] {
            let user_id: u64 = user.discord_id.parse()?;
            player_mentions.push(format!("<@{}>", user_id));
        }
        
        let embed = CreateEmbed::new()
            .title("🔔 QUOTA REACHED!")
            .description(format!(
                "**8 players ready for pickup!**\n\n{}\n\nPlayers have 2 minutes to confirm. A runner will generate teams shortly.",
                player_mentions.join(" ")
            ))
            .color(0xffd43b)
            .footer(CreateEmbedFooter::new("Waiting for team generation..."));
        
        channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await?;
    }
    
    Ok(())
}
