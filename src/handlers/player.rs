// Combined game handlers
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use serenity::all::{Context, EditMember, GuildId};

use tracing::{info, warn};

use crate::Database;
use crate::models::{
    CommandContext, SessionPlayer, Group, Rank, Role, Roles, Server, Session, SessionStatus, Team,
};

/// Get player's rank from their Discord roles
pub async fn get_player_rank(
    ctx: &Context,
    guild_id: GuildId,
    user_id: serenity::all::UserId,
) -> Option<Rank> {
    if let Ok(member) = guild_id.member(&ctx.http, user_id).await {
        // Check all member roles and find the first matching rank
        for role_id in &member.roles {
            if let Some(rank) = Rank::from_role_id(*role_id) {
                return Some(rank);
            }
        }
    }
    None
}

/// Validate that the server has rank roles configured
pub async fn validate_rank_roles(
    ctx: &Context,
    guild_id: GuildId,
) -> Result<Vec<String>> {
    let mut missing_roles = Vec::new();
    
    // Get all guild roles
    let guild_roles = match ctx.http.get_guild_roles(guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            warn!("Failed to fetch guild roles: {}", e);
            return Err(anyhow!("Failed to fetch guild roles"));
        }
    };
    
    let guild_role_ids: Vec<_> = guild_roles.iter().map(|r| r.id).collect();
    
    // Check if all rank roles exist in the guild
    for rank_role_id in Rank::all_role_ids() {
        if !guild_role_ids.contains(&rank_role_id) {
            if let Some(rank) = Rank::from_role_id(rank_role_id) {
                missing_roles.push(rank.name().to_string());
            }
        }
    }
    
    Ok(missing_roles)
}

/// Create missing rank roles in the guild
pub async fn create_rank_roles(
    ctx: &Context,
    guild_id: GuildId,
) -> Result<Vec<String>> {
    use serenity::all::Colour;
    use serenity::builder::EditRole;
    
    let mut created_roles = Vec::new();
    
    // Get all guild roles to check which are missing
    let guild_roles = ctx.http.get_guild_roles(guild_id).await?;
    let guild_role_ids: Vec<_> = guild_roles.iter().map(|r| r.id).collect();
    
    // Create missing rank roles
    for rank_role_id in Rank::all_role_ids() {
        if !guild_role_ids.contains(&rank_role_id) {
            if let Some(rank) = Rank::from_role_id(rank_role_id) {
                // Define colors for each rank
                let color = match rank {
                    Rank::Beginner    => Colour::from_rgb(150, 150, 150), // Gray
                    Rank::Novice      => Colour::from_rgb(139, 195, 74),  // Light Green
                    Rank::Apprentice  => Colour::from_rgb(76, 175, 80),   // Green
                    Rank::Journeyman  => Colour::from_rgb(33, 150, 243),  // Blue
                    Rank::Master      => Colour::from_rgb(156, 39, 176),  // Purple
                    Rank::MasterElite => Colour::from_rgb(233, 30, 99),   // Pink
                    Rank::Grandmaster => Colour::from_rgb(255, 215, 0),   // Gold
                };
                
                let role_builder = EditRole::new()
                    .name(rank.name())
                    .colour(color)
                    .hoist(false)
                    .mentionable(true);
                
                match guild_id.create_role(&ctx.http, role_builder).await {
                    Ok(created_role) => {
                        info!("Created rank role: {} (ID: {})", rank.name(), created_role.id);
                        created_roles.push(rank.name().to_string());
                    },
                    Err(e) => {
                        warn!("Failed to create role {}: {}", rank.name(), e);
                    }
                }
            }
        }
    }
    
    Ok(created_roles)
}

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
    let mut rng = rand::rng();
    let mut player_list: Vec<SessionPlayer> = players.to_vec();
    player_list.shuffle(&mut rng);
    let team_size = player_list.len() / 2;
    let team1 = player_list[0..team_size].to_vec();
    let team2 = player_list[team_size..].to_vec();
    (team1, team2)
}


