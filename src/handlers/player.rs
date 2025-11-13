// Combined game handlers
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use serenity::all::{Context, EditMember, GuildId};

use tracing::{info, warn};

use crate::Database;
use crate::models::{
    CommandContext, SessionPlayer, Group, Rank, Role, Roles, Server, Session, SessionStatus, Team,
    DEFAULT_RANK,
};

/// Get player's rank from their Discord roles
pub async fn get_player_rank(
    ctx: &Context,
    db: &Database,
    guild_id: GuildId,
    user_id: serenity::all::UserId,
) -> Option<Rank> {
    if let Ok(member) = guild_id.member(&ctx.http, user_id).await {
        // Check all member roles and find the first matching rank
        for role_id in &member.roles {
            if let Some(rank) = Rank::from_role_id(*role_id, db, guild_id.get()).await {
                return Some(rank);
            }
        }
    }
    None
}

/// Get or assign player rank - creates ranks if needed and assigns default rank if player has no rank
pub async fn get_or_assign_player_rank(
    ctx:      &Context,
    db:       &Database,
    guild_id: GuildId,
    user_id:  serenity::all::UserId,
) -> Result<Rank> {
    // First check if player already has a rank
    if let Some(rank) = get_player_rank(ctx, db, guild_id, user_id).await {
        return Ok(rank);
    }
    
    // Player has no rank - check if rank roles exist
    let missing_roles = validate_rank_roles(ctx, db, guild_id).await?;
    
    // If ranks are missing, create them
    if !missing_roles.is_empty() {
        info!("Rank roles missing, creating them automatically for guild {}", guild_id);
        create_rank_roles(ctx, db, guild_id).await?;
    }
    
    // Get the default rank role ID from config
    let default_role_ids = DEFAULT_RANK.role_ids(db, guild_id.get()).await;
    
    if default_role_ids.is_empty() {
        return Err(anyhow!("Failed to find {} role after creation", DEFAULT_RANK.name()));
    }
    
    let default_role_id = default_role_ids[0];
    
    // Assign default rank role to the player
    match guild_id.member(&ctx.http, user_id).await {
        Ok(member) => {
            match member.add_role(&ctx.http, default_role_id).await {
                Ok(_) => {
                    info!("Assigned {} rank to user {}", DEFAULT_RANK.name(), user_id);
                    Ok(DEFAULT_RANK)
                },
                Err(e) => {
                    warn!("Failed to assign {} role to user {}: {}", DEFAULT_RANK.name(), user_id, e);
                    Err(anyhow!("Failed to assign {} rank: {}", DEFAULT_RANK.name(), e))
                }
            }
        },
        Err(e) => {
            warn!("Failed to fetch member {} in guild {}: {}", user_id, guild_id, e);
            Err(anyhow!("Failed to fetch member: {}", e))
        }
    }
}

/// Validate that the server has rank roles configured
pub async fn validate_rank_roles(
    ctx: &Context,
    db: &Database,
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
    
    // Check each rank to see if it has any roles configured or existing
    for rank in [
        Rank::Beginner,
        Rank::Newcomer,
        Rank::Novice,
        Rank::Apprentice,
        Rank::Journeyman,
        Rank::Expert,
        Rank::Master,
        Rank::MasterElite,
        Rank::Grandmaster,
    ] {
        let configured_ids = rank.role_ids(db, guild_id.get()).await;
        
        // Check if this rank has any roles that exist in the guild by ID
        let has_role_by_id = configured_ids.iter().any(|id| guild_role_ids.contains(id));
        
        if !has_role_by_id {
            // Fallback: search for role by name (case-insensitive)
            let rank_name = rank.name().to_lowercase();
            let found_role = guild_roles.iter().find(|r| r.name.to_lowercase() == rank_name);
            
            if let Some(role) = found_role {
                // Found a role with matching name! Auto-save it to config
                info!("Found existing role '{}' (ID: {}) by name, saving to config", role.name, role.id);
                
                // Save this role ID to the database config
                let role_id_str = role.id.get().to_string();
                if let Err(e) = db.config.set_config(rank.config_key(), &role_id_str, guild_id.get()).await {
                    warn!("Failed to save found role {} to config: {}", rank.name(), e);
                } else {
                    info!("Saved {} role ID to config: {}", rank.name(), role_id_str);
                }
            } else {
                // Role doesn't exist by ID or name
                missing_roles.push(rank.name().to_string());
            }
        }
    }
    
    Ok(missing_roles)
}

