// Combined session handlers
use std::sync::Arc;

use anyhow::{ anyhow, Result };
use rand::rng;
use rand::seq::SliceRandom;
use serenity::all::{
    ChannelId,
    Context,
    CreateEmbed as CE,
    CreateEmbedFooter as CEF,
    CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
    EditMember,
    GuildId,
    RoleId,
    UserId,
};
use tracing::{info, warn};

use crate::database::Database;
use crate::models::command::{CommandContext};
use crate::models::player::Role;
use crate::models::session::{ Group, Session, SessionPlayer, SessionStatus, Team };

/// Checks if a user has the specified role.
///
/// * `cc` - The command context.
/// * `role` - The role to check for.
pub async fn check_role(
    cc: &CommandContext<'_>,
    role: &Role,
) -> Result<bool> {
    if let Some(guild_id) = cc.intax.guild_id {
        let member = guild_id.member(&cc.ctx.http, cc.intax.user.id).await;
        if let Ok(member) = member {
            info!("Checking if user has {} role with ID: {}", role.name(), role.id());
            return Ok(member.roles.contains(&RoleId::new(role.id())));
        } else {
            warn!("Failed to fetch member for user {} in guild {}: {:?}", cc.intax.user.id, guild_id, member.as_ref().err());
        }
    }
    Ok(false)
}

/// Splits the players into two teams.
///
/// * `players` - The players to split into teams.
pub fn split_into_teams(players: &[SessionPlayer]) -> (Vec<SessionPlayer>, Vec<SessionPlayer>) {
    let mut rng = rng();
    let mut player_list: Vec<SessionPlayer> = players.to_vec();
    player_list.shuffle(&mut rng);
    let team_size = player_list.len() / 2;
    let team1 = player_list[0..team_size].to_vec();
    let team2 = player_list[team_size..].to_vec();
    (team1, team2)
}


