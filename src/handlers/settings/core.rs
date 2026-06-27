use crate::handlers::settings::alerts::{build_join_alert_embed, build_leave_alert_embed};
use crate::handlers::settings::user_prefs_system::{get_user_prefs_menu_system, get_user_prefs_navigation_info, UserPrefsPage};
use crate::handlers::settings::utils::{create_paragraph_input_with_value, create_short_input_opt, track_dm_activity};
use crate::Database;
use anyhow::Result;
use serenity::all::{
  ActionRowComponent as ARC, ButtonStyle as BS, ChannelId as CHID, ComponentInteraction as CI, ComponentInteractionDataKind as CIDK, Context, CreateActionRow as CAR,
  CreateButton as CB, CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateModal, EditMessage, GetMessages as GM,
  GuildId as GI, ModalInteraction as MI, RoleId, UserId as UI,
};
use std::sync::Arc;
use tracing::{debug, warn};

/// Handle settings button interactions in DMs
pub async fn handle_settings_button(ctx: &Context, interaction: &CI, db: &Arc<Database>) -> Result<()> {
  let user_id = interaction.user.id;
  let button_id = &interaction.data.custom_id;
  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  debug!("{} pressed {}", user_tag, button_id);

  // Update activity timestamp for DM cleanup tracking
  track_dm_activity(ctx, user_id).await;

  match button_id.as_str() {
    // Handle user prefs navigation
    button_id if get_user_prefs_navigation_info(button_id).is_some() => {
      let target_page = get_user_prefs_navigation_info(button_id).unwrap();
      debug!("Navigating to user prefs page: {:?} from button: {}", target_page, button_id);
      
      // Special handling for PingSettings page - needs guild context and database access
      if target_page == UserPrefsPage::PingSettings {
        debug!("Using custom builder for PingSettings page with guild_id: {:?}", interaction.guild_id);
        let response = build_ping_settings_response(user_id, interaction.guild_id, db).await?;
        interaction.create_response(&ctx.http, response).await?;
        debug!("Successfully sent PingSettings response");
        return Ok(());
      }
      
      debug!("Using standard menu system for page: {:?}", target_page);
      let system = get_user_prefs_menu_system();
      let settings = db.players.get_prefs(user_id).await?;

      if let Some(response) = system.build_response(target_page, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
      return Ok(());
    }
    "settings_toggle_dm" => {
      // Toggle DM alerts
      let _new_state = db.players.toggle_pm_hot_alert(user_id).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let settings = db.players.get_prefs(user_id).await?;
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::AlertSettings, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_queue_expiration" => {
      // Navigate to queue timeout selection page
      let settings = db.players.get_prefs(user_id).await?;
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::QueueTimeoutSettings, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    button_id if button_id.starts_with("settings_queue_expiration:") => {
      // Handle auto-leave time selection or cancel
      let time_str = button_id.split(':').nth(1).unwrap_or("30m");

      if time_str == "cancel" {
        // Go back to queue settings
        let settings = db.players.get_prefs(user_id).await?;
        let system = get_user_prefs_menu_system();

        if let Some(response) = system.build_response(UserPrefsPage::QueueSettings, &settings) {
          interaction.create_response(&ctx.http, response).await?;
        }
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

        // Go back to queue settings after selection
        let system = get_user_prefs_menu_system();

        if let Some(response) = system.build_response(UserPrefsPage::QueueSettings, &settings) {
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    "settings_vc_auto_leave" => {
      // Toggle VC disconnect preference
      let mut settings = db.players.get_prefs(user_id).await?;
      settings.vc_auto_leave = !settings.vc_auto_leave;
      db.players.update_prefs(user_id, &settings).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::QueueSettings, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_vc_leave_queue" => {
      // Toggle leave queue on VC disconnect preference
      let mut settings = db.players.get_prefs(user_id).await?;
      settings.vc_leave_queue = !settings.vc_leave_queue;
      db.players.update_prefs(user_id, &settings).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::QueueSettings, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_vc_auto_join" => {
      // Toggle VC auto-queue preference
      let mut settings = db.players.get_prefs(user_id).await?;
      settings.vc_auto_join = !settings.vc_auto_join;
      db.players.update_prefs(user_id, &settings).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::QueueSettings, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_ping_notifications" => {
      debug!("Ping notifications button pressed. Guild context: {:?}", interaction.guild_id);
      
      // Toggle ping notifications - only works in guild context
      if let Some(guild_id) = interaction.guild_id {
        debug!("Guild context found: {}", guild_id);
        
        // Get current ping notification preference for this server
        let current = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);
        debug!("Current ping state for guild {}: {:?}", guild_id, current);
        
        let new_value = match current {
          Some(true) => Some(false),
          Some(false) => Some(true),
          None => Some(true), // Default to enabled on first interaction
        };

        debug!("Setting new ping state for guild {} to: {:?}", guild_id, new_value);
        db.user_server_prefs.set_ping_notification_enabled(user_id, guild_id, new_value).await?;

        // Handle role assignment/removal based on new preference
        let ping_role_str = db.config.get_ping_role_id(guild_id).await.ok().flatten();
        if let Some(ref role_str) = ping_role_str {
          if let Ok(role_id) = role_str.parse::<u64>() {
            let role_id = RoleId::new(role_id);
            if let Ok(member) = guild_id.member(&ctx.http, user_id).await {
              if new_value == Some(true) {
                if !member.roles.contains(&role_id) {
                  let _ = member.add_role(&ctx.http, role_id).await;
                }
              } else if new_value == Some(false) {
                if member.roles.contains(&role_id) {
                  let _ = member.remove_role(&ctx.http, role_id).await;
                }
              }
            }
          }
        }
      } else {
        debug!("No guild context - ping notifications button is disabled in DM");
      }

      // Refresh the ping settings page with updated button state
      let response = build_ping_settings_response(user_id, interaction.guild_id, db).await?;
      interaction.create_response(&ctx.http, response).await?;
    }
    "user_prefs_ping_back" => {
      // Back button from ping settings menu
      let settings = db.players.get_prefs(user_id).await?;
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::Main, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_edit_alert" => {
      // Show modal for customizing join announcement embed
      let settings = db.players.get_prefs(user_id).await?;
      let modal = CreateModal::new("settings_modal_announcement", "Customize join announcement").components(vec![
        create_short_input_opt("HEX color", "join_alert_color", "e.g., 3447003 or FF5733", &format!("{:06X}", settings.join_alert_color)),
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
    "settings_ping_server_select" => {
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(guild_id_str) = values.first() {
          if let Ok(guild_id) = guild_id_str.parse::<u64>() {
            let guild_id = GI::new(guild_id);
            let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown server".to_string());

            // Get current ping notification preference for this server
            let ping_enabled = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);

            // Determine button state and label
            let (enabled, label) = match ping_enabled {
              Some(true) => (true, "Ping notifications enabled"),
              Some(false) => (false, "Ping notifications disabled"),
              None => (true, "Enable ping notifications"), // Default to enabled on first interaction
            };

            let toggle_button = CB::new(format!("settings_toggle_ping_notification_{}", guild_id.get()))
              .label(label)
              .style(if enabled { BS::Success } else { BS::Danger });

            let components = vec![
              CAR::Buttons(vec![toggle_button]),
              CAR::Buttons(vec![CB::new("settings_ping_back").label("Back").style(BS::Secondary)]),
            ];

            let embed = CE::new()
              .title(format!("Ping notifications - {}", guild_name))
              .description("Toggle whether you want to receive ping notifications when games are ready in this server.")
              .color(0x5865F2);

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
            interaction.create_response(&ctx.http, response).await?;
          }
        }
      }
    }
    button_id if button_id.starts_with("settings_toggle_ping_notification_") => {
      let guild_id_str = button_id.strip_prefix("settings_toggle_ping_notification_").unwrap();
      if let Ok(guild_id) = guild_id_str.parse::<u64>() {
        let guild_id = GI::new(guild_id);

        // Get current preference
        let current = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);

        // Toggle: None -> Some(true), Some(true) -> Some(false), Some(false) -> Some(true)
        let new_value = match current {
          None => Some(true),
          Some(true) => Some(false),
          Some(false) => Some(true),
        };

        db.user_server_prefs.set_ping_notification_enabled(user_id, guild_id, new_value).await?;

        // Handle role assignment/removal based on new preference
        let ping_role_str = db.config.get_ping_role_id(guild_id).await?;
        if let Some(ref role_str) = ping_role_str {
          if let Ok(role_id) = role_str.parse::<u64>() {
            let role_id = RoleId::new(role_id);
            let member = guild_id.member(&ctx.http, user_id).await;

            if let Ok(member) = member {
              if new_value == Some(true) {
                // Add role if enabling
                if !member.roles.contains(&role_id) {
                  let _ = member.add_role(&ctx.http, role_id).await;
                }
              } else if new_value == Some(false) {
                // Remove role if disabling
                if member.roles.contains(&role_id) {
                  let _ = member.remove_role(&ctx.http, role_id).await;
                }
              }
            }
          }
        }

        // Update the button to reflect new state
        let guild_name = ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown server".to_string());
        let (enabled, label) = match new_value {
          Some(true) => (true, "Ping notifications enabled"),
          Some(false) => (false, "Ping notifications disabled"),
          None => (true, "Enable ping notifications"),
        };

        let toggle_button = CB::new(format!("settings_toggle_ping_notification_{}", guild_id.get()))
          .label(label)
          .style(if enabled { BS::Success } else { BS::Danger });

        let components = vec![
          CAR::Buttons(vec![toggle_button]),
          CAR::Buttons(vec![CB::new("settings_ping_back").label("Back").style(BS::Secondary)]),
        ];

        let embed = CE::new()
          .title(format!("Ping notifications - {}", guild_name))
          .description("Toggle whether you want to receive ping notifications when games are ready in this server.")
          .color(0x5865F2);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "settings_ping_back" => {
      // Return to main settings menu
      let settings = db.players.get_prefs(user_id).await?;
      let system = get_user_prefs_menu_system();

      if let Some(response) = system.build_response(UserPrefsPage::Main, &settings) {
        interaction.create_response(&ctx.http, response).await?;
      }
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
              0
                // Color field
                if !trimmed.is_empty() => {
                  let hex_str = trimmed.trim_start_matches('#');
                  if let Ok(color) = u32::from_str_radix(hex_str, 16) {
                    if (0..=0xFFFFFF).contains(&color) {
                      settings.join_alert_color = color;
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
              0
                // Color field
                if !trimmed.is_empty() => {
                  let hex_str = trimmed.trim_start_matches('#');
                  if let Ok(color) = u32::from_str_radix(hex_str, 16) {
                    if (0..=0xFFFFFF).contains(&color) {
                      settings.join_alert_color = color;
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
  let system = get_user_prefs_menu_system();

  if let Some(response) = system.build_response(UserPrefsPage::Main, &settings) {
    interaction.create_response(&ctx.http, response).await?;
  }
  Ok(())
}

/// Update the settings menu embed (for modal interactions)
async fn update_settings_menu_from_modal(ctx: &Context, interaction: &MI, db: &Arc<Database>) -> Result<()> {
  let user_id = interaction.user.id;
  let settings = db.players.get_prefs(user_id).await?;
  let system = get_user_prefs_menu_system();

  // Find the settings menu message in the DM channel and update it
  if let Ok(channel) = user_id.create_dm_channel(&ctx.http).await {
    // Get recent messages to find the settings menu
    if let Ok(messages) = channel.messages(&ctx.http, GM::new().limit(10)).await {
      // Find the most recent message from the bot with the settings embed
      for msg in messages {
        if msg.author.id == ctx.cache.current_user().id && msg.embeds.iter().any(|e| e.title.as_deref() == Some("qBot preferences")) {
          // Update this message
          if let Some(embed) = system.build_embed(UserPrefsPage::Main, &settings) {
            let components = system.build_components(UserPrefsPage::Main, &settings).unwrap_or_default();
            let mut message = msg.clone();
            message.edit(&ctx.http, EditMessage::new().embed(embed).components(components)).await?;
          }
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

/// Build ping settings response with proper button state
pub async fn build_ping_settings_response(
  user_id: UI,
  guild_id: Option<GI>,
  db: &Arc<Database>,
) -> Result<CIR> {
  debug!("Building ping settings response for user {} with guild context: {:?}", user_id, guild_id);
  
  let system = get_user_prefs_menu_system();
  let settings = db.players.get_prefs(user_id).await?;
  
  // Build the base embed
  let embed = system.build_embed(UserPrefsPage::PingSettings, &settings)
    .unwrap_or_else(|| CE::new()
      .title("Ping settings")
      .description("Manage per-server ping notification preferences")
      .color(0x5865F2)
      .field("Note", "Ping settings are per-server and managed from the server dashboard. This menu is read-only in DM context.", false)
    );
  
  let mut components = Vec::new();
  
  // Add ping toggle button if we have guild context
  if let Some(guild_id) = guild_id {
    // Fetch current ping state
    let ping_enabled = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);
    debug!("Ping state for user {} in guild {}: {:?}", user_id, guild_id, ping_enabled);
    
    // Determine button style based on state
    let style = match ping_enabled {
      Some(true) => BS::Success,  // Green when enabled
      Some(false) | None => BS::Secondary,  // Gray when disabled or not set
    };
    
    let button = CB::new("settings_ping_notifications")
      .label("Ping notifications")
      .style(style);
    
    debug!("Adding enabled ping button with style: {:?}", style);
    components.push(CAR::Buttons(vec![button]));
  } else {
    // In DM context, show disabled button
    debug!("No guild context - adding disabled ping button");
    let button = CB::new("settings_ping_notifications")
      .label("Ping notifications")
      .style(BS::Secondary)
      .disabled(true);
    
    components.push(CAR::Buttons(vec![button]));
  }
  
  // Add back button
  components.push(CAR::Buttons(vec![
    CB::new("user_prefs_ping_back")
      .label("Back")
      .style(BS::Secondary)
  ]));
  
  debug!("Built ping settings response with {} component rows", components.len());
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(components)))
}
