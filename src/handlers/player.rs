// Combined game handlers
use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use serenity::all::{Context as Ctx, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, GuildId as GI, Member, UserId as UI};

use tracing::{info, warn, error};

use crate::{ComponentContext as CC, Database as DB};
use crate::models::{
    CommandContext as CmC, SessionPlayer as SP, Rank, Role, Server, SessionStatus as SS, Team,
    DEFAULT_RANK,
};

/// Helper: Get member with cache-first strategy
async fn get_member_cached(ctx: &Ctx, guild_id: GI, user_id: UI) -> Option<Member> {
    // Try cache first (fast path, no API call)
    let member = if let Some(guild) = ctx.cache.guild(guild_id) {
        guild.members.get(&user_id).cloned()
    } else {
        None
    };

    // Fallback to HTTP if not in cache
    match member {
        Some(m) => Some(m),
        None => guild_id.member(&ctx.http, user_id).await.ok(),
    }
}

/// Get player's rank from their Discord roles
pub async fn get_player_rank(ctx: &Ctx, db: &DB, guild_id: GI, user_id: UI) -> Option<Rank> {
    let member = get_member_cached(ctx, guild_id, user_id).await?;

    // Load rank mappings once for this guild (ordered by position, low to high)
    let mappings = Rank::load_rank_mappings(db, guild_id.get()).await;

    // Collect all matching ranks for this member
    let mut matched_ranks: Vec<Rank> = Vec::new();
    for role_id in &member.roles {
        if let Some(rank) = Rank::from_role_id_cached(*role_id, &mappings) {
            matched_ranks.push(rank);
        }
    }

    if matched_ranks.len() > 1 {
        warn!(
            "User {} has multiple rank roles: {:?}",
            member.user.name,
            matched_ranks.iter().map(|r| r.name()).collect::<Vec<_>>()
        );
    }

    // Return highest rank (highest position = highest rank)
    matched_ranks.into_iter().max_by_key(|r| r.position())
}

/// Get player's rank from their Discord roles using pre-loaded mappings (no DB calls)
pub fn get_player_rank_cached(member: &Member, mappings: &[(Rank, Vec<serenity::all::RoleId>)]) -> Option<Rank> {
    // Collect all matching ranks for this member
    let mut matched_ranks: Vec<Rank> = Vec::new();
    for role_id in &member.roles {
        if let Some(rank) = Rank::from_role_id_cached(*role_id, mappings) {
            matched_ranks.push(rank);
        }
    }

    if matched_ranks.len() > 1 {
        warn!(
            "User {} has multiple rank roles: {:?}",
            member.user.name,
            matched_ranks.iter().map(|r| r.name()).collect::<Vec<_>>()
        );
    }

    // Return highest rank (highest position = highest rank)
    matched_ranks.into_iter().max_by_key(|r| r.position())
}

/// Get or assign player rank - creates ranks if needed and assigns default rank if player has no rank
pub async fn get_or_assign_player_rank(ctx: &Ctx, db: &DB, guild_id: GI, user_id: UI) -> Result<Rank> {
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
            let username = member.user.tag();
            match member.add_role(&ctx.http, default_role_id).await {
                Ok(_) => {
                    info!("Assigned {} rank to user {}", DEFAULT_RANK.name(), username);
                    Ok(DEFAULT_RANK)
                },
                Err(e) => {
                    warn!("Failed to assign {} role to user {}: {}", DEFAULT_RANK.name(), username, e);
                    Err(anyhow!("Failed to assign {} rank: {}", DEFAULT_RANK.name(), e))
                }
            }
        },
        Err(e) => {
            warn!("Failed to fetch member {} in guild {}: {}", user_id, guild_id, e);
            Err(anyhow!("Failed to fetch member: {e}"))
        }
    }
}

