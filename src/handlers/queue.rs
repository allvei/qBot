// CHECK ME
use std::sync::Arc;

use anyhow::Result;
use serenity::all::{ChannelId, Context, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage};
use serenity::builder::CreateEmbedFooter;
use tracing::info;

use crate::database::Database;
use crate::models::command::CommandContext;
use crate::models::group::Group;

/// Handles the `/join` and `/leave` commands, which allow players to join or leave the queue.
///
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
pub async fn queue<'a>(cc: &'a CommandContext<'a>) -> Result<()> {
    info!("Processing queue command from user {}", cc.intax.user.id);
    let client: u64 = cc.intax.user.id.into();
    let _channel = cc.intax.channel_id;
    info!("Channel ID: {}", cc.intax.channel_id);

    info!("Getting active group with channel ID {}", cc.intax.channel_id.get());
    // Get active group with session
    let mut group = cc.db.get_group(cc.intax.channel_id.get()).await?;

    // Ensure group has at least one session
    if group.session.is_empty() {
        info!("No active sessions found, creating a new session");
        group.create_session();
    } else {
        info!("Found existing sessions: {}", group.session.len());
    }

    // Get player info - try to get user or create if not exists
    let player = match cc.db.get_user(client).await {
        Ok(user) => {
            info!("Found user in db!");
            user
        }
        Err(_) => {
            info!("Creating new user in db!");
            cc.db.new_user(client).await?
        }
    };

    // Check if player is already in session
    // Get the last (active) session
    let session = group.session.last_mut().expect("No active session found");
    if session.pool.iter().any(|sp| sp.discord_id == client) {
        // Remove a player from the session
        let session = group.session.last_mut().expect("No active session found");
        info!("Removing player {} from session", player.discord_id);
        session.pool.retain(|sp| sp.discord_id != client);

        let embed = CreateEmbed::new()
            .title("Left Queue")
            .description(format!("**{}** left the queue", cc.intax.user.name))
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", session.pool.len())));

        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;

        info!("User {} ({}) left queue", cc.intax.user.name, client);
    } else {
        // Add player to session
        let session = group.session.last_mut().expect("No active session found");
        match session.add_player(&player) {
            Ok(_) => info!("Player {} added to session", player.discord_id),
            Err(e) => {
                info!("Failed to add player to session: {}", e);
                let embed = CreateEmbed::new().title("Queue Error").description(format!("Failed to join queue: {}", e)).color(0xFF0000);

                let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).ephemeral(true));

                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        }

        let embed = CreateEmbed::new()
            .title("Joined Queue")
            .description(format!("**{}** joined the queue", cc.intax.user.name))
            .footer(CreateEmbedFooter::new(format!("Queue: {}/8", session.pool.len())));

        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;

        info!("User {} ({}) joined queue", cc.intax.user.name, client);

        // Check if session is full
        if session.pool.len() >= 8 {
            info!("Session is now full with {} players", session.pool.len());
            notify_session_ready(cc.ctx, &cc.db, &group).await?;
        }
    }

    info!("Command processed successfully, sending response");
    Ok(())
}

/// Handles the `/status` command, which shows the current queue status.
///
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
pub async fn status<'a>(cc: &'a CommandContext<'a>) -> Result<()> {
    info!("Processing queue status command");
    // Get active group with session
    // TODO: Replace hardcoded 0 with the actual queue channel ID
    let group = cc.db.get_group(cc.intax.channel_id.get()).await?;

    // If group has no sessions or session pool, return empty count
    let count = if group.session.is_empty() {
        0
    } else {
        group.session.last().expect("No active session found").pool.len()
    };

    let description = if count == 0 {
        "Queue is empty. Use `/join` to join!".to_string()
    } else {
        let mut parts = vec![format!("**{} players in queue:**\n", count)];
        // Ensure we have a session and access its pool
        if let Some(session) = group.session.last() {
            for (i, member) in session.pool.iter().enumerate() {
                // Use discord_id as display name if needed
                let name = format!("user_{}", member.discord_id);
                parts.push(format!("{}.{}", i + 1, name));
            }
        }
        parts.join("\n")
    };

    let embed = CreateEmbed::new()
        .title("Queue Status")
        .description(description)
        .footer(CreateEmbedFooter::new(format!("Queue: {}/8", count)));

    let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).ephemeral(true));

    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// Notify session ready when the queue quota is reached.
///
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `_guild_id`  - The ID of the guild where the command was issued.
async fn notify_session_ready(
    ctx: &Context,
    db: &Arc<Database>,
    group: &Group,
) -> Result<()> {
    // Use the guild_id from the group
    let config = db.get_config(Some(group.guild_id)).await?;

    // Send notification to log channel
    if config.ic_log != 0 {
        let channel = ChannelId::new(config.ic_log);

        let mut player_mentions = Vec::new();
        // Ensure we have a session before accessing its pool
        if let Some(session) = group.session.last() {
            let pool_len = session.pool.len().min(8); // Take at most 8 players
            for member in &session.pool[..pool_len] {
                player_mentions.push(format!("<@{}>", member.discord_id));
            }
        }

        let embed = CreateEmbed::new()
            .title("QUOTA REACHED!")
            .description(format!(
                "**8 players ready for pickup!**\n\n{}\n\nPlayers have 2 minutes to confirm. A runner will generate teams shortly.",
                player_mentions.join(" ")
            ))
            .footer(CreateEmbedFooter::new("Awaiting team generation..."));

        channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await?;
    }

    Ok(())
}
