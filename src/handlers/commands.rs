use anyhow::{anyhow, Result};
use serenity::all::{
    CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, Permissions,
};
use serenity::builder::EditRole;
use tracing::{error, info, warn};

use crate::Server;
use crate::admin::create_group_channels;
use crate::models::{CommandContext as CC, Role};
use crate::repositories::Repository;
use super::player::{check_role, create_rank_roles};
use super::settings::{build_settings_embed, build_settings_buttons};

/// Helper: Create a Discord role with error handling
async fn create_role_with_error(
    cc: &CC<'_>,
    guild_id: serenity::all::GuildId,
    name: &str,
    color: u32,
) -> Result<Option<serenity::all::Role>> {
    match guild_id.create_role(&cc.ctx.http,
        EditRole::new()
            .name(name)
            .colour(color)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => Ok(Some(role)),
        Err(e) => {
            let error_embed = CE::new()
                .title(format!("Failed to Create {name} Role"))
                .description(format!("Error: {e}"))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            Ok(None)
        }
    }
}

/// `/roleadd` - Create runner and admin roles for the bot
pub async fn cmd_role_add(cc: &CC<'_>) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can create roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    let loading_embed = CE::new()
        .title("Creating Roles")
        .description("Creating Runner and Admin roles...")
        .color(0xffaa00);

    let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Create Runner role
    let runner_role = match create_role_with_error(cc, guild_id, "PUG Runner", 0x3498db).await? {
        Some(role) => role,
        None => return Ok(()), // Error already handled
    };

    // Create Admin role
    let admin_role = match create_role_with_error(cc, guild_id, "PUG Admin", 0xe74c3c).await? {
        Some(role) => role,
        None => return Ok(()), // Error already handled
    };

    // Save to database
    if let Err(e) = cc.db.config.set_config("runner_role", &runner_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {e}");
    }
    if let Err(e) = cc.db.config.set_config("admin_role", &admin_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {e}");
    }

    // Create rank roles
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    info!("[{}] Creating rank roles", guild_name);
    if let Err(e) = create_rank_roles(cc.ctx, &cc.db, guild_id).await {
        warn!("[{}] Failed to create rank roles: {}", guild_name, e);
    }

    let success_embed = CE::new()
        .title("Roles Created!")
        .description(format!(
            "Successfully created bot roles:\n\n\
            • Runner Role: <@&{}>\n\
            • Admin Role: <@&{}>\n\
            • Rank Roles: Created\n\n\
            **Note:** Assign these roles to users who should manage PUGs.",
            runner_role.id,
            admin_role.id
        ))
        .color(0x00ff00);

    cc.intax.edit_response(&cc.ctx.http,
        serenity::all::EditInteractionResponse::new().embed(success_embed)
    ).await?;

    Ok(())
}

