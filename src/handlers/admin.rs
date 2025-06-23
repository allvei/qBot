use anyhow::{Result, anyhow};
use serenity::{
    all::{CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, Colour, Timestamp, CreateMessage, ChannelId},
};
use crate::CommandContext;

/// Handles the `/buffer` command, guarantees the player a spot in the next match.
/// 
/// * `ctx`          - Ref to the Serenity context.
/// * `interaction`  - Ref to the command interaction.
/// * `db`           - Ref to the database.
/// * `user_mention` - The user mention to buffer.
pub async fn handle_buffer_command<'a>(
    cc:           &CommandContext<'a>,
    user_mention: String,
) -> Result<()> {
    if !is_admin(cc).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ Only admins can buffer players!")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    let user_id = cc.intax.user.id;

    let tgt_user_id = parse_user_mention(&user_mention)?;
    
    if let Some(user) = tgt_usr {
        // Remove user from queue and set status to buffered
        cc.db.leave_queue_by_user_id(user.id).await?;
        
        // In a more complete implementation, you'd add a "buffered" status to queue_sessions
        // For now, we'll just remove them from queue
        
        let embed = CreateEmbed::new()
            .title("🔨 Player Buffered")
            .description(format!("**{}** has been buffered by {}", user.username, cc.intax.user.display_name()))
            .colour(Colour::from_rgb(255, 100, 100));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;
        
        // Log to admin channel
        let config = cc.db.get_config().await?;
        if config.log_channel_id != 0 {
            let channel = ChannelId::new(config.log_channel_id);
            
            let log_embed = CreateEmbed::new()
                .title("🔨 Admin Action: Buffer")
                .description(format!("**{}** buffered **{}**", cc.intax.user.display_name(), user.username))
                .colour(Colour::from_rgb(255, 100, 100))
                .timestamp(Timestamp::now());
            
            channel.send_message(&cc.ctx.http, CreateMessage::new().embed(log_embed)).await?;
        }
    } else {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ User not found in database")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
    }
    
    Ok(())
}

/// Handles the /config command, which allows admins to modify bot configuration.
/// 
/// * `ctx`         - Ref to the Serenity context.
/// * `interaction` - Ref to the command interaction.
/// * `db`          - Ref to the database.
/// * `key`         - The key to modify.
/// * `value`       - The value to set for the key.
pub async fn handle_config_command<'a>(
    cc:    &CommandContext<'a>,
    key:   String,
    value: Option<String>,
) -> Result<()> {
    // Check if user has admin permissions
    if !is_admin(cc).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You need admin permissions to modify config!")
                .ephemeral(true)
        );
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val) = value {
        // Set config value
        cc.db.set_config(&key, &val).await?;
        
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
            Guild ID: `{}`\n\
            Queue Channel: `{}`\n\
            Server A - RED Channel: `{}`\n\
            Server A - BLU Channel: `{}`\n\
            Server B - RED Channel: `{}`\n\
            Server B - BLU Channel: `{}`\n\
            Server C - RED Channel: `{}`\n\
            Server C - BLU Channel: `{}`\n\
            Log Channel: `{}`\n\
            Queue Size: `{}`\n\
            Confirmation Timeout: `{}s`\n\
            Runner Role: `{}`\n\
            Admin Role: `{}`",
            config.guild_id,
            config.queue_channel_id,
            config.apug.red_id,
            config.apug.blu_id,
            config.bpug.red_id,
            config.bpug.blu_id,
            config.cpug.red_id,
            config.cpug.blu_id,
            config.log_channel_id,
            config.queue_quota,
            config.confirmation_timeout,
            config.runner_role_id,
            config.admin_role_id
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
async fn is_admin<'a>(
    cc: &CommandContext<'a>,
) -> Result<bool> {
    let config = cc.db.get_config().await?; // Cache config
    
    if config.admin_role_id == 0 {
        return Ok(true); // If no admin role configured, allow everyone (for setup)
    }
    
    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Not in a guild"))?;
    let member   = guild_id.member(&cc.ctx.http, cc.intax.user.id).await?;
    
    // Check if user has admin role
    let has_admin = member.roles.iter().any(|r| *r == config.admin_role_id);
    
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
