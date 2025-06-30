// CHECK ME
use serenity::{
    all::{
        CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage,
        Context, ChannelId, UserId, RoleId, EditMember, GuildId
    },
};
use serenity::builder::CreateEmbedFooter;
use anyhow::{Result, anyhow};
use crate::{database::Database, models::{player::Player as DbUser, session::{SessionPlayer, SessionStatus, Group, Team}, command::CommandContext}};
use rand::seq::SliceRandom;
use rand::rng;
use std::sync::Arc;

/// Checks if the user has runner permissions.
/// 
/// * `cc` - Ref to the command context.
async fn check_runner_permissions(cc: &CommandContext<'_>) -> Result<bool> {
    let config = cc.db.get_config().await?;
    
    // If no runner roles configured, allow everyone
    if config.id_runner == 0 && config.id_admin == 0 {
        return Ok(true);
    }
    
    if let Some(guild_id) = cc.intax.guild_id {
        if let Ok(member) = guild_id.member(&cc.ctx.http, cc.intax.user.id).await {
            // Check admin or runner role
            let is_admin = member.roles.contains(&RoleId::new(config.id_admin));
            let is_runner = member.roles.contains(&RoleId::new(config.id_runner));
            if is_admin || is_runner {
                return Ok(true);
            }
        }
    }
    
    Ok(false)
}

/// Splits the players into two teams.
/// 
/// * `players` - The players to split into teams.
pub fn split_into_teams(players: &[SessionPlayer]) -> (Vec<DbUser>, Vec<DbUser>) {
    let mut rng = rng();
    let mut player_list: Vec<DbUser> = players.iter().map(|sp| sp.player.clone()).collect();
    player_list.shuffle(&mut rng);
    
    let team_size = player_list.len() / 2;
    let team1 = player_list[0..team_size].to_vec();
    let team2 = player_list[team_size..].to_vec();
    (team1, team2)
}

/// Handles the `/shuffle` command, which shuffles the queue.
pub async fn handle_shuffle_command(
    cc:    &CommandContext<'_>,
) -> Result<()> {
    // Check permissions
    if !check_runner_permissions(cc).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You don't have permission to use this command.")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Get active group with session
    let group = cc.db.get_group().await?;
    
    if group.session.players.len() < 8 {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(format!("❌ Not enough players in session. Need 8, have {}.", group.session.players.len()))
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Collect players and split into teams (synchronous shuffle so no !Send types live across await)
    let all_players: Vec<DbUser> = group.session.players.iter().map(|sp| sp.player.clone()).collect();
    let (red_team, blu_team) = split_into_teams(&group.session.players);
    
    // Update session with team assignments
    let mut updated_group = group.clone();
    
    // Assign teams to players
    for player in &red_team {
        if let Some(pos) = updated_group.session.players.iter().position(|sp| sp.player.discord_id == player.discord_id) {
            updated_group.session.players[pos].team = Some(Team::Red);
        }
    }
    
    for player in &blu_team {
        if let Some(pos) = updated_group.session.players.iter().position(|sp| sp.player.discord_id == player.discord_id) {
            updated_group.session.players[pos].team = Some(Team::Blu);
        }
    }
    
    // Update the session status
    updated_group.session.status = SessionStatus::Hot;
    
    // Create session and update database
    cc.db.update_group(&updated_group).await?;
    
    let red_team_names: Vec<String> = red_team.iter().map(|u| format!("<@{}>", u.discord_id)).collect();
    let blu_team_names: Vec<String> = blu_team.iter().map(|u| format!("<@{}>", u.discord_id)).collect();
    
    let embed = CreateEmbed::new()
        .title("🎲 Teams Generated!")
        .description(format!(
            "**Session ID:** `{}`\n\n**🔴 RED Team:**\n{}\n\n**🔵 BLU Team:**\n{}",
            stringify!(updated_group.session.id),
            red_team_names.join("\n"),
            blu_team_names.join("\n")
        ))
        .color(0x51cf66)
        .footer(CreateEmbedFooter::new("Use /accept to confirm teams"));
    
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .ephemeral(true)
    );
    
    cc.intax.create_response(&cc.ctx.http, response).await?;
    
    // Log to channel
    let config = cc.db.get_config().await?;
    if config.cid_log != 0 {
        let channel = ChannelId::new(config.cid_log);
        
        let log_embed = CreateEmbed::new()
            .title("📋 Session Created")
            .description(format!(
                "**Session:** `{}`\n**Generated by:** {}\n\n**🔴 RED:** {}\n**🔵 BLU:** {}",
                stringify!(updated_group.session.id),
                cc.intax.user.display_name(),
                red_team_names.join(", "),
                blu_team_names.join(", ")
            ))
            .color(0x339af0)
            .footer(CreateEmbedFooter::new("Awaiting acceptance..."));
        
        channel.send_message(&cc.ctx.http, serenity::all::CreateMessage::new().embed(log_embed)).await?;
    }
    
    Ok(())
}

