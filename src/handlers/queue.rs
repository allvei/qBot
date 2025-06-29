use anyhow::Result;
use serenity::{
    all::{Context, CreateInteractionResponse, CreateInteractionResponseMessage, GuildId, Interaction, ChannelId, CreateEmbed, CreateMessage},
    builder::CreateEmbedFooter,
    model::prelude::*,
};
use std::sync::Arc;
use crate::{
    database::Database,
    models::{session::{Group, SessionStatus, Session, SessionPlayer}, player::Player as DbUser, command::CommandContext},
};
use tracing::info;

/// Handles the `/queue` command, which allows players to join or leave the queue.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
pub async fn handle_queue_command<'a>(
    cc:           &'a CommandContext<'a>,
) -> Result<()> {
    let client: u64 = cc.intax.user.id.into();
    let channel   = cc.intax.channel_id;
    
    // Get active group with session
    let mut group = cc.db.get_group_idle().await?;
    
    // Get player info
    let player = cc.db.get_or_create_player(client).await?;
    
    // Check if player is already in session
    if group.session.players.iter().any(|sp| sp.player.discord_id == client) {
        /// Remove a player from the session
        group.session.remove_member(client);
        cc.db.update_group(&group).await?;
        
        let embed = CreateEmbed::new()
            .title("Left Queue")
            .description(format!("**{}** left the queue", cc.intax.user.name))
            .color(0xff6b6b)
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", group.session.players.len())));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;

        info!("User {} ({}) left queue", cc.intax.user.name, client);
    } else {
        // Add player to session
        group.session.add_player(player);
        cc.db.update_group(&group).await?;
        
        let embed = CreateEmbed::new()
            .title("Joined Queue")
            .description(format!("**{}** joined the queue", cc.intax.user.name))
            .color(0x51cf66)
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", group.session.players.len())));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;

        info!("User {} ({}) joined queue", cc.intax.user.name, client);
        
        // Check if session is full
        if group.session.players.len() >= 8 {
            notify_session_ready(&cc.ctx, &cc.db, &group).await?;
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
    cc:           &'a CommandContext<'a>,
) -> Result<()> {
    // Get active group with session
    let group = cc.db.get_group_idle().await?;
    let count = group.session.players.len();
    
    let description = if count == 0 {
        "Queue is empty. Use `/queue join` to join!".to_string()
    } else {
        let mut parts = vec![format!("**{} players in queue:**\n", count)];
        for (i, member) in group.session.players.iter().enumerate() {
            // Use discord_id as display name if needed
            let name = format!("user_{}", member.player.discord_id);
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

/// Notify session ready when the queue quota is reached.
/// 
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `_guild_id`  - The ID of the guild where the command was issued.
async fn notify_session_ready(
    ctx:       &Context,
    db:        &Arc<Database>,
    group:     &Group,
) -> Result<()> {
    let config = db.get_config().await?;
    
    // Send notification to log channel
    if config.cid_log != 0 {
        let channel = ChannelId::new(config.cid_log);
        
        let mut player_mentions = Vec::new();
        for member in &group.session.players[..8] {
            player_mentions.push(format!("<@{}>", member.player.discord_id));
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
