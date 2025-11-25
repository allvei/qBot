use anyhow::Result;
use serenity::all::{
    ComponentInteraction, ModalInteraction, Context, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateActionRow as CAR, CreateButton as CB,
    ButtonStyle as BS, EditMessage, CreateInputText, InputTextStyle, CreateActionRow,
    CreateModal, CreateEmbedFooter,
};
use std::sync::Arc;
use tracing::{info, warn};

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

/// Check if text contains newline spam
/// Returns true if there are excessive newlines relative to actual content
fn is_newline_spam(text: &str) -> bool {
    let newline_count = text.matches('\n').count();
    let non_whitespace_chars = text.chars().filter(|c| !c.is_whitespace()).count();
    let lines: Vec<&str> = text.lines().collect();
    
    // Count non-empty lines (lines with actual content)
    let non_empty_lines = lines.iter().filter(|line| !line.trim().is_empty()).count();
    // Count short lines (1-3 characters) - common in spam patterns
    let short_lines = lines.iter()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && trimmed.len() <= 3
        })
        .count();
    
    // Consider it spam if ANY of these conditions are met:
    // 1. More than 4 consecutive newlines anywhere
    // 2. More newlines than actual content characters (stricter ratio)
    // 3. More than 5 newlines total with very little content
    // 4. High ratio of newlines to non-empty lines (lots of spacing)
    // 5. Many short lines separated by newlines (spam pattern)
    text.contains("\n\n\n\n\n")
        || (newline_count > 0 && non_whitespace_chars > 0 && newline_count > non_whitespace_chars * 2)
        || (newline_count > 5 && non_whitespace_chars < 15)
        || (non_empty_lines > 0 && newline_count > non_empty_lines * 3)
        || (short_lines >= 3 && newline_count >= short_lines * 2)
}

/// Process text and replace with spam message if newline spam detected
fn sanitize_announcement_text(text: &str) -> String {
    if is_newline_spam(text) {
        // Pick a random spam replacement message
        use rand::Rng;
        let idx = rand::rng().random_range(0..SPAM_REPLACEMENT_MESSAGES.len());
        SPAM_REPLACEMENT_MESSAGES[idx].to_string()
    } else {
        text.to_string()
    }
}

