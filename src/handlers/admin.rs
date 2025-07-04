// CHECK ME

use anyhow::{Result, anyhow};
use serenity::all::{CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, Colour};
use tracing::info;

use crate::CommandContext;

/// Handles the `/buffer` command, guarantees the player a spot in the next match.
/// 
/// * `user_mention` - The user mention to buffer.
pub async fn handle_buffer_command(
    cc:           &CommandContext<'_>,
    user_mention: String,
) -> Result<()> {
    info!("[admin] Processing buffer command for user mention: {}", user_mention);
    if !is_admin(cc).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ Only admins can buffer players!")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    Ok(())
}

/// Handles the /config command, which allows admins to modify bot configuration.
/// 
/// * `key`         - The key to modify.
/// * `value`       - The value to set for the key.
pub async fn handle_config_command(
    cc:    &CommandContext<'_>,
    key:   String,
    value: Option<String>,
) -> Result<()> {
    info!("[admin] Processing config command: key={}, value={:?}", key, value);
    info!("[admin] Checking admin permissions for config command");
    // Check if user has admin permissions
    if !is_admin(cc).await? {
        info!("[admin] Permission denied: User is not an admin");
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You need admin permissions to modify config!")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val) = value {
        info!("[admin] Setting config {} = {}", key, val);
        // Set config value
        cc.db.get_config().await?;
        
        let embed = CreateEmbed::new()
            .title("⚙️ Config Updated")
            .description(format!("Set `{}` = `{}`", key, val))
            .colour(Colour::from_rgb(81, 207, 102));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed).ephemeral(true)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        // Get current config
        let config = cc.db.get_config().await?;
        
        let config_text = format!(
            "**Current Configuration:**\n\
            Queue Channel: `{}`\n\
            RED Channel: `{}`\n\
            BLU Channel: `{}`\n\
            Log Channel: `{}`\n\
            Queue Size: `{}`\n\
            Confirmation Timeout: `{}s`\n\
            Runner Role: `{}`\n\
            Admin Role: `{}`",
            config.ic_queue,
            config.ic_red,
            config.ic_blue,
            config.ic_log,
            config.quota,
            config.join_timeout,
            config.i_runner,
            config.i_admin
        );
        
        let embed = CreateEmbed::new()
            .title("⚙️ Bot Configuration")
            .description(config_text)
            .colour(Colour::from_rgb(51, 175, 240));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed).ephemeral(true)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;
    }
    
    Ok(())
}

/// Checks if the user has admin permissions.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
async fn is_admin(cc: &CommandContext<'_>) -> Result<bool> {
    info!("[admin] Checking if user {} is an admin", cc.intax.user.id);
    let config = cc.db.get_config().await?; // Cache config
    
    if config.i_admin == 0 {
        return Ok(true); // If no admin role configured, allow everyone (for setup)
    }
    
    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Not in a guild"))?;
    let member   = guild_id.member(&cc.ctx.http, cc.intax.user.id).await?;
    
    // Check if user has admin role
    let has_admin = member.roles.iter().any(|r| *r == config.i_admin);
    
    Ok(has_admin)
}

/// Parses a user mention to get the Discord ID.
/// 
/// * `mention` - The user mention to parse.
fn parse_user_mention(mention: &str) -> Result<u64> {
    // Parse Discord user mention format: <@!123456789> or <@123456789>
    let mention = mention.trim();
    if mention.starts_with("<@!") && mention.ends_with('>') {
        Ok(mention[3..mention.len()-1].to_string().parse::<u64>().unwrap())
    } else if mention.starts_with("<@") && mention.ends_with('>') {
        Ok(mention[2..mention.len()-1].to_string().parse::<u64>().unwrap())
    } else {
        // Assume it's already a raw user ID
        Ok(mention.parse::<u64>().unwrap())
    }
}
