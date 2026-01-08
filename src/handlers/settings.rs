use anyhow::Result;
use serenity::all::{
    ComponentInteraction, ModalInteraction, Context, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateActionRow as CAR, CreateActionRow, CreateButton as CB,
    ButtonStyle as BS, EditMessage, CreateInputText, InputTextStyle, CreateModal,
    CreateEmbedFooter, CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK,
    CreateSelectMenuOption as CSMO, RoleId,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};
use crate::models::buttons::*;

use crate::{Database, row};

/// Messages to replace description spam with
const SPAM_REPLACEMENT_MESSAGES: &[&str] = &[
    "If this is shart city, then I am the mayor",
    "*proceeds to triple-dribble in front of your goal and die*",
    "If it wasn't for xCape, I'd be GM already...",
    "@ me = free pocket medic",
    "idk, glop bomb is the best way to score",
    "#removemedic",
    "#justiceforsleepy",
    "#justiceforwerxify",
    "im so fucking ass",
    "Rawr x3 nuzzles how are you pounces on you you're so warm"
];

/// Messages to replace footer spam with
const FOOTER_SPAM_REPLACEMENT_MESSAGES: &[&str] = &[
    "Mmmm, feet :3",
    "Go team!",
    "PUG PUG PUG!",
    "GG!",
    "qBot is best bot",
];

const SANITIZE_ALERTS_ENABLED: bool = false;
const MAX_ALERT_NEWLINES: usize = 4;
const MAX_ALERT_CHARS: usize = 180;

/// Check if text exceeds alert message limits (max 4 newlines, 180 chars)
fn exceeds_alert_limits(text: &str) -> bool {
    text.matches('\n').count() > MAX_ALERT_NEWLINES || text.chars().count() > MAX_ALERT_CHARS
}

/// Process text and replace with spam message if limits exceeded
fn sanitize_announcement_text(text: &str) -> String {
    if SANITIZE_ALERTS_ENABLED && exceeds_alert_limits(text) {
        use rand::Rng;
        let idx = rand::rng().random_range(0..SPAM_REPLACEMENT_MESSAGES.len());
        return SPAM_REPLACEMENT_MESSAGES[idx].to_string();
    }
    text.to_string()
}

/// Process footer text and replace with spam message if limits exceeded
fn sanitize_footer_text(text: &str) -> String {
    if SANITIZE_ALERTS_ENABLED && exceeds_alert_limits(text) {
        use rand::Rng;
        let idx = rand::rng().random_range(0..FOOTER_SPAM_REPLACEMENT_MESSAGES.len());
        return FOOTER_SPAM_REPLACEMENT_MESSAGES[idx].to_string();
    }
    text.to_string()
}