/// `/rolelink` - Link existing runner and admin roles (supports multiple roles per type)
pub async fn cmd_role_link(cc: &CC<'_>, runner_role: Option<String>, admin_role: Option<String>) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can link roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // If no parameters provided, show current configuration
    if runner_role.is_none() && admin_role.is_none() {
        let current_runner = cc.db.config.get_config_value("runner_role", guild_id.get()).await?;
        let current_admin = cc.db.config.get_config_value("admin_role", guild_id.get()).await?;

        let runner_display = format_role_mentions(current_runner);
        let admin_display = format_role_mentions(current_admin);

        let embed = CE::new()
            .title("Current Role Configuration")
            .description(format!(
                "**Current Roles:**\n\
                • Runner: {runner_display}\n\
                • Admin: {admin_display}\n\n\
                **Usage:**\n\
                `/rolelink runner_role:@Role1 @Role2 admin_role:@AdminRole`\n\
                Supports multiple roles per type (space or comma separated)",
            ));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let mut updated_roles = Vec::new();

    // Link runner roles if provided
    if let Some(role_str) = runner_role {
        let role_ids = parse_multiple_role_ids(&role_str)?;
        let role_ids_str = role_ids.join(",");
        cc.db.config.set_config("runner_role", &role_ids_str, guild_id.get()).await?;
        let display = role_ids.iter()
            .map(|id| format!("<@&{id}>"))
            .collect::<Vec<_>>()
            .join(", ");
        updated_roles.push(format!("• Runner: {display}"));
    }

    // Link admin roles if provided
    if let Some(role_str) = admin_role {
        let role_ids = parse_multiple_role_ids(&role_str)?;
        let role_ids_str = role_ids.join(",");
        cc.db.config.set_config("admin_role", &role_ids_str, guild_id.get()).await?;
        let display = role_ids.iter()
            .map(|id| format!("<@&{id}>"))
            .collect::<Vec<_>>()
            .join(", ");
        updated_roles.push(format!("• Admin: {display}"));
    }

    let success_embed = CE::new()
        .title("Roles Linked!")
        .description(format!(
            "Successfully linked roles:\n{}",
            updated_roles.join("\n")
        ))
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/roleremove` - Remove runner and admin role configuration
pub async fn cmd_role_remove(cc: &CC<'_>, role_type: String) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can remove role configuration!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    let role_key = match role_type.to_lowercase().as_str() {
        "runner" => "runner_role",
        "admin" => "admin_role",
        "both" | "all" => {
            // Remove both
            cc.db.config.delete_config("runner_role", guild_id.get()).await?;
            cc.db.config.delete_config("admin_role", guild_id.get()).await?;

            let success_embed = CE::new()
                .title("Roles Removed")
                .description("Removed both Runner and Admin role configurations.\n\n\
                    **Note:** The Discord roles themselves were not deleted.")
                .color(0x00ff00);

            let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
        _ => {
            let response = CIR::Message(CIRM::new()
                .content("Invalid role type. Use `runner`, `admin`, or `both`")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Remove single role
    cc.db.config.delete_config(role_key, guild_id.get()).await?;

    let success_embed = CE::new()
        .title("Role Removed")
        .description(format!(
            "Removed {role_type} role configuration.\n\n\
            **Note:** The Discord role itself was not deleted."
        ))
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/rankroleadd` - Add Discord role(s) to a rank (supports multiple roles at once)
pub async fn cmd_rank_add(cc: &CC<'_>, rank_name: String, role_mentions: String) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can configure rank roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Parse the rank
    let rank = parse_rank_name(&rank_name)?;
    if rank.is_none() {
        let response = CIR::Message(CIRM::new()
            .content("Invalid rank name. Valid ranks: Beginner, Newcomer, Novice, Apprentice, Journeyman, Expert, Master, MasterElite, Grandmaster")
            .ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    let rank = rank.unwrap();

    // Parse multiple role mentions (space-separated)
    let role_mentions_vec: Vec<&str> = role_mentions.split_whitespace().collect();
    let guild_roles = cc.ctx.http.get_guild_roles(guild_id).await?;

    let mut roles_to_add = Vec::new();
    for role_mention in role_mentions_vec {
        let role_id_str = parse_role_id(role_mention)?;
        let role_id = match role_id_str.parse::<u64>() {
            Ok(id) => serenity::all::RoleId::new(id),
            Err(_) => {
                let response = CIR::Message(CIRM::new()
                    .content(format!("Invalid role format: {role_mention}. Please mention roles or provide role IDs."))
                    .ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        };

        // Verify the role exists
        let role = match guild_roles.iter().find(|r| r.id == role_id) {
            Some(r) => r,
            None => {
                let response = CIR::Message(CIRM::new()
                    .content(format!("Role {role_mention} not found in this server."))
                    .ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        };

        roles_to_add.push((role_id, role.name.clone()));
    }

    // Get existing role IDs for this rank
    let mut existing_ids = rank.role_ids(&cc.db, guild_id.get()).await;
    let mut added_roles = Vec::new();
    let mut skipped_roles = Vec::new();

    for (role_id, role_name) in roles_to_add {
        if existing_ids.contains(&role_id) {
            skipped_roles.push(role_name);
        } else {
            existing_ids.push(role_id);
            info!("Added role '{}' (ID: {}) to rank {} for guild {}", role_name, role_id, rank.name(), guild_id);
            added_roles.push(role_name);
        }
    }

    // Save to database
    let role_ids_str = existing_ids.iter()
        .map(|id| id.get().to_string())
        .collect::<Vec<_>>()
        .join(",");
    cc.db.config.set_config(rank.config_key(), &role_ids_str, guild_id.get()).await?;

    let mut description = String::new();
    if !added_roles.is_empty() {
        description.push_str(&format!("**Added {} role(s) to rank {}:**\n", added_roles.len(), rank.name()));
        for role in &added_roles {
            description.push_str(&format!("  • {role}\n"));
        }
    }
    if !skipped_roles.is_empty() {
        description.push_str(&format!("\n**Skipped {} role(s) (already configured):**\n", skipped_roles.len()));
        for role in &skipped_roles {
            description.push_str(&format!("  • {role}\n"));
        }
    }
    description.push_str(&format!("\nThis rank now has {} role(s) configured.", existing_ids.len()));

    let success_embed = CE::new()
        .title("Rank Roles Updated")
        .description(description)
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/rankroleremove` - Remove Discord role(s) from a rank (supports multiple roles at once)
pub async fn cmd_rank_remove(cc: &CC<'_>, rank_name: String, role_mentions: String) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can configure rank roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

    // Parse the rank
    let rank = parse_rank_name(&rank_name)?;
    if rank.is_none() {
        let response = CIR::Message(CIRM::new()
            .content("Invalid rank name. Valid ranks: Beginner, Newcomer, Novice, Apprentice, Journeyman, Expert, Master, MasterElite, Grandmaster")
            .ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    let rank = rank.unwrap();

    // Parse multiple role mentions (space-separated)
    let role_mentions_vec: Vec<&str> = role_mentions.split_whitespace().collect();
    let guild_roles = cc.ctx.http.get_guild_roles(guild_id).await?;

    let mut roles_to_remove = Vec::new();
    for role_mention in role_mentions_vec {
        let role_id_str = parse_role_id(role_mention)?;
        let role_id = match role_id_str.parse::<u64>() {
            Ok(id) => serenity::all::RoleId::new(id),
            Err(_) => {
                let response = CIR::Message(CIRM::new()
                    .content(format!("Invalid role format: {role_mention}. Please mention roles or provide role IDs."))
                    .ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        };

        // Get role name for display
        let role_name = guild_roles.iter()
            .find(|r| r.id == role_id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| format!("Unknown ({role_id})"));

        roles_to_remove.push((role_id, role_name));
    }

    // Get existing role IDs for this rank
    let mut existing_ids = rank.role_ids(&cc.db, guild_id.get()).await;
    let mut removed_roles = Vec::new();
    let mut not_found_roles = Vec::new();

    for (role_id, role_name) in roles_to_remove {
        if existing_ids.contains(&role_id) {
            existing_ids.retain(|id| *id != role_id);
            log_rank_remove(&role_name, rank.name());
            removed_roles.push(role_name);
        } else {
            not_found_roles.push(role_name);
        }
    }

    // Save to database
    let role_ids_str = existing_ids.iter()
        .map(|id| id.get().to_string())
        .collect::<Vec<_>>()
        .join(",");
    cc.db.config.set_config(rank.config_key(), &role_ids_str, guild_id.get()).await?;

    let mut description = String::new();
    if !removed_roles.is_empty() {
        description.push_str(&format!("**Removed {} role(s) from rank {}:**\n", removed_roles.len(), rank.name()));
        for role in &removed_roles {
            description.push_str(&format!("  • {role}\n"));
        }
    }
    if !not_found_roles.is_empty() {
        description.push_str(&format!("\n**Skipped {} role(s) (not configured for this rank):**\n", not_found_roles.len()));
        for role in &not_found_roles {
            description.push_str(&format!("  • {role}\n"));
        }
    }
    description.push_str(&format!("\nThis rank now has {} role(s) configured.", existing_ids.len()));

    let success_embed = CE::new()
        .title("Rank Roles Updated")
        .description(description)
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/rankrolelist` - List all role mappings for a rank
pub async fn cmd_rank_list(cc: &CC<'_>, rank_name: Option<String>) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can view rank role configurations!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_roles = cc.ctx.http.get_guild_roles(guild_id).await?;

    let mut description = String::new();

    // If specific rank provided, show only that rank
    if let Some(rank_name_str) = rank_name {
        let rank = parse_rank_name(&rank_name_str)?;
        if rank.is_none() {
            let response = CIR::Message(CIRM::new()
                .content("Invalid rank name. Valid ranks: Beginner, Newcomer, Novice, Apprentice, Journeyman, Expert, Master, MasterElite, Grandmaster")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
        let rank = rank.unwrap();

        let role_ids = rank.role_ids(&cc.db, guild_id.get()).await;
        let elo = rank.elo_from_config(&cc.db, guild_id.get()).await;
        description.push_str(&format!("**{elo} [ELO: {}]**:\n", rank.name()));

        if role_ids.is_empty() {
            description.push_str("  *No roles configured*\n");
        } else {
            for role_id in role_ids {
                let _ = guild_roles.iter()
                    .find(|r| r.id == role_id)
                    .map(|r| r.name.clone())
                    .unwrap_or_else(|| format!("Unknown ({role_id})"));
                description.push_str(&format!("- <@&{role_id}>\n"));
            }
        }
    } else {
        // Show all ranks
        use crate::models::Rank;
        let all_ranks = [
            Rank::Beginner,
            Rank::Newcomer,
            Rank::Novice,
            Rank::Apprentice,
            Rank::Journeyman,
            Rank::Expert,
            Rank::Master,
            Rank::MasterElite,
            Rank::Grandmaster,
        ];

        for rank in all_ranks {
            let role_ids = rank.role_ids(&cc.db, guild_id.get()).await;
            let elo = rank.elo_from_config(&cc.db, guild_id.get()).await;
            description.push_str(&format!("**{elo} [ELO: {}]**:\n", rank.name()));

            if role_ids.is_empty() {
                description.push_str("  *No roles configured*\n");
            } else {
                for role_id in role_ids {
                    let _ = guild_roles.iter()
                        .find(|r| r.id == role_id)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| format!("Unknown ({role_id})"));
                    description.push_str(&format!("- <@&{role_id}>\n"));
                }
            }
            description.push('\n');
        }
    }

    let embed = CE::new()
        .title("Rank Role Mappings")
        .description(description)
        .color(0x3498db);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// Parse rank name to Rank enum
pub fn parse_rank_name(rank_name: &str) -> Result<Option<crate::models::Rank>> {
    use crate::models::Rank;
    let rank = match rank_name.to_lowercase().as_str() {
        "beginner"                     => Some(Rank::Beginner),
        "newcomer"                     => Some(Rank::Newcomer),
        "novice"                       => Some(Rank::Novice),
        "apprentice"                   => Some(Rank::Apprentice),
        "journeyman"                   => Some(Rank::Journeyman),
        "expert"                       => Some(Rank::Expert),
        "master"                       => Some(Rank::Master),
        "masterelite" | "master elite" => Some(Rank::MasterElite),
        "grandmaster"                  => Some(Rank::Grandmaster),
        _ => None,
    };
    Ok(rank)
}

/// Parse role ID from mention format <@&123456> or raw ID
pub fn parse_role_id(role_str: &str) -> Result<String> {
    if role_str.starts_with("<@&") && role_str.ends_with('>') {
        Ok(role_str[3..role_str.len()-1].to_string())
    } else {
        Ok(role_str.to_string())
    }
}

/// Format role IDs as Discord mentions for display
fn format_role_mentions(role_ids_str: Option<String>) -> String {
    role_ids_str.map(|r| {
        r.split(',')
            .map(|id| format!("<@&{}>", id.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    }).unwrap_or_else(|| "Not set".to_string())
}

/// Parse multiple role IDs from a string containing space or comma separated role mentions/IDs
fn parse_multiple_role_ids(role_str: &str) -> Result<Vec<String>> {
    let mut role_ids = Vec::new();

    // Split by both spaces and commas, then filter empty strings
    for part in role_str.split([' ', ',']) {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            role_ids.push(parse_role_id(trimmed)?);
        }
    }

    if role_ids.is_empty() {
        return Err(anyhow!("No valid role IDs found"));
    }

    Ok(role_ids)
}

fn log_rank_remove(rank_name: &str, role_name: &str) {
    info!("- Removed role '{}' from rank {}", role_name, rank_name);
}

/// `/setupadd` - Creates both roles and a new group with channels
pub async fn cmd_setup_add(cc: &CC<'_>, server: &mut Server) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can run setup!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    let guild_name = cc.ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());

    let loading_embed = CE::new()
        .title("Setting Up PUG Bot")
        .description("Creating roles and group channels...\nThis may take a moment.")
        .color(0xffaa00);

    let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Step 1: Create Runner role
    let runner_role = match guild_id.create_role(&cc.ctx.http,
        EditRole::new()
            .name("PUG Runner")
            .colour(0x3498db)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create Runner role: {e}"))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Step 2: Create Admin role
    let admin_role = match guild_id.create_role(&cc.ctx.http,
        EditRole::new()
            .name("PUG Admin")
            .colour(0xe74c3c)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create Admin role: {e}"))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Step 3: Save roles to database
    if let Err(e) = cc.db.config.set_config("runner_role", &runner_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {e}");
    }
    if let Err(e) = cc.db.config.set_config("admin_role", &admin_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {e}");
    }

    // Step 4: Create rank roles
    info!("[{}] Creating rank roles", guild_name);
    if let Err(e) = create_rank_roles(cc.ctx, &cc.db, guild_id).await {
        warn!("[{}] Failed to create rank roles: {}", guild_name, e);
    }

    // Step 5: Create group channels
    let (category_id, dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel) =
        match create_group_channels(cc.ctx, guild_id).await {
            Ok(channels) => channels,
            Err(e) => {
                let error_embed = CE::new()
                    .title("Setup Failed")
                    .description(format!("Failed to create channels: {e}\n\nRoles were created successfully."))
                    .color(0xff0000);

                cc.intax.edit_response(&cc.ctx.http,
                    serenity::all::EditInteractionResponse::new().embed(error_embed)
                ).await?;
                return Ok(());
            }
        };

    // Step 6: Create temporary Group and publish dashboard
    use crate::models::{Group, Channels, TeamChannel};
    use serenity::all::MessageId;

    let mut temp_group = Group {
        group_id: 0,
        quota: crate::DEFAULT_QUOTA,
        timeout: crate::DEFAULT_TIMEOUT,
        dashboard_msg: MessageId::new(1),
        channels: Channels {
            queue_chat: queue_channel,
            queue_vc: queue_vc_channel,
            teams: vec![TeamChannel {
                red_vc: red_channel,
                blu_vc: blue_channel,
            }],
            dashboard: dashboard_channel,
        },
        sessions: vec![],
        connect_info: None,
    };

    // Publish the dashboard to get message ID
    match temp_group.dash_publish(cc.ctx, dashboard_channel, &cc.db, guild_id.get()).await {
        Ok(_) => {
            let dashboard_msg_id = temp_group.dashboard_msg.get();

            // Step 7: Save group to database
            let group_config = crate::database::repositories::group::GroupConfig {
                dashboard_channel_id: dashboard_channel.get(),
                chat_channel_id: queue_channel.get(),
                queue_vc_id: queue_vc_channel.get(),
                red_vc_id: red_channel.get(),
                blu_vc_id: blue_channel.get(),
                quota: crate::DEFAULT_QUOTA,
            };
            match cc.db.groups.create_group(
                guild_id.get(),
                dashboard_msg_id,
                group_config,
            ).await {
                Ok(db_group) => {
                    info!("[{}] Group {} saved to database", guild_name, db_group.group_id);

                    // Add group to in-memory server and create initial session
                    if let Err(e) = server.add_group(db_group.clone()) {
                        error!("Failed to add group to server: {e}");
                    }

                    let success_embed = CE::new()
                        .title("Setup Complete!")
                        .description(format!(
                            "PUG bot is now fully configured!\n\n\
                            **Roles Created:**\n\
                            • Runner: <@&{}>\n\
                            • Admin: <@&{}>\n\
                            • Rank Roles: Created\n\n\
                            **Group Created:**\n\
                            • Dashboard: <#{}>\n\
                            • Queue Text: <#{}>\n\
                            • Queue Voice: <#{}>\n\
                            • Red Team: <#{}>\n\
                            • Blue Team: <#{}>\n\
                            • Category: <#{}>\n\n\
                            **Ready to use!** Players can join the queue now.",
                            runner_role.id,
                            admin_role.id,
                            dashboard_channel.get(),
                            queue_channel.get(),
                            queue_vc_channel.get(),
                            red_channel.get(),
                            blue_channel.get(),
                            category_id.get()
                        ))
                        .color(0x00ff00);

                    cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(success_embed)
                    ).await?;
                },
                Err(e) => {
                    // Database save failed - clean up everything
                    info!("[{}] Database save failed, cleaning up channels and dashboard", guild_name);
                    let _ = dashboard_channel.delete_message(&cc.ctx.http, dashboard_msg_id).await;
                    let _ = dashboard_channel.delete(&cc.ctx.http).await;
                    let _ = queue_channel.delete(&cc.ctx.http).await;
                    let _ = queue_vc_channel.delete(&cc.ctx.http).await;
                    let _ = red_channel.delete(&cc.ctx.http).await;
                    let _ = blue_channel.delete(&cc.ctx.http).await;
                    let _ = category_id.delete(&cc.ctx.http).await;

                    let error_embed = CE::new()
                        .title("Setup Failed")
                        .description(format!("Failed to save group to database: {e}\n\nChannels were cleaned up. Roles remain."))
                        .color(0xff0000);

                    cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(error_embed)
                    ).await?;
                }
            }
        },
        Err(e) => {
            // Dashboard creation failed - clean up the created channels
            info!("[{}] Dashboard creation failed, cleaning up channels", guild_name);
            let _ = dashboard_channel.delete(&cc.ctx.http).await;
            let _ = queue_channel.delete(&cc.ctx.http).await;
            let _ = queue_vc_channel.delete(&cc.ctx.http).await;
            let _ = red_channel.delete(&cc.ctx.http).await;
            let _ = blue_channel.delete(&cc.ctx.http).await;
            let _ = category_id.delete(&cc.ctx.http).await;

            let error_embed = CE::new()
                .title("Setup Failed")
                .description(format!("Failed to create dashboard: {e}\n\nChannels were cleaned up. Roles remain."))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
        }
    }

    Ok(())
}

/// `/setuplink` - Links existing roles and channels
pub async fn cmd_setup_link(cc: &CC<'_>) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can run setup!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let embed = CE::new()
        .title("Link Existing Configuration")
        .description(
            "To link existing roles and channels, use these commands:\n\n\
            **Link Roles:**\n\
            `/rolelink runner_role:@Runner admin_role:@Admin`\n\n\
            **Link Group Channels:**\n\
            `/grouplink` (run in the dashboard channel)\n\n\
            Or create new ones with:\n\
            • `/roleadd` - Create new roles\n\
            • `/groupadd` - Create new group channels"
        )
        .color(0x3498db);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/groupremove` - Remove a group from the server
///
/// * `group_id` - The ID of the group to remove (0 = auto-detect from current channel)
pub async fn cmd_group_remove(cc: &CC<'_>, server: &mut Server, group_id: u8) -> Result<()> {
    info!("Processing /groupremove for group_id: {}", group_id);

    // Check admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can remove groups!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.expect("Guild ID not found");
    let channel_id = cc.intax.channel_id;

    // Determine which group to remove
    let group_index = if group_id == 0 {
        // Auto-detect group from current channel
        server.groups.iter().position(|g| g.contains_channel(channel_id))
    } else {
        // Use provided group_id
        server.groups.iter().position(|g| g.group_id == group_id)
    };

    match group_index {
        Some(index) => {
            let group = &server.groups[index];
            let actual_group_id = group.group_id;
            let channels = group.channels.clone();

            // Send response immediately before deleting channels
            let loading_embed = CE::new()
                .title("Removing Group")
                .description(format!(
                    "Removing group {actual_group_id} and deleting all associated channels...\n\nThis may take a moment.",
                ))
                .color(0xffaa00);

            let response = CIR::Message(CIRM::new().embed(loading_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;

            // Get user for DM
            let user = cc.intax.user.clone();

            // Remove from database
            match cc.db.groups.delete(actual_group_id).await {
                Ok(_) => {

                    // Remove from in-memory server
                    server.groups.remove(index);

                    // Get category ID from one of the channels before deleting them
                    let category_id = match channels.dashboard.to_channel(&cc.ctx.http).await {
                        Ok(channel) => {
                            if let Some(guild_channel) = channel.guild() {
                                guild_channel.parent_id
                            } else {
                                None
                            }
                        },
                        Err(e) => {
                            warn!("Failed to get dashboard channel info: {e}");
                            None
                        }
                    };

                    // Delete Discord channels
                    let mut deleted_channels = Vec::new();
                    let mut failed_channels = Vec::new();

                    // Delete dashboard channel
                    if let Err(e) = channels.dashboard.delete(&cc.ctx.http).await {
                        warn!("Failed to delete dashboard channel: {e}");
                        failed_channels.push("dashboard");
                    } else {
                        deleted_channels.push("dashboard");
                    }

                    // Delete queue text channel
                    if let Err(e) = channels.queue_chat.delete(&cc.ctx.http).await {
                        warn!("Failed to delete queue text channel: {e}");
                        failed_channels.push("queue text");
                    } else {
                        deleted_channels.push("queue text");
                    }

                    // Delete queue voice channel
                    if let Err(e) = channels.queue_vc.delete(&cc.ctx.http).await {
                        warn!("Failed to delete queue voice channel: {e}");
                        failed_channels.push("queue voice");
                    } else {
                        deleted_channels.push("queue voice");
                    }

                    // Delete team voice channels
                    for (i, team) in channels.teams.iter().enumerate() {
                        if let Err(e) = team.red_vc.delete(&cc.ctx.http).await {
                            warn!("Failed to delete red team channel {}: {}", i, e);
                            failed_channels.push("red team");
                        } else {
                            deleted_channels.push("red team");
                        }

                        if let Err(e) = team.blu_vc.delete(&cc.ctx.http).await {
                            warn!("Failed to delete blue team channel {}: {}", i, e);
                            failed_channels.push("blue team");
                        } else {
                            deleted_channels.push("blue team");
                        }
                    }

                    // Delete the category after all channels are deleted
                    if let Some(cat_id) = category_id {
                        if let Err(e) = cat_id.delete(&cc.ctx.http).await {
                            warn!("Failed to delete category: {e}");
                            failed_channels.push("category");
                        } else {
                            deleted_channels.push("category");

                        }
                    }

                    let mut description = format!("Successfully removed group {actual_group_id}.");

                    if !deleted_channels.is_empty() {
                        description.push_str(&format!(
                            "\n\n**Deleted {} channel{}:**\n• {}",
                            deleted_channels.len(),
                            if deleted_channels.len() == 1 { "" } else { "s" },
                            deleted_channels.join("\n• ")
                        ));
                    }

                    if !failed_channels.is_empty() {
                        description.push_str(&format!(
                            "\n\n**Failed to delete {} channel{}:**\n• {}",
                            failed_channels.len(),
                            if failed_channels.len() == 1 { "" } else { "s" },
                            failed_channels.join("\n• ")
                        ));
                    }

                    let success_embed = CE::new()
                        .title("Group Removed")
                        .description(description)
                        .color(0x00ff00);

                    // Try to edit the original response first
                    if let Err(e) = cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(success_embed.clone())
                    ).await {
                        warn!("Failed to edit response (channel may be deleted): {e}");

                        // If that fails, send a DM to the user
                        if let Err(dm_err) = user.direct_message(&cc.ctx.http,
                            serenity::all::CreateMessage::new().embed(success_embed)
                        ).await {warn!("Failed to send DM to user: {}", dm_err);
                        } else {
                            info!("Sent group removal confirmation via DM to user {}", user.id);
                        }
                    }
                },
                Err(e) => {
                    warn!("[Guild: {}] Failed to remove group {} from database: {}", guild_id, actual_group_id, e);

                    let error_embed = CE::new()
                        .title("Failed to Remove Group")
                        .description(format!("Error: {e}"))
                        .color(0xff0000);

                    // Edit the loading message with error
                    cc.intax.edit_response(&cc.ctx.http,
                        serenity::all::EditInteractionResponse::new().embed(error_embed)
                    ).await?;
                }
            }
        },
        None => {
            // Group not found
            let error_message = if group_id == 0 {
                "No group found for this channel. Use this command in a channel that belongs to a group."
            } else {
                "Group not found with the specified ID."
            };

            let groups_list = if server.groups.is_empty() {
                "No groups configured for this server.".to_string()
            } else {
                let mut list = String::from("Available groups:\n");
                for g in &server.groups {
                    list.push_str(&format!("• Group {} (Dashboard: <#{}>)\n", g.group_id, g.channels.dashboard.get()));
                }
                list
            };

            let error_embed = CE::new()
                .title("Group Not Found")
                .description(format!(
                    "{error_message}\n\n{groups_list}",
                ))
                .color(0xff0000);

            let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }

    Ok(())
}

/// `/toggledm` - Toggle DM notifications when a game is ready
pub async fn cmd_toggle_dm(cc: &CC<'_>) -> Result<()> {
    let user_id = cc.intax.user.id;

    // Toggle the DM preference
    let new_state = cc.db.users.toggle_dm_enabled(user_id).await?;

    let (status_text, status_emoji) = if new_state {
        ("enabled", "🔔")
    } else {
        ("disabled", "🔕")
    };

    let embed = CE::new()
        .title("DM Notifications Updated")
        .description(format!(
            "{status_emoji} DM notifications are now **{status_text}**\n\n\
            You will {a} receive a DM when a game is ready.\n",
            a = if new_state { "now" } else { "no longer" }
        ))
        .color(if new_state { 0x00ff00 } else { 0xff9900 });

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/settings` - Open personal settings menu in DMs
pub async fn cmd_settings(cc: &CC<'_>) -> Result<()> {
    use serenity::all::CreateMessage as CM;

    let user_id = cc.intax.user.id;

    // Acknowledge the command first
    let response = CIR::Message(CIRM::new()
        .content("Opening your settings in DMs...")
        .ephemeral(true)
    );
    cc.intax.create_response(&cc.ctx.http, response).await?;

    // Get current settings and user structs
    let settings = cc.db.users.get_settings(user_id).await?;
    let user     = cc.ctx.http.get_user(user_id).await?;

    // Use helper functions from settings module to build embed and buttons
    let embed   = build_settings_embed(&settings);
    let buttons = build_settings_buttons(&settings);
 
    // Try to send DM
    match user.direct_message(&cc.ctx.http, CM::new().embed(embed).components(buttons)).await {
        Ok(msg) => {
            // Track this message for cleanup after 10 minutes of inactivity
            if let Some(dm_tracker) = cc.ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
                dm_tracker.track_message(user_id, msg.channel_id, msg.id, user.tag()).await;
            }
            info!("Sent settings menu to user {}", user.tag());
        }
        Err(e) => {
            warn!("Failed to send settings DM to user {}: {}", user.tag(), e);

            // Update the ephemeral response with error
            let error_embed = CE::new()
                .title("Cannot Send DM")
                .description(
                    "I couldn't send you a DM! Please check that:\n\
                    • You have DMs enabled in your Discord privacy settings\n\
                    • You haven't blocked the bot\n\n\
                    To enable DMs: User Settings → Privacy & Safety → Allow direct messages from server members"
                )
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
        }
    }

    Ok(())
}