/// Process footer text and replace with spam message if newline spam detected
fn sanitize_footer_text(text: &str) -> String {
    if is_newline_spam(text) {
        // Pick a random footer spam replacement message
        use rand::Rng;
        let idx = rand::rng().random_range(0..FOOTER_SPAM_REPLACEMENT_MESSAGES.len());
        FOOTER_SPAM_REPLACEMENT_MESSAGES[idx].to_string()
    } else {
        text.to_string()
    }
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
            let new_state = db.users.toggle_dm_enabled(user_id).await?;

            let (status_text, emoji) = if new_state {
                ("enabled", "🟢 Enabled")
            } else {
                ("disabled", "❌")
            };

            let response = CIR::UpdateMessage(
                CIRM::new().content(format!("DM Alerts: {emoji} {status_text}"))
            );
            interaction.create_response(&ctx.http, response).await?;

            // Update the settings menu
            update_settings_menu(ctx, interaction, db).await?;
        }
        "settings_auto_leave" => {
            // Show modal for auto-leave timeout
            let modal = CreateModal::new("settings_modal_auto_leave", "Auto-remove timer")
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Minutes (1-60)", "auto_remove_minutes")
                            .placeholder("Default: 30 minutes")
                            .min_length(1)
                            .max_length(2)
                            .required(true)
                    )
                ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_vc_disconnect" => {
            // Toggle VC disconnect preference
            let mut settings = db.users.get_settings(user_id).await?;
            settings.vc_kick = !settings.vc_kick;
            db.users.update_settings(user_id, &settings).await?;

            let (status_text, emoji) = if settings.vc_kick {
                ("Yes - Disconnect me", "🟢 Enabled")
            } else {
                ("No - Keep me in VC", "❌")
            };

            let response = CIR::UpdateMessage(
                CIRM::new().content(format!("VC Disconnect on Leave: {emoji} {status_text}"))
            );
            interaction.create_response(&ctx.http, response).await?;

            // Update the settings menu
            update_settings_menu(ctx, interaction, db).await?;
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
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Paragraph, "Message", "announcement_description")
                        .placeholder("e.g., Kafri: defense")
                        .value(settings.announcement_description.unwrap_or_default())
                        .required(false).max_length(2000)
                    ),
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short,     "Footer text", "announcement_footer_text")
                        .placeholder("e.g., Good luck!")
                        .value(settings.announcement_footer_text.unwrap_or_default())
                        .required(false).max_length(2048)
                    ),
                    CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short,     "Thumbnail URL", "announcement_thumbnail")
                        .placeholder("https://example.com/thumb.png")
                        .value(settings.announcement_thumbnail.unwrap_or_default())
                        .required(false).max_length(512)
                    ),
                ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_edit_leave_alert" => {
            // Show modal for customizing leave announcement embed
            let settings = db.users.get_settings(user_id).await?;
            let modal = CreateModal::new("settings_modal_leave_announcement", "Customize leave announcement")
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Color (hex, optional)", "leave_announcement_color")
                            .placeholder("e.g., 3447003 or FF5733")
                            .value(format!("{:06X}", settings.announcement_color))
                            .required(false)
                            .min_length(6)
                            .max_length(6)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Paragraph, "Description", "leave_announcement_description")
                            .placeholder("e.g., {name} has left. Use {user} for mention")
                            .value(settings.leave_announcement_description.unwrap_or_default())
                            .required(false)
                            .max_length(2000)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Footer Text", "leave_announcement_footer_text")
                            .placeholder("e.g., See you next time!")
                            .value(settings.leave_announcement_footer_text.unwrap_or_default())
                            .required(false)
                            .max_length(2048)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Thumbnail URL", "leave_announcement_thumbnail")
                            .placeholder("https://example.com/thumb.png")
                            .value(settings.leave_announcement_thumbnail.unwrap_or_default())
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
                            1 => settings.announcement_description = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            2 => settings.announcement_footer_text = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            3 => settings.announcement_thumbnail   = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
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
        "settings_modal_auto_leave" => {
            // Get the value from the modal
            if let Some(action_row) = interaction.data.components.first() {
                if let Some(component) = action_row.components.first() {
                    if let serenity::all::ActionRowComponent::InputText(input) = component {
                        if let Some(value) = &input.value {
                            if !value.is_empty() {
                        match value.parse::<i64>() {
                            Ok(minutes) if minutes >= 1 && minutes <= 60 => {
                                db.users.update_setting_field(user_id, "auto_remove_minutes", minutes).await?;

                                let status_text = format!("set to {} minute{}", minutes, if minutes == 1 { "" } else { "s" });

                                let response = CIR::Message(
                                    CIRM::new()
                                        .content(format!("Auto-remove timer: {}", status_text))
                                        .ephemeral(true)
                                );
                                interaction.create_response(&ctx.http, response).await?;

                                // Update the original settings menu
                                update_settings_menu_from_modal(ctx, interaction, db).await?;
                            }
                            _ => {
                                let response = CIR::Message(
                                    CIRM::new()
                                        .content("Invalid value! Please enter a number between 1 and 60.")
                                        .ephemeral(true)
                                );
                                interaction.create_response(&ctx.http, response).await?;
                            }
                        }
                        }
                        }
                    }
                }
            }
        }
        "settings_modal_leave_announcement" => {
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
                            1 => settings.leave_announcement_description = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            2 => settings.leave_announcement_footer_text = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            3 => settings.leave_announcement_thumbnail   = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            _ => {}
                        }
                    }
                }
            }

            // Update settings in database
            db.users.update_settings(user_id, &settings).await?;

            // Build preview embed
            let preview_embed = build_leave_announcement_embed(ctx, user_id, None, &settings).await;

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
                "**Auto-remove timer:** {} minute{}\n",
                minutes,
                if minutes == 1 { "" } else { "s" }
            )
        })
        .color(settings.announcement_color as u32)
        .footer(CreateEmbedFooter::new("¹Kicks you from the voice channel when you are not in the queue."))
}

