use anyhow::Result;
use serenity::{
    all::{CommandInteraction, Context, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, Colour, Timestamp, CreateMessage, ChannelId},
};
use std::sync::Arc;
use crate::database::Database;

pub async fn handle_bench_command(
    ctx: &Context,
    interaction: &CommandInteraction,
    db: Arc<Database>,
    user_mention: String,
) -> Result<()> {
    // Check if user has admin permissions
    if !has_admin_permissions(ctx, interaction, db.clone()).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You need admin permissions to bench players!")
                .ephemeral(true)
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    // Parse user mention to get Discord ID
    let target_user_id = parse_user_mention(&user_mention)?;
    
    // Get user from database
    let target_user = db.get_user_by_discord_id(&target_user_id).await?;
    
    if let Some(user) = target_user {
        // Remove user from queue and set status to benched
        db.leave_queue_by_user_id(user.id).await?;
        
        // In a more complete implementation, you'd add a "benched" status to queue_sessions
        // For now, we'll just remove them from queue
        
        let embed = CreateEmbed::new()
            .title("🔨 Player Benched")
            .description(format!("**{}** has been benched by {}", user.username, interaction.user.display_name()))
            .colour(Colour::from_rgb(255, 100, 100));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed)
        );
        
        interaction.create_response(&ctx.http, response).await?;
        
        // Log to admin channel
        let config = db.get_config().await?;
        if !config.log_channel_id.is_empty() {
            let channel_id: u64 = config.log_channel_id.parse()?;
            let channel = ChannelId::new(channel_id);
            
            let log_embed = CreateEmbed::new()
                .title("🔨 Admin Action: Bench")
                .description(format!("**{}** benched **{}**", interaction.user.display_name(), user.username))
                .colour(Colour::from_rgb(255, 100, 100))
                .timestamp(Timestamp::now());
            
            channel.send_message(&ctx.http, CreateMessage::new().embed(log_embed)).await?;
        }
    } else {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ User not found in database")
                .ephemeral(true)
        );
        interaction.create_response(&ctx.http, response).await?;
    }
    
    Ok(())
}

pub async fn handle_config_command(
    ctx: &Context,
    interaction: &CommandInteraction,
    db: Arc<Database>,
    key: String,
    value: Option<String>,
) -> Result<()> {
    // Check if user has admin permissions
    if !has_admin_permissions(ctx, interaction, db.clone()).await? {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content("❌ You need admin permissions to modify config!")
                .ephemeral(true)
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val) = value {
        // Set config value
        db.set_config(&key, &val).await?;
        
        let embed = CreateEmbed::new()
            .title("⚙️ Config Updated")
            .description(format!("Set `{}` = `{}`", key, val))
            .colour(Colour::from_rgb(81, 207, 102));
        
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed).ephemeral(true)
        );
        
        interaction.create_response(&ctx.http, response).await?;
    } else {
        // Get current config
        let config = db.get_config().await?;
        
        let config_text = format!(
            "**Current Configuration:**\n\
            Guild ID: `{}`\n\
            Queue Channel: `{}`\n\
            Server A - RED Channel: `{}`\n\
            Server A - BLU Channel: `{}`\n\
            Server A Channel: `{}`\n\
            Server B - RED Channel: `{}`\n\
            Server B - BLU Channel: `{}`\n\
            Server B Channel: `{}`\n\
            Server C - RED Channel: `{}`\n\
            Server C - BLU Channel: `{}`\n\
            Server C Channel: `{}`\n\
            Log Channel: `{}`\n\
            Queue Size: `{}`\n\
            Confirmation Timeout: `{}s`\n\
            Runner Role: `{}`\n\
            Admin Role: `{}`",
            config.guild_id,
            config.queue_channel_id,
            config.red_a_channel_id,
            config.blu_a_channel_id,
            config.server_a_channel_id,
            config.red_b_channel_id,
            config.blu_b_channel_id,
            config.server_b_channel_id,
            config.red_c_channel_id,
            config.blu_c_channel_id,
            config.server_c_channel_id,
            config.log_channel_id,
            config.queue_size,
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
        
        interaction.create_response(&ctx.http, response).await?;
    }
    
    Ok(())
}

async fn has_admin_permissions(
    ctx: &Context,
    interaction: &CommandInteraction,
    db: Arc<Database>,
) -> Result<bool> {
    let config = db.get_config().await?;
    
    if config.admin_role_id.is_empty() {
        return Ok(true); // If no admin role configured, allow everyone (for setup)
    }
    
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Not in a guild"))?;
    let member = guild_id.member(&ctx.http, interaction.user.id).await?;
    
    // Check if user has admin role
    let has_admin = member.roles.iter().any(|r| r.to_string() == config.admin_role_id);
    
    Ok(has_admin)
}

fn parse_user_mention(mention: &str) -> Result<String> {
    // Parse Discord user mention format: <@!123456789> or <@123456789>
    let mention = mention.trim();
    if mention.starts_with("<@!") && mention.ends_with('>') {
        Ok(mention[3..mention.len()-1].to_string())
    } else if mention.starts_with("<@") && mention.ends_with('>') {
        Ok(mention[2..mention.len()-1].to_string())
    } else {
        // Assume it's already a raw user ID
        Ok(mention.to_string())
    }
}