/// Moves players back to the queue channel.
async fn move_players_to_queue_channel(game: Session, group: Group, guild_id: GuildId, ctx: &Context) -> Result<()> {
    // Check if queue channel is configured
    if group.channels.queue_vc != 0 {
        for player in &game.pool {
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
/// * `ctx`      - Ref to the Serenity context.
/// * `db`       - Ref to the database.
/// * `group`    - The group containing team channel information.
/// * `game`     - The game with assigned teams.
/// * `guild_id` - The ID of the guild where the game is taking place.
async fn move_players_to_team_channels(
    ctx:      &Context,
    _db:      &Arc<Database>,
    group:    Group,
    game:     &mut Session,
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
    for player in &game.pool {
        if let Some(team) = &player.team {
            let target_channel = match team {
                Team::Unassigned => continue,
                Team::Red        => red_vc,
                Team::Blu        => blu_vc,
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
pub async fn queue<'a>(cc: &'a CommandContext<'a>, guild: &mut Server) -> Result<()> {
    info!("Processing queue command from user {}", cc.intax.user.id);
    let user         = cc.intax.user.id;
    let channel      = cc.intax.channel_id;
    let command_name = &cc.intax.data.name;
    
    // Handle leave command
    if command_name == "leave" {
        let mut found = false;
        let mut queue_count = 0;
        
        let group = guild.get_group(channel)?;
        
        // Find and remove player from any game
        for game in &mut group.sessions {
            if game.status == SessionStatus::Idle {
                let initial_len = game.pool.len();
                game.pool.retain(|p| p.player.discord_id != user);
                if game.pool.len() < initial_len {
                    found = true;
                    queue_count = game.pool.len();
                    info!("Removed player from game. Queue now has {} players", queue_count);
                    break;
                }
            }
        }
        
        if found {
            cc.reply(&format!("❌ Left the queue! ({}/{} players)", queue_count, group.quota)).await?;
        }
        
        group.dash_update(cc.ctx).await?;
        
        return Ok(());
    }
    
    // Handle join command
    // Validate player has a rank
    let guild_id = match cc.intax.guild_id {
        Some(id) => id,
        None => {
            cc.reply("❌ This command can only be used in a server.").await?;
            return Ok(());
        }
    };
    
    let rank = match get_player_rank(cc.ctx, guild_id, user).await {
        Some(rank) => rank,
        None => {
            cc.reply("❌ You must have a rank role assigned before joining the queue. Please contact an admin.").await?;
            return Ok(());
        }
    };
    
    info!("Player {} has rank: {:?}", user, rank);

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

    let group = guild.get_group(channel).unwrap();
    
    // Check if we have an idle session available
    match group.get_sessions_by_status(&SessionStatus::Idle).len() {
        0 => {
            cc.reply("❌ No queue available. A game is currently in progress.").await?;
            return Ok(());
        },
        1 => {
            info!("Found one existing idle session");
        },
        n => {
            return Err(anyhow!("Found more than one idle game ({}). This is unexpected. ", n));
        },
    }

    // Check if player is already in game
    if group.get_user_session(user).await.is_ok() {
        info!("Player {} is already in the queue", player.discord_id);
    } else {
        // Pass the rank to queue_player
        let mut manager = cc.manager.lock().await;
        let server = manager.get_server(guild_id)?;
        let group = server.get_group(channel)?;
        let queue = group.get_queue().await?;
        queue.add_player(player.discord_id, rank);
        
        // Check if we should hot the game
        if group.is_quota() {
            group.hot(cc.ctx).await;
        }
        
        group.dash_update(cc.ctx).await?;
    }
    
    // Always acknowledge (silently if already in queue)
    let current_queue = group.get_queue().await.unwrap().pool.len();
    cc.reply(&format!("✅ Joined the queue! ({}/{} players)", current_queue, group.quota)).await?;

    // Update dashboard
    group.dash_update(cc.ctx).await?;
    
    info!("Command processed successfully, sending response");
    Ok(())
}

/// `/status`
pub async fn status<'a>(cc: &'a CommandContext<'a>, guild: &mut Server) -> Result<()> {
    info!("Processing queue status command");
    let channel = cc.intax.channel_id;
    
    let (queue_count, queue_list, quota) = {
        let group = guild.get_group(channel).unwrap();
        
        let idle_games = group.get_sessions_by_status(&SessionStatus::Idle);
        
        if idle_games.is_empty() {
            (0, "No active queue found.".to_string(), group.quota)
        } else {
            let game = &idle_games[0];
            let count = game.pool.len();
            let list = if count > 0 {
                game.pool.iter()
                    .enumerate()
                    .map(|(i, p)| format!("{}. <@{}>", i + 1, p.player.discord_id))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                "Queue is empty".to_string()
            };
            (count, list, group.quota)
        }
    }; // Manager lock is dropped here
    
    if queue_count == 0 && queue_list == "No active queue found." {
        cc.reply("No active queue found.").await?;
    } else {
        cc.reply(&format!("**Queue Status ({}/{} players)**\n{}", queue_count, quota, queue_list)).await?;
    }
    
    Ok(())
}

/// `/shuffle`
pub async fn shuffle(cc: &CommandContext<'_>, guild: &mut Server) -> Result<()> {
    info!("Processing shuffle command");
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.reply("Only runners can shuffle teams!").await?;
        return Ok(());
    }

    // Get active group with game
    let group = guild.get_group(cc.intax.channel_id).unwrap();
    let quota = group.quota as usize;

    if group.sessions.is_empty() {
        cc.reply("No active games.").await?;
        return Ok(());
    }

    let game = group.sessions.last().unwrap();

    if game.pool.len() < quota {
        cc.reply(&format!("Not enough players in game. Need {} more.", quota - game.pool.len())).await?;
        return Ok(());
    }

    // Collect players and split into teams (synchronous shuffle so no !Send types live across await)
    let (mut red_team, mut blu_team) = split_into_teams(&game.pool);
    let mut updated_group = group.clone();

    // Assign teams using GamePlayer's team method
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

    // Update dashboard
    group.dash_update(cc.ctx).await?;
    
    cc.reply(&embed_content).await?;
    Ok(())
}

/// `/accept`
pub async fn accept(cc: &CommandContext<'_>, guild: &mut Server) -> Result<()> {
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.reply("Only runners can accept games!").await?;
        return Ok(());
    }

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id;
    let group = guild.get_group(channel_id).unwrap();

    // Check hot game count first
    let hot_game_count = group.sessions.iter().filter(|g| g.status == SessionStatus::Hot).count();
    match hot_game_count {
        0 => {
            cc.reply("No hot games found in this group.").await?;
            return Ok(());
        },
        1 => {
            info!("Found one existing hot game");
        },
        n => {
            return Err(anyhow!("Found more than one hot game ({}). This is unexpected.", n));
        },
    }
    
    // Now get mutable access to the hot game
    let hot_game = group.sessions
        .iter_mut()
        .find(|g| g.status == SessionStatus::Hot)
        .unwrap(); // Safe because we verified count above
    
    hot_game.push();


    // Update dashboard
    group.dash_update(cc.ctx).await?;

    cc.reply("Game accepted! Players moved to team channels.").await?;


    Ok(())
}

/// `/end`
///
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
/// * `game_id`  - The ID of the game to end.
pub async fn end(cc: &CommandContext<'_>, guild: &mut Server) -> Result<()> {
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.reply("Only runners can end games!").await?;
        return Ok(());
    }

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id;
    let group = guild.get_group(channel_id).unwrap();

    if let Ok(game) = group.get_user_session(cc.intax.user.id).await {
        game.status = SessionStatus::Pull;

        // TODO: Persist group changes to DB if needed (no update_group method exists)
        // You may need to implement this in your database layer.

        // Move players to queue channel if we're in a guild
        if let Some(guild_id) = cc.intax.guild_id {
            move_players_to_queue_channel(
                game.clone(),
                group.clone(),
                guild_id,
                cc.ctx
            ).await?;
        }
        
        cc.reply("Game has been ended. Players will be moved back to queue.").await?;
    } else {
        cc.reply("No active game found to end.").await?;
    }

    // Update dashboard
    group.dash_update(cc.ctx).await?;

    Ok(())
}
