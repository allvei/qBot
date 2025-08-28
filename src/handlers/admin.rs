// CHECK ME

use anyhow::Result;
use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use tracing::info;

use crate::handlers::player::check_role;
use crate::models::player::Role;
use crate::{CommandContext as CC};

/// `/buffer`
///
/// * `user_mention` - The user mention to buffer.
pub async fn buffer(
    cc: &CC<'_>,
    user_mention: String,
) -> Result<()> {
    info!("Processing buffer command for user mention: {}", user_mention);
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can buffer players!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    // TODO: Actually buffer the player
    Ok(())
}

/// `/config`
///
/// * `key`         - The key to modify.
/// * `value`       - The value to set for the key.
pub async fn config(
    cc: &CC<'_>,
    key: String,
    value: Option<
        String,
    >,
) -> Result<()> {
    info!("Processing config: key={}, value={:?}", key, value);
    if !check_role(cc, &Role::Admin).await? {
        info!("User is not an admin");
        let response = CIR::Message(CIRM::new().content("Only session admins can modify the config!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }

    if let Some(
        val,
    ) = value {
        info!("Setting config {} = {}", key, val);

        cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found").get()).await?;

        let embed = CE::new().title("Config Updated").description(format!("Set `{}` = `{}`", key, val));

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    } else {
        let config = match cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found").get()).await {
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
            config.guild_id, config.dashboard_tc_id, config.red_vc_id, config.blu_vc_id, config.join_timeout, config.runner_r_id, config.admin_r_id
        );

        let embed = CE::new().title("Bot Configuration").description(config_text);

        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

        cc.intax.create_response(&cc.ctx.http, response).await?;
    }

    Ok(())
}

/// `/init_dashboard`
pub async fn init_dashboard(
    cc: &CC<'_>,
) -> Result<()> {
    info!("Processing init_dashboard command");
    if !check_role(cc, &Role::Admin).await? {
        let response = CIR::Message(CIRM::new().content("Only admins can set up the dashboard!").ephemeral(true));
        cc.intax.create_response(&cc.ctx.http, response).await?;
        return Ok(());
    }
    
    let channel_id = cc.intax.channel_id.get();
    
    // Get the group from database using channel_id since there can be multiple groups per guild
    let group = match cc.db.get_group_by_channel(channel_id).await {
        Ok(group) => group,
        Err(e) => {
            let response = CIR::Message(CIRM::new().content(format!("Failed to get group for this channel: {}", e)).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };
    
    match group.init_dashboard(cc.ctx, channel_id).await {
        Ok(true) => {
            let response = CIR::Message(CIRM::new().content("Dashboard setup complete!").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        },
        Ok(false) => {
            let response = CIR::Message(CIRM::new().content("Failed to set up dashboard: channel ID mismatch.").ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        },
        Err(e) => {
            let response = CIR::Message(CIRM::new().content(format!("Failed to set up dashboard: {}", e)).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    }
    
    Ok(())
}

/// Parses a user mention to get the Discord ID.
///
/// * `mention` - The user mention to parse.
fn parse_user_mention(
    mention: &str,
) -> Result<u64> {
    // Parse Discord user mention format: <@!123456789> or <@123456789>
    let mention =
        mention
            .trim();
    if mention.starts_with("<@!") && mention.ends_with('>') {
        Ok(mention[3..mention.len() - 1].to_string().parse::<u64>().unwrap())
    } else if mention.starts_with("<@") && mention.ends_with('>') {
        Ok(mention[2..mention.len() - 1].to_string().parse::<u64>().unwrap())
    } else {
        // Assume it's already a raw user ID
        Ok(mention.parse::<u64>().unwrap())
    }
}
