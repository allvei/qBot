use anyhow::{anyhow, Result};
use serenity::all::{
    CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, GuildId, Permissions,
};
use serenity::builder::EditRole;
use tracing::{info, warn};

use crate::models::{CommandContext as CC, Role};
use super::player::{check_role, create_rank_roles};

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
    let runner_role = match guild_id.create_role(&cc.ctx.http, 
        EditRole::new()
            .name("PUG Runner")
            .colour(0x3498db)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Failed to Create Runner Role")
                .description(format!("Error: {}", e))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Create Admin role
    let admin_role = match guild_id.create_role(&cc.ctx.http,
        EditRole::new()
            .name("PUG Admin")
            .colour(0xe74c3c)
            .permissions(Permissions::empty())
    ).await {
        Ok(role) => role,
        Err(e) => {
            let error_embed = CE::new()
                .title("Failed to Create Admin Role")
                .description(format!("Error: {}", e))
                .color(0xff0000);

            cc.intax.edit_response(&cc.ctx.http,
                serenity::all::EditInteractionResponse::new().embed(error_embed)
            ).await?;
            return Ok(());
        }
    };

    // Save to database
    if let Err(e) = cc.db.config.set_config("runner_role", &runner_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save runner_role config: {}", e);
    }
    if let Err(e) = cc.db.config.set_config("admin_role", &admin_role.id.to_string(), guild_id.get()).await {
        warn!("Failed to save admin_role config: {}", e);
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

/// `/rolelink` - Link existing runner and admin roles
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

        let embed = CE::new()
            .title("Current Role Configuration")
            .description(format!(
                "**Current Roles:**\n\
                • Runner: {}\n\
                • Admin: {}\n\n\
                **Usage:**\n\
                `/rolelink runner_role:@Runner admin_role:@Admin`",
                current_runner.map(|r| format!("<@&{}>", r)).unwrap_or_else(|| "Not set".to_string()),
                current_admin.map(|r| format!("<@&{}>", r)).unwrap_or_else(|| "Not set".to_string())
            ));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let mut updated_roles = Vec::new();

    // Link runner role if provided
    if let Some(role_str) = runner_role {
        let role_id = parse_role_id(&role_str)?;
        cc.db.config.set_config("runner_role", &role_id, guild_id.get()).await?;
        updated_roles.push(format!("• Runner: <@&{}>", role_id));
    }

    // Link admin role if provided
    if let Some(role_str) = admin_role {
        let role_id = parse_role_id(&role_str)?;
        cc.db.config.set_config("admin_role", &role_id, guild_id.get()).await?;
        updated_roles.push(format!("• Admin: <@&{}>", role_id));
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
            "Removed {} role configuration.\n\n\
            **Note:** The Discord role itself was not deleted.",
            role_type
        ))
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/rankroleadd` - Add Discord role(s) to a rank (supports multiple roles at once)
pub async fn cmd_rank_role_add(cc: &CC<'_>, rank_name: String, role_mentions: String) -> Result<()> {
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
                    .content(format!("Invalid role format: {}. Please mention roles or provide role IDs.", role_mention))
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
                    .content(format!("Role {} not found in this server.", role_mention))
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
            description.push_str(&format!("  • {}\n", role));
        }
    }
    if !skipped_roles.is_empty() {
        description.push_str(&format!("\n**Skipped {} role(s) (already configured):**\n", skipped_roles.len()));
        for role in &skipped_roles {
            description.push_str(&format!("  • {}\n", role));
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

/// `/rankroleremove` - Remove a Discord role from a rank
pub async fn cmd_rank_role_remove(cc: &CC<'_>, rank_name: String, role_mention: String) -> Result<()> {
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

    // Parse role ID from mention
    let role_id_str = parse_role_id(&role_mention)?;
    let role_id = match role_id_str.parse::<u64>() {
        Ok(id) => serenity::all::RoleId::new(id),
        Err(_) => {
            let response = CIR::Message(CIRM::new()
                .content("Invalid role format. Please mention a role or provide a role ID.")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Get existing role IDs for this rank
    let mut existing_ids = rank.role_ids(&cc.db, guild_id.get()).await;
    
    // Check if this role is configured for this rank
    if !existing_ids.contains(&role_id) {
        let response = CIR::Message(CIRM::new()
            .content(format!("Role is not configured for rank {}!", rank.name()))
            .ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Get role name for display
    let guild_roles = cc.ctx.http.get_guild_roles(guild_id).await?;
    let role_name = guild_roles.iter()
        .find(|r| r.id == role_id)
        .map(|r| r.name.clone())
        .unwrap_or_else(|| format!("Unknown ({})", role_id));

    // Remove the role ID
    existing_ids.retain(|id| *id != role_id);
    let role_ids_str = existing_ids.iter()
        .map(|id| id.get().to_string())
        .collect::<Vec<_>>()
        .join(",");

    // Save to database
    cc.db.config.set_config(rank.config_key(), &role_ids_str, guild_id.get()).await?;

    info!("Removed role '{}' (ID: {}) from rank {} for guild {}", role_name, role_id, rank.name(), guild_id);

    let success_embed = CE::new()
        .title("Rank Role Removed")
        .description(format!(
            "Successfully removed role '{}' from rank **{}**.\n\n\
            This rank now has {} role(s) configured.",
            role_name, rank.name(), existing_ids.len()
        ))
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/rankrolelist` - List all role mappings for a rank
pub async fn cmd_rank_role_list(cc: &CC<'_>, rank_name: Option<String>) -> Result<()> {
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
        description.push_str(&format!("**{}** ({} role(s)):\n", rank.name(), role_ids.len()));
        
        if role_ids.is_empty() {
            description.push_str("  *No roles configured*\n");
        } else {
            for role_id in role_ids {
                let role_name = guild_roles.iter()
                    .find(|r| r.id == role_id)
                    .map(|r| r.name.clone())
                    .unwrap_or_else(|| format!("Unknown ({})", role_id));
                description.push_str(&format!("  • {} (<@&{}>)\n", role_name, role_id));
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
            description.push_str(&format!("**{}** ({} role(s)):\n", rank.name(), role_ids.len()));
            
            if role_ids.is_empty() {
                description.push_str("  *No roles configured*\n");
            } else {
                for role_id in role_ids {
                    let role_name = guild_roles.iter()
                        .find(|r| r.id == role_id)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| format!("Unknown ({})", role_id));
                    description.push_str(&format!("  • {} (<@&{}>)\n", role_name, role_id));
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
fn parse_rank_name(rank_name: &str) -> Result<Option<crate::models::Rank>> {
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
