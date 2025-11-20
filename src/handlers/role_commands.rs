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

/// `/rankroleadd` - Add an existing Discord role to a rank (supports multiple roles per rank)
pub async fn cmd_rank_role_add(cc: &CC<'_>, rank_name: String, role_mention: String) -> Result<()> {
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can configure rank roles!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
    
    // Parse the rank
    use crate::models::Rank;
    let rank = match rank_name.to_lowercase().as_str() {
        "beginner"                     => Rank::Beginner,
        "newcomer"                     => Rank::Newcomer,
        "novice"                       => Rank::Novice,
        "apprentice"                   => Rank::Apprentice,
        "journeyman"                   => Rank::Journeyman,
        "expert"                       => Rank::Expert,
        "master"                       => Rank::Master,
        "masterelite" | "master elite" => Rank::MasterElite,
        "grandmaster"                  => Rank::Grandmaster,
        _ => {
            let response = CIR::Message(CIRM::new()
                .content("Invalid rank name. Valid ranks: Beginner, Newcomer, Novice, Apprentice, Journeyman, Expert, Master, MasterElite, Grandmaster")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

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

    // Verify the role exists
    let guild_roles = cc.ctx.http.get_guild_roles(guild_id).await?;
    let role = match guild_roles.iter().find(|r| r.id == role_id) {
        Some(r) => r,
        None => {
            let response = CIR::Message(CIRM::new()
                .content("Role not found in this server.")
                .ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    // Get existing role IDs for this rank
    let mut existing_ids = rank.role_ids(&cc.db, guild_id.get()).await;
    
    // Check if this role is already configured for this rank
    if existing_ids.contains(&role_id) {
        let response = CIR::Message(CIRM::new()
            .content(format!("Role '{}' is already configured for rank {}!", role.name, rank.name()))
            .ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    // Add the new role ID
    existing_ids.push(role_id);
    let role_ids_str = existing_ids.iter()
        .map(|id| id.get().to_string())
        .collect::<Vec<_>>()
        .join(",");

    // Save to database
    cc.db.config.set_config(rank.config_key(), &role_ids_str, guild_id.get()).await?;

    info!("Added role '{}' (ID: {}) to rank {} for guild {}", role.name, role_id, rank.name(), guild_id);

    let success_embed = CE::new()
        .title("Rank Role Added")
        .description(format!(
            "Successfully added role '{}' to rank **{}**.\n\n\
            This rank now has {} role(s) configured.",
            role.name, rank.name(), existing_ids.len()
        ))
        .color(0x00ff00);

    let response = CIR::Message(CIRM::new().embed(success_embed).ephemeral(true));
    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// Parse role ID from mention format <@&123456> or raw ID
fn parse_role_id(role_str: &str) -> Result<String> {
    if role_str.starts_with("<@&") && role_str.ends_with('>') {
        Ok(role_str[3..role_str.len()-1].to_string())
    } else {
        Ok(role_str.to_string())
    }
}
