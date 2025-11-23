use anyhow::Result;
use serenity::all::{
    ComponentInteraction, ModalInteraction, Context, CreateEmbed as CE, CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM, CreateActionRow as CAR, CreateButton as CB,
    ButtonStyle as BS, EditMessage, CreateInputText, InputTextStyle, CreateActionRow,
    CreateModal, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage,
};
use std::sync::Arc;
use tracing::{info, warn};

use crate::Database;

/// Handle settings button interactions in DMs
pub async fn handle_settings_button(
    ctx: &Context,
    interaction: &ComponentInteraction,
    db: &Arc<Database>,
) -> Result<()> {
    let user_id = interaction.user.id;
    let button_id = &interaction.data.custom_id;

    info!("User {} clicked settings button: {}", user_id, button_id);

    // Update activity timestamp for DM cleanup tracking
    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
        dm_tracker.update_activity(user_id).await;
    }

    match button_id.as_str() {
        "settings_toggle_dm" => {
            // Toggle DM notifications
            let new_state = db.users.toggle_dm_enabled(user_id).await?;

            let (status_text, emoji) = if new_state {
                ("enabled", "🟢")
            } else {
                ("disabled", "❌")
            };

            let response = CIR::UpdateMessage(
                CIRM::new().content(format!("DM Notifications: {emoji} {status_text}"))
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
            settings.vc_disconnect_on_leave = !settings.vc_disconnect_on_leave;
            db.users.update_settings(user_id, &settings).await?;

            let (status_text, emoji) = if settings.vc_disconnect_on_leave {
                ("Yes - Disconnect me", "🟢")
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
        "settings_customize_announcement" => {
            // Show modal for customizing join announcement embed
            let settings = db.users.get_settings(user_id).await?;
            let modal = CreateModal::new("settings_modal_announcement", "Customize join announcement")
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Color (hex, optional)", "announcement_color")
                            .placeholder("e.g., 3447003 or FF5733")
                            .value(format!("{:06X}", settings.announcement_color))
                            .required(false)
                            .min_length(6)
                            .max_length(6)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Title (optional)", "announcement_title")
                            .placeholder("e.g., Player Joined!")
                            .value(settings.announcement_title.unwrap_or_default())
                            .required(false)
                            .max_length(256)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Paragraph, "Description (optional)", "announcement_description")
                            .placeholder("e.g., Welcome! Use {rank} for rank")
                            .value(settings.announcement_description.unwrap_or_default())
                            .required(false)
                            .max_length(2000)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Footer Text (optional)", "announcement_footer_text")
                            .placeholder("e.g., Good luck!")
                            .value(settings.announcement_footer_text.unwrap_or_default())
                            .required(false)
                            .max_length(2048)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Thumbnail URL (optional)", "announcement_thumbnail")
                            .placeholder("https://example.com/thumb.png")
                            .value(settings.announcement_thumbnail.unwrap_or_default())
                            .required(false)
                            .max_length(512)
                    ),
                ]);

            let response = CIR::Modal(modal);
            interaction.create_response(&ctx.http, response).await?;
        }
        "settings_customize_leave_announcement" => {
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
                        CreateInputText::new(InputTextStyle::Short, "Title (optional)", "leave_announcement_title")
                            .placeholder("e.g., Player Left")
                            .value(settings.leave_announcement_title.unwrap_or_default())
                            .required(false)
                            .max_length(256)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Paragraph, "Description (optional)", "leave_announcement_description")
                            .placeholder("e.g., {name} has left. Use {user} for mention")
                            .value(settings.leave_announcement_description.unwrap_or_default())
                            .required(false)
                            .max_length(2000)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Footer Text (optional)", "leave_announcement_footer_text")
                            .placeholder("e.g., See you next time!")
                            .value(settings.leave_announcement_footer_text.unwrap_or_default())
                            .required(false)
                            .max_length(2048)
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "Thumbnail URL (optional)", "leave_announcement_thumbnail")
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
                                        if color >= 0 && color <= 0xFFFFFF {
                                            settings.announcement_color = color;
                                        }
                                    }
                                }
                            },
                            1 => settings.announcement_title = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            2 => settings.announcement_description = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            3 => settings.announcement_footer_text = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            4 => settings.announcement_thumbnail = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            _ => {}
                        }
                    }
                }
            }
            
            // Update settings in database
            db.users.update_settings(user_id, &settings).await?;
            
            let embed = build_settings_embed(&settings);
            let buttons = build_settings_buttons(&settings);

            let response = CIR::Message(
                CIRM::new()
                    .content("Join announcement customized! Your settings are shown below.")
                    .embed(embed)
                    .components(buttons)
                    .ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            
            // Send preview of the announcement embed in DM
            if let Ok(user) = ctx.http.get_user(user_id).await {
                // Use shared function with example rank for preview
                let preview_embed = build_join_announcement_embed(ctx, user_id, None, &settings, "Journeyman").await;
                let preview_message = CreateMessage::new()
                    .content("**Preview of your join announcement:**")
                    .embed(preview_embed);
                    
                if let Ok(msg) = user.direct_message(&ctx.http, preview_message).await {
                    // Track this message for cleanup
                    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
                        dm_tracker.track_message(user_id, msg.channel_id, msg.id).await;
                    }
                    info!("Sent announcement preview to user {}", user_id);
                }
            }
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

                                let settings = db.users.get_settings(user_id).await?;
                                let embed = build_settings_embed(&settings);
                                let buttons = build_settings_buttons(&settings);

                                let response = CIR::Message(
                                    CIRM::new()
                                        .content(format!("Auto-remove timer: {}", status_text))
                                        .embed(embed)
                                        .components(buttons)
                                        .ephemeral(true)
                                );
                                interaction.create_response(&ctx.http, response).await?;
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
                            1 => settings.leave_announcement_title = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            2 => settings.leave_announcement_description = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            3 => settings.leave_announcement_footer_text = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            4 => settings.leave_announcement_thumbnail = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
                            _ => {}
                        }
                    }
                }
            }
            
            // Update settings in database
            db.users.update_settings(user_id, &settings).await?;
            
            let embed = build_settings_embed(&settings);
            let buttons = build_settings_buttons(&settings);

            let response = CIR::Message(
                CIRM::new()
                    .content("Leave announcement customized! Your settings are shown below.")
                    .embed(embed)
                    .components(buttons)
                    .ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            
            // Send preview of the leave announcement embed in DM
            if let Ok(user) = ctx.http.get_user(user_id).await {
                let preview_embed = build_leave_announcement_embed(ctx, user_id, None, &settings).await;
                let preview_message = CreateMessage::new()
                    .content("**Preview of your leave announcement:**")
                    .embed(preview_embed);
                    
                if let Ok(msg) = user.direct_message(&ctx.http, preview_message).await {
                    // Track this message for cleanup
                    if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
                        dm_tracker.track_message(user_id, msg.channel_id, msg.id).await;
                    }
                    info!("Sent leave announcement preview to user {}", user_id);
                }
            }
        }
        _ => {
            warn!("Unknown settings modal: {}", modal_id);
        }
    }

    Ok(())
}