/// Create missing rank roles in the guild
pub async fn create_rank_roles(
    ctx: &Context,
    db: &Database,
    guild_id: GuildId,
) -> Result<Vec<String>> {
    use serenity::all::Colour;
    use serenity::builder::EditRole;
    use std::collections::HashMap;
    
    let mut created_roles = Vec::new();
    let mut rank_id_map: HashMap<&str, Vec<u64>> = HashMap::new();
    
    // Get all guild roles to check which are missing
    let guild_roles = ctx.http.get_guild_roles(guild_id).await?;
    let guild_role_ids: Vec<_> = guild_roles.iter().map(|r| r.id).collect();
    
    // Check each rank and create missing roles (in reverse order so GM is at top)
    for rank in [
        Rank::Grandmaster,
        Rank::MasterElite,
        Rank::Master,
        Rank::Expert,
        Rank::Journeyman,
        Rank::Apprentice,
        Rank::Novice,
        Rank::Newcomer,
        Rank::Beginner,
    ] {
        let existing_ids = rank.role_ids(db, guild_id.get()).await;
        let mut role_ids_for_rank = Vec::new();
        
        // Check if this rank has any roles in the guild by ID
        let has_role_by_id = existing_ids.iter().any(|id| guild_role_ids.contains(id));
        
        if !has_role_by_id {
            // Check if a role with this name exists (case-insensitive)
            let rank_name = rank.name().to_lowercase();
            let found_role = guild_roles.iter().find(|r| r.name.to_lowercase() == rank_name);
            
            if let Some(role) = found_role {
                // Found existing role by name, use it instead of creating
                info!("Found existing role '{}' (ID: {}) by name during creation", role.name, role.id);
                role_ids_for_rank.push(role.id.get());
            } else {
                // No role exists for this rank by ID or name, create one
            let color = match rank {
                Rank::Beginner    => Colour::from_rgb(150, 150, 150), // Gray
                Rank::Newcomer    => Colour::from_rgb(205, 220, 57),  // Yellow-Green
                Rank::Novice      => Colour::from_rgb(139, 195, 74),  // Light Green
                Rank::Apprentice  => Colour::from_rgb(76, 175, 80),   // Green
                Rank::Journeyman  => Colour::from_rgb(33, 150, 243),  // Blue
                Rank::Expert      => Colour::from_rgb(103, 58, 183),  // Deep Purple
                Rank::Master      => Colour::from_rgb(156, 39, 176),  // Purple
                Rank::MasterElite => Colour::from_rgb(233, 30, 99),   // Pink
                Rank::Grandmaster => Colour::from_rgb(255, 215, 0),   // Gold
            };
            
            let role_builder = EditRole::new()
                .name(rank.name())
                .colour(color)
                .hoist(true)  // Display role members separately in the member list
                .mentionable(false);  // Prevent @mentions to avoid spam
            
            match guild_id.create_role(&ctx.http, role_builder).await {
                Ok(created_role) => {
                    info!("Created rank role: {} (ID: {})", rank.name(), created_role.id);
                    created_roles.push(rank.name().to_string());
                    role_ids_for_rank.push(created_role.id.get());
                },
                Err(e) => {
                    warn!("Failed to create role {}: {}", rank.name(), e);
                }
            }
            }
        } else {
            // Keep existing role IDs
            role_ids_for_rank = existing_ids.iter().map(|id| id.get()).collect();
        }
        
        // Store role IDs for this rank if any exist
        if !role_ids_for_rank.is_empty() {
            rank_id_map.insert(rank.config_key(), role_ids_for_rank);
        }
    }
    
    // Save all rank role IDs to database config
    for (config_key, role_ids) in rank_id_map {
        let role_ids_str = role_ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        
        if let Err(e) = db.config.set_config(config_key, &role_ids_str, guild_id.get()).await {
            warn!("Failed to save rank config {}: {}", config_key, e);
        } else {
            info!("Saved rank config {}: {}", config_key, role_ids_str);
        }
    }
    
    Ok(created_roles)
}

/// Validate that runner and admin roles are configured
pub async fn validate_system_roles(
    ctx: &Context,
    db: &Database,
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
    
    // Check runner and admin roles
    for role in [Role::Runner, Role::Admin] {
        let role_key = role.config_key();
        
        // Check if role is configured
        let configured_role_id = role.id(db, guild_id.get()).await;
        
        let has_role = if let Some(role_id) = configured_role_id {
            // Check if the configured role still exists in the guild
            guild_roles.iter().any(|r| r.id == role_id)
        } else {
            false
        };
        
        if !has_role {
            // Fallback: search for role by name (case-insensitive)
            let role_name = role.name().to_lowercase();
            let found_role = guild_roles.iter().find(|r| r.name.to_lowercase() == role_name);
            
            if let Some(found) = found_role {
                // Found a role with matching name! Auto-save it to config
                info!("Found existing role '{}' (ID: {}) by name, saving to config", found.name, found.id);
                
                // Save this role ID to the database config
                let role_id_str = found.id.get().to_string();
                if let Err(e) = db.config.set_config(role_key, &role_id_str, guild_id.get()).await {
                    warn!("Failed to save found role {} to config: {}", role.name(), e);
                } else {
                    info!("Saved {} role ID to config: {}", role.name(), role_id_str);
                }
            } else {
                // Role doesn't exist by ID or name
                missing_roles.push(role.name().to_string());
            }
        }
    }
    
    Ok(missing_roles)
}