/// Update a player's rank and synchronize Discord roles
/// Removes old rank roles and adds new rank role
pub async fn update_player_rank_with_roles(
    ctx: &Ctx,
    db: &DB,
    guild_id: GI,
    user_id: UI,
    old_rank: Rank,
    new_rank: Rank,
) -> Result<()> {
    if old_rank == new_rank {
        return Ok(());
    }

    let member = match get_member_cached(ctx, guild_id, user_id).await {
        Some(m) => m,
        None => return Err(anyhow!("Could not find member {} in guild {}", user_id, guild_id)),
    };

    // Get role IDs for old and new ranks
    let old_role_ids = old_rank.role_ids(db, guild_id.get()).await;
    let new_role_ids = new_rank.role_ids(db, guild_id.get()).await;

    // Remove old rank roles that the member has
    for role_id in &old_role_ids {
        if member.roles.contains(role_id) {
            if let Err(e) = member.remove_role(&ctx.http, role_id).await {
                warn!("Failed to remove {} role from {}: {}", old_rank.name(), member.user.tag(), e);
            } else {
                info!("Removed {} role from {}", old_rank.name(), member.user.tag());
            }
        }
    }

    // Add new rank role (use first configured role)
    if let Some(new_role_id) = new_role_ids.first() {
        if !member.roles.contains(new_role_id) {
            if let Err(e) = member.add_role(&ctx.http, new_role_id).await {
                warn!("Failed to add {} role to {}: {}", new_rank.name(), member.user.tag(), e);
            } else {
                info!("Added {} role to {}", new_rank.name(), member.user.tag());
            }
        }
    } else {
        warn!("No role configured for rank {} in guild {}", new_rank.name(), guild_id);
    }

    Ok(())
}

/// Validate that the server has rank roles configured
pub async fn validate_rank_roles(ctx: &Ctx, db: &DB, guild_id: GI) -> Result<Vec<String>> {
    let mut missing_roles = Vec::new();

    // Get all guild roles
    let guild_roles = match ctx.http.get_guild_roles(guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            warn!("Failed to fetch guild roles: {e}");
            return Err(anyhow!("Failed to fetch guild roles"));
        }
    };

    let guild_role_ids: Vec<_> = guild_roles.iter().map(|r| r.id).collect();

    // Check each rank to see if it has any roles configured or existing
    // Process more specific ranks first (MasterElite before Master) to avoid false matches
    for rank in [
        Rank::Beginner,
        Rank::Newcomer,
        Rank::Novice,
        Rank::Apprentice,
        Rank::Journeyman,
        Rank::Expert,
        Rank::MasterElite,  // Check before Master to avoid "Master Elite" matching "Master"
        Rank::Master,
        Rank::Grandmaster,
    ] {
        let configured_ids = rank.role_ids(db, guild_id.get()).await;

        // Check if this rank has any roles that exist in the guild by ID
        let has_role_by_id = configured_ids.iter().any(|id| guild_role_ids.contains(id));

        if !has_role_by_id {
            // Fallback: search for ALL roles that contain the rank name as a whole word (case-insensitive)
            // This handles variants like "Journeyman", "Journeyman EU", "Journeyman NA", "Retired Journeyman"
            let rank_name = rank.name().to_lowercase();
            let matching_roles: Vec<_> = guild_roles.iter()
                .filter(|r| {
                    let role_name_lower = r.name.to_lowercase();
                    let words: Vec<&str> = role_name_lower.split(|c: char| !c.is_alphanumeric())
                        .filter(|w| !w.is_empty())
                        .collect();
                    
                    // For "Master", exclude roles that also contain "Elite" (those are "Master Elite")
                    if rank_name == "master" && words.contains(&"elite") {
                        return false;
                    }
                    
                    // Match if role name contains rank name as a complete word
                    words.contains(&rank_name.as_str())
                })
                .collect();

            if !matching_roles.is_empty() {
                // Found one or more roles matching this rank! Auto-save all of them to ranks table
                let role_ids: Vec<serenity::all::RoleId> = matching_roles.iter()
                    .map(|r| {
                        info!("Found existing role '{}' matching {}, saving to ranks table",
                            r.name, rank.name());
                        r.id
                    })
                    .collect();

                // Save role IDs to the ranks table
                if let Err(e) = db.ranks.update_rank_role_ids(guild_id.get(), rank.position(), &role_ids).await {
                    warn!("Failed to save found roles for {} to ranks: {}", rank.name(), e);
                } else {
                    info!("Saved {} role IDs to ranks table ({})", rank.name(), matching_roles.len());
                }
            } else {
                // No roles exist by ID or name match
                missing_roles.push(rank.name().to_string());
            }
        }
    }

    Ok(missing_roles)
}