/// Handle settings button interactions in DMs
pub async fn handle_settings_button(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let user_id   = interaction.user.id;
    let button_id = &interaction.data.custom_id;
    let username  = &interaction.user.name;

    info!("[DM] {} pressed {}", username, button_id);

    // Update activity timestamp for DM cleanup tracking
    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
        dm_tracker.update_activity(user_id).await;
    }

    match button_id.as_str() {
        "settings_toggle_dm" => {
            // Toggle DM alerts
            let _new_state = db.users.toggle_dm_enabled(user_id).await?;

            // Acknowledge and update the settings menu directly (no popup)
            let settings = db.users.get_settings(user_id).await?;
            let embed = build_settings_embed(&settings);
            let buttons = build_settings_buttons(&settings);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_timeout" => {
            // Show time selection buttons inline (replace current message temporarily)
            let settings = db.users.get_settings(user_id).await?;
            let current_minutes = settings.expiry_duration.as_secs() / 60;
            
            let time_buttons = vec![
                CB::new("settings_timeout:30m").label("30 min").style(if current_minutes == 30 { BS::Success } else { BS::Secondary }),
                CB::new("settings_timeout:1h") .label("1 hour").style(if current_minutes == 60 { BS::Success } else { BS::Secondary }),
                CB::new("settings_timeout:2h") .label("2 hours").style(if current_minutes == 120 { BS::Success } else { BS::Secondary }),
                CB::new("settings_timeout:3h") .label("3 hours").style(if current_minutes == 180 { BS::Success } else { BS::Secondary }),
                CB::new("settings_timeout:4h") .label("4 hours").style(if current_minutes == 240 { BS::Success } else { BS::Secondary }),
            ];
            
            let cancel_button = vec![
                CB::new("settings_timeout:cancel").label("Cancel").style(BS::Danger),
            ];

            let embed = CE::new()
                .title("Set timeout length")
                .description("Choose how long before you're automatically removed from the queue:")
                .color(settings.announcement_color as u32);

            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(embed)
                    .components(vec![CAR::Buttons(time_buttons), CAR::Buttons(cancel_button)])
            );

            interaction.create_response(&ctx.http, response).await?;
        }
        button_id if button_id.starts_with("settings_timeout:") => {
            // Handle auto-leave time selection or cancel
            let time_str = button_id.split(':').nth(1).unwrap_or("30m");
            
            if time_str == "cancel" {
                // Just restore the settings menu
                let settings = db.users.get_settings(user_id).await?;
                let embed = build_settings_embed(&settings);
                let buttons = build_settings_buttons(&settings);

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(embed).components(buttons)
                );
                interaction.create_response(&ctx.http, response).await?;
            } else {
                let minutes = match time_str {
                    "30m" => 30,
                    "1h"  => 60,
                    "2h"  => 120,
                    "3h"  => 180,
                    "4h"  => 240,
                    _     => 30,
                };

                // Update user settings
                let mut settings = db.users.get_settings(user_id).await?;
                settings.expiry_duration = Duration::from_secs(minutes as u64 * 60);
                db.users.update_settings(user_id, &settings).await?;

                // Update the settings menu directly (no confirmation popup)
                let embed = build_settings_embed(&settings);
                let buttons = build_settings_buttons(&settings);

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(embed).components(buttons)
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        "settings_vc_disconnect" => {
            // Toggle VC disconnect preference
            let mut settings = db.users.get_settings(user_id).await?;
            settings.vc_kick = !settings.vc_kick;
            db.users.update_settings(user_id, &settings).await?;

            // Acknowledge and update the settings menu directly (no popup)
            let embed = build_settings_embed(&settings);
            let buttons = build_settings_buttons(&settings);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_edit_alert" => {
            // Show modal for customizing join announcement embed
            let settings = db.users.get_settings(user_id).await?;
            let modal = CreateModal::new("settings_modal_announcement", "Customize join announcement")
                .components(vec![
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short,     "HEX Color", "announcement_color")
                        .placeholder("e.g., 3447003 or FF5733")
                        .value(format!("{:06X}", settings.announcement_color))
                        .required(false).min_length(6).max_length(6)
                    ),
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Paragraph, "Message", "alert_desc")
                        .placeholder("e.g., Kafri: defense")
                        .value(settings.alert_desc.unwrap_or_default())
                        .required(false).max_length(2000)
                    ),
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short,     "Footer text", "alert_footer_text")
                        .placeholder("e.g., Good luck!")
                        .value(settings.alert_footer_text.unwrap_or_default())
                        .required(false).max_length(2048)
                    ),
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short,     "Thumbnail URL", "alert_footer_thumbnail")
                        .placeholder("https://example.com/thumb.png")
                        .value(settings.alert_footer_thumbnail.unwrap_or_default())
                        .required(false).max_length(512)
                    ),
                ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_edit_leave_alert" => {
            // Show modal for customizing leave announcement embed
            let settings = db.users.get_settings(user_id).await?;
            let modal = CreateModal::new("settings_modal_leave_alert", "Customize leave announcement")
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Color (hex, optional)", "leave_alert_color")
                            .placeholder("e.g., 3447003 or FF5733")
                            .value(format!("{:06X}", settings.announcement_color))
                            .required(false)
                            .min_length(6)
                            .max_length(6)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Paragraph, "Description", "leave_alert_desc")
                            .placeholder("e.g., {name} has left. Use {user} for mention")
                            .value(settings.leave_alert_desc.unwrap_or_default())
                            .required(false)
                            .max_length(2000)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Footer Text", "leave_alert_footer_text")
                            .placeholder("e.g., See you next time!")
                            .value(settings.leave_alert_footer_text.unwrap_or_default())
                            .required(false)
                            .max_length(2048)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Thumbnail URL", "leave_alert_footer_thumbnail")
                            .placeholder("https://example.com/thumb.png")
                            .value(settings.leave_alert_footer_thumbnail.unwrap_or_default())
                            .required(false)
                            .max_length(512)
                    ),
                ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        _ => {
            warn!("Unknown settings button: {}", button_id);
        }
    }

    Ok(())
}

