// CHECK ME

use anyhow::Result;
use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use tracing::{error, info};

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
/// If no configuration exists, it will create a new one with the provided values.
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

    // Get the guild ID from the interaction
    let guild_id = match cc.intax.guild_id {
        Some(id) => id.get(),
        None => {
            let embed = CE::new().title("Error").description("This command must be used in a guild.").color(0xFF0000);
            let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        }
    };

    if let Some(val) = value {
        info!("Setting config {} = {}", key, val);

        // Try to get the config first to see if it exists
        let config_exists = cc.db.get_config(Some(guild_id)).await.is_ok();

        // If we're creating a new config, we need to make sure all required fields are set
        if !config_exists && key.to_lowercase() == "init" && val.to_lowercase() == "true" {
            // Create a new configuration with default values
            // These will be overridden by subsequent /config commands

            // Log the guild ID we're using for configuration
            info!("Initializing configuration for guild ID: {}", guild_id);

            let default_configs = [
                // The guild_id is critical and must be set correctly
                ("guild_id", guild_id.to_string()),
                ("runner_role_id", crate::models::config::ID_RUNNER.to_string()),
                ("admin_role_id", crate::models::config::ID_ADMIN.to_string()),
                ("queue_channel_id", crate::models::config::ID_QUEUE.to_string()),
                ("log_channel_id", crate::models::config::ID_DASHBOARD.to_string()),
                ("buffer_channel_id", crate::models::config::ID_CHAT.to_string()),
                ("red_channel_id", crate::models::config::ID_RED.to_string()),
                ("blue_channel_id", crate::models::config::ID_BLU.to_string()),
            ];

            for (k, v) in default_configs {
                if let Err(e) = cc.db.set_config(k, &v, guild_id).await {
                    error!("Failed to set default config {}: {}", k, e);
                }
            }

            // After setting up the config, also create a default group
            info!("Creating default group for guild {}", guild_id);
            let queue_channel_id = crate::models::config::ID_QUEUE;
            let log_channel_id = crate::models::config::ID_DASHBOARD;
            let buffer_channel_id = crate::models::config::ID_CHAT;
            let red_channel_id = crate::models::config::ID_RED;
            let blue_channel_id = crate::models::config::ID_BLU;

            match cc
                .db
                .new_group(
                    guild_id,
                    log_channel_id,    // dashboard
                    buffer_channel_id, // chat
                    queue_channel_id,  // queue
                    red_channel_id,    // red
                    blue_channel_id,   // blue
                    8,                 // default session quota
                )
                .await
            {
                Ok(_) => info!("Successfully created default group for guild {}", guild_id),
                Err(e) => error!("Failed to create default group for guild {}: {}", guild_id, e),
            }

            let embed = CE::new()
                .title("Configuration Created")
                .description("Default configuration has been created. Use `/config` without parameters to view it, and `/config key value` to modify specific values.")
                .color(0x00FF00);
            let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
            return Ok(());
        } else {
            // Set the specific config value
            cc.db.set_config(&key, &val, guild_id).await?;

            let embed = CE::new().title("Config Updated").description(format!("Set `{}` = `{}`", key, val));
            let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
            cc.intax.create_response(&cc.ctx.http, response).await?;
        }
    } else {
        let config = match cc.db.get_config(Some(guild_id)).await {
            Ok(cfg) => cfg,
            Err(_) => {
                // No config exists yet, automatically initialize it
                info!("No configuration found for guild {}, auto-initializing", guild_id);

                // Create a new configuration with default values
                info!("Initializing configuration for guild ID: {}", guild_id);

                let default_configs = [
                    // The guild_id is critical and must be set correctly
                    ("guild_id", guild_id.to_string()),
                    ("runner_role_id", crate::models::config::ID_RUNNER.to_string()),
                    ("admin_role_id", crate::models::config::ID_ADMIN.to_string()),
                    ("queue_channel_id", crate::models::config::ID_QUEUE.to_string()),
                    ("log_channel_id", crate::models::config::ID_DASHBOARD.to_string()),
                    ("buffer_channel_id", crate::models::config::ID_CHAT.to_string()),
                    ("red_channel_id", crate::models::config::ID_RED.to_string()),
                    ("blue_channel_id", crate::models::config::ID_BLU.to_string()),
                ];

                // Insert all default config values
                for (k, v) in default_configs {
                    info!("Setting default config {} = {}", k, v);
                    if let Err(e) = cc.db.set_config(k, &v, guild_id).await {
                        error!("Failed to set default config {}: {}", k, e);
                    }
                }

                // After setting up the config, also create a default group
                info!("Creating default group for guild {} during auto-initialization", guild_id);
                let queue_channel_id = crate::models::config::ID_QUEUE;
                let log_channel_id = crate::models::config::ID_DASHBOARD;
                let buffer_channel_id = crate::models::config::ID_CHAT;
                let red_channel_id = crate::models::config::ID_RED;
                let blue_channel_id = crate::models::config::ID_BLU;

                match cc
                    .db
                    .new_group(
                        guild_id,
                        log_channel_id,    // dashboard
                        buffer_channel_id, // chat
                        queue_channel_id,  // queue
                        red_channel_id,    // red
                        blue_channel_id,   // blue
                        8,                 // default session quota
                    )
                    .await
                {
                    Ok(_) => info!("Successfully created default group for guild {}", guild_id),
                    Err(e) => error!("Failed to create default group for guild {}: {}", guild_id, e),
                }

                // Now try to get the config again
                match cc.db.get_config(Some(guild_id)).await {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        // Still failed after initialization attempt
                        let embed = CE::new().title("Configuration Error").description(format!("Failed to initialize configuration: {}", e)).color(0xFF0000); // Red color for error

                        let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));

                        cc.intax.create_response(&cc.ctx.http, response).await?;
                        return Ok(());
                    }
                }
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