/// Create missing rank roles in the guild
pub async fn create_rank_roles(ctx: &Ctx, db: &DB, guild_id: GI) -> Result<Vec<String>> {
    use serenity::all::Colour;
    use serenity::builder::EditRole;

    let mut created_roles = Vec::new();

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
            // Check if ANY roles contain this rank name as a whole word (case-insensitive)
            // This handles variants like "Journeyman", "Journeyman EU", "Journeyman NA", "Retired Journeyman"
            let rank_name = rank.name().to_lowercase();
            let matching_roles: Vec<_> = guild_roles.iter()
                .filter(|r| {
                    let role_name_lower = r.name.to_lowercase();
                    let words: Vec<&str> = role_name_lower.split(|c: char| !c.is_alphanumeric())
                        .filter(|w| !w.is_empty())
                        .collect();
                    
                    // For "Master", exclude roles that also contain "Elite" (those are "Master Elite")
                    if rank_name == "master" && words.contains(&"elite") {
                        return false;
                    }
                    
                    // Match if role name contains rank name as a complete word
                    words.contains(&rank_name.as_str())
                })
                .collect();

            if !matching_roles.is_empty() {
                // Found existing role(s) matching this rank, use them instead of creating
                for role in matching_roles {
                    info!("Found existing role '{}' matching {} during creation", role.name, rank.name());
                    role_ids_for_rank.push(role.id.get());
                }
            } else {
                // No role exists for this rank by ID or name, create one
            let color = match rank {
                Rank::Beginner    => Colour::from_rgb(150, 150, 150), // Gray
                Rank::Newcomer    => Colour::from_rgb(205, 220, 57),  // Yellow-Green
                Rank::Novice      => Colour::from_rgb(139, 195, 74),  // Light Green
                Rank::Apprentice  => Colour::from_rgb(76 , 175, 80),  // Green
                Rank::Journeyman  => Colour::from_rgb(33 , 150, 243), // Blue
                Rank::Expert      => Colour::from_rgb(103, 58,  183), // Deep Purple
                Rank::Master      => Colour::from_rgb(156, 39,  176), // Purple
                Rank::MasterElite => Colour::from_rgb(233, 30,  99),  // Pink
                Rank::Grandmaster => Colour::from_rgb(255, 215, 0),   // Gold
            };

            let role_builder = EditRole::new()
                .name(rank.name())
                .colour(color)
                .hoist(true)  // Display role members separately in the member list
                .mentionable(false);
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
            match guild_id.create_role(&ctx.http, role_builder).await {
                Ok(created_role) => {
                    info!("[{}] Added rank: {}", guild_name, rank.name());
                    created_roles.push(rank.name().to_string());
                    role_ids_for_rank.push(created_role.id.get());
                },
                Err(_e) => {
                    warn!("[{}] Failed to create rank: {}", guild_name, rank.name());
                }
            }
            }
        } else {
            // Keep existing role IDs
            role_ids_for_rank = existing_ids.iter().map(|id| id.get()).collect();
        }

        // Save role IDs for this rank to the ranks table
        if !role_ids_for_rank.is_empty() {
            let role_ids: Vec<serenity::all::RoleId> = role_ids_for_rank.iter()
                .map(|id| serenity::all::RoleId::new(*id))
                .collect();
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
            if let Err(e) = db.ranks.update_rank_role_ids(guild_id.get(), rank.position(), &role_ids).await {
                warn!("[{}] Failed to save rank {}: {}", guild_name, rank.name(), e);
            } else {
                info!("[{}] Saved rank {} role IDs", guild_name, rank.name());
            }
        }
    }

    Ok(created_roles)
}