/// Handle modal submissions for settings
pub async fn handle_settings_modal(
    ctx: &Context,
    interaction: &ModalInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let user_id = interaction.user.id;
    let modal_id = &interaction.data.custom_id;

    // Update activity timestamp for DM cleanup tracking
    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
        dm_tracker.update_activity(user_id).await;
    }

    match modal_id.as_str() {
        "settings_modal_announcement" => {
            // Get all input values from the modal
            let mut settings = db.users.get_settings(user_id).await?;

            // Extract values from modal components
            for (idx, action_row) in interaction.data.components.iter().enumerate() {
                if let Some(serenity::all::ActionRowComponent::InputText(input)) = action_row.components.first() {
                    if let Some(value) = &input.value {
                        let trimmed = value.trim();

                        match idx {
                            0 => {
                                // Color field
                                if !trimmed.is_empty() {
                                    let hex_str = trimmed.trim_start_matches('#');
                                    if let Ok(color) = i64::from_str_radix(hex_str, 16) {
                                        if (0..=0xFFFFFF).contains(&color) {
                                            settings.announcement_color = color;
                                        }
                                    }
                                }
                            },
                            1 => settings.alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            2 => settings.alert_footer_text = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            3 => settings.alert_footer_thumbnail   = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            _ => {}
                        }
                    }
                }
            }

            // Update settings in database
            db.users.update_settings(user_id, &settings).await?;

            // Build preview embed
            let preview_embed = build_join_announcement_embed(ctx, user_id, None, &settings, "Journeyman").await;

            // Send ephemeral preview as interaction response (dismissible)
            let response = CIR::Message(
                CIRM::new()
                    .content("**Preview of your join announcement:**")
                    .embed(preview_embed)
                    .ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;

            // Update the original settings menu
            update_settings_menu_from_modal(ctx, interaction, db).await?;
        }
        "settings_modal_leave_alert" => {
            // Get all input values from the modal
            let mut settings = db.users.get_settings(user_id).await?;

            // Extract values from modal components
            for (idx, action_row) in interaction.data.components.iter().enumerate() {
                if let Some(serenity::all::ActionRowComponent::InputText(input)) = action_row.components.first() {
                    if let Some(value) = &input.value {
                        let trimmed = value.trim();

                        match idx {
                            0 => {
                                // Color field
                                if !trimmed.is_empty() {
                                    let hex_str = trimmed.trim_start_matches('#');
                                    if let Ok(color) = i64::from_str_radix(hex_str, 16) {
                                        if color >= 0 && color <= 0xFFFFFF {
                                            settings.announcement_color = color;
                                        }
                                    }
                                }
                            },
                            1 => settings.leave_alert_desc               = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            2 => settings.leave_alert_footer_text        = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            3 => settings.leave_alert_footer_thumbnail   = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            _ => {}
                        }
                    }
                }
            }

            // Update settings in database
            db.users.update_settings(user_id, &settings).await?;

            // Build preview embed
            let preview_embed = build_leave_alert_embed(ctx, user_id, None, &settings).await;

            // Send ephemeral preview as interaction response (dismissible)
            let response = CIR::Message(
                CIRM::new()
                    .content("**Preview of your leave announcement:**")
                    .embed(preview_embed)
                    .ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;

            // Update the original settings menu
            update_settings_menu_from_modal(ctx, interaction, db).await?;
        }
        _ => {
            warn!("Unknown settings modal: {}", modal_id);
        }
    }

    Ok(())
}

/// Update the settings menu embed (for button interactions)
async fn update_settings_menu(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let user_id = interaction.user.id;
    let settings = db.users.get_settings(user_id).await?;

    let embed = build_settings_embed(&settings);
    let buttons = build_settings_buttons(&settings);

    // Get a mutable reference to the message
    let mut message = interaction.message.clone();
    message.edit(&ctx.http, EditMessage::new().embed(embed).components(buttons)).await?;

    Ok(())
}

/// Update the settings menu embed (for modal interactions)
async fn update_settings_menu_from_modal(
    ctx: &Context,
    interaction: &ModalInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let user_id = interaction.user.id;
    let settings = db.users.get_settings(user_id).await?;

    let embed = build_settings_embed(&settings);
    let buttons = build_settings_buttons(&settings);

    // Find the settings menu message in the DM channel and update it
    if let Ok(channel) = user_id.create_dm_channel(&ctx.http).await {
        // Get recent messages to find the settings menu
        if let Ok(messages) = channel.messages(&ctx.http, serenity::all::GetMessages::new().limit(10)).await {
            // Find the most recent message from the bot with the settings embed
            for msg in messages {
                if msg.author.id == ctx.cache.current_user().id && msg.embeds.iter().any(|e| e.title.as_deref() == Some("qBot user settings")) {
                    // Update this message
                    let mut message = msg.clone();
                    message.edit(&ctx.http, EditMessage::new().embed(embed).components(buttons)).await?;
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Build settings embed
pub fn build_settings_embed(settings: &crate::database::repositories::UserSettings) -> CE {
    CE::new()
        .title("qBot user settings")
        .description({
            let minutes = settings.expiry_duration.as_secs() / 60;
            format!(
                "**Timeout length:** {} minute{}\n",
                minutes,
                if minutes == 1 { "" } else { "s" }
            )
        })
        .color(settings.announcement_color as u32)
        .footer(CreateEmbedFooter::new("VC Kick - kicks you from the vc when you leave the queue."))
}

/// Build settings buttons
pub fn build_settings_buttons(settings: &crate::database::repositories::UserSettings) -> Vec<CAR> {
    vec![
        row!([
            toggle("settings_toggle_dm",        "DM alerts", settings.dm_alerts),
            toggle("settings_vc_disconnect",    "VC kick*",  settings.vc_kick)
        ]),
        row!([
            edit("settings_timeout", "Set timeout length"),
        ]),
        row!([
            edit("settings_edit_alert",       "Edit join alert"),
            edit("settings_edit_leave_alert", "Edit leave alert"),
        ]),
    ]
}

/// Build a join announcement embed (used for both actual announcements and previews)
pub async fn build_join_announcement_embed(
    ctx:       &Context,
    user_id:   serenity::all::UserId,
    guild_id:  Option<serenity::all::GuildId>,
    settings:  &crate::database::repositories::UserSettings,
    rank_name: &str,
) -> CE {
    // Get display name - try member nickname first, then user name, then user ID
    let display_name = if let Some(gid) = guild_id {
        // With guild context - try to get member for nickname
        let member = gid.member(&ctx.http, user_id).await.ok();
        if let Some(m) = member {
            m.display_name().to_string()
        } else {
            // Fallback to fetching user directly
            ctx.http.get_user(user_id).await
                .map(|u| u.name.clone())
                .unwrap_or_else(|_| user_id.to_string())
        }
    } else {
        // For preview without guild context, fetch from HTTP API
        ctx.http.get_user(user_id).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| user_id.to_string())
    };

    // Build description with template support
    // If custom description is set (even if empty), use it; otherwise use default
    let description = match &settings.alert_desc {
        Some(custom_desc) if !custom_desc.trim().is_empty() => {
            // Sanitize newline spam only for actual announcements (not previews)
            let text_to_use = if guild_id.is_some() {
                sanitize_announcement_text(custom_desc)
            } else {
                custom_desc.to_string()
            };
            
            // Replace template variables
            Some(text_to_use
                .replace("{user}", &format!("<@{}>", user_id))
                .replace("{rank}", rank_name)
                .replace("{name}", &display_name))
        }
        Some(_) => None, // Empty string means no description
        None => None,
    };

    // Create embed with title showing nickname + "joined the queue"
    let mut embed = CE::new()
        .title(format!("{display_name} joined the queue"))
        .color(settings.announcement_color as u32);
    
    // Only add description if there is one
    if let Some(desc) = description {
        embed = embed.description(desc);
    }

    // Add custom footer
    if let Some(footer_text) = &settings.alert_footer_text {
        // Sanitize footer spam only for actual announcements (not previews)
        let footer_to_use = if guild_id.is_some() {
            sanitize_footer_text(footer_text)
        } else {
            footer_text.to_string()
        };
        
        let mut footer = CreateEmbedFooter::new(footer_to_use);
        if let Some(footer_icon) = &settings.alert_footer_icon {
            footer = footer.icon_url(footer_icon);
        }
        embed = embed.footer(footer);
    }

    // Add thumbnail
    if let Some(thumbnail) = &settings.alert_footer_thumbnail {
        embed = embed.thumbnail(thumbnail);
    }

    embed
}

/// Build a leave announcement embed (used for both actual announcements and previews)
pub async fn build_leave_alert_embed(
    ctx: &Context,
    user_id: serenity::all::UserId,
    guild_id: Option<serenity::all::GuildId>,
    settings: &crate::database::repositories::UserSettings,
) -> CE {
    // Get display name - try member nickname first, then user name, then user ID
    let display_name = if let Some(gid) = guild_id {
        // With guild context - try to get member for nickname
        let member = gid.member(&ctx.http, user_id).await.ok();
        if let Some(m) = member {
            m.display_name().to_string()
        } else {
            // Fallback to fetching user directly
            ctx.http.get_user(user_id).await
                .map(|u| u.name.clone())
                .unwrap_or_else(|_| user_id.to_string())
        }
    } else {
        // For preview without guild context, fetch from HTTP API
        ctx.http.get_user(user_id).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| user_id.to_string())
    };

    // Build description with template support
    // If custom description is set (even if empty), use it; otherwise use default
    let description = match &settings.leave_alert_desc {
        Some(custom_desc) if !custom_desc.trim().is_empty() => {
            // Sanitize newline spam only for actual announcements (not previews)
            let text_to_use = if guild_id.is_some() {
                sanitize_announcement_text(custom_desc)
            } else {
                custom_desc.to_string()
            };
            
            // Replace template variables (no rank for leave)
            Some(text_to_use
                .replace("{user}", &format!("<@{}>", user_id))
                .replace("{name}", &display_name))
        }
        Some(_) => None, // Empty string means no description
        None => Some(format!("<@{}> left the queue!", user_id)), // Default for new users
    };

    // Create embed with title showing nickname + "left the queue"
    let mut embed = CE::new()
        .color(settings.announcement_color as u32);
    
    // Only add description if there is one
    if let Some(desc) = description {
        embed = embed.description(desc);
    }

    // Add custom footer if provided
    if let Some(footer_text) = &settings.leave_alert_footer_text {
        // Sanitize footer spam only for actual announcements (not previews)
        let footer_to_use = if guild_id.is_some() {
            sanitize_footer_text(footer_text)
        } else {
            footer_text.to_string()
        };
        
        let mut footer = CreateEmbedFooter::new(footer_to_use);
        if let Some(footer_icon) = &settings.leave_alert_footer_icon {
            footer = footer.icon_url(footer_icon);
        }
        embed = embed.footer(footer);
    }

    // Add custom thumbnail if provided
    if let Some(thumbnail) = &settings.leave_alert_footer_thumbnail {
        embed = embed.thumbnail(thumbnail);
    }

    embed
}

/// Server settings structure for display
pub struct ServerSettings {
    pub runner_role:       Option<String>,
    pub admin_role:        Option<String>,
    pub dynamic_elo:       bool,
}

/// Build server settings embed
pub fn build_server_settings_embed(settings: &ServerSettings, guild_name: &str) -> CE {
    let runner_display = settings.runner_role.as_ref()
        .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
        .unwrap_or_else(|| "*Not configured*".to_string());
    
    let admin_display = settings.admin_role.as_ref()
        .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
        .unwrap_or_else(|| "*Not configured*".to_string());

    CE::new()
        .title(format!("{guild_name} Server Settings"))
        .field("Runner Role", runner_display, false)
        .field("Admin Role", admin_display, false)
        .color(0x5865F2) // Discord blurple
}

/// Build server settings buttons and select menus
pub fn build_server_settings_buttons(settings: &ServerSettings) -> Vec<CAR> {
    let runner_default = settings.runner_role.as_ref()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|id| vec![RoleId::new(id)]);
    let admin_default = settings.admin_role.as_ref()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|id| vec![RoleId::new(id)]);

    vec![
        row!([
            toggle("server_settings_dynamic_elo", "Dynamic ELO", settings.dynamic_elo),
        ]),
        CAR::SelectMenu(
            CSM::new("server_settings_runner_role", CSMK::Role { default_roles: runner_default })
                .placeholder("Select Runner Role")
                .min_values(0)
                .max_values(1)
        ),
        CAR::SelectMenu(
            CSM::new("server_settings_admin_role", CSMK::Role { default_roles: admin_default })
                .placeholder("Select Admin Role")
                .min_values(0)
                .max_values(1)
        ),
    ]
}

/// Handle server settings button interactions
pub async fn handle_server_settings_button(
    ctx:         &Context,
    interaction: &ComponentInteraction,
    db:          &Arc<Database>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let button_id = &interaction.data.custom_id;

    info!("[Server Settings] {} pressed {}", interaction.user.name, button_id);

    match button_id.as_str() {
        "server_settings_dynamic_elo" => {
            // Toggle dynamic ELO
            let current = db.config.get_config_value("active_elo_enabled", guild_id.get()).await?
                .map(|v| v.parse::<bool>().unwrap_or(false))
                .unwrap_or(false);
            
            let new_state = !current;
            db.config.set_config("active_elo_enabled", &new_state.to_string(), guild_id.get()).await?;

            // Update the settings menu
            let settings = get_server_settings(db, guild_id.get()).await?;
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let embed = build_server_settings_embed(&settings, &guild_name);
            let buttons = build_server_settings_buttons(&settings);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_runner_role" => {
            // Handle runner role selection
            if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } = &interaction.data.kind {
                let role_id = values.first().map(|r| r.get().to_string());
                
                if let Some(id) = &role_id {
                    db.config.set_config("runner_role", id, guild_id.get()).await?;
                } else {
                    db.config.delete_config("runner_role", guild_id.get()).await?;
                }

                // Update the settings menu
                let settings = get_server_settings(db, guild_id.get()).await?;
                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
                let embed = build_server_settings_embed(&settings, &guild_name);
                let buttons = build_server_settings_buttons(&settings);

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(embed).components(buttons)
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        "server_settings_admin_role" => {
            // Handle admin role selection
            if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } = &interaction.data.kind {
                let role_id = values.first().map(|r| r.get().to_string());
                
                if let Some(id) = &role_id {
                    db.config.set_config("admin_role", id, guild_id.get()).await?;
                } else {
                    db.config.delete_config("admin_role", guild_id.get()).await?;
                }

                // Update the settings menu
                let settings = get_server_settings(db, guild_id.get()).await?;
                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
                let embed = build_server_settings_embed(&settings, &guild_name);
                let buttons = build_server_settings_buttons(&settings);

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(embed).components(buttons)
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        _ => {
            warn!("Unknown server settings button: {}", button_id);
        }
    }

    Ok(())
}

/// Get server settings from database
pub async fn get_server_settings(db: &Arc<Database>, guild_id: u64) -> Result<ServerSettings> {
    let runner_role = db.config.get_config_value("runner_role", guild_id).await?;
    let admin_role = db.config.get_config_value("admin_role", guild_id).await?;
    let dynamic_elo = db.config.get_config_value("active_elo_enabled", guild_id).await?
        .map(|v| v.parse::<bool>().unwrap_or(false))
        .unwrap_or(false);

    Ok(ServerSettings {
        runner_role,
        admin_role,
        dynamic_elo,
    })
}

/// Handle server settings modal submissions (currently unused - roles use select menus)
pub async fn handle_server_settings_modal(
    _ctx: &Context,
    interaction: &ModalInteraction,
    _db: &Arc<Database>,
) -> Result<()> {
    let modal_id = &interaction.data.custom_id;
    warn!("Unknown server settings modal: {}", modal_id);
    Ok(())
}

// ============================================================================
// Group Settings
// ============================================================================

/// Group settings structure for display
pub struct GroupSettings {
    pub group_id:     u8,
    pub name:         Option<String>,
    pub quota:        u8,
    pub timeout:      u16,
    pub connect_info: Option<String>,
}

/// Build group settings embed
pub fn build_group_settings_embed(settings: &GroupSettings) -> CE {
    let name_display = settings.name.as_ref()
        .map(|s| s.clone())
        .unwrap_or_else(|| format!("Group {}", settings.group_id));
    let connect_display = settings.connect_info.as_ref()
        .map(|s| format!("`{s}`"))
        .unwrap_or_else(|| "*Not configured*".to_string());

    CE::new()
        .title(format!("{name_display} Settings"))
        .field("Name",    name_display.clone(), true)
        .field("Quota",   format!("{} players", settings.quota), true)
        .field("Timeout", format!("{} minutes", settings.timeout), true)
        .field("Connect Info", connect_display, false)
        .color(0x5865F2) // Discord blurple
}

/// Build group settings buttons with group_id embedded in custom_id
pub fn build_group_settings_buttons(group_id: u8) -> Vec<CAR> {
    vec![
        row!([
            edit(&format!("group_settings_edit_name_{group_id}"),    "Edit Name"),
            edit(&format!("group_settings_edit_quota_{group_id}"),   "Edit Quota"),
            edit(&format!("group_settings_edit_timeout_{group_id}"), "Edit Timeout"),
        ]),
        row!([
            edit(&format!("group_settings_edit_connect_{group_id}"), "Edit Connect Info"),
        ]),
    ]
}

/// Build group selector for choosing which group to configure
pub fn build_group_selector(groups: &[crate::models::Group]) -> CAR {
    let options: Vec<CSMO> = groups.iter().map(|g| {
        let label = g.display_name();
        let value = g.group_id.to_string();
        CSMO::new(label, value)
    }).collect();

    CAR::SelectMenu(
        CSM::new("group_settings_select", CSMK::String { options })
            .placeholder("Select a group...")
            .min_values(1)
            .max_values(1)
    )
}

/// Handle group settings button interactions
pub async fn handle_group_settings_button(
    ctx: &Context,
    interaction: &ComponentInteraction,
    _db: &Arc<Database>,
    manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
    let guild_id  = interaction.guild_id.expect("Guild ID not found");
    let button_id = &interaction.data.custom_id;

    info!("[Group Settings] {} pressed {}", interaction.user.name, button_id);

    // Extract group_id from button custom_id (format: group_settings_edit_<action>_<group_id>)
    let group_id: u8 = button_id
        .rsplit('_')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid button ID format: {}", button_id))?;

    // Get the group by ID
    let mut manager_lock = manager.lock().await;
    let group = {
        let server = manager_lock.get_server(guild_id)?;
        server.groups.iter()
            .find(|g| g.group_id == group_id)
            .ok_or_else(|| anyhow::anyhow!("Group {} not found", group_id))?
            .clone()
    };
    let settings = GroupSettings {
        group_id:     group.group_id,
        name:         group.name.clone(),
        quota:        group.quota,
        timeout:      group.timeout,
        connect_info: group.connect_info.clone(),
    };
    drop(manager_lock);

    // Match button action (button_id format: group_settings_edit_<action>_<group_id>)
    if button_id.starts_with("group_settings_edit_name_") {
        let modal = CreateModal::new(format!("group_settings_modal_name_{group_id}"), "Set Group Name")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Group Name", "name")
                        .placeholder("e.g., NA PUGs, EU Competitive")
                        .value(settings.name.unwrap_or_default())
                        .required(false)
                        .max_length(50)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else if button_id.starts_with("group_settings_edit_quota_") {
        let modal = CreateModal::new(format!("group_settings_modal_quota_{group_id}"), "Set Queue Quota")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Quota (2-100)", "quota")
                        .placeholder("Number of players required")
                        .value(settings.quota.to_string())
                        .required(true)
                        .min_length(1)
                        .max_length(3)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else if button_id.starts_with("group_settings_edit_timeout_") {
        let modal = CreateModal::new(format!("group_settings_modal_timeout_{group_id}"), "Set Timeout")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Timeout (minutes)", "timeout")
                        .placeholder("Minutes before auto-removal from queue")
                        .value(settings.timeout.to_string())
                        .required(true)
                        .min_length(1)
                        .max_length(4)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else if button_id.starts_with("group_settings_edit_connect_") {
        let modal = CreateModal::new(format!("group_settings_modal_connect_{group_id}"), "Set Server Connect Info")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Paragraph, "Connect Command", "connect_info")
                        .placeholder("e.g., connect 192.168.1.1:27015; password secret")
                        .value(settings.connect_info.unwrap_or_default())
                        .required(false)
                        .max_length(500)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else {
        warn!("Unknown group settings button: {}", button_id);
    }

    Ok(())
}

/// Handle group selection from the selector menu
pub async fn handle_group_settings_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
    _db: &Arc<Database>,
    manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");

    info!("[Group Settings] {} selected group", interaction.user.name);

    // Extract selected group_id from the interaction
    let group_id: u8 = match &interaction.data.kind {
        serenity::all::ComponentInteractionDataKind::StringSelect { values } => {
            values.first()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("Invalid group selection"))?
        }
        _ => return Err(anyhow::anyhow!("Expected string select interaction")),
    };

    // Get the group by ID
    let mut manager_lock = manager.lock().await;
    let group = {
        let server = manager_lock.get_server(guild_id)?;
        server.groups.iter()
            .find(|g| g.group_id == group_id)
            .ok_or_else(|| anyhow::anyhow!("Group not found"))?
            .clone()
    };
    drop(manager_lock);

    let settings = GroupSettings {
        group_id:     group.group_id,
        name:         group.name.clone(),
        quota:        group.quota,
        timeout:      group.timeout,
        connect_info: group.connect_info.clone(),
    };

    let embed = build_group_settings_embed(&settings);
    let buttons = build_group_settings_buttons(settings.group_id);

    let response = CIR::UpdateMessage(
        CIRM::new().embed(embed).components(buttons)
    );
    interaction.create_response(&ctx.http, response).await?;

    Ok(())
}

/// Handle group settings modal submissions
pub async fn handle_group_settings_modal(
    ctx: &Context,
    interaction: &ModalInteraction,
    db: &Arc<Database>,
    manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let modal_id = &interaction.data.custom_id;

    info!("[Group Settings] {} submitted modal {}", interaction.user.name, modal_id);

    // Extract group_id from modal custom_id (format: group_settings_modal_<action>_<group_id>)
    let group_id: u8 = modal_id
        .rsplit('_')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

    // Get the group by ID
    let mut manager_lock = manager.lock().await;
    let group = {
        let server = manager_lock.get_server(guild_id)?;
        server.groups.iter_mut()
            .find(|g| g.group_id == group_id)
            .ok_or_else(|| anyhow::anyhow!("Group {} not found", group_id))?
    };

    if modal_id.starts_with("group_settings_modal_name_") {
        // Extract name value
        let name_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let name = if name_str.trim().is_empty() {
            None
        } else {
            Some(name_str.trim().to_string())
        };

        // Update in-memory
        group.name = name.clone();

        // Update in database
        db.groups.update_name(guild_id.get(), group.group_id, name.as_deref()).await?;

        // Get updated settings and refresh the menu
        let settings = GroupSettings {
            group_id:     group.group_id,
            name:         group.name.clone(),
            quota:        group.quota,
            timeout:      group.timeout,
            connect_info: group.connect_info.clone(),
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("group_settings_modal_quota_") {
        // Extract quota value
        let quota_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let quota: u8 = match quota_str.trim().parse() {
            Ok(q) if (2..=100).contains(&q) => q,
            _ => {
                let response = CIR::Message(
                    CIRM::new()
                        .content("Invalid quota. Must be between 2 and 100.")
                        .ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Update in-memory
        group.quota = quota;

        // Update in database
        db.set_group(
            guild_id.get(),
            group.channels.queue_vc.get(),
            group.channels.dashboard.get(),
            group.channels.queue_chat.get(),
            group.channels.teams[0].red_vc.get(),
            group.channels.teams[0].blu_vc.get(),
            quota,
        ).await?;

        // Update dashboard
        group.queue_dash_update(ctx, guild_id.get()).await;

        // Get updated settings and refresh the menu
        let settings = GroupSettings {
            group_id:     group.group_id,
            name:         group.name.clone(),
            quota:        group.quota,
            timeout:      group.timeout,
            connect_info: group.connect_info.clone(),
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("group_settings_modal_timeout_") {
        // Extract timeout value
        let timeout_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let timeout: u16 = match timeout_str.trim().parse() {
            Ok(t) if t > 0 => t,
            _ => {
                let response = CIR::Message(
                    CIRM::new()
                        .content("Invalid timeout. Must be a positive number.")
                        .ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Update in-memory
        group.timeout = timeout;

        // Get updated settings and refresh the menu
        let settings = GroupSettings {
            group_id:     group.group_id,
            name:         group.name.clone(),
            quota:        group.quota,
            timeout:      group.timeout,
            connect_info: group.connect_info.clone(),
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("group_settings_modal_connect_") {
        // Extract connect info value
        let connect_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let connect_info = if connect_str.trim().is_empty() {
            None
        } else {
            Some(connect_str.trim().to_string())
        };

        // Update in-memory
        group.connect_info = connect_info;

        // Update dashboard
        group.queue_dash_update(ctx, guild_id.get()).await;

        // Get updated settings and refresh the menu
        let settings = GroupSettings {
            group_id:     group.group_id,
            name:         group.name.clone(),
            quota:        group.quota,
            timeout:      group.timeout,
            connect_info: group.connect_info.clone(),
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else {
        warn!("Unknown group settings modal: {}", modal_id);
    }

    Ok(())
}
