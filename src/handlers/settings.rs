use anyhow::{Result, anyhow};
use serenity::all::{
    ComponentInteraction, ModalInteraction, Context, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateActionRow as CAR, CreateActionRow, CreateButton as CB,
    ButtonStyle as BS, EditMessage, CreateInputText, InputTextStyle, CreateModal,
    CreateEmbedFooter, CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK,
    CreateSelectMenuOption as CSMO, RoleId, GuildId as GI,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::Database;

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

    info!("{} pressed {}", username, button_id);

    // Update activity timestamp for DM cleanup tracking
    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
        dm_tracker.update_activity(user_id).await;
    }

    match button_id.as_str() {
        "settings_toggle_dm" => {
            // Toggle DM alerts
            let _new_state = db.users.toggle_dm_enabled(user_id).await?;

            // Acknowledge and update the settings menu directly (no popup)
            let settings = db.users.get_prefs(user_id).await?;
            let embed    = build_settings_embed(&settings);
            let buttons  = build_settings_buttons(&settings);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_timeout" => {
            // Show time selection buttons inline (replace current message temporarily)
            let settings = db.users.get_prefs(user_id).await?;
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
                let settings = db.users.get_prefs(user_id).await?;
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
                let mut settings = db.users.get_prefs(user_id).await?;
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
        "settings_vc_auto_leave" => {
            // Toggle VC disconnect preference
            let mut settings = db.users.get_prefs(user_id).await?;
            settings.vc_auto_leave = !settings.vc_auto_leave;
            db.users.update_settings(user_id, &settings).await?;

            // Acknowledge and update the settings menu directly (no popup)
            let embed = build_settings_embed(&settings);
            let buttons = build_settings_buttons(&settings);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_vc_auto_join" => {
            // Toggle VC auto-queue preference
            let mut settings = db.users.get_prefs(user_id).await?;
            settings.vc_auto_join = !settings.vc_auto_join;
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
            let settings = db.users.get_prefs(user_id).await?;
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
            let settings = db.users.get_prefs(user_id).await?;
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
        "settings_quota_alert" => {
            // Show quota threshold selection buttons based on current group's quota
            let settings = db.users.get_prefs(user_id).await?;
            
            // Try to determine the current group context from the interaction
            let (guild_id, group_id, group_quota) = if let (Some(gid), channel) = (interaction.guild_id, interaction.channel_id) {
                // Find which group this channel belongs to
                let data = ctx.data.read().await;
                let manager = data.get::<crate::models::GuildKey>();
                
                if let Some(manager_ref) = manager {
                    let mut manager: tokio::sync::MutexGuard<'_, crate::models::Manager> = manager_ref.lock().await;
                    if let Ok(group) = manager.get_group_by_channel(gid, channel) {
                        (gid, group.group_id, group.quota)
                    } else {
                        // Default values if no group found
                        (gid, 0, 12u8)
                    }
                } else {
                    (gid, 0, 12u8)
                }
            } else {
                // Default values if no guild context
                (serenity::all::GuildId::new(0), 0, 12u8)
            };
            
            // Get current threshold for this specific group
            let current_threshold = settings.group_quota_thresholds.get(&(guild_id.get(), group_id)).copied();
            
            // Generate relative threshold buttons (quota - 4, -3, -2, -1)
            let mut threshold_buttons = Vec::new();
            for offset in 1..=4u8 {
                let threshold = group_quota.saturating_sub(offset);
                let button_label = if threshold == 0 {
                    "Any players".to_string()
                } else {
                    format!("{} players", threshold)
                };
                
                threshold_buttons.push(
                    CB::new(&format!("settings_quota_alert:{}", threshold))
                        .label(button_label)
                        .style(if current_threshold == Some(threshold) { BS::Success } else { BS::Secondary })
                );
            }
            
            let disable_button = vec![
                CB::new("settings_quota_alert:disable").label("Disable").style(if current_threshold.is_none() { BS::Danger } else { BS::Secondary }),
            ];

            let embed = CE::new()
                .title("Set quota alert threshold")
                .description(format!(
                    "Choose how many players should be in the queue before you get a DM notification:\nGroup quota: {} players",
                    group_quota
                ))
                .color(settings.announcement_color as u32);

            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(embed)
                    .components(vec![CAR::Buttons(threshold_buttons), CAR::Buttons(disable_button)])
            );

            interaction.create_response(&ctx.http, response).await?;
        }
        button_id if button_id.starts_with("settings_quota_alert:") => {
            // Handle quota threshold selection
            let threshold_str = button_id.split(':').nth(1).unwrap_or("disable");
            
            // Parse the threshold value (could be "disable" or a number)
            let new_threshold = if threshold_str == "disable" {
                None
            } else {
                threshold_str.parse().ok()
            };

            // Get guild and group context from interaction
            let (guild_id, group_id) = if let (Some(gid), channel) = (interaction.guild_id, interaction.channel_id) {
                // Find which group this channel belongs to
                let data = ctx.data.read().await;
                let manager = data.get::<crate::models::GuildKey>();
                
                if let Some(manager_ref) = manager {
                    let mut manager: tokio::sync::MutexGuard<'_, crate::models::Manager> = manager_ref.lock().await;
                    if let Ok(group) = manager.get_group_by_channel(gid, channel) {
                        (gid, group.group_id)
                    } else {
                        (gid, 0)
                    }
                } else {
                    (gid, 0)
                }
            } else {
                (serenity::all::GuildId::new(0), 0)
            };

            // Update user settings with per-group threshold
            let mut settings = db.users.get_prefs(user_id).await?;
            if let Some(threshold) = new_threshold {
                settings.group_quota_thresholds.insert((guild_id.get(), group_id), threshold);
            } else {
                settings.group_quota_thresholds.remove(&(guild_id.get(), group_id));
            }
            db.users.update_settings(user_id, &settings).await?;

            // Update the settings menu directly
            let embed = build_settings_embed(&settings);
            let buttons = build_settings_buttons(&settings);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
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
    use crate::database::repositories::is_valid_user_text;

    let user_id = interaction.user.id;
    let modal_id = &interaction.data.custom_id;

    // Update activity timestamp for DM cleanup tracking
    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
        dm_tracker.update_activity(user_id).await;
    }

    match modal_id.as_str() {
        "settings_modal_announcement" => {
            // Get all input values from the modal
            let mut settings = db.users.get_prefs(user_id).await?;

            // Extract and validate values from modal components
            for (idx, action_row) in interaction.data.components.iter().enumerate() {
                if let Some(serenity::all::ActionRowComponent::InputText(input)) = action_row.components.first() {
                    if let Some(value) = &input.value {
                        let trimmed = value.trim();

                        // Validate text fields for allowed characters (skip color and URL fields)
                        if (idx == 1 || idx == 2) && !trimmed.is_empty() && !is_valid_user_text(trimmed) {
                            let field_name = if idx == 1 { "Message" } else { "Footer text" };
                            let response = CIR::Message(
                                CIRM::new()
                                    .content(format!("**Error:** {} contains invalid characters. Only ASCII printable and extended characters are allowed.", field_name))
                                    .ephemeral(true)
                            );
                            interaction.create_response(&ctx.http, response).await?;
                            return Ok(());
                        }

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
            let mut settings = db.users.get_prefs(user_id).await?;

            // Extract and validate values from modal components
            for (idx, action_row) in interaction.data.components.iter().enumerate() {
                if let Some(serenity::all::ActionRowComponent::InputText(input)) = action_row.components.first() {
                    if let Some(value) = &input.value {
                        let trimmed = value.trim();

                        // Validate text fields for allowed characters (skip color and URL fields)
                        if (idx == 1 || idx == 2) && !trimmed.is_empty() && !is_valid_user_text(trimmed) {
                            let field_name = if idx == 1 { "Description" } else { "Footer text" };
                            let response = CIR::Message(
                                CIRM::new()
                                    .content(format!("**Error:** {} contains invalid characters. Only ASCII printable and extended characters are allowed.", field_name))
                                    .ephemeral(true)
                            );
                            interaction.create_response(&ctx.http, response).await?;
                            return Ok(());
                        }

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
    let settings = db.users.get_prefs(user_id).await?;

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
    let settings = db.users.get_prefs(user_id).await?;

    let embed = build_settings_embed(&settings);
    let buttons = build_settings_buttons(&settings);

    // Find the settings menu message in the DM channel and update it
    if let Ok(channel) = user_id.create_dm_channel(&ctx.http).await {
        // Get recent messages to find the settings menu
        if let Ok(messages) = channel.messages(&ctx.http, serenity::all::GetMessages::new().limit(10)).await {
            // Find the most recent message from the bot with the settings embed
            for msg in messages {
                if msg.author.id == ctx.cache.current_user().id && msg.embeds.iter().any(|e| e.title.as_deref() == Some("qBot preferences")) {
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
    use crate::handlers::settings_menu::AsSettingsMenu;
    settings.as_settings_menu().build_embed()
}

/// Build settings buttons
pub fn build_settings_buttons(settings: &crate::database::repositories::UserSettings) -> Vec<CAR> {
    use crate::handlers::settings_menu::AsSettingsMenu;
    settings.as_settings_menu().build_components()
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
        None => None,
    };

    // Create embed with title showing nickname + "left the queue"
    let mut embed = CE::new()
        .title(format!("{} left the queue", display_name))
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
    pub runner_role: Option<String>,
    pub admin_role:  Option<String>,
}

/// Build server settings embed
pub fn build_server_settings_embed(settings: &ServerSettings, guild_name: &str) -> CE {
    use crate::handlers::settings_menu::{AsSettingsMenu, ServerSettingsDisplay};
    let display = ServerSettingsDisplay {
        guild_name:  guild_name.to_string(),
        runner_role: settings.runner_role.clone(),
        admin_role:  settings.admin_role.clone(),
    };
    display.as_settings_menu().build_embed()
}

/// Build server settings buttons and select menus
pub fn build_server_settings_buttons(settings: &ServerSettings, guild_name: &str) -> Vec<CAR> {
    use crate::handlers::settings_menu::{AsSettingsMenu, ServerSettingsDisplay};
    let display = ServerSettingsDisplay {
        guild_name:  guild_name.to_string(),
        runner_role: settings.runner_role.clone(),
        admin_role:  settings.admin_role.clone(),
    };
    display.as_settings_menu().build_components()
}

/// Handle server settings button interactions
pub async fn handle_server_settings_button(
    ctx:         &Context,
    interaction: &ComponentInteraction,
    db:          &Arc<Database>,
    manager:     &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let button_id = &interaction.data.custom_id;

    info!("{} pressed {}", interaction.user.name, button_id);

    match button_id.as_str() {
        "server_settings_dynamic_elo" => {
            // Toggle dynamic ELO
            let current = db.config.get_config_item("active_elo_enabled", guild_id).await?
                .map(|v| v.parse::<bool>().unwrap_or(false))
                .unwrap_or(false);
            
            let new_state = !current;
            db.config.set_config("active_elo_enabled", &new_state.to_string(), guild_id).await?;

            // Return to rank configuration menu
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let rank_roles = get_all_rank_roles(db, guild_id).await?;
            let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
            
            let display = crate::handlers::settings_menu::RankConfigDisplay {
                guild_name,
                rank_roles,
                dynamic_elo,
                default_rank,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_roles" => {
            // Show role configuration menu
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let settings = get_server_settings(db, guild_id).await?;
            
            let display = crate::handlers::settings_menu::RoleConfigDisplay {
                guild_name,
                runner_role: settings.runner_role,
                admin_role: settings.admin_role,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_roles_back" => {
            // Go back to main server settings
            let settings = get_server_settings(db, guild_id).await?;
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let embed = build_server_settings_embed(&settings, &guild_name);
            let buttons = build_server_settings_buttons(&settings, &guild_name);

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
                    db.config.set_config("runner_role", id, guild_id).await?;
                } else {
                    db.config.delete_config("runner_role", guild_id).await?;
                }

                // Return to role configuration menu
                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
                let settings = get_server_settings(db, guild_id).await?;
                
                let display = crate::handlers::settings_menu::RoleConfigDisplay {
                    guild_name,
                    runner_role: settings.runner_role,
                    admin_role: settings.admin_role,
                };

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(display.build_embed()).components(display.build_components())
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        "server_settings_admin_role" => {
            // Handle admin role selection
            if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } = &interaction.data.kind {
                let role_id = values.first().map(|r| r.get().to_string());
                
                if let Some(id) = &role_id {
                    db.config.set_config("admin_role", id, guild_id).await?;
                } else {
                    db.config.delete_config("admin_role", guild_id).await?;
                }

                // Return to role configuration menu
                let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
                let settings = get_server_settings(db, guild_id).await?;
                
                let display = crate::handlers::settings_menu::RoleConfigDisplay {
                    guild_name,
                    runner_role: settings.runner_role,
                    admin_role: settings.admin_role,
                };

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(display.build_embed()).components(display.build_components())
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        "server_settings_ranks" => {
            // Show rank configuration menu
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let rank_roles = get_all_rank_roles(db, guild_id).await?;
            let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
            
            let display = crate::handlers::settings_menu::RankConfigDisplay {
                guild_name,
                rank_roles,
                dynamic_elo,
                default_rank,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_ranks_back" => {
            // Go back to main server settings
            let settings = get_server_settings(db, guild_id).await?;
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let embed = build_server_settings_embed(&settings, &guild_name);
            let buttons = build_server_settings_buttons(&settings, &guild_name);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_rank_select" => {
            // Handle rank selection from dropdown (value is now rank name)
            if let serenity::all::ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                if let Some(rank_name) = values.first() {
                    let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
                    
                    if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
                        let display = crate::handlers::settings_menu::RankRoleConfigDisplay {
                            guild_name,
                            rank_name: guild_rank.name.clone(),
                            rank_key: rank_name.clone(),
                            elo: guild_rank.elo,
                            role_id: guild_rank.role_id,
                        };

                        let response = CIR::UpdateMessage(
                            CIRM::new().embed(display.build_embed()).components(display.build_components())
                        );
                        interaction.create_response(&ctx.http, response).await?;
                    }
                }
            }
        }
        "server_settings_rank_link_role" => {
            // Handle role selection for linking existing rank
            let selected_role_id = if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } = &interaction.data.kind {
                values.first().copied().ok_or_else(|| anyhow!("No role selected"))?
            } else {
                return Err(anyhow!("No role selected"));
            };

            // Get the role name to use as default
            let role_name = match guild_id.roles(&ctx.http).await {
                Ok(roles) => {
                    if let Some(role) = roles.get(&selected_role_id) {
                        role.name.clone()
                    } else {
                        // Fallback to role ID if role not found
                        selected_role_id.get().to_string()
                    }
                },
                Err(_) => {
                    // Fallback to role ID if API call fails
                    selected_role_id.get().to_string()
                }
            };

            // Show modal to specify rank name and ELO for the selected role
            use serenity::all::{CreateModal, CreateActionRow, CreateInputText, InputTextStyle};
            
            let modal = CreateModal::new(
                format!("server_settings_rank_modal_link_{}", selected_role_id.get()),
                "Link Existing Rank"
            )
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Rank Name", "name")
                        .placeholder("e.g., Champion, Legend, Elite")
                        .value(&role_name)
                        .required(true)
                        .max_length(30)
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "ELO Threshold", "elo")
                        .placeholder("Minimum ELO for this rank")
                        .required(true)
                        .min_length(1)
                        .max_length(3)
                ),
            ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_rank_back" => {
            // Go back to rank list
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let rank_roles = get_all_rank_roles(db, guild_id).await?;
            let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
            
            let display = crate::handlers::settings_menu::RankConfigDisplay {
                guild_name,
                rank_roles,
                dynamic_elo,
                default_rank,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        _ if button_id.starts_with("server_settings_rank_edit_") => {
            // Handle rank name/ELO edit button
            let rank_name = button_id.strip_prefix("server_settings_rank_edit_").unwrap();
            if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
                use serenity::all::{CreateModal, CreateActionRow, CreateInputText, InputTextStyle};
                
                let modal = CreateModal::new(
                    format!("server_settings_rank_modal_{}", rank_name),
                    format!("Edit {} Rank", guild_rank.name)
                )
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Rank Name", "name")
                            .placeholder("e.g., Beginner, Expert, Champion")
                            .value(&guild_rank.name)
                                .required(true)
                                .max_length(30)
                        ),
                        CreateActionRow::InputText(
                            CreateInputText::new(InputTextStyle::Short, "ELO Threshold", "elo")
                                .placeholder("Minimum ELO for this rank")
                                .value(guild_rank.elo.to_string())
                                .required(true)
                                .min_length(1)
                                .max_length(3)
                        ),
                    ]);

                let response = CIR::Modal(modal);
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        "server_settings_rank_add" => {
            // Show modal to add a new rank
            use serenity::all::{CreateModal, CreateActionRow, CreateInputText, InputTextStyle};
            
            let modal = CreateModal::new("server_settings_rank_modal_add", "Add New Rank")
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Rank Name", "name")
                            .placeholder("e.g., Champion, Legend, Elite")
                            .required(true)
                            .max_length(30)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "ELO Threshold", "elo")
                            .placeholder("Minimum ELO for this rank")
                            .required(true)
                            .min_length(1)
                            .max_length(3)
                    ),
                ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_rank_link" => {
            // Show role selector for linking existing rank
            let response = CIR::UpdateMessage(
                CIRM::new()
                    .embed(CE::new()
                        .title("Link ranks")
                        .description("Select a Discord role to link to a new rank. The role will be used to assign this rank to players automatically.")
                        .color(0x5865F2))
                    .components(vec![
                        CAR::SelectMenu(
                            CSM::new("server_settings_rank_link_role", CSMK::Role { default_roles: None })
                                .placeholder("Select a Discord role to link")
                                .min_values(1)
                                .max_values(1)
                        ),
                        CAR::Buttons(vec![
                            CB::new("server_settings_ranks_back")
                                .label("Back to Ranks")
                                .style(BS::Secondary),
                        ])
                    ])
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        _ if button_id.starts_with("server_settings_rank_delete_") => {
            // Remove rank from DB
            let rank_name = button_id.strip_prefix("server_settings_rank_delete_").unwrap();
            // Delete rank from DB by name
            db.ranks.delete_rank(guild_id, rank_name).await?;
            
            info!("{} deleted rank {}", interaction.user.name, rank_name);

            // Return to rank list
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let rank_roles = get_all_rank_roles(db, guild_id).await?;
            let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
            
            let display = crate::handlers::settings_menu::RankConfigDisplay {
                guild_name,
                rank_roles,
                dynamic_elo,
                default_rank,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        _ if button_id.starts_with("server_settings_rank_role_") => {
            // Handle role selector for linking Discord role to rank
            let rank_name = button_id.strip_prefix("server_settings_rank_role_").unwrap();
            
            // Get selected role from interaction
            let selected_role_id = if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } = &interaction.data.kind {
                values.first().copied().ok_or_else(|| anyhow!("No role selected"))?
            } else {
                return Err(anyhow!("No role selected"));
            };

            // Update rank's linked role in DB
            db.ranks.update_rank_role(guild_id, rank_name, selected_role_id).await?;
            
            let role_display = format!("<@&{}>", selected_role_id.get());
            info!("{} linked rank {} to role {}", interaction.user.name, rank_name, role_display);

            // Refresh the rank config display
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
                let display = crate::handlers::settings_menu::RankRoleConfigDisplay {
                    guild_name,
                    rank_name: guild_rank.name.clone(),
                    rank_key: rank_name.to_string(),
                    elo: guild_rank.elo,
                    role_id: guild_rank.role_id,
                };

                let response = CIR::UpdateMessage(
                    CIRM::new().embed(display.build_embed()).components(display.build_components())
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }
        "server_settings_default_rank_select" => {
            if let serenity::all::ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                if let Some(rank_name) = values.first() {
                    let new_rank = crate::models::types::Rank::from_name(rank_name.trim());
                    let new_elo = new_rank.default_rank_elo();

                    db.config.set_config("default_rank", new_rank.name(), guild_id).await?;
                    db.config.set_config("default_elo", &new_elo.to_string(), guild_id).await?;

                    let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
                    let rank_roles = get_all_rank_roles(db, guild_id).await?;
                    let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
                    
                    let display = crate::handlers::settings_menu::RankConfigDisplay {
                        guild_name,
                        rank_roles,
                        dynamic_elo,
                        default_rank,
                    };

                    let response = CIR::UpdateMessage(
                        CIRM::new().embed(display.build_embed()).components(display.build_components())
                    );
                    interaction.create_response(&ctx.http, response).await?;
                }
            }
        }
        "server_settings_groups" => {
            // Show group configuration menu
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let groups = db.groups.get_groups_for_guild(guild_id).await?;
            
            let display = crate::handlers::settings_menu::GroupListDisplay {
                guild_name,
                groups,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_groups_back" => {
            // Go back to main server settings
            let settings = get_server_settings(db, guild_id).await?;
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let embed = build_server_settings_embed(&settings, &guild_name);
            let buttons = build_server_settings_buttons(&settings, &guild_name);

            let response = CIR::UpdateMessage(
                CIRM::new().embed(embed).components(buttons)
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_create_roles" => {
            // Create runner, admin, and rank roles
            use serenity::all::Permissions;
            use serenity::builder::EditRole;

            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());

            // Create Runner role if not configured
            let runner_role = db.config.get_config_item("runner_role", guild_id).await?;
            if runner_role.is_none() {
                match guild_id.create_role(&ctx.http,
                    EditRole::new()
                        .name("PUG Runner")
                        .colour(crate::RUNNER)
                        .permissions(Permissions::empty())
                ).await {
                    Ok(role) => {
                        if let Err(e) = db.config.set_config("runner_role", &role.id.to_string(), guild_id).await {
                            warn!("Failed to save runner_role config: {e}");
                        }
                        info!("[{}] Created PUG Runner role", guild_name);
                    },
                    Err(e) => {
                        warn!("[{}] Failed to create PUG Runner role: {}", guild_name, e);
                    }
                }
            }

            // Create Admin role if not configured
            let admin_role = db.config.get_config_item("admin_role", guild_id).await?;
            if admin_role.is_none() {
                match guild_id.create_role(&ctx.http,
                    EditRole::new()
                        .name("PUG Admin")
                        .colour(crate::ADMIN)
                        .permissions(Permissions::empty())
                ).await {
                    Ok(role) => {
                        if let Err(e) = db.config.set_config("admin_role", &role.id.to_string(), guild_id).await {
                            warn!("Failed to save admin_role config: {e}");
                        }
                        info!("[{}] Created PUG Admin role", guild_name);
                    },
                    Err(e) => {
                        warn!("[{}] Failed to create PUG Admin role: {}", guild_name, e);
                    }
                }
            }

            // Initialize default ranks in database
            if let Err(e) = db.ranks.init_default_ranks(guild_id).await {
                warn!("[{}] Failed to initialize default ranks: {}", guild_name, e);
            } else {
                info!("[{}] Initialized default ranks", guild_name);
            }

            // Return to role configuration menu
            let settings = get_server_settings(db, guild_id).await?;
            
            let display = crate::handlers::settings_menu::RoleConfigDisplay {
                guild_name,
                runner_role: settings.runner_role,
                admin_role: settings.admin_role,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        "server_settings_create_group" => {
            // Create a new group with channels
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());

            // Create the category and channels
            match crate::handlers::admin::create_group_channels(ctx, guild_id).await {
                Ok((category_id, dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel)) => {
                    use crate::models::{Group, Channels, TeamChannel};
                    use serenity::all::MessageId;

                    let mut temp_group = Group {
                        guild_id,
                        group_id:       0,
                        name:           None,
                        quota:          crate::DEFAULT_QUOTA,
                        timeout:        crate::DEFAULT_HOT_JOIN_TIMEOUT,
                        dashboard_msg:  MessageId::new(1),
                        channels:       Channels {
                            queue_chat: queue_channel,
                            queue_vc:   queue_vc_channel,
                            teams:      vec![TeamChannel {
                                red_vc: red_channel,
                                blu_vc: blue_channel,
                            }],
                            dashboard: dashboard_channel,
                        },
                        sessions:            vec![],
                        connect_info:        None,
                        team_balance_method: crate::models::TeamBalanceMethod::default(),
                        dm_alert_enabled:    false,
                        dm_alert_threshold:  0,
                        dm_alert_users:      vec![],
                    };

                    // Publish the dashboard to get the actual message ID
                    match temp_group.dash_publish(ctx, dashboard_channel, db, guild_id).await {
                        Ok(_) => {
                            let dashboard_msg_id = temp_group.dashboard_msg.get();
                            info!("[{}] Dashboard message created with ID {}", guild_name, dashboard_msg_id);

                            // Create the group in the database
                            let group_config = crate::database::repositories::group::GroupConfig {
                                dashboard_channel_id: dashboard_channel.get(),
                                chat_channel_id: queue_channel.get(),
                                queue_vc_id: queue_vc_channel.get(),
                                red_vc_id: red_channel.get(),
                                blu_vc_id: blue_channel.get(),
                                quota: crate::DEFAULT_QUOTA,
                            };
                            match db.groups.create_group(guild_id, dashboard_msg_id, group_config).await {
                                Ok(db_group) => {
                                    info!("[{}] Group {} saved to database", guild_name, db_group.group_id);

                                    // Add group to in-memory server
                                    let mut manager_lock = manager.lock().await;
                                    if let Ok(server) = manager_lock.get_server(guild_id) {
                                        if let Err(e) = server.add_group(db_group.clone()) {
                                            error!("Failed to add group to server: {e}");
                                        }
                                    }
                                    drop(manager_lock);

                                    // Refresh the settings menu
                                    let settings = get_server_settings(db, guild_id).await?;
                                    let embed = build_server_settings_embed(&settings, &guild_name);
                                    let buttons = build_server_settings_buttons(&settings, &guild_name);

                                    let response = CIR::UpdateMessage(
                                        CIRM::new().embed(embed).components(buttons)
                                    );
                                    interaction.create_response(&ctx.http, response).await?;
                                },
                                Err(e) => {
                                    // Database save failed - clean up
                                    let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;
                                    let _ = dashboard_channel.delete(&ctx.http).await;
                                    let _ = queue_channel.delete(&ctx.http).await;
                                    let _ = queue_vc_channel.delete(&ctx.http).await;
                                    let _ = red_channel.delete(&ctx.http).await;
                                    let _ = blue_channel.delete(&ctx.http).await;
                                    let _ = category_id.delete(&ctx.http).await;

                                    warn!("[{}] Failed to save group to database: {}", guild_name, e);
                                    let response = CIR::Message(
                                        CIRM::new().content(format!("Failed to save group: {e}")).ephemeral(true)
                                    );
                                    interaction.create_response(&ctx.http, response).await?;
                                }
                            }
                        },
                        Err(e) => {
                            // Dashboard creation failed - clean up
                            let _ = dashboard_channel.delete(&ctx.http).await;
                            let _ = queue_channel.delete(&ctx.http).await;
                            let _ = queue_vc_channel.delete(&ctx.http).await;
                            let _ = red_channel.delete(&ctx.http).await;
                            let _ = blue_channel.delete(&ctx.http).await;
                            let _ = category_id.delete(&ctx.http).await;

                            warn!("[{}] Failed to create dashboard: {}", guild_name, e);
                            let response = CIR::Message(
                                CIRM::new().content(format!("Failed to create dashboard: {e}")).ephemeral(true)
                            );
                            interaction.create_response(&ctx.http, response).await?;
                        }
                    }
                },
                Err(e) => {
                    warn!("[{}] Failed to create channels: {}", guild_name, e);
                    let response = CIR::Message(
                        CIRM::new().content(format!("Failed to create channels: {e}")).ephemeral(true)
                    );
                    interaction.create_response(&ctx.http, response).await?;
                }
            }
        }
        "server_settings_group_select" => {
            // Handle group selection from dropdown - show modal with all settings
            if let serenity::all::ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                if let Some(group_id_str) = values.first() {
                    if let Ok(group_id) = group_id_str.parse::<u8>() {
                        // Find the group
                        let groups = db.groups.get_groups_for_guild(guild_id).await?;
                        if let Some(group) = groups.iter().find(|g| g.group_id == group_id) {
                            let modal = CreateModal::new(format!("server_settings_group_modal_{group_id}"), "Edit Group Settings")
                                .components(vec![
                                    CreateActionRow::InputText(
                                        CreateInputText::new(InputTextStyle::Short, "Name", "name")
                                            .placeholder("e.g., NA PUGs, EU Competitive")
                                            .value(group.name.clone().unwrap_or_default())
                                            .required(false)
                                            .max_length(50)
                                    ),
                                    CreateActionRow::InputText(
                                        CreateInputText::new(InputTextStyle::Short, "Quota (2-100)", "quota")
                                            .placeholder("Number of players required")
                                            .value(group.quota.to_string())
                                            .required(true)
                                            .min_length(1)
                                            .max_length(3)
                                    ),
                                    CreateActionRow::InputText(
                                        CreateInputText::new(InputTextStyle::Short, "Hot Join Timeout (seconds)", "timeout")
                                            .placeholder("Seconds for missing players to join VC")
                                            .value(group.timeout.to_string())
                                            .required(true)
                                            .min_length(1)
                                            .max_length(3)
                                    ),
                                    CreateActionRow::InputText(
                                        CreateInputText::new(InputTextStyle::Paragraph, "Connect Info", "connect")
                                            .placeholder("e.g., connect 192.168.1.1:27015; password secret")
                                            .value(group.connect_info.clone().unwrap_or_default())
                                            .required(false)
                                            .max_length(500)
                                    ),
                                ]);

                            let response = CIR::Modal(modal);
                            interaction.create_response(&ctx.http, response).await?;
                        }
                    }
                }
            }
        }
        "server_settings_group_back" => {
            // Go back to group list
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
            let groups = db.groups.get_groups_for_guild(guild_id).await?;
            
            let display = crate::handlers::settings_menu::GroupListDisplay {
                guild_name,
                groups,
            };

            let response = CIR::UpdateMessage(
                CIRM::new().embed(display.build_embed()).components(display.build_components())
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        _ if button_id.starts_with("server_settings_group_select_") => {
            // Handle group selection from button - show modal with all settings
            let group_id_str = button_id.strip_prefix("server_settings_group_select_").unwrap();
            if let Ok(group_id) = group_id_str.parse::<u8>() {
                // Find the group
                let groups = db.groups.get_groups_for_guild(guild_id).await?;
                if let Some(group) = groups.iter().find(|g| g.group_id == group_id) {
                    let modal = CreateModal::new(format!("server_settings_group_modal_{group_id}"), "Edit Group Settings")
                        .components(vec![
                            CreateActionRow::InputText(
                                CreateInputText::new(InputTextStyle::Short, "Name", "name")
                                    .placeholder("e.g., NA PUGs, EU Competitive")
                                    .value(group.name.clone().unwrap_or_default())
                                    .required(false)
                                    .max_length(50)
                            ),
                            CreateActionRow::InputText(
                                CreateInputText::new(InputTextStyle::Short, "Quota (2-100)", "quota")
                                    .placeholder("Number of players required")
                                    .value(group.quota.to_string())
                                    .required(true)
                                    .min_length(1)
                                    .max_length(3)
                            ),
                            CreateActionRow::InputText(
                                CreateInputText::new(InputTextStyle::Short, "Hot Join Timeout (seconds)", "timeout")
                                    .placeholder("Seconds to wait before starting game")
                                    .value(group.timeout.to_string())
                                    .required(false)
                                    .min_length(1)
                                    .max_length(4)
                            ),
                        ]);

                    let response = CIR::Modal(modal);
                    interaction.create_response(&ctx.http, response).await?;
                } else {
                    warn!("Group {group_id} not found for guild {guild_id}");
                }
            } else {
                warn!("Invalid group ID in button: {group_id_str}");
            }
        }
        _ => {
            warn!("Unknown server settings button: {}", button_id);
        }
    }

    Ok(())
}

/// Get all rank roles for display (name, elo, role_id)
async fn get_all_rank_roles(db: &Arc<Database>, guild_id: GI) -> Result<Vec<(String, u16, RoleId)>> {
    let guild_ranks = db.ranks.get_or_init_ranks(guild_id).await?;
    
    let result: Vec<(String, u16, RoleId)> = guild_ranks.into_iter()
        .map(|gr| (gr.name, gr.elo, gr.role_id))
        .collect();
    
    Ok(result)
}

/// Convert rank key to display name
fn rank_key_to_name(key: &str) -> String {
    match key {
        "beginner"     => "Beginner"    .to_string(),
        "newcomer"     => "Newcomer"    .to_string(),
        "novice"       => "Novice"      .to_string(),
        "apprentice"   => "Apprentice"  .to_string(),
        "journeyman"   => "Journeyman"  .to_string(),
        "expert"       => "Expert"      .to_string(),
        "master"       => "Master"      .to_string(),
        "master_elite" => "Master Elite".to_string(),
        "grandmaster"  => "Grandmaster" .to_string(),
        _              => key           .to_string(),
    }
}

/// Convert rank key to position index
fn rank_key_to_position(key: &str) -> u8 {
    match key {
        "beginner"     => 0,
        "newcomer"     => 1,
        "novice"       => 2,
        "apprentice"   => 3,
        "journeyman"   => 4,
        "expert"       => 5,
        "master"       => 6,
        "master_elite" => 7,
        "grandmaster"  => 8,
        _              => 4, // Default to journeyman
    }
}

/// Get server settings from database
pub async fn get_server_settings(db: &Arc<Database>, guild_id: GI) -> Result<ServerSettings> {
    let runner_role = db.config.get_config_item("runner_role", guild_id).await?;
    let admin_role = db.config.get_config_item("admin_role", guild_id).await?;

    Ok(ServerSettings {
        runner_role,
        admin_role,
    })
}

/// Get rank settings from database (for rank configuration menu)
pub async fn get_rank_settings(db: &Arc<Database>, guild_id: GI) -> Result<(bool, String)> {
    let dynamic_elo = db.config.get_config_item("active_elo_enabled", guild_id).await?
        .map(|v| v.parse::<bool>().unwrap_or(false))
        .unwrap_or(false);
    let default_rank = db.config.get_config_item("default_rank", guild_id).await?
        .unwrap_or_else(|| crate::models::DEFAULT_RANK.name().to_string());

    Ok((dynamic_elo, default_rank))
}

/// Handle server settings modal submissions
pub async fn handle_server_settings_modal(
    ctx: &Context,
    interaction: &ModalInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let modal_id = &interaction.data.custom_id;

    info!("{} submitted modal {}", interaction.user.name, modal_id);

    if modal_id == "server_settings_rank_modal_add" {
        // Handle add new rank modal
        let mut name_value = String::new();
        let mut elo_value = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input) = component {
                    match input.custom_id.as_str() {
                        "name" => name_value = input.value.clone().unwrap_or_default(),
                        "elo" => elo_value = input.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        let name = name_value.trim();
        if name.is_empty() {
            let response = CIR::Message(
                CIRM::new().content("Rank name cannot be empty.").ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let elo: u16 = match elo_value.trim().parse() {
            Ok(e) => e,
            _ => {
                let response = CIR::Message(
                    CIRM::new().content("Invalid ELO. Must be a valid number.").ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Check if rank name already exists
        if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, name).await {
            let response = CIR::Message(
                CIRM::new().content("A rank with this name already exists. Please choose a different name.").ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Create a new Discord role for this rank
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string());
        let role_name = name.to_string();
        
        let role_id = match guild_id.create_role(&ctx.http,
            serenity::all::EditRole::new()
                .name(&role_name)
                .colour(serenity::all::Color::from_rgb(128, 128, 128))
                .hoist(false)
                .mentionable(true)
                .permissions(serenity::all::Permissions::empty())
        ).await {
            Ok(role) => {
                info!("[{}] Created new role {} for rank {}", guild_name, role.name, name);
                role.id
            },
            Err(e) => {
                warn!("[{}] Failed to create role for rank {}: {}", guild_name, name, e);
                let response = CIR::Message(
                    CIRM::new().content("Failed to create Discord role. Please check bot permissions.").ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Add rank to DB with the created role ID
        db.ranks.add_rank(guild_id, name, elo, role_id).await?;
        info!("{} added rank '{}' with ELO {} and role {}", interaction.user.name, name, elo, role_id.get());

        // Return to rank configuration menu
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
        let rank_roles = get_all_rank_roles(db, guild_id).await?;
        let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
        
        let display = crate::handlers::settings_menu::RankConfigDisplay {
            guild_name,
            rank_roles,
            dynamic_elo,
            default_rank,
        };

        let response = CIR::UpdateMessage(
            CIRM::new().embed(display.build_embed()).components(display.build_components())
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("server_settings_rank_modal_link_") {
        // Handle link existing rank modal
        let role_id_str = modal_id.strip_prefix("server_settings_rank_modal_link_").unwrap();
        let role_id = match role_id_str.parse::<u64>() {
            Ok(id) => serenity::all::RoleId::new(id),
            Err(_) => {
                let response = CIR::Message(
                    CIRM::new().content("Invalid role ID.").ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        let mut name_value = String::new();
        let mut elo_value = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input) = component {
                    match input.custom_id.as_str() {
                        "name" => name_value = input.value.clone().unwrap_or_default(),
                        "elo" => elo_value = input.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        let name = name_value.trim();
        if name.is_empty() {
            let response = CIR::Message(
                CIRM::new().content("Rank name cannot be empty.").ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let elo: u16 = match elo_value.trim().parse() {
            Ok(e) => e,
            _ => {
                let response = CIR::Message(
                    CIRM::new().content("Invalid ELO. Must be a valid number.").ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Check if rank name already exists
        if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, name).await {
            let response = CIR::Message(
                CIRM::new().content("A rank with this name already exists. Please choose a different name.").ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Add rank to DB with the selected role ID
        db.ranks.add_rank(guild_id, name, elo, role_id).await?;
        info!("{} linked rank '{}' with ELO {} to role {}", interaction.user.name, name, elo, role_id.get());

        // Return to rank configuration menu
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
        let rank_roles = get_all_rank_roles(db, guild_id).await?;
        let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
        
        let display = crate::handlers::settings_menu::RankConfigDisplay {
            guild_name,
            rank_roles,
            dynamic_elo,
            default_rank,
        };

        let response = CIR::UpdateMessage(
            CIRM::new().embed(display.build_embed()).components(display.build_components())
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("server_settings_rank_modal_") {
        // Handle rank name/ELO edit modal
        let old_rank_name = modal_id
            .strip_prefix("server_settings_rank_modal_")
            .ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

        let mut name_value = String::new();
        let mut elo_value = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input) = component {
                    match input.custom_id.as_str() {
                        "name" => name_value = input.value.clone().unwrap_or_default(),
                        "elo" => elo_value = input.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        let new_name = name_value.trim();
        if new_name.is_empty() {
            let response = CIR::Message(
                CIRM::new().content("Rank name cannot be empty.").ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let elo: u16 = match elo_value.trim().parse() {
            Ok(e) => e,
            _ => {
                let response = CIR::Message(
                    CIRM::new().content("Invalid ELO. Must be a valid number.").ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Check if new rank name already exists (and it's not the same rank being renamed)
        if new_name != old_rank_name {
            if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, new_name).await {
                let response = CIR::Message(
                    CIRM::new().content("A rank with this name already exists. Please choose a different name.").ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        }

        // Update rank in DB using name instead of position
        db.ranks.update_rank_name(guild_id, old_rank_name, new_name).await?;
        db.ranks.update_rank_elo(guild_id, new_name, elo).await?;

        // Return to rank configuration menu
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
        let rank_roles = get_all_rank_roles(db, guild_id).await?;
        let (dynamic_elo, default_rank) = get_rank_settings(db, guild_id).await?;
        
        let display = crate::handlers::settings_menu::RankConfigDisplay {
            guild_name,
            rank_roles,
            dynamic_elo,
            default_rank,
        };

        let response = CIR::UpdateMessage(
            CIRM::new().embed(display.build_embed()).components(display.build_components())
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("server_settings_group_modal_") {
        // Handle group settings modal submission
        let group_id: u8 = modal_id
            .strip_prefix("server_settings_group_modal_")
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

        // Extract all values from the modal
        let mut name_value = String::new();
        let mut quota_value = String::new();
        let mut timeout_value = String::new();
        let mut connect_value = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input) = component {
                    match input.custom_id.as_str() {
                        "name" => name_value = input.value.clone().unwrap_or_default(),
                        "quota" => quota_value = input.value.clone().unwrap_or_default(),
                        "timeout" => timeout_value = input.value.clone().unwrap_or_default(),
                        "connect" => connect_value = input.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        // Parse and validate quota
        let quota: u8 = match quota_value.trim().parse() {
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

        // Parse and validate timeout
        let timeout: u16 = match timeout_value.trim().parse() {
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

        let name = if name_value.trim().is_empty() { None } else { Some(name_value.trim().to_string()) };
        let connect_info = if connect_value.trim().is_empty() { None } else { Some(connect_value.trim().to_string()) };

        // Update in database
        db.groups.update_name(guild_id, group_id, name.as_deref()).await?;
        db.groups.update_quota(guild_id, group_id, quota).await?;
        db.groups.update_timeout(guild_id, group_id, timeout).await?;
        if connect_info.is_some() || connect_value.trim().is_empty() {
            db.groups.update_connect_info(guild_id, group_id, connect_info.as_deref()).await?;
        }

        // Return to group list
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Server".to_string());
        let groups = db.groups.get_groups_for_guild(guild_id).await?;
        
        let display = crate::handlers::settings_menu::GroupListDisplay {
            guild_name,
            groups,
        };

        let response = CIR::UpdateMessage(
            CIRM::new().embed(display.build_embed()).components(display.build_components())
        );
        interaction.create_response(&ctx.http, response).await?;
    } else {
        warn!("Unknown server settings modal: {}", modal_id);
    }

    Ok(())
}

// ============================================================================
// Group Settings
// ============================================================================

/// Group settings structure for display
pub struct GroupSettings {
    pub group_id:            u8,
    pub name:                Option<String>,
    pub quota:               u8,
    pub timeout:             u16,
    pub connect_info:        Option<String>,
    pub team_balance_method: crate::models::TeamBalanceMethod,
}

/// Build group settings embed
pub fn build_group_settings_embed(settings: &GroupSettings) -> CE {
    use crate::handlers::settings_menu::{AsSettingsMenu, GroupSettingsDisplay};
    let display = GroupSettingsDisplay {
        group_id:            settings.group_id,
        name:                settings.name.clone(),
        quota:               settings.quota,
        timeout:             settings.timeout,
        connect_info:        settings.connect_info.clone(),
        team_balance_method: settings.team_balance_method,
    };
    display.as_settings_menu().build_embed()
}

/// Build group settings buttons with group_id embedded in custom_id
pub fn build_group_settings_buttons(group_id: u8, team_balance_method: crate::models::TeamBalanceMethod) -> Vec<CAR> {
    use crate::handlers::settings_menu::{AsSettingsMenu, GroupSettingsDisplay};
    let display = GroupSettingsDisplay {
        group_id,
        name:                None,
        quota:               0,
        timeout:             0,
        connect_info:        None,
        team_balance_method,
    };
    display.as_settings_menu().build_components()
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
        group_id:            group.group_id,
        name:                group.name.clone(),
        quota:               group.quota,
        timeout:             group.timeout,
        connect_info:        group.connect_info.clone(),
        team_balance_method: group.team_balance_method,
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
        let modal = CreateModal::new(format!("group_settings_modal_timeout_{group_id}"), "Set Hot Join Timeout")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Timeout (seconds)", "timeout")
                        .placeholder("Seconds for missing players to join VC when queue goes hot")
                        .value(settings.timeout.to_string())
                        .required(true)
                        .min_length(1)
                        .max_length(3)
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
        group_id:            group.group_id,
        name:                group.name.clone(),
        quota:               group.quota,
        timeout:             group.timeout,
        connect_info:        group.connect_info.clone(),
        team_balance_method: group.team_balance_method,
    };

    let embed = build_group_settings_embed(&settings);
    let buttons = build_group_settings_buttons(settings.group_id, settings.team_balance_method);

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

        // Update in-memory and build settings while holding lock
        group.name = name.clone();
        let settings = GroupSettings {
            group_id:            group.group_id,
            name:                group.name.clone(),
            quota:               group.quota,
            timeout:             group.timeout,
            connect_info:        group.connect_info.clone(),
            team_balance_method: group.team_balance_method,
        };
        drop(manager_lock);

        // Update in database (after releasing lock)
        db.groups.update_name(guild_id, group_id, name.as_deref()).await?;

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id, settings.team_balance_method);

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
            guild_id,
            group.channels.queue_vc.get(),
            group.channels.dashboard.get(),
            group.channels.queue_chat.get(),
            group.channels.teams[0].red_vc.get(),
            group.channels.teams[0].blu_vc.get(),
            quota,
        ).await?;

        // Update dashboard
        group.queue_dash_update(ctx, guild_id).await;

        // Get updated settings and refresh the menu
        let settings = GroupSettings {
            group_id:            group.group_id,
            name:                group.name.clone(),
            quota:               group.quota,
            timeout:             group.timeout,
            connect_info:        group.connect_info.clone(),
            team_balance_method: group.team_balance_method,
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id, settings.team_balance_method);

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
            group_id:            group.group_id,
            name:                group.name.clone(),
            quota:               group.quota,
            timeout:             group.timeout,
            connect_info:        group.connect_info.clone(),
            team_balance_method: group.team_balance_method,
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id, settings.team_balance_method);

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
        group.queue_dash_update(ctx, guild_id).await;

        // Get updated settings and refresh the menu
        let settings = GroupSettings {
            group_id:            group.group_id,
            name:                group.name.clone(),
            quota:               group.quota,
            timeout:             group.timeout,
            connect_info:        group.connect_info.clone(),
            team_balance_method: group.team_balance_method,
        };

        let embed = build_group_settings_embed(&settings);
        let buttons = build_group_settings_buttons(settings.group_id, settings.team_balance_method);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else {
        warn!("Unknown group settings modal: {}", modal_id);
    }

    Ok(())
}

/// Handle group settings team balance method selection
pub async fn handle_group_settings_balance_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Arc<Database>,
    manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let custom_id = &interaction.data.custom_id;

    info!("[Group Settings] {} selected team balance method", interaction.user.name);

    // Extract group_id from custom_id (format: group_settings_balance_<group_id>)
    let group_id: u8 = custom_id
        .rsplit('_')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid custom_id format: {}", custom_id))?;

    // Extract selected value
    let method_str = match &interaction.data.kind {
        serenity::all::ComponentInteractionDataKind::StringSelect { values } => {
            values.first()
                .ok_or_else(|| anyhow::anyhow!("No value selected"))?
                .clone()
        }
        _ => return Err(anyhow::anyhow!("Expected string select interaction")),
    };

    let method = crate::models::TeamBalanceMethod::from_str(&method_str);

    // Update in-memory and database
    let mut manager_lock = manager.lock().await;
    let group = {
        let server = manager_lock.get_server(guild_id)?;
        server.groups.iter_mut()
            .find(|g| g.group_id == group_id)
            .ok_or_else(|| anyhow::anyhow!("Group {} not found", group_id))?
    };

    group.team_balance_method = method;

    // Update in database
    db.groups.update_team_balance_method(guild_id, group_id, method).await?;

    // Get updated settings and refresh the menu
    let settings = GroupSettings {
        group_id:            group.group_id,
        name:                group.name.clone(),
        quota:               group.quota,
        timeout:             group.timeout,
        connect_info:        group.connect_info.clone(),
        team_balance_method: group.team_balance_method,
    };

    let embed = build_group_settings_embed(&settings);
    let buttons = build_group_settings_buttons(settings.group_id, settings.team_balance_method);

    let response = CIR::UpdateMessage(
        CIRM::new().embed(embed).components(buttons)
    );
    interaction.create_response(&ctx.http, response).await?;

    Ok(())
}

// ============================================================================
// Player Settings (Admin editing of player info)
// ============================================================================

/// Player settings structure for admin editing
pub struct PlayerSettings {
    pub user_id:  serenity::all::UserId,
    pub username: String,
    pub steam_id: Option<u64>,
    pub elo:      u16,
    pub division: String,
    pub games:    u32,
    pub wins:     u32,
}

/// Build player settings embed
pub fn build_player_settings_embed(settings: &PlayerSettings) -> CE {
    use crate::handlers::settings_menu::{AsSettingsMenu, PlayerSettingsDisplay};
    let display = PlayerSettingsDisplay {
        user_id:  settings.user_id,
        username: settings.username.clone(),
        steam_id: settings.steam_id,
        elo:      settings.elo,
        division: settings.division.clone(),
        games:    settings.games,
        wins:     settings.wins,
    };
    display.as_settings_menu().build_embed()
}

/// Build player settings buttons
pub fn build_player_settings_buttons(user_id: serenity::all::UserId) -> Vec<CAR> {
    use crate::handlers::settings_menu::{AsSettingsMenu, PlayerSettingsDisplay};
    let display = PlayerSettingsDisplay {
        user_id,
        username: String::new(),
        steam_id: None,
        elo:      0,
        division: String::new(),
        games:    0,
        wins:     0,
    };
    display.as_settings_menu().build_components()
}

/// Handle player settings button interactions
pub async fn handle_player_settings_button(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let button_id = &interaction.data.custom_id;
    
    info!("[Player Settings] {} pressed {}", interaction.user.name, button_id);

    // Extract user_id from button custom_id (format: player_settings_edit_<action>_<user_id>)
    let target_user_id: u64 = button_id
        .rsplit('_')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid button ID format: {}", button_id))?;
    
    let target_uid = serenity::all::UserId::new(target_user_id);

    // Get current player data
    let player = db.users.get(target_uid).await?;
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let guild_elo = db.elos.get(target_uid, guild_id).await?;

    if button_id.starts_with("player_settings_edit_steam_") {
        let modal = CreateModal::new(format!("player_settings_modal_steam_{target_user_id}"), "Edit Steam ID")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Steam ID (64-bit)", "steam_id")
                        .placeholder("e.g., 76561198012345678")
                        .value(player.steam_id.map(|id| id.to_string()).unwrap_or_default())
                        .required(false)
                        .max_length(20)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else if button_id.starts_with("player_settings_edit_elo_") {
        let modal = CreateModal::new(format!("player_settings_modal_elo_{target_user_id}"), "Edit ELO")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "ELO", "elo")
                        .placeholder("e.g., 50")
                        .value(guild_elo.elo.to_string())
                        .required(true)
                        .min_length(1)
                        .max_length(3)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else if button_id.starts_with("player_settings_edit_division_") {
        let modal = CreateModal::new(format!("player_settings_modal_division_{target_user_id}"), "Edit Rank")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Rank", "division")
                        .placeholder("e.g., Gold, Silver, Bronze")
                        .value(guild_elo.division.name())
                        .required(true)
                        .max_length(20)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else if button_id.starts_with("player_settings_edit_alerts_") {
        // Get target user's current alert settings
        let user_settings = db.users.get_prefs(target_uid).await?;
        
        let modal = CreateModal::new(format!("player_settings_modal_alerts_{target_user_id}"), "Edit Player Alerts")
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "HEX Color", "announcement_color")
                        .placeholder("e.g., 3447003 or FF5733")
                        .value(format!("{:06X}", user_settings.announcement_color))
                        .required(false)
                        .min_length(6)
                        .max_length(6)
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Paragraph, "Join Alert Message", "alert_desc")
                        .placeholder("e.g., Kafri: defense")
                        .value(user_settings.alert_desc.unwrap_or_default())
                        .required(false)
                        .max_length(2000)
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "Join Alert Footer", "alert_footer_text")
                        .placeholder("e.g., Good luck!")
                        .value(user_settings.alert_footer_text.unwrap_or_default())
                        .required(false)
                        .max_length(2048)
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Paragraph, "Leave Alert Message", "leave_alert_desc")
                        .placeholder("e.g., See you next time!")
                        .value(user_settings.leave_alert_desc.unwrap_or_default())
                        .required(false)
                        .max_length(2000)
                ),
            ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
    } else {
        warn!("Unknown player settings button: {}", button_id);
    }

    Ok(())
}

/// Handle player settings modal submissions
pub async fn handle_player_settings_modal(
    ctx: &Context,
    interaction: &ModalInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let guild_id = interaction.guild_id.expect("Guild ID not found");
    let modal_id = &interaction.data.custom_id;

    info!("[Player Settings] {} submitted modal {}", interaction.user.name, modal_id);

    // Extract user_id from modal custom_id (format: player_settings_modal_<action>_<user_id>)
    let target_user_id: u64 = modal_id
        .rsplit('_')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;
    
    let target_uid = serenity::all::UserId::new(target_user_id);

    if modal_id.starts_with("player_settings_modal_steam_") {
        let steam_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let steam_id: Option<u64> = if steam_str.trim().is_empty() {
            None
        } else {
            match steam_str.trim().parse() {
                Ok(id) => Some(id),
                Err(_) => {
                    let response = CIR::Message(
                        CIRM::new()
                            .content("Invalid Steam ID. Must be a 64-bit number.")
                            .ephemeral(true)
                    );
                    interaction.create_response(&ctx.http, response).await?;
                    return Ok(());
                }
            }
        };

        db.users.update_steam_id(&target_uid, steam_id).await?;

        // Refresh the settings menu
        let player = db.users.get(target_uid).await?;
        let guild_elo = db.elos.get(target_uid, guild_id).await?;
        let username = ctx.http.get_user(target_uid).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| target_user_id.to_string());

        let settings = PlayerSettings {
            user_id:  target_uid,
            username,
            steam_id: player.steam_id,
            elo:      guild_elo.elo,
            division: guild_elo.division.name().to_string(),
            games:    guild_elo.games,
            wins:     guild_elo.wins,
        };

        let embed = build_player_settings_embed(&settings);
        let buttons = build_player_settings_buttons(target_uid);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("player_settings_modal_elo_") {
        let elo_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let elo: u16 = match elo_str.trim().parse() {
            Ok(e) => e,
            _ => {
                let response = CIR::Message(
                    CIRM::new()
                        .content("Invalid ELO. Must be a valid number.")
                        .ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Get current division and calculate new rank from ELO
        let guild_elo = db.elos.get(target_uid, guild_id).await?;
        let old_rank = guild_elo.division;
        let new_rank = crate::models::types::Rank::from_elo(elo, db, guild_id).await;

        // Update ELO and rank in database
        db.elos.set(target_uid, guild_id, elo, new_rank).await?;
        
        if old_rank != new_rank {
            info!("Updated rank for {}: {} -> {}", target_uid, old_rank.name(), new_rank.name());
        }

        // Refresh the settings menu
        let player = db.users.get(target_uid).await?;
        let guild_elo = db.elos.get(target_uid, guild_id).await?;
        let username = ctx.http.get_user(target_uid).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| target_user_id.to_string());

        let settings = PlayerSettings {
            user_id:  target_uid,
            username,
            steam_id: player.steam_id,
            elo:      guild_elo.elo,
            division: guild_elo.division.name().to_string(),
            games:    guild_elo.games,
            wins:     guild_elo.wins,
        };

        let embed = build_player_settings_embed(&settings);
        let buttons = build_player_settings_buttons(target_uid);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("player_settings_modal_division_") {
        let division_str = interaction.data.components.first()
            .and_then(|row| row.components.first())
            .and_then(|c| {
                if let serenity::all::ActionRowComponent::InputText(input) = c {
                    input.value.clone()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let new_rank = crate::models::types::Rank::from_name(division_str.trim());

        // Get current data and calculate new ELO from rank
        let guild_elo = db.elos.get(target_uid, guild_id).await?;
        let old_rank = guild_elo.division;
        let new_elo = new_rank.default_rank_elo();

        // Update ELO and rank in database
        db.elos.set(target_uid, guild_id, new_elo, new_rank).await?;
        
        if old_rank != new_rank {
            info!("Updated rank for {}: {} -> {}", target_uid, old_rank.name(), new_rank.name());
        }

        // Refresh the settings menu
        let player = db.users.get(target_uid).await?;
        let guild_elo = db.elos.get(target_uid, guild_id).await?;
        let username = ctx.http.get_user(target_uid).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| target_user_id.to_string());

        let settings = PlayerSettings {
            user_id:  target_uid,
            username,
            steam_id: player.steam_id,
            elo:      guild_elo.elo,
            division: guild_elo.division.name().to_string(),
            games:    guild_elo.games,
            wins:     guild_elo.wins,
        };

        let embed = build_player_settings_embed(&settings);
        let buttons = build_player_settings_buttons(target_uid);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;
    } else if modal_id.starts_with("player_settings_modal_alerts_") {
        // Extract values from modal components
        let mut user_settings = db.users.get_prefs(target_uid).await?;

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
                                        user_settings.announcement_color = color;
                                    }
                                }
                            }
                        },
                        1 => user_settings.alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                        2 => user_settings.alert_footer_text = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                        3 => user_settings.leave_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                        _ => {}
                    }
                }
            }
        }

        // Update target user's settings
        db.users.update_settings(target_uid, &user_settings).await?;

        // Refresh the player settings menu
        let player = db.users.get(target_uid).await?;
        let guild_elo = db.elos.get(target_uid, guild_id).await?;
        let username = ctx.http.get_user(target_uid).await
            .map(|u| u.name.clone())
            .unwrap_or_else(|_| target_user_id.to_string());

        let settings = PlayerSettings {
            user_id:  target_uid,
            username,
            steam_id: player.steam_id,
            elo:      guild_elo.elo,
            division: guild_elo.division.name().to_string(),
            games:    guild_elo.games,
            wins:     guild_elo.wins,
        };

        let embed = build_player_settings_embed(&settings);
        let buttons = build_player_settings_buttons(target_uid);

        let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(buttons)
        );
        interaction.create_response(&ctx.http, response).await?;

        info!("[Player Settings] Updated alerts for user {}", target_uid);
    } else {
        warn!("Unknown player settings modal: {}", modal_id);
    }

    Ok(())
}