/// Validate that runner and admin roles are configured
pub async fn validate_system_roles(ctx: &Ctx, db: &DB, guild_id: GI) -> Result<Vec<String>> {
    let mut missing_roles = Vec::new();

    // Get all guild roles
    let guild_roles = match ctx.http.get_guild_roles(guild_id).await {
        Ok(roles) => roles,
        Err(e) => {
            warn!("Failed to fetch guild roles: {e}");
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
                info!("Found existing role '{}', saving to config", found.name);

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

async fn deny_command(cc: &CmC<'_>, role: &Role) -> Result<()> {
    info!("[{}] User {} does not have {} role", cc.guild_name(), cc.intax.user.name, role.name());
    let response = CIR::Message(CIRM::new().content(format!("This command is reserved for {}s", role.name().to_lowercase())).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// Checks if a user has the specified role.
/// Prioritizes Discord permissions (Administrator or Manage Server) over configured role.
pub async fn check_role(cc: &CmC<'_>, role: &Role) -> Result<bool> {
    use serenity::all::Permissions;

    if let Some(guild_id) = cc.intax.guild_id {
        let member = match get_member_cached(cc.ctx, guild_id, cc.intax.user.id).await {
            Some(m) => m,
            None => {
                warn!("[{}] Failed to fetch member for user {}", cc.guild_name(), cc.intax.user.name);
                return Ok(false);
            }
        };

        // For Admin role: Check Discord permissions first (Administrator or Manage Server)
        if matches!(role, Role::Admin) {
            if let Some(guild_ref) = guild_id.to_guild_cached(&cc.ctx.cache) {
                let perms = guild_ref.member_permissions(&member);
                if perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) {
                    return Ok(true);
                }
            }
        }

        // Check configured roles (supports multiple)
        let role_ids = role.ids(&cc.db, guild_id.get()).await;
        if !role_ids.is_empty() {
            // User has the role if they have ANY of the configured roles
            if role_ids.iter().any(|role_id| member.roles.contains(role_id)) {
                return Ok(true);
            } else {
                deny_command(cc, role).await?;
                return Ok(false);
            }
        } else {
            deny_command(cc, role).await?;
            return Ok(false);
        }
    }
    Ok(false)
}

pub async fn check_adm(cc: &CmC<'_>) -> Result<bool> {
    check_role(cc, &Role::Admin).await
}

pub async fn check_run(cc: &CmC<'_>) -> Result<bool> {
    check_role(cc, &Role::Runner).await
}

/// Checks if a user has the specified role (for component interactions).
/// Prioritizes Discord permissions (Administrator or Manage Server) over configured role.
pub async fn check_component_role(cc: &CC<'_>, role: &Role) -> Result<bool> {
    use serenity::all::Permissions;

    if let Some(guild_id) = cc.component.guild_id {
        let member = match get_member_cached(cc.ctx, guild_id, cc.component.user.id).await {
            Some(m) => m,
            None => {
                warn!("[{}] Failed to fetch member for user {}", cc.guild_name(), cc.component.user.name);
                return Ok(false);
            }
        };

        // For Admin role: Check Discord permissions first (Administrator or Manage Server)
        if matches!(role, Role::Admin) {
            if let Some(guild_ref) = guild_id.to_guild_cached(&cc.ctx.cache) {
                let perms = guild_ref.member_permissions(&member);
                if perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) {
                    info!("User {} has Discord admin/manage permissions", cc.component.user.name);
                    return Ok(true);
                }
            }
        }

        // Check configured roles (supports multiple)
        let role_ids = role.ids(&cc.db, guild_id.get()).await;
        if !role_ids.is_empty() {
            // User has the role if they have ANY of the configured roles
            return Ok(role_ids.iter().any(|role_id| member.roles.contains(role_id)));
        } else {
            let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
            info!("[{}] Role {} not configured", guild_name, role.name());
        }
    }
    Ok(false)
}

/// Splits the players into two teams.
pub fn split_into_teams(players: &[SP]) -> (Vec<SP>, Vec<SP>) {
    let mut rng = rand::rng();
    let mut player_list: Vec<SP> = players.to_vec();
    player_list.shuffle(&mut rng);
    let team_size = player_list.len() / 2;
    let team1 = player_list[0..team_size].to_vec();
    let team2 = player_list[team_size..].to_vec();
    (team1, team2)
}

//
// Queue functions
//

/// `/join` and `/leave`
pub async fn queue<'a>(cc: &'a CmC<'a>, guild: &mut Server) -> Result<()> {
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
            if game.status == SS::Idle {
                let initial_len = game.pool.len();
                game.pool.retain(|p| p.player.user_id != user);
                if game.pool.len() < initial_len {
                    found = true;
                    queue_count = game.pool.len();
                    break;
                }
            }
        }

        if found {
            cc.reply(&format!("Left the queue! ({queue_count}/{} players)", group.quota)).await?;
        }

        group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;

        return Ok(());
    }

    // Handle join command
    // Validate player has a rank
    let guild_id = match cc.intax.guild_id {
        Some(id) => id,
        None => {
            cc.reply("This command can only be used in a server.").await?;
            return Ok(());
        }
    };

    // Get or assign player rank (auto-creates ranks and assigns Apprentice if needed)
    let rank = match get_or_assign_player_rank(cc.ctx, &cc.db, guild_id, user).await {
        Ok(rank) => rank,
        Err(e) => {
            cc.reply(&format!("Failed to get or assign rank: {e}. Please contact an admin.")).await?;
            return Ok(());
        }
    };

    // Get player info or create a new one (use fast path without extra API call)
    let mut player = match cc.db.get_user(user, cc.ctx).await {
        Ok(mut player) => {
            // If player has no ELO in database, use rank-based ELO
            if player.elo == 0 {
                info!("DEBUG: Player {} has ELO 0, setting to {} from Discord rank {}", user, rank.default_rank_elo(), rank.name());
                player.elo = rank.default_rank_elo();
                player.rank = rank;
                // Update database with the rank-based ELO
                if let Err(e) = cc.db.users.update_elo(user, Some(player.elo)).await {
                    warn!("Failed to update player ELO in database: {}", e);
                }
            } else {
                // Player has stored ELO, check for ELO mismatch with Discord rank
                let elo_mismatch = player.elo <= 30 && rank.default_rank_elo() > 30;
                
                if elo_mismatch {
                    warn!("ELO MISMATCH DETECTED in queue: Player {} has ELO {} but Discord rank {} (default ELO {}). Auto-correcting...", 
                          user, player.elo, rank.name(), rank.default_rank_elo());
                    
                    player.elo = rank.default_rank_elo();
                    player.rank = rank;
                    
                    // Update the database with the corrected ELO
                    if let Err(e) = cc.db.users.update_elo(user, Some(player.elo)).await {
                        error!("Failed to auto-correct ELO for player {} in queue: {}", user, e);
                    } else {
                        info!("Successfully auto-corrected ELO for player {} in queue to {} (rank: {})", 
                              user, player.elo, rank.name());
                    }
                } else {
                    // Player has stored ELO, keep their ELO and only update rank if it makes sense
                    info!("DEBUG: Player {} has custom ELO {}, keeping it instead of Discord rank ELO {}", user, player.elo, rank.default_rank_elo());
                    // Don't override rank - keep whatever rank matches their current ELO
                    player.update_rank_from_elo(&cc.db, guild_id.get()).await;
                }
            }
            player
        },
        Err(_) => {
            // New player - use rank-based ELO
            info!("DEBUG: New player {}, setting ELO to {} from Discord rank {}", user, rank.default_rank_elo(), rank.name());
            let mut new_player = cc.db.new_user(user, cc.ctx).await?;
            new_player.elo = rank.default_rank_elo();
            new_player.rank = rank;
            // Update database with the rank-based ELO
            if let Err(e) = cc.db.users.update_elo(user, Some(new_player.elo)).await {
                warn!("Failed to update new player ELO in database: {}", e);
            }
            new_player
        }
    };

    // Set discord tag from interaction user data (already available, no API call needed)
    player.tag = cc.intax.user.tag();

    let group = guild.get_group(channel)?;

    // Check if we have an idle session
    let idle_sessions = group.get_sessions_by_status(&SS::Idle);
    if idle_sessions.is_empty() {
        cc.reply("No queue available. A match is currently in progress.").await?;
        return Ok(());
    } else if idle_sessions.len() > 1 {
        return Err(anyhow!("Found more than one idle game ({}). This is unexpected.", idle_sessions.len()));
    }

    // Check if player is already in game
    if group.get_user_session(user).await.is_err() {
        let mut manager = cc.manager.lock().await;
        let server = manager.get_server(guild_id)?;
        let group  = server.get_group(channel)?;

        // Check if we can add to idle session (not Hot/Live)
        let idle_sessions = group.get_sessions_by_status(&SS::Idle);
        if idle_sessions.is_empty() {
            // No idle session means match is in progress
            drop(manager);
            cc.reply("Cannot join queue while match is in progress. Please wait for current match to end.").await?;
            return Ok(());
        }

        let queue = group.get_queue().await?;
        queue.add_player(player);

        if group.is_quota() {
            group.hot(cc.ctx, Some(guild_id), Some(&cc.db), Some(cc.manager.clone())).await?;
        }

        group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;
    }

    // Always acknowledge (silently if already in queue)
    let current_queue = match group.get_queue().await {
        Ok(session) => session.pool.len(),
        Err(_) => 0
    };
    cc.reply(&format!("Joined the queue! ({current_queue}/{} players)", group.quota)).await?;

    // Update dashboard
    group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;

    Ok(())
}