/// Handles the `/accept` command, which accepts a session.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
/// * `session_id`  - The ID of the session to accept.
pub async fn handle_accept_command(
    cc:    &CommandContext<'_>,
    session_id: Option<String>,
) -> Result<()> {
    // Check permissions
    if !check_runner_permissions(cc).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You don't have permission to use this command.")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Get the group with active session
    let mut group = cc.db.get_group().await?;
    
    // If a specific session ID was provided, ensure it matches
    if let Some(id) = session_id {
        // Convert both ids to strings for comparison
        let session_id_str = format!("{:?}", group.session.id);
        if session_id_str != id {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!("❌ Session with ID {} not found.", id))
                    .ephemeral(true)
            );
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    }

    // Update session status to Push
    group.session.status = SessionStatus::Push;
    cc.db.update_group(&group).await?;
    
    // Move players to team channels
    move_players_to_team_channels(cc.ctx, &cc.db, &group, cc.intax.guild_id.unwrap()).await?;

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content("✅ Session accepted! Players moved to team channels.")
            .ephemeral(true)
    );
    
    cc.intax.create_response(&cc.ctx.http, response).await?;
    
    // Log acceptance
    let config = cc.db.get_config().await?;
    if config.cid_log != 0 {
        let channel = ChannelId::new(config.cid_log);
        
        let log_embed = CreateEmbed::new()
            .title("✅ Session Accepted")
            .description(format!(
                "**Session ID:** `{:?}`\n**Accepted by:** {}",
                group.session.id,
                cc.intax.user.display_name()
            ))
            .color(0x51cf66)
            .footer(CreateEmbedFooter::new("Session in progress..."));
        
        channel.send_message(&cc.ctx.http, serenity::all::CreateMessage::new().embed(log_embed)).await?;
    }
    
    Ok(())
}

/// Handles the `/end` command, which ends a session.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
/// * `session_id`  - The ID of the session to end.
pub async fn handle_end_command(
    cc:    &CommandContext<'_>,
    session_id: Option<String>,
) -> Result<()> {
    // Check permissions
    if !check_runner_permissions(cc).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You don't have permission to use this command.")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Find the session to end
    let session = if let Some(id) = session_id {
        cc.db.get_session_by_uuid(&id).await?
    } else {
        cc.db.get_latest_push_session().await?
    };

    // End the session
    let session_id_str = format!("{:?}", session.id);
    cc.db.end_session(session_id_str.clone()).await?;
    
    // Parse session ID for webhook
    let session_id_int = session_id_str.parse::<i64>().unwrap_or_default();
    move_players_to_queue_channel(cc.ctx, &cc.db, session_id_int, cc.intax.guild_id.unwrap()).await?;

    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(format!("🏁 Session `{:?}` ended! Players moved back to queue.", session.id))
            .ephemeral(true)
    );
    
    cc.intax.create_response(&cc.ctx.http, response).await?;
    
    // Log session end
    let config = cc.db.get_config().await?;
    if config.cid_log != 0 {
        let channel = ChannelId::new(config.cid_log);
        
        let log_embed = CreateEmbed::new()
            .title("🏁 Session Ended")
            .description(format!(
                "**Session ID:** `{:?}`\n**Ended by:** {}",
                session.id,
                cc.intax.user.display_name()
            ))
            .color(0xff6b6b)
            .footer(CreateEmbedFooter::new("Session completed"));
        
        channel.send_message(&cc.ctx.http, serenity::all::CreateMessage::new().embed(log_embed)).await?;
    }
    
    Ok(())
}

/// Moves players back to the queue channel.
/// 
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `session_id` - The ID of the session.
/// * `guild_id`   - The ID of the guild where the session is taking place.
async fn move_players_to_queue_channel(
    ctx: &Context,
    db: &Database,
    session_id: i64,
    guild_id: serenity::model::id::GuildId,
) -> Result<()> {
    let config = db.pull().await?;
    // Convert session_id to string for database call
    let session_id_str = session_id.to_string();
    let players = db.get_session_players(&session_id_str).await?;
    
    if config.cid_queue != 0 {
        for player in players {
            let user_id = player.player.discord_id;
            // Try to move the user back to queue
            let _ = ctx.http.edit_member(
                guild_id,
                UserId::new(user_id),
                &EditMember::new().voice_channel(ChannelId::new(config.cid_queue)),
                Some("Moving player back to queue voice channel")
            ).await;
        }
    }

    
    Ok(())
}

/// Moves players to their respective team channels.
/// 
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `group`      - The group with the session.
/// * `guild_id`   - The ID of the guild where the session is taking place.
async fn move_players_to_team_channels(
    ctx: &Context,
    db: &Arc<Database>,
    group: &Group,
    guild_id: GuildId,
) -> Result<()> {
    // Get config
    let config = db.get_config().await?;
    
    // Get red/blue voice channel IDs from the first team in the group
    if group.teams.is_empty() {
        return Err(anyhow!("No team channels configured for this group"));
    }
    
    let red_channel_id = group.teams[0].red;
    let blue_channel_id = group.teams[0].blu;
    
    if red_channel_id == 0 || blue_channel_id == 0 {
        return Err(anyhow!("Voice channel IDs not configured for this group"));
    }
    
    let redvc = ChannelId::new(red_channel_id);
    let bluvc = ChannelId::new(blue_channel_id);
    let bluvc = ChannelId::new(blue_channel_id);
    
    // Move players to red/blu voice channels
    for player in &group.session.players {
        // Move player to their team's voice channel
        if let Some(team) = &player.team {
            let target_channel = match team {
                Team::Red => redvc,
                Team::Blu => bluvc,
            };
            
            // Move member to channel
            let member = guild_id.member(&ctx.http, player.player.discord_id).await;
            if let Ok(mut member) = member {
                // Attempt to move member
                let _ = member.edit(&ctx.http, EditMember::new().voice_channel(target_channel)).await;
            }
        }
    }
    
    Ok(())
}

