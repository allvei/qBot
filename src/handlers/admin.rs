// CHECK ME

use anyhow::Result;
use serenity::all::{Colour, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage};
use tracing::info;

use crate::handlers::role::check_role;
use crate::models::player::Role;
use crate::CommandContext;

/// Handles the `/buffer` command, guarantees the player a spot in the next match.
///
/// * `user_mention` - The user mention to buffer.
pub async fn buffer(cc: &CommandContext<'_>, user_mention: String) -> Result<()> {
    info!("[admin.rs] Processing buffer command for user mention: {}", user_mention);
    if !check_role(cc, &Role::Admin).await? {
        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Only admins can buffer players!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    Ok(())
}

/// Handles the /config command, which allows admins to modify bot configuration.
///
/// * `key`         - The key to modify.
/// * `value`       - The value to set for the key.
pub async fn config(cc: &CommandContext<'_>, key: String, value: Option<String>) -> Result<()> {
    info!("[admin.rs] Processing config command: key={}, value={:?}", key, value);
    info!("[admin.rs] Checking admin permissions for config command");
    // Check if user has admin permissions
    if !check_role(cc, &Role::Admin).await? {
        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Only session admins can modify the config!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val) = value {
        info!("[admin.rs] Setting config {} = {}", key, val);
        // Set config value
        cc.db.get_config().await?;

        let embed = CreateEmbed::new().title("Config Updated")
                                      .description(format!("Set `{}` = `{}`", key, val))
                                      .colour(Colour::from_rgb(81, 207, 102));

        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        // Get current config
        let config = match cc.db.get_config().await {
            Ok(cfg) => cfg,
            Err(e) => {
                let err_embed = CreateEmbed::new().title("Failed to Load Config")
                                                  .description(format!("Error: {e}\nPlease create a config using `/config`."))
                                                  .colour(Colour::from_rgb(219, 47, 56));
                let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(err_embed).ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        };

        let config_text = format!("**Current Configuration:**\n\
                                  Guild: `{}`\n\
                                  Queue Channel: `{}`\n\
                                  RED Channel: `{}`\n\
                                  BLU Channel: `{}`\n\
                                  Confirmation Timeout: `{}s`\n\
                                  Runner Role: `{}`\n\
                                  Admin Role: `{}`",
                                  config.guild_id, config.ic_queue, config.ic_red, config.ic_blue, config.join_timeout, config.i_runner, config.i_admin);

        let embed = CreateEmbed::new().title("Bot Configuration").description(config_text).colour(Colour::from_rgb(51, 175, 240));

        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    }

    Ok(())
}

/// Parses a user mention to get the Discord ID.
///
/// * `mention` - The user mention to parse.
fn parse_user_mention(mention: &str) -> Result<u64> {
    // Parse Discord user mention format: <@!123456789> or <@123456789>
    let mention = mention.trim();
    if mention.starts_with("<@!") && mention.ends_with('>') {
        Ok(mention[3..mention.len() - 1].to_string().parse::<u64>().unwrap())
    } else if mention.starts_with("<@") && mention.ends_with('>') {
        Ok(mention[2..mention.len() - 1].to_string().parse::<u64>().unwrap())
    } else {
        // Assume it's already a raw user ID
        Ok(mention.parse::<u64>().unwrap())
    }
}
