use serenity::all::{EditMessage, Context, ComponentInteraction as CI, ModalInteraction as MI, UserId as UI, CreateActionRow as CAR, CreateInteractionResponse as CIR, CreateButton as CB, ButtonStyle as BS, CreateEmbed as CE, CreateInteractionResponseMessage as CIRM, ActionRowComponent as ARC, CreateModal, GetMessages as GM, ChannelId as CHID};
use anyhow::Result;
use tracing::{warn, debug};
use std::sync::Arc;
use crate::Database;
use crate::handlers::settings::utils::{track_dm_activity, create_short_input_opt, create_paragraph_input_with_value};
use crate::handlers::{build_settings_buttons, build_settings_embed};
use crate::handlers::settings::alerts::{build_join_alert_embed, build_leave_alert_embed};

/// Handle settings button interactions in DMs
pub async fn handle_settings_button(ctx: &Context, interaction: &CI, db: &Arc<Database>) -> Result<()> {
  let user_id = interaction.user.id;
  let button_id = &interaction.data.custom_id;
  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  debug!("{} pressed {}", user_tag, button_id);

  // Update activity timestamp for DM cleanup tracking
  track_dm_activity(ctx, user_id).await;

  match button_id.as_str() {
    "settings_toggle_dm" => {
      // Toggle DM alerts
      let _new_state = db.players.toggle_pm_hot_alert(user_id).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let settings = db.players.get_prefs(user_id).await?;
      let embed = build_settings_embed(&settings);
      let buttons = build_settings_buttons(&settings);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
      interaction.create_response(&ctx.http, response).await?;
    }
    "settings_queue_expiration" => {
      // Show time selection buttons inline (replace current message temporarily)
      let settings = db.players.get_prefs(user_id).await?;
      let current_minutes = settings.queue_expiration;

      let time_buttons = vec![
        CB::new("settings_queue_expiration:30m").label("30 min").style(if current_minutes == 30 { BS::Success } else { BS::Secondary }),
        CB::new("settings_queue_expiration:1h").label("1 hour").style(if current_minutes == 60 { BS::Success } else { BS::Secondary }),
        CB::new("settings_queue_expiration:2h").label("2 hours").style(if current_minutes == 120 { BS::Success } else { BS::Secondary }),
        CB::new("settings_queue_expiration:3h").label("3 hours").style(if current_minutes == 180 { BS::Success } else { BS::Secondary }),
        CB::new("settings_queue_expiration:4h").label("4 hours").style(if current_minutes == 240 { BS::Success } else { BS::Secondary }),
      ];

      let cancel_button = vec![CB::new("settings_queue_expiration:cancel").label("Cancel").style(BS::Danger)];

      let embed = CE::new().title("Set expiration duration").description("Choose how long before you're automatically removed from the queue:").color(settings.join_alert_color);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(time_buttons), CAR::Buttons(cancel_button)]));

      interaction.create_response(&ctx.http, response).await?;
    }
    button_id if button_id.starts_with("settings_queue_expiration:") => {
      // Handle auto-leave time selection or cancel
      let time_str = button_id.split(':').nth(1).unwrap_or("30m");

      if time_str == "cancel" {
        // Just restore the settings menu
        let settings = db.players.get_prefs(user_id).await?;
        let embed = build_settings_embed(&settings);
        let buttons = build_settings_buttons(&settings);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
        interaction.create_response(&ctx.http, response).await?;
      } else {
        let minutes = match time_str {
          "30m" => 30,
          "1h" => 60,
          "2h" => 120,
          "3h" => 180,
          "4h" => 240,
          _ => 30,
        };

        // Update user settings
        let mut settings = db.players.get_prefs(user_id).await?;
        settings.queue_expiration = minutes;
        db.players.update_prefs(user_id, &settings).await?;

        // Update the settings menu directly (no confirmation popup)
        let embed = build_settings_embed(&settings);
        let buttons = build_settings_buttons(&settings);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_vc_auto_leave" => {
      // Toggle VC disconnect preference
      let mut settings = db.players.get_prefs(user_id).await?;
      settings.vc_auto_leave = !settings.vc_auto_leave;
      db.players.update_prefs(user_id, &settings).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let embed = build_settings_embed(&settings);
      let buttons = build_settings_buttons(&settings);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
      interaction.create_response(&ctx.http, response).await?;
    }
    "settings_vc_auto_join" => {
      // Toggle VC auto-queue preference
      let mut settings = db.players.get_prefs(user_id).await?;
      settings.vc_auto_join = !settings.vc_auto_join;
      db.players.update_prefs(user_id, &settings).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let embed = build_settings_embed(&settings);
      let buttons = build_settings_buttons(&settings);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
      interaction.create_response(&ctx.http, response).await?;
    }
    "settings_edit_alert" => {
      // Show modal for customizing join announcement embed
      let settings = db.players.get_prefs(user_id).await?;
      let modal = CreateModal::new("settings_modal_announcement", "Customize join announcement").components(vec![
        create_short_input_opt("HEX Color", "join_alert_color", "e.g., 3447003 or FF5733", &format!("{:06X}", settings.join_alert_color)),
        create_paragraph_input_with_value("Message", "join_alert", "e.g., Kafri: defense", &settings.join_alert_desc.unwrap_or_default()),
        create_short_input_opt("Footer text", "join_alert_footer", "e.g., Good luck!", &settings.join_alert_footer.unwrap_or_default()),
        create_short_input_opt("Thumbnail URL", "join_alert_img", "https://example.com/thumb.png", &settings.join_alert_img.unwrap_or_default()),
      ]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "settings_edit_leave_alert" => {
      // Show modal for customizing leave announcement embed
      let settings = db.players.get_prefs(user_id).await?;
      let modal = CreateModal::new("settings_modal_leave_alert", "Customize leave announcement").components(vec![
        create_short_input_opt("Color (hex, optional)", "leave_alert_color", "e.g., 3447003 or FF5733", &format!("{:06X}", settings.leave_alert_color)),
        create_paragraph_input_with_value("Description", "leave_alert", "e.g., {name} has left. Use {user} for mention", &settings.leave_alert_desc.unwrap_or_default()),
        create_short_input_opt("Footer text", "leave_alert_footer", "e.g., See you next time!", &settings.leave_alert_footer.unwrap_or_default()),
        create_short_input_opt("Thumbnail URL", "leave_alert_img", "https://example.com/thumb.png", &settings.leave_alert_img.unwrap_or_default()),
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
pub async fn handle_settings_modal(ctx: &Context, interaction: &MI, db: &Arc<Database>) -> Result<()> {
  use crate::db::repo::is_valid_user_text;

  let user_id = interaction.user.id;
  let modal_id = &interaction.data.custom_id;

  // Update activity timestamp for DM cleanup tracking
  if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
    dm_tracker.update_activity(user_id).await;
  }

  match modal_id.as_str() {
    "settings_modal_announcement" => {
      // Get all input values from the modal
      let mut settings = db.players.get_prefs(user_id).await?;

      // Extract and validate values from modal components
      for (idx, action_row) in interaction.data.components.iter().enumerate() {
        if let Some(ARC::InputText(input)) = action_row.components.first() {
          if let Some(value) = &input.value {
            let trimmed = value.trim();

            // Validate text fields for allowed characters (skip color and URL fields)
            if (idx == 1 || idx == 2) && !trimmed.is_empty() && !is_valid_user_text(trimmed) {
              let field_name = if idx == 1 { "Message" } else { "Footer text" };
              let response = CIR::Message(
                CIRM::new().content(format!("**Error:** {} contains invalid characters. Only ASCII printable and extended characters are allowed.", field_name)).ephemeral(true),
              );
              interaction.create_response(&ctx.http, response).await?;
              return Ok(());
            }

            match idx {
              0 => {
                // Color field
                if !trimmed.is_empty() {
                  let hex_str = trimmed.trim_start_matches('#');
                  if let Ok(color) = u32::from_str_radix(hex_str, 16) {
                    if (0..=0xFFFFFF).contains(&color) {
                      settings.join_alert_color = color;
                    }
                  }
                }
              }
              1 => settings.join_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
              2 => settings.join_alert_footer = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
              3 => settings.join_alert_img = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
              _ => {}
            }
          }
        }
      }

      // Update settings in database
      db.players.update_prefs(user_id, &settings).await?;

      // Build preview embed
      let preview_embed = build_join_alert_embed(ctx, user_id, None, &settings, "Journeyman", None).await;

      // Send ephemeral preview as interaction response (dismissible)
      let response = CIR::Message(CIRM::new().content("**Preview of your join announcement:**").embed(preview_embed).ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;

      // Update the original settings menu
      update_settings_menu_from_modal(ctx, interaction, db).await?;
    }
    "settings_modal_leave_alert" => {
      // Get all input values from the modal
      let mut settings = db.players.get_prefs(user_id).await?;

      // Extract and validate values from modal components
      for (idx, action_row) in interaction.data.components.iter().enumerate() {
        if let Some(ARC::InputText(input)) = action_row.components.first() {
          if let Some(value) = &input.value {
            let trimmed = value.trim();

            // Validate text fields for allowed characters (skip color and URL fields)
            if (idx == 1 || idx == 2) && !trimmed.is_empty() && !is_valid_user_text(trimmed) {
              let field_name = if idx == 1 { "Description" } else { "Footer text" };
              let response = CIR::Message(
                CIRM::new().content(format!("**Error:** {} contains invalid characters. Only ASCII printable and extended characters are allowed.", field_name)).ephemeral(true),
              );
              interaction.create_response(&ctx.http, response).await?;
              return Ok(());
            }

            match idx {
              0 => {
                // Color field
                if !trimmed.is_empty() {
                  let hex_str = trimmed.trim_start_matches('#');
                  if let Ok(color) = u32::from_str_radix(hex_str, 16) {
                    if (0..=0xFFFFFF).contains(&color) {
                      settings.join_alert_color = color;
                    }
                  }
                }
              }
              1 => settings.leave_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
              2 => settings.leave_alert_footer = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
              3 => settings.leave_alert_img = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
              _ => {}
            }
          }
        }
      }

      // Update settings in database
      db.players.update_prefs(user_id, &settings).await?;

      // Build preview embed
      let preview_embed = build_leave_alert_embed(ctx, user_id, None, &settings, None).await;

      // Send ephemeral preview as interaction response (dismissible)
      let response = CIR::Message(CIRM::new().content("**Preview of your leave announcement:**").embed(preview_embed).ephemeral(true));
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

/// Update settings and rebuild embed/buttons for response
async fn update_settings_and_respond<F, Fut>(interaction: &CI, ctx: &Context, db: &Database, user_id: UI, update_fn: F) -> Result<()>
where
  F: FnOnce() -> Fut,
  Fut: std::future::Future<Output = anyhow::Result<()>>,
{
  // Apply the settings update
  update_fn().await?;

  // Get updated settings and rebuild UI
  let settings = db.players.get_prefs(user_id).await?;
  let embed = build_settings_embed(&settings);
  let buttons = build_settings_buttons(&settings);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

/// Update the settings menu embed (for modal interactions)
async fn update_settings_menu_from_modal(ctx: &Context, interaction: &MI, db: &Arc<Database>) -> Result<()> {
  let user_id = interaction.user.id;
  let settings = db.players.get_prefs(user_id).await?;

  let embed = build_settings_embed(&settings);
  let buttons = build_settings_buttons(&settings);

  // Find the settings menu message in the DM channel and update it
  if let Ok(channel) = user_id.create_dm_channel(&ctx.http).await {
    // Get recent messages to find the settings menu
    if let Ok(messages) = channel.messages(&ctx.http, GM::new().limit(10)).await {
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

/// Create a standard update response with embed and components
fn create_update_response(embed: CE, components: Vec<CAR>) -> CIR {
  CIR::UpdateMessage(CIRM::new().embed(embed).components(components))
}

/// Parse channel ID from hex string
pub fn parse_cid(hex_str: &str) -> Result<CHID> {
  Ok(CHID::new(u64::from_str_radix(hex_str, 16)?))
}

/// Parse optional channel ID from hex string (returns None if "0")
pub fn parse_opt_cid(hex_str: &str) -> Result<Option<CHID>> {
  if hex_str == "0" {
    Ok(None)
  } else {
    Ok(Some(parse_cid(hex_str)?))
  }
}

/// Parse message ID from hex string
pub fn parse_mid(hex_str: &str) -> Result<u64> {
  Ok(u64::from_str_radix(hex_str, 16)?)
}

/// Create a standard settings embed with title and description
fn create_settings_embed(title: &str, description: &str, color: u32) -> CE {
  CE::new().title(title).description(description).color(color)
}