/// Moves players back to the queue channel.
///
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `group`      - The group containing session and queue info.
/// * `session`    - The session with players to move.
/// * `guild_id`   - The ID of the guild where the session is taking place.
async fn move_players_to_queue_channel(
    ctx: &Context,
    _db: &Arc<Database>,
    group: &Group,
    session: &Session,
    guild_id: GuildId
) -> Result<()> {
    // Check if queue channel is configured
    if group.queue_id != 0 {
        for player in &session.pool {
            let user_id = player.player.discord_id;
            // Try to move the user back to queue
            let _ = ctx.http.edit_member(
                guild_id,
                UserId::new(user_id),
                &EditMember::new().voice_channel(ChannelId::new(group.queue_id)),
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
/// * `group`      - The group containing team channel information.
/// * `session`    - The session with assigned teams.
/// * `guild_id`   - The ID of the guild where the session is taking place.
async fn move_players_to_team_channels(
    ctx: &Context,
    _db: &Arc<Database>,
    group: &Group,
    session: &Session,
    guild_id: GuildId
) -> Result<()> {
    // Get red/blue voice channel IDs from the first team in the group
    if group.teams.is_empty() {
        return Err(anyhow!("No team channels configured for this group"));
    }
    let red_vc_id = group.teams[0].red_vc_id;
    let blue_tc_id = group.teams[0].blu_vc_id;
    if red_vc_id == 0 || blue_tc_id == 0 {
        return Err(anyhow!("Voice channel IDs not configured for this group"));
    }
    let redvc = ChannelId::new(red_vc_id);
    let bluvc = ChannelId::new(blue_tc_id);

    // Move players to red/blu voice channels
    for player in &session.pool {
        if let Some(team) = &player.team {
            let target_channel = match team {
                Team::Red => redvc,
                Team::Blu => bluvc,
            };
            let user_id = player.player.discord_id;
            if let Ok(mut member) = guild_id.member(&ctx.http, user_id).await {
                let _ = member.edit(
                    &ctx.http,
                    EditMember::new().voice_channel(target_channel)
                ).await;
            }
        }
    }

    Ok(())
}

//
// Queue functions
//

/// `/join` and `/leave`
pub async fn queue<'a>(cc: &'a CommandContext<'a>) -> Result<()> {
    info!("Processing queue command from user {}", cc.intax.user.id);
    let client_id  = cc.intax.user.id.into();
    let channel_id = cc.intax.channel_id.get();
    
    // Get group of current channel
    let mut group = cc.db.get_group_by_channel(channel_id).await?;
    
    // Check if we have idle sessions
    match group.get_sessions_by_status(&SessionStatus::Idle).len() {
        0 => {
            info!("No idle sessions found, creating a new session");
            group.create_session();
        },
        1 => {
            info!("Found one existing idle session");
        },
        n => {
            return Err(anyhow!("Found more than one idle session ({}). This is unexpected. ", n));
        },
    }

    // Get player info or create a new one
    let player = match cc.db.get_user(client_id).await {
        Ok(player) => {
            info!("Found user in db!");
            player
        }
        Err(_) => {
            info!("Creating new user in db!");
            cc.db.new_user(client_id).await?
        }
    };

    // Check if player is already in session
    if group.get_player_session_status(&player)?.is_active() {
        info!("Player {} is already in a session", player.discord_id);
        return Ok(());
    }


    info!("Command processed successfully, sending response");
    Ok(())
}

/// `/status`
pub async fn status<'a>(cc: &'a CommandContext<'a>) -> Result<()> {
    info!("Processing queue status command");
    // Get active group with session
    let group = cc.db.get_group_by_channel(cc.intax.channel_id.get()).await?;

    // If group has no sessions or session pool, return empty count
    let count = if group.sessions.is_empty() {
        0
    } else {
        group.sessions.last().expect("No active session found").pool.len()
    };

    let description = if count == 0 {
        "Queue is empty. Use `/join` to join!".to_string()
    } else {
        let mut parts = vec![format!("**{} players in queue:**\n", count)];
        // Ensure we have a session and access its pool
        if let Some(session) = group.sessions.last() {
            for (i, member) in session.pool.iter().enumerate() {
                // Use discord_id as display name if needed
                let name = format!("user_{}", member.player.discord_id);
                parts.push(format!("{}.{}", i + 1, name));
            }
        }
        parts.join("\n")
    };

    let embed = CE::new()
        .title("Queue Status")
        .description(description)
        .footer(CEF::new(format!("Queue: {}/8", count)));

    let response = CIR::Message(
        CIRM::new().embed(embed).ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/shuffle`
pub async fn shuffle(cc: &CommandContext<'_>) -> Result<()> {
    info!("Processing shuffle command");
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.create_bot_reply("Only runners can shuffle teams!").await?;
        return Ok(());
    }

    // Get active group with session
    // TODO: Replace 0 with the actual queue channel ID for the group you want
    let group = cc.db.get_group_by_channel(0).await?; // <-- FIX: supply correct queue_id

    if group.sessions.is_empty() {
        cc.create_bot_reply("No active sessions.").await?;
        return Ok(());
    }

    let session = group.sessions.last().unwrap();

    if session.pool.len() < 8 {
        cc.create_bot_reply(
            &format!("Not enough players in session. Need {} more.", 8 - session.pool.len())
        ).await?;
        return Ok(());
    }

    // Collect players and split into teams (synchronous shuffle so no !Send types live across await)
    let (mut red_team, mut blu_team) = split_into_teams(&session.pool);
    let mut updated_group = group.clone();

    // Assign teams using SessionPlayer's team method
    for sp in &mut red_team {
        sp.team(Team::Red);
    }
    for sp in &mut blu_team {
        sp.team(Team::Blu);
    }

    // Update pool with new team assignments
    updated_group.sessions.last_mut().unwrap().pool.clear();
    updated_group.sessions.last_mut().unwrap().pool.extend(red_team.into_iter());
    updated_group.sessions.last_mut().unwrap().pool.extend(blu_team.into_iter());

    updated_group.sessions.last_mut().unwrap().status = SessionStatus::Hot;
    // TODO: Persist updated_group changes to DB if needed (no update_group method exists)
    // You may need to implement this in your database layer.

    let red_team_names: Vec<String> = updated_group.sessions.last().unwrap().pool.iter().filter(|sp| sp.team == Some(Team::Red)).map(|sp| format!("<@{}>", sp.player.discord_id)).collect();
    let blu_team_names: Vec<String> = updated_group.sessions.last().unwrap().pool.iter().filter(|sp| sp.team == Some(Team::Blu)).map(|sp| format!("<@{}>", sp.player.discord_id)).collect();

    let embed = CE::new()
        .title("Teams Generated!")
        .description(
            format!(
                "**Session ID:** `{}`\n\n**🔴 RED Team:**\n{}\n\n**🔵 BLU Team:**\n{}",
                stringify!(updated_group.session.last().unwrap().id),
                red_team_names.join("\n"),
                blu_team_names.join("\n")
            )
        )
        .footer(CEF::new("Use /accept to confirm teams"));

    let response = CIR::Message(
        CIRM::new().embed(embed).ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Log to channel
    let config = cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found").get()).await?;
    if config.log_tc_id != 0 {
        let channel = ChannelId::new(config.log_tc_id);

        let log_embed = CE::new()
            .title("Session Created")
            .description(
                format!(
                    "**Session:** `{}`\n**Generated by:** {}\n\n**🔴 RED:** {}\n**🔵 BLU:** {}",
                    stringify!(updated_group.sessions.last().unwrap().id),
                    cc.intax.user.display_name(),
                    red_team_names.join(", "),
                    blu_team_names.join(", ")
                )
            )
            .footer(CEF::new("Awaiting acceptance..."));

        channel.send_message(
            &cc.ctx.http,
            serenity::all::CreateMessage::new().embed(log_embed)
        ).await?;
    }

    Ok(())
}

/// `/accept`
pub async fn accept(cc: &CommandContext<'_>, session_id: &Option<String>) -> Result<()> {
    info!(
        "Processing accept command for session ID: {}",
        session_id.clone().unwrap_or("None".to_string())
    );
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.create_bot_reply("Only runners can accept sessions!").await?;
        return Ok(());
    }

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id.get();
    let mut group = cc.db.get_group_by_channel(channel_id).await?;

    match group.get_sessions_by_status(&SessionStatus::Hot).len() {
        0 => {
            cc.create_bot_reply("No hot sessions found in this group.").await?;
            return Ok(());
        },
        1 => {
            info!("Found one existing hot session");
        },
        n => {
            return Err(anyhow!("Found more than one hot session ({}). This is unexpected. ", n));
        },
    };

    let target_session = &mut group.get_sessions_by_status(&SessionStatus::Hot)[0];
    
    // Update session status to Push
    target_session.push();

    cc.create_bot_reply("Session accepted! Players moved to team channels.").await?;

    // Log acceptance
    let config = cc.db.get_config(cc.intax.guild_id.unwrap().get()).await?;
    if config.log_tc_id != 0 {
        let channel = ChannelId::new(config.log_tc_id);

        let log_embed = CE::new()
            .title("Session Accepted")
            .description(
                format!(
                    "**Session ID:** `{:?}`\n**Accepted by:** {}",
                    target_session.session_id,
                    cc.intax.user.display_name()
                )
            )
            .footer(CEF::new("Session in progress..."));

        channel.send_message(
            &cc.ctx.http,
            serenity::all::CreateMessage::new().embed(log_embed)
        ).await?;
    }

    Ok(())
}

/// `/end`
///
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
/// * `session_id`  - The ID of the session to end.
pub async fn end(cc: &CommandContext<'_>, session_id: Option<String>) -> Result<()> {
    info!(
        "Processing end command for session ID: {}",
        session_id.clone().unwrap_or("None".to_string())
    );
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.create_bot_reply("Only runners can end sessions!").await?;
        return Ok(());
    }

    // TODO: Replace 0 with the actual queue channel ID for the group you want
    let mut group = cc.db.get_group_by_channel(0).await?; // <-- FIX: supply correct queue_id

    // Find the session to end based on ID or status
    let session_index = if let Some(id) = session_id.clone() {
        // Find session with matching ID
        group.sessions.iter().position(|s| format!("{:?}", s.session_id) == id)
    } else {
        // Find session with Push status
        group.sessions.iter().position(|s| s.status == SessionStatus::Push)
    };

    if let Some(index) = session_index {
        // Set the session status to Pull (ended)
        group.sessions[index].status = SessionStatus::Pull;

        // TODO: Persist group changes to DB if needed (no update_group method exists)
        // You may need to implement this in your database layer.

        // Move players to queue channel if we're in a guild
        if let Some(guild_id) = cc.intax.guild_id {
            move_players_to_queue_channel(
                cc.ctx,
                &cc.db,
                &group,
                &group.sessions[index],
                guild_id
            ).await?;
        }
    } else {
        cc.create_bot_reply("No active session found to end.").await?;
        return Ok(());
    }

    let session_id_display = if let Some(idx) = session_index {
        format!("{:?}", group.sessions[idx].session_id)
    } else {
        "unknown".to_string()
    };

    cc.create_bot_reply(
        &format!("Session {} has been ended. Players will be moved back to queue.", session_id_display)
    ).await?;

    // Log session end
    let config = cc.db.get_config(cc.intax.guild_id.unwrap().get()).await?;
    if config.log_tc_id != 0 {
        let channel = ChannelId::new(config.log_tc_id);

        let log_embed = CE::new()
            .title("Session Ended")
            .description(
                format!(
                    "**Session ID:** `{:?}`\n**Ended by:** {}",
                    session_id_display,
                    cc.intax.user.display_name()
                )
            )
            .footer(CEF::new("Session completed"));

        channel.send_message(
            &cc.ctx.http,
            serenity::all::CreateMessage::new().embed(log_embed)
        ).await?;
    }

    Ok(())
}
