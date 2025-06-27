use anyhow::{Result, anyhow};
use serenity::{
    all::{CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, Colour, Timestamp, CreateMessage, ChannelId},
};
use crate::CommandContext;

/// Handles the `/buffer` command, guarantees the player a spot in the next match.
/// 
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

    let userid = cc.intax.user.id;
    let t_userid = parse_user_mention(&user_mention)?;
    
    if let Some(user) = t_userid {
        let embed = CreateEmbed::new()
            .title("🔨 Player Buffered")
            .description(format!("**{}** has been buffered by {}", user, cc.intax.user.display_name()))
            .colour(Colour::from_rgb(255, 100, 100));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed)
        );
        
        cc.intax.create_response(&cc.ctx.http, response).await?;
        
        // Log to admin channel
        let config = cc.db.get_config().await?;
        if config.cid_log != 0 {
            let channel = ChannelId::new(config.cid_log);
            
            let log_embed = CreateEmbed::new()
                .title("🔨 Admin Action: Buffer")
                .description(format!("**{}** buffered **{}**", cc.intax.user.display_name(), user.user))
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
            Queue Channel: `{}`\n\
            RED Channel: `{}`\n\
            BLU Channel: `{}`\n\
            Log Channel: `{}`\n\
            Queue Size: `{}`\n\
            Confirmation Timeout: `{}s`\n\
            Runner Role: `{}`\n\
            Admin Role: `{}`",
            config.cid_queue,
            config.cid_red,
            config.cid_blue,
            config.cid_log,
            config.queue_quota,
            config.confirmation_timeout,
            config.id_runner,
            config.id_admin
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
    
    if config.id_admin == 0 {
        return Ok(true); // If no admin role configured, allow everyone (for setup)
    }
    
    let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("Not in a guild"))?;
    let member   = guild_id.member(&cc.ctx.http, cc.intax.user.id).await?;
    
    // Check if user has admin role
    let has_admin = member.roles.iter().any(|r| *r == config.id_admin);
    
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