/// Build settings buttons
pub fn build_settings_buttons(settings: &crate::database::repositories::UserSettings) -> Vec<CAR> {
    vec![
        CAR::Buttons(vec![
            CB::new("settings_toggle_dm")
                .label(if settings.dm_alerts { "DM alerts enabled" } else { "DM alerts disabled" })
                .style(if settings.dm_alerts { BS::Success } else { BS::Secondary }),
            CB::new("settings_vc_disconnect")
                .label(if settings.vc_kick { "VC kick¹ enabled" } else { "VC kick¹ disabled" })
                .style(if settings.vc_kick { BS::Success } else { BS::Secondary }),
        ]),
        CAR::Buttons(vec![
            CB::new("settings_auto_leave")
                .label("Change auto-remove time")
                .style(BS::Primary),
        ]),
        CAR::Buttons(vec![
            CB::new("settings_edit_alert")
                .label("Edit join alert")
                .style(BS::Primary),
            CB::new("settings_edit_leave_alert")
                .label("Edit leave alert")
                .style(BS::Primary),
        ]),
    ]
}

/// Build a join announcement embed (used for both actual announcements and previews)
pub async fn build_join_announcement_embed(
    ctx: &Context,
    user_id: serenity::all::UserId,
    guild_id: Option<serenity::all::GuildId>,
    settings: &crate::database::repositories::UserSettings,
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
    let description = if let Some(custom_desc) = &settings.announcement_description {
        // Sanitize newline spam only for actual announcements (not previews)
        let text_to_use = if guild_id.is_some() {
            sanitize_announcement_text(custom_desc)
        } else {
            custom_desc.to_string()
        };
        
        // Replace template variables
        text_to_use
            .replace("{user}", &format!("<@{}>", user_id))
            .replace("{rank}", rank_name)
            .replace("{name}", &display_name)
    } else {
        // Default description
        format!("<@{}> joined the queue!", user_id)
    };

    // Create embed with title showing nickname + "joined the queue"
    let mut embed = CE::new()
        .title(format!("{display_name} joined the queue"))
        .description(description)
        .color(settings.announcement_color as u32);

    // Add custom footer
    if let Some(footer_text) = &settings.announcement_footer_text {
        // Sanitize footer spam only for actual announcements (not previews)
        let footer_to_use = if guild_id.is_some() {
            sanitize_footer_text(footer_text)
        } else {
            footer_text.to_string()
        };
        
        let mut footer = CreateEmbedFooter::new(footer_to_use);
        if let Some(footer_icon) = &settings.announcement_footer_icon {
            footer = footer.icon_url(footer_icon);
        }
        embed = embed.footer(footer);
    }

    // Add thumbnail
    if let Some(thumbnail) = &settings.announcement_thumbnail {
        embed = embed.thumbnail(thumbnail);
    }

    embed
}

/// Build a leave announcement embed (used for both actual announcements and previews)
pub async fn build_leave_announcement_embed(
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
    let description = if let Some(custom_desc) = &settings.leave_announcement_description {
        // Sanitize newline spam only for actual announcements (not previews)
        let text_to_use = if guild_id.is_some() {
            sanitize_announcement_text(custom_desc)
        } else {
            custom_desc.to_string()
        };
        
        // Replace template variables (no rank for leave)
        text_to_use
            .replace("{user}", &format!("<@{}>", user_id))
            .replace("{name}", &display_name)
    } else {
        // Default description
        format!("<@{}> left the queue!", user_id)
    };

    // Create embed with title showing nickname + "left the queue"
    let mut embed = CE::new()
        .title(format!("{display_name} left the queue"))
        .description(description)
        .color(settings.announcement_color as u32);

    // Add custom footer if provided
    if let Some(footer_text) = &settings.leave_announcement_footer_text {
        // Sanitize footer spam only for actual announcements (not previews)
        let footer_to_use = if guild_id.is_some() {
            sanitize_footer_text(footer_text)
        } else {
            footer_text.to_string()
        };
        
        let mut footer = CreateEmbedFooter::new(footer_to_use);
        if let Some(footer_icon) = &settings.leave_announcement_footer_icon {
            footer = footer.icon_url(footer_icon);
        }
        embed = embed.footer(footer);
    }

    // Add custom thumbnail if provided
    if let Some(thumbnail) = &settings.leave_announcement_thumbnail {
        embed = embed.thumbnail(thumbnail);
    }

    embed
}