/// Update the settings menu embed
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

/// Build settings embed
pub fn build_settings_embed(settings: &crate::database::repositories::UserSettings) -> CE {
    CE::new()
        .title("Your Personal Settings")
        .description(format!(
            "Configure your queue experience!\n\n\
            **Current Settings:**\n\
            **DM notifications:** {}\n\
            **Auto-remove timer:** {} minute{}\n\
            **Disconnect from VC on leave:** {}\n\n\
            Click the buttons below to change your settings.",
            if settings.dm_enabled { "🟢" } else { "🔴" },
            settings.auto_remove_minutes,
            if settings.auto_remove_minutes == 1 { "" } else { "s" },
            if settings.vc_disconnect_on_leave { "🟢" } else { "🔴" }
        ))
        .color(settings.announcement_color as u32)
        .footer(serenity::all::CreateEmbedFooter::new("Tip: All settings are saved automatically"))
}

/// Build settings buttons
pub fn build_settings_buttons(settings: &crate::database::repositories::UserSettings) -> Vec<CAR> {
    vec![
        CAR::Buttons(vec![
            CB::new("settings_toggle_dm")
                .label("DM notifications")
                .style(if settings.dm_enabled { BS::Success } else { BS::Secondary }),
            CB::new("settings_auto_leave")
                .label("Auto-remove timer")
                .style(BS::Primary),
        ]),
        CAR::Buttons(vec![
            CB::new("settings_vc_disconnect")
                .label("Disconnect VC on leave")
                .style(if settings.vc_disconnect_on_leave { BS::Success } else { BS::Secondary }),
        ]),
        CAR::Buttons(vec![
            CB::new("settings_customize_announcement")
                .label("Edit join announcement")
                .style(BS::Primary),
            CB::new("settings_customize_leave_announcement")
                .label("Edit leave announcement")
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
    // Get member to access nickname and avatar
    let (display_name, avatar_url) = if let Some(gid) = guild_id {
        let member = gid.member(&ctx.http, user_id).await.ok();
        let display_name = member.as_ref()
            .map(|m| m.display_name().to_string())
            .unwrap_or_else(|| ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string()));
        let avatar_url = ctx.cache.user(user_id).map(|u| u.face());
        (display_name, avatar_url)
    } else {
        // For preview without guild context
        let username = ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string());
        let avatar_url = ctx.cache.user(user_id).map(|u| u.face());
        (username, avatar_url)
    };
    
    // Build description with template support
    let description = if let Some(custom_desc) = &settings.announcement_description {
        // Replace template variables
        custom_desc
            .replace("{user}", &format!("<@{}>", user_id))
            .replace("{rank}", rank_name)
            .replace("{name}", &display_name)
    } else {
        // Default description
        format!("<@{}> joined the queue!", user_id)
    };
    
    // Create embed with author showing nickname + "joined the queue"
    let mut embed = CE::new()
        .author({
            let mut author = CreateEmbedAuthor::new(format!("{} joined the queue", display_name));
            if let Some(url) = avatar_url {
                author = author.icon_url(url);
            }
            author
        })
        .description(description)
        .color(settings.announcement_color as u32);
    
    // Add custom title if provided
    if let Some(title) = &settings.announcement_title {
        embed = embed.title(title);
    }
    
    // Add custom footer if provided
    if let Some(footer_text) = &settings.announcement_footer_text {
        let mut footer = CreateEmbedFooter::new(footer_text);
        if let Some(footer_icon) = &settings.announcement_footer_icon {
            footer = footer.icon_url(footer_icon);
        }
        embed = embed.footer(footer);
    }
    
    // Add custom thumbnail if provided
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
    // Get member to access nickname and avatar
    let (display_name, avatar_url) = if let Some(gid) = guild_id {
        let member = gid.member(&ctx.http, user_id).await.ok();
        let display_name = member.as_ref()
            .map(|m| m.display_name().to_string())
            .unwrap_or_else(|| ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string()));
        let avatar_url = ctx.cache.user(user_id).map(|u| u.face());
        (display_name, avatar_url)
    } else {
        // For preview without guild context
        let username = ctx.cache.user(user_id).map(|u| u.name.clone()).unwrap_or_else(|| user_id.to_string());
        let avatar_url = ctx.cache.user(user_id).map(|u| u.face());
        (username, avatar_url)
    };
    
    // Build description with template support
    let description = if let Some(custom_desc) = &settings.leave_announcement_description {
        // Replace template variables (no rank for leave)
        custom_desc
            .replace("{user}", &format!("<@{}>", user_id))
            .replace("{name}", &display_name)
    } else {
        // Default description
        format!("<@{}> left the queue!", user_id)
    };
    
    // Create embed with author showing nickname + "left the queue"
    let mut embed = CE::new()
        .author({
            let mut author = CreateEmbedAuthor::new(format!("{} left the queue", display_name));
            if let Some(url) = avatar_url {
                author = author.icon_url(url);
            }
            author
        })
        .description(description)
        .color(settings.announcement_color as u32);
    
    // Add custom title if provided
    if let Some(title) = &settings.leave_announcement_title {
        embed = embed.title(title);
    }
    
    // Add custom footer if provided
    if let Some(footer_text) = &settings.leave_announcement_footer_text {
        let mut footer = CreateEmbedFooter::new(footer_text);
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
