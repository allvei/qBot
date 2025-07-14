// CHECK ME

use anyhow::Result;
use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use tracing::info;

use crate::handlers::role::check_role;
use crate::models::command::CommandContext;
use crate::models::player::Role;

/// Handles the `/buffer` command, guarantees the player a spot in the next match.
///
/// * `user_mention` - The user mention to buffer.
pub async fn buffer<'a>(
    cc: &'a CommandContext<'a>,
    user_mention: String,
) -> Result<(), anyhow::Error> {
    info!("Processing buffer command for user mention: {}", user_mention);
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can buffer players!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    // TODO: Actually buffer the player
    Ok(())
}

/// Handles the /config command, which allows admins to modify bot configuration.
///
/// * `key`         - The key to modify.
/// * `value`       - The value to set for the key.
pub async fn config<'a>(
    cc: &'a CommandContext<'a>,
    key: String,
    value: Option<String>,
) -> Result<()> {
    info!("Processing config: key={}, value={:?}", key, value);
    if !check_role(cc, &Role::Admin).await? {
        info!("User is not an admin");
        let response = CIR::Message(CIRM::new().content("Only session admins can modify the config!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(val) = value {
        info!("Setting config {} = {}", key, val);

        cc.db.get_config().await?;

        let embed = CE::new().title("Config Updated").description(format!("Set `{}` = `{}`", key, val));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        let config = match cc.db.get_config().await {
            Ok(cfg) => cfg,
            Err(e) => {
                let err_embed = CE::new().title("Failed to Load Config").description(format!("Error: {e}\nPlease create a config using `/config`."));
                let response = CIR::Message(CIRM::new().embed(err_embed).ephemeral(true));
                cc.intax.create_response(&cc.ctx.http, response).await?;
                return Ok(());
            }
        };

        let config_text = format!(
            "**Current Configuration:**\n\
                                  Guild: `{}`\n\
                                  Queue Channel: `{}`\n\
                                  RED Channel: `{}`\n\
                                  BLU Channel: `{}`\n\
                                  Confirmation Timeout: `{}s`\n\
                                  Runner Role: `{}`\n\
                                  Admin Role: `{}`",
            config.guild_id, config.ic_queue, config.ic_red, config.ic_blue, config.join_timeout, config.i_runner, config.i_admin
        );

        let embed = CE::new().title("Bot Configuration").description(config_text);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

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