/// `/status`
pub async fn status<'a>(cc: &'a CmC<'a>, guild: &mut Server) -> Result<()> {
    let channel = cc.intax.channel_id;

    let (queue_count, queue_list, quota) = {
        let group = guild.get_group(channel)?;

        let idle_games = group.get_sessions_by_status(&SS::Idle);

        if idle_games.is_empty() {
            (0, "No active queue found.".to_string(), group.quota)
        } else {
            let game = &idle_games[0];
            let count = game.pool.len();
            let list = if count > 0 {
                game.pool.iter().enumerate()
                    .map(|(i, p)| format!("{}. <@{}>", i + 1, p.player.user_id))
                    .collect::<Vec<_>>().join("\n")
            } else {
                "Queue is empty".to_string()
            };
            (count, list, group.quota)
        }
    }; // Manager lock is dropped here

    if queue_count == 0 && queue_list == "No active queue found." {
        cc.reply("No active queue found.").await?;
    } else {
        cc.reply(&format!("**Queue Status ({queue_count}/{quota} players)**\n{queue_list}")).await?;
    }

    Ok(())
}

/// `/shuffle`
pub async fn shuffle(cc: &CmC<'_>, guild: &mut Server) -> Result<()> {
        if !check_run(cc).await? { return Ok(()); }

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
    last_session.status = SS::Hot;

    let red_team_names: Vec<String> = last_session.pool.iter()
        .filter(|sp| sp.team == Some(Team::Red))
        .map(|sp| format!("<@{}>", sp.player.user_id))
        .collect();
    let blu_team_names: Vec<String> = last_session.pool.iter()
        .filter(|sp| sp.team == Some(Team::Blu))
        .map(|sp| format!("<@{}>", sp.player.user_id))
        .collect();

    let embed_content = format!(
        "**Teams Generated!**\n\n**Red Team:**\n{}\n\n**Blue Team:**\n{}",
        red_team_names.join("\n"),
        blu_team_names.join("\n")
    );

    // Update dashboard
    group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;

    cc.reply(&embed_content).await?;
    Ok(())
}