/// Checks if a user has the specified role.
/// Prioritizes Discord permissions (Administrator or Manage Server) over configured role.
///
/// * `cc` - The command context.
/// * `role` - The role to check for.
pub async fn check_role(
    cc: &CommandContext<'_>,
    role: &Role,
) -> Result<bool> {
    use serenity::all::Permissions;
    
    if let Some(guild_id) = cc.intax.guild_id {
        // Get the member
        let member = match guild_id.member(&cc.ctx.http, cc.intax.user.id).await {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to fetch member for user {} in guild {}: {:?}", cc.intax.user.id, guild_id, e);
                return Ok(false);
            }
        };
        
        // For Admin role: Check Discord permissions first (Administrator or Manage Server)
        if matches!(role, Role::Admin) {
            if let Some(guild_ref) = guild_id.to_guild_cached(&cc.ctx.cache) {
                let perms = guild_ref.member_permissions(&member);
                if perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) {
                    info!("User {} has Discord admin/manage permissions", cc.intax.user.id);
                    return Ok(true);
                }
            }
        }
        
        // Check configured role
        if let Some(role_id) = role.id(&cc.db, guild_id.get()).await {
            info!("Checking if user has {} role with ID: {}", role.name(), role_id);
            return Ok(member.roles.contains(&role_id));
        } else {
            info!("Role {} not configured for guild {}", role.name(), guild_id);
        }
    }
    Ok(false)
}

/// Checks if a user has the specified role (for component interactions).
/// Prioritizes Discord permissions (Administrator or Manage Server) over configured role.
///
/// * `cc` - The component context.
/// * `role` - The role to check for.
pub async fn check_component_role(
    cc: &crate::models::ComponentContext<'_>,
    role: &Role,
) -> Result<bool> {
    use serenity::all::Permissions;
    
    if let Some(guild_id) = cc.component.guild_id {
        // Get the member
        let member = match guild_id.member(&cc.ctx.http, cc.component.user.id).await {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to fetch member for user {} in guild {}: {:?}", cc.component.user.id, guild_id, e);
                return Ok(false);
            }
        };
        
        // For Admin role: Check Discord permissions first (Administrator or Manage Server)
        if matches!(role, Role::Admin) {
            if let Some(guild_ref) = guild_id.to_guild_cached(&cc.ctx.cache) {
                let perms = guild_ref.member_permissions(&member);
                if perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) {
                    info!("User {} has Discord admin/manage permissions", cc.component.user.id);
                    return Ok(true);
                }
            }
        }
        
        // Check configured role
        if let Some(role_id) = role.id(&cc.db, guild_id.get()).await {
            info!("Checking if user {} has {} role with ID: {}", cc.component.user.name, role.name(), role_id);
            return Ok(member.roles.contains(&role_id));
        } else {
            info!("Role {} not configured for guild {}", role.name(), guild_id);
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
    
    // Get or assign player rank (auto-creates ranks and assigns Apprentice if needed)
    let rank = match get_or_assign_player_rank(cc.ctx, &cc.db, guild_id, user).await {
        Ok(rank) => {
            info!("Player {} has rank: {:?}", user, rank);
            rank
        },
        Err(e) => {
            cc.reply(&format!("❌ Failed to get or assign rank: {}. Please contact an admin.", e)).await?;
            return Ok(());
        }
    };

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

    let group = guild.get_group(channel)?;
    
    // Ensure we have an idle session (create if needed)
    let idle_sessions = group.get_sessions_by_status(&SessionStatus::Idle);
    if idle_sessions.is_empty() {
        info!("No idle session found, creating one");
        // Create session in the in-memory group
        let mut manager = cc.manager.lock().await;
        let server = manager.get_server(guild_id)?;
        let group = server.get_group(channel)?;
        group.create_session();
    } else if idle_sessions.len() > 1 {
        return Err(anyhow!("Found more than one idle game ({}). This is unexpected.", idle_sessions.len()));
    } else {
        info!("Found one existing idle session");
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
    let current_queue = match group.get_queue().await {
        Ok(session) => session.pool.len(),
        Err(_) => 0
    };
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
        let group = guild.get_group(channel)?;
        
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
    let group = guild.get_group(cc.intax.channel_id)?;
    let quota = group.quota as usize;

    if group.sessions.is_empty() {
        cc.reply("No active games.").await?;
        return Ok(());
    }

    let game = group.sessions.last().ok_or_else(|| anyhow!("No active game"))?;
    
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
    let last_session = updated_group.sessions.last_mut()
        .ok_or_else(|| anyhow!("No session available for team assignment"))?;
    last_session.pool.clear();
    last_session.pool.extend(red_team.into_iter());
    last_session.pool.extend(blu_team.into_iter());
    last_session.status = SessionStatus::Hot;

    let red_team_names: Vec<String> = last_session.pool.iter()
        .filter(|sp| sp.team == Some(Team::Red))
        .map(|sp| format!("<@{}>", sp.player.discord_id))
        .collect();
    let blu_team_names: Vec<String> = last_session.pool.iter()
        .filter(|sp| sp.team == Some(Team::Blu))
        .map(|sp| format!("<@{}>", sp.player.discord_id))
        .collect();

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
    let group = guild.get_group(channel_id)?;

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
        .ok_or_else(|| anyhow!("Hot game not found after verification"))?;
    
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
    let group = guild.get_group(channel_id)?;

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
