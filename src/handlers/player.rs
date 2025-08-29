// Combined session handlers
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use serenity::all::{
    Context,
    CreateEmbed as CE,
    CreateEmbedFooter as CEF,
    CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
    EditMember,
    GuildId,
};
use tracing::{info, warn, error};
use crate::models::command::{CommandContext};
use crate::models::player::Role;
use crate::models::data::{ Group, Session, SessionPlayer, SessionStatus, Team };
use crate::Database;

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
            return Ok(member.roles.contains(&role.id()));
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
    let mut rng = rand::thread_rng();
    let mut player_list: Vec<SessionPlayer> = players.to_vec();
    player_list.shuffle(&mut rng);
    let team_size = player_list.len() / 2;
    let team1 = player_list[0..team_size].to_vec();
    let team2 = player_list[team_size..].to_vec();
    (team1, team2)
}


/// Moves players back to the queue channel.
async fn move_players_to_queue_channel(session: Session, group: Group, guild_id: GuildId, ctx: &Context) -> Result<()> {
    // Check if queue channel is configured
    if group.channels.queue_vc != 0 {
        for player in &session.pool {
            // Try to move the user back to queue
            let _ = ctx.http.edit_member(
                guild_id,
                player.player.discord_id,
                &EditMember::new().voice_channel(group.channels.queue_vc),
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
    group: Group,
    session: &mut Session,
    guild_id: GuildId
) -> Result<()> {
    // Get red/blue voice channel IDs from the first team in the group
    if group.channels.teams.is_empty() {
        return Err(anyhow!("No team channels configured for this group"));
    }
    let red_vc = group.channels.teams[0].red_vc;
    let blu_vc = group.channels.teams[0].blu_vc;
    if red_vc == 0 || blu_vc == 0 {
        return Err(anyhow!("Voice channel IDs not configured for this group"));
    }

    // Move players to red/blu voice channels
    for player in &session.pool {
        if let Some(team) = &player.team {
            let target_channel = match team {
                Team::Red => red_vc,
                Team::Blu => blu_vc,
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
    let user = cc.intax.user.id;
    let channel = cc.intax.channel_id;
    let command_name = &cc.intax.data.name;
    
    // Get group from database as base configuration
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    // Handle leave command
    if command_name == "leave" {
        let mut found = false;
        let mut queue_count = 0;
        
        // Scope the manager lock to avoid Send issues
        {
            let mut manager = match cc.manager.lock() {
                Ok(manager) => manager,
                Err(poisoned) => {
                    error!("Manager mutex poisoned, recovering: {}", poisoned);
                    poisoned.into_inner()
                }
            };
            let group = manager.get_or_create_group(channel, base_group.clone());
            
            // Find and remove player from any session
            for session in &mut group.sessions {
                if session.status == SessionStatus::Idle {
                    let initial_len = session.pool.len();
                    session.pool.retain(|p| p.player.discord_id != user);
                    if session.pool.len() < initial_len {
                        found = true;
                        queue_count = session.pool.len();
                        info!("Removed player from session. Queue now has {} players", queue_count);
                        break;
                    }
                }
            }
        } // Manager lock is dropped here
        
        if found {
            cc.create_bot_reply(&format!("❌ Left the queue! ({}/12 players)", queue_count)).await?;
        } else {
            cc.create_bot_reply("You are not in the queue!").await?;
        }
        
        return Ok(());
    }
    
    // Handle join command
    // Get player info or create a new one
    let player = match cc.db.get_user(user).await {
        Ok(player) => {
            info!("Found user in db!");
            player
        }
        Err(_) => {
            info!("Creating new user in db!");
            cc.db.new_user(user).await?
        }
    };

    let mut queue_count = 0;
    let mut already_in_queue = false;
    
    // Scope the manager lock to avoid Send issues
    {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
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

        // Check if player is already in session
        if group.get_user_session(user).is_some() {
            info!("Player {} is already in a session", player.discord_id);
            already_in_queue = true;
        } else {
            // Add player to the session
            if let Some(session) = group.sessions.last_mut() {
                if session.status == SessionStatus::Idle {
                    session.pool.push(SessionPlayer::construct(player));
                    queue_count = session.pool.len();
                    info!("Added player to session. Queue now has {} players", queue_count);
                }
            }
        }
    } // Manager lock is dropped here
    
    if already_in_queue {
        cc.create_bot_reply("You are already in the queue!").await?;
    } else {
        cc.create_bot_reply(&format!("✅ Joined the queue! ({}/12 players)", queue_count)).await?;
    }

    info!("Command processed successfully, sending response");
    Ok(())
}

/// `/status`
pub async fn status<'a>(cc: &'a CommandContext<'a>) -> Result<()> {
    info!("Processing queue status command");
    let channel = cc.intax.channel_id;
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    let (queue_count, queue_list) = {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
        let idle_sessions = group.get_sessions_by_status(&SessionStatus::Idle);
        
        if idle_sessions.is_empty() {
            (0, "No active queue found.".to_string())
        } else {
            let session = &idle_sessions[0];
            let count = session.pool.len();
            let list = if count > 0 {
                session.pool.iter()
                    .enumerate()
                    .map(|(i, p)| format!("{}. <@{}>", i + 1, p.player.discord_id))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                "Queue is empty".to_string()
            };
            (count, list)
        }
    }; // Manager lock is dropped here
    
    if queue_count == 0 && queue_list == "No active queue found." {
        cc.create_bot_reply("No active queue found.").await?;
    } else {
        let status_message = format!("**Queue Status ({}/12 players)**\n{}", queue_count, queue_list);
        cc.create_bot_reply(&status_message).await?;
    }
    
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
    let group = cc.db.get_group_by_channel(cc.intax.channel_id).await?;

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

    let embed_content = format!(
        "**🎲 Teams Generated!**\n\n**🔴 Red Team:**\n{}\n\n**🔵 Blue Team:**\n{}",
        red_team_names.join("\n"),
        blu_team_names.join("\n")
    );
    
    cc.create_bot_reply(&embed_content).await?;
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
    let channel_id = cc.intax.channel_id;
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
    target_session.status = SessionStatus::Push;

    cc.create_bot_reply("Session accepted! Players moved to team channels.").await?;


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

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id;
    let mut group = cc.db.get_group_by_channel(channel_id).await?;

    if let Some(mut session) = group.get_user_session(cc.intax.user.id) {
        session.status = SessionStatus::Pull;

        // TODO: Persist group changes to DB if needed (no update_group method exists)
        // You may need to implement this in your database layer.

        // Move players to queue channel if we're in a guild
        if let Some(guild_id) = cc.intax.guild_id {
            move_players_to_queue_channel(
                session.clone(),
                group.clone(),
                guild_id,
                cc.ctx
            ).await?;
        }
        
        cc.create_bot_reply("Session has been ended. Players will be moved back to queue.").await?;
    } else {
        cc.create_bot_reply("No active session found to end.").await?;
    }

    Ok(())
}