/// `/accept`
pub async fn accept(cc: &CmC<'_>, guild: &mut Server) -> Result<()> {
        if !check_run(cc).await? { return Ok(()); }

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id;
    let group = guild.get_group(channel_id)?;

    // Check hot game count first
    let hot_game_count = group.sessions.iter().filter(|g| g.status == SS::Hot).count();
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
    let hot_game = group.sessions.iter_mut()
        .find(|g| g.status == SS::Hot)
        .ok_or_else(|| anyhow!("Hot game not found after verification"))?;

    hot_game.push();

    // Update dashboard
    group.queue_dash_update(cc.ctx, cc.intax.guild_id.unwrap().get()).await;

    cc.reply("Game accepted! Players moved to team channels.").await?;

    Ok(())
}

pub async fn end(cc: &CmC<'_>, guild: &mut Server) -> Result<()> {
        if !check_run(cc).await? { return Ok(()); }

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id;
    let group = guild.get_group(channel_id)?;

    // Check if there's an active game to end
    let has_active = group.sessions.iter().any(|s| s.status == SS::Hot || s.status == SS::Live);

    if !has_active {
        cc.reply("No active game found to end.").await?;
        return Ok(());
    }

    // Use Group::pull() to properly move players back and handle re-queueing
    if let Some(guild_id) = cc.intax.guild_id {
        group.pull(cc.ctx, guild_id, &cc.db, Some(cc.manager.clone())).await?;
        cc.reply("Game has been ended. Players moved back to queue.").await?;
    } else {
        cc.reply("This command can only be used in a server.").await?;
    }

    Ok(())
}
