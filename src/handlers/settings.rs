use crate::colours::RED;
use anyhow::{anyhow, Result};
use serenity::all::{
  ActionRowComponent as ARC, ButtonStyle as BS, ChannelId as CI, ComponentInteraction, ComponentInteractionDataKind as CIDK, Context, CreateActionRow as CAR, CreateButton as CB,
  CreateEmbed as CE, CreateEmbedFooter, CreateInputText as CIT, CreateInteractionResponse as CIR, CreateInteractionResponseFollowup as CIRF,
  CreateInteractionResponseMessage as CIRM, CreateModal, CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO, EditMessage, EditRole as ER,
  GetMessages as GM, GuildId as GI, InputTextStyle as ITS, MessageId as MI, ModalInteraction, PermissionOverwrite as PO, PermissionOverwriteType as POT, Permissions, RoleId,
  UserId as UI,
};
use serenity::all::{ChannelType, Color};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::guild_name;
use crate::handlers::settings_menu::{
  build_player_settings_menu, create_selection_menu, AsSettingsMenu, CategoryListDisplay, CategorySettingsDisplay, FormatListDisplay, PlayerSettingsDisplay, RankConfigDisplay,
  RankRoleConfigDisplay, ServerConfigDisplay, ServerSettingsDisplay, RANK_CONFIG_TOGGLES, SERVER_CONFIG_TOGGLES,
};
use crate::Database;

// Helper methods to reduce code duplication

/// Update settings and rebuild embed/buttons for response
async fn update_settings_and_respond<F, Fut>(interaction: &ComponentInteraction, ctx: &Context, db: &Database, user_id: UI, update_fn: F) -> Result<()>
where
  F: FnOnce() -> Fut,
  Fut: std::future::Future<Output = anyhow::Result<()>>,
{
  // Apply the settings update
  update_fn().await?;

  // Get updated settings and rebuild UI
  let settings = db.users.get_prefs(user_id).await?;
  let embed = build_settings_embed(&settings);
  let buttons = build_settings_buttons(&settings);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

/// Create a standard update response with embed and components
fn create_update_response(embed: CE, components: Vec<CAR>) -> CIR {
  CIR::UpdateMessage(CIRM::new().embed(embed).components(components))
}

/// Macro to fetch player data and create PlayerSettings struct
macro_rules! get_player_settings {
  ($db:expr, $ctx:expr, $target_uid:expr, $guild_id:expr, $target_user_id:expr) => {{
    let player = $db.users.check_user($target_uid, None).await?;
    let guild_elo = $db.elo.get($target_uid, $guild_id, $db).await?;
    let username = $ctx.http.get_user($target_uid).await.map(|u| u.name.clone()).unwrap_or_else(|_| $target_user_id.to_string());

    PlayerSettings {
      user_id: $target_uid,
      username,
      steam_id: player.steam_id,
      elo: guild_elo.elo,
      rank: guild_elo.rank.name.clone(),
      games: guild_elo.games,
      wins: guild_elo.wins,
    }
  }};
}

/// Macro to extract modal input text value
macro_rules! get_modal_input {
  ($interaction:expr, $index:expr) => {{
    $interaction.data.components.get($index).and_then(|row| row.components.first()).and_then(|c| if let ARC::InputText(input) = c { input.value.clone() } else { None }).unwrap_or_default()
  }};
  ($interaction:expr) => {{
    get_modal_input!($interaction, 0)
  }};
}

/// Helper function to create and send embed/button response
async fn send_embed_button_response(
  interaction: &ComponentInteraction,
  ctx: &Context,
  embed: CE,
  components: Vec<CAR>,
) -> Result<()> {
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

/// Helper function for modal interactions
async fn send_embed_button_response_modal(
  interaction: &ModalInteraction,
  ctx: &Context,
  embed: CE,
  components: Vec<CAR>,
) -> Result<()> {
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

/// Macro to refresh category settings and send response for component interactions
macro_rules! refresh_category_settings {
  ($interaction:expr, $ctx:expr, $category:expr) => {{
    let settings = CategorySettings::from_category($category);
    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);
    send_embed_button_response($interaction, $ctx, embed, buttons).await
  }};
}

/// Macro to refresh category settings and send response for modal interactions
macro_rules! refresh_category_settings_modal {
  ($interaction:expr, $ctx:expr, $category:expr) => {{
    let settings = CategorySettings::from_category($category);
    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);
    send_embed_button_response_modal($interaction, $ctx, embed, buttons).await
  }};
}

/// Parse channel ID from hex string
fn parse_cid(hex_str: &str) -> Result<CI> {
  Ok(CI::new(u64::from_str_radix(hex_str, 16)?))
}

/// Parse optional channel ID from hex string (returns None if "0")
fn parse_opt_cid(hex_str: &str) -> Result<Option<CI>> {
  if hex_str == "0" {
    Ok(None)
  } else {
    Ok(Some(parse_cid(hex_str)?))
  }
}

/// Parse message ID from hex string
fn parse_mid(hex_str: &str) -> Result<u64> {
  Ok(u64::from_str_radix(hex_str, 16)?)
}

/// Create a short text input field for modals
fn create_input_sh(label: &str, id: &str, placeholder: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).required(true))
}

/// Create a short text input field with value for modals
fn create_value_input_sh(label: &str, id: &str, placeholder: &str, value: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).value(value).required(true))
}

/// Create a short text input field with optional value for modals
fn create_short_input_opt(label: &str, id: &str, placeholder: &str, value: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).value(value).required(false))
}

/// Create a short text input field with constraints for modals
fn create_input_sh_cap(label: &str, id: &str, placeholder: &str, min_len: u16, max_len: u16) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).required(true).min_length(min_len).max_length(max_len))
}

/// Create a short text input field with value and constraints for modals
fn create_value_input_sh_cap(label: &str, id: &str, placeholder: &str, value: &str, min_len: u16, max_len: u16) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).value(value).required(true).min_length(min_len).max_length(max_len))
}

/// Create a paragraph text input field for modals
fn create_paragraph_input(label: &str, id: &str, placeholder: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Paragraph, label, id).placeholder(placeholder).required(false))
}

/// Create a paragraph text input field with value for modals
fn create_paragraph_input_with_value(label: &str, id: &str, placeholder: &str, value: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Paragraph, label, id).placeholder(placeholder).value(value).required(false))
}

/// Create a paragraph text input field with constraints for modals
fn create_paragraph_input_constrained(label: &str, id: &str, placeholder: &str, max_len: u16) -> CAR {
  CAR::InputText(CIT::new(ITS::Paragraph, label, id).placeholder(placeholder).required(false).max_length(max_len))
}

/// Helper function to send navigation response
async fn send_nav_response(interaction: &ComponentInteraction, ctx: &Context, response: Result<CIR>) -> Result<()> {
  interaction.create_response(&ctx.http, response?).await?;
  Ok(())
}

/// Helper function to send navigation response for modals
async fn send_nav_response_modal(interaction: &ModalInteraction, ctx: &Context, response: Result<CIR>) -> Result<()> {
  interaction.create_response(&ctx.http, response?).await?;
  Ok(())
}

/// Macro to send navigation response with specific nav function
macro_rules! send_nav {
  ($interaction:expr, $ctx:expr, $db:expr, $nav_fn:expr, $($arg:expr),*) => {{
    send_nav_response($interaction, $ctx, $nav_fn($ctx, $db, $($arg),*).await).await
  }};
}

/// Macro to send navigation response for modals
macro_rules! send_nav_modal {
  ($interaction:expr, $ctx:expr, $db:expr, $nav_fn:expr, $($arg:expr),*) => {{
    send_nav_response_modal($interaction, $ctx, $nav_fn($ctx, $db, $($arg),*).await).await
  }};
}

/// Helper function to get role name with fallback to role ID
async fn get_role_name_with_fallback(ctx: &Context, guild_id: GI, role_id: RoleId) -> String {
  guild_id.roles(&ctx.http)
    .await
    .ok()
    .and_then(|roles| roles.get(&role_id).map(|role| role.name.clone()))
    .unwrap_or_else(|| role_id.get().to_string())
}

/// Track DM activity for cleanup
async fn track_dm_activity(ctx: &Context, user_id: UI) {
  if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
    dm_tracker.update_activity(user_id).await;
  }
}

/// Create a standard settings embed with title and description
fn create_settings_embed(title: &str, description: &str, color: u32) -> CE {
  CE::new().title(title).description(description).color(color)
}

/// Helper function for sending error responses in component interactions
async fn send_component_error_response(interaction: &ComponentInteraction, ctx: &Context, message: &str) {
  let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
  if let Err(e) = interaction.create_response(&ctx.http, response).await {
    error!("Failed to send error response: {e}");
  }
}

/// Helper function for sending error responses in modal interactions
async fn send_modal_error_response(interaction: &ModalInteraction, ctx: &Context, message: &str) {
  let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
  if let Err(e) = interaction.create_response(&ctx.http, response).await {
    error!("Failed to send error response: {e}");
  }
}

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
  "Rawr x3 nuzzles how are you pounces on you you're so warm",
];

/// Messages to replace footer spam with
const FOOTER_SPAM_REPLACEMENT_MESSAGES: &[&str] = &["Mmmm, feet :3", "Go team!", "PUG PUG PUG!", "GG!", "qBot is best bot"];

const SANITIZE_ALERTS_ENABLED: bool = false;
const MAX_ALERT_NEWLINES: usize = 4;
const MAX_ALERT_CHARS: usize = 180;

/// Check if text exceeds alert message limits (max 4 newlines, 180 chars)
fn exceeds_alert_limits(text: &str) -> bool {
  text.matches('\n').count() > MAX_ALERT_NEWLINES || text.chars().count() > MAX_ALERT_CHARS
}

/// Process text and replace with a random message from `replacements` if limits exceeded
fn sanitize_text(text: &str, replacements: &[&str]) -> String {
  if SANITIZE_ALERTS_ENABLED && exceeds_alert_limits(text) {
    use rand::RngExt;
    let mut rng = rand::rng();
    let idx = rng.random_range(0..replacements.len());
    return replacements[idx].to_string();
  }
  text.to_string()
}

/// Handle settings button interactions in DMs
pub async fn handle_settings_button(ctx: &Context, interaction: &ComponentInteraction, db: &Arc<Database>) -> Result<()> {
  let user_id = interaction.user.id;
  let button_id = &interaction.data.custom_id;
  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  debug!("{} pressed {}", user_tag, button_id);

  // Update activity timestamp for DM cleanup tracking
  track_dm_activity(ctx, user_id).await;

  match button_id.as_str() {
    "settings_toggle_dm" => {
      // Toggle DM alerts
      let _new_state = db.users.toggle_pm_hot_alert(user_id).await?;

      // Acknowledge and update the settings menu directly (no popup)
      let settings = db.users.get_prefs(user_id).await?;
      let embed = build_settings_embed(&settings);
      let buttons = build_settings_buttons(&settings);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
      interaction.create_response(&ctx.http, response).await?;
    }
    "settings_timeout" => {
      // Show time selection buttons inline (replace current message temporarily)
      let settings = db.users.get_prefs(user_id).await?;
      let current_minutes = settings.timeout;

      let time_buttons = vec![
        CB::new("settings_timeout:30m").label("30 min").style(if current_minutes == 30 { BS::Success } else { BS::Secondary }),
        CB::new("settings_timeout:1h").label("1 hour").style(if current_minutes == 60 { BS::Success } else { BS::Secondary }),
        CB::new("settings_timeout:2h").label("2 hours").style(if current_minutes == 120 { BS::Success } else { BS::Secondary }),
        CB::new("settings_timeout:3h").label("3 hours").style(if current_minutes == 180 { BS::Success } else { BS::Secondary }),
        CB::new("settings_timeout:4h").label("4 hours").style(if current_minutes == 240 { BS::Success } else { BS::Secondary }),
      ];

      let cancel_button = vec![CB::new("settings_timeout:cancel").label("Cancel").style(BS::Danger)];

      let embed = CE::new().title("Set timeout length").description("Choose how long before you're automatically removed from the queue:").color(settings.join_alert_color as u32);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(time_buttons), CAR::Buttons(cancel_button)]));

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
        let mut settings = db.users.get_prefs(user_id).await?;
        settings.timeout = minutes;
        db.users.update_settings(user_id, &settings).await?;

        // Update the settings menu directly (no confirmation popup)
        let embed = build_settings_embed(&settings);
        let buttons = build_settings_buttons(&settings);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
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

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
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

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
      interaction.create_response(&ctx.http, response).await?;
    }
    "settings_edit_alert" => {
      // Show modal for customizing join announcement embed
      let settings = db.users.get_prefs(user_id).await?;
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
      let settings = db.users.get_prefs(user_id).await?;
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
pub async fn handle_settings_modal(ctx: &Context, interaction: &ModalInteraction, db: &Arc<Database>) -> Result<()> {
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
      let mut settings = db.users.get_prefs(user_id).await?;

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
      db.users.update_settings(user_id, &settings).await?;

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
      let mut settings = db.users.get_prefs(user_id).await?;

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
      db.users.update_settings(user_id, &settings).await?;

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

/// Update the settings menu embed (for modal interactions)
async fn update_settings_menu_from_modal(ctx: &Context, interaction: &ModalInteraction, db: &Arc<Database>) -> Result<()> {
  let user_id = interaction.user.id;
  let settings = db.users.get_prefs(user_id).await?;

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

/// Build settings embed
pub fn build_settings_embed(settings: &crate::db::repo::UserSettings) -> CE {
  use AsSettingsMenu;
  settings.as_settings_menu().build_embed()
}

/// Build settings buttons
pub fn build_settings_buttons(settings: &crate::db::repo::UserSettings) -> Vec<CAR> {
  use AsSettingsMenu;
  settings.as_settings_menu().build_components()
}

/// Build a join announcement embed (used for both actual announcements and previews)
pub async fn build_join_alert_embed(ctx: &Context, user_id: UI, guild_id: Option<GI>, settings: &crate::db::repo::UserSettings, rank_name: &str, sg_name: Option<&str>) -> CE {
  // Get display name - try member nickname first, then user name, then user ID
  let display_name = if let Some(gid) = guild_id {
    // With guild context - try to get member for nickname
    let member = gid.member(&ctx.http, user_id).await.ok();
    if let Some(m) = member {
      m.display_name().to_string()
    } else {
      // Fallback to fetching user directly
      ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
    }
  } else {
    // For preview without guild context, fetch from HTTP API
    ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
  };

  // Build description with template support
  // If custom description is set (even if empty), use it; otherwise use default
  let description = match &settings.join_alert_desc {
    Some(custom_desc) if !custom_desc.trim().is_empty() => {
      // Sanitize newline spam only for actual announcements (not previews)
      let text_to_use = if guild_id.is_some() { sanitize_text(custom_desc, SPAM_REPLACEMENT_MESSAGES) } else { custom_desc.to_string() };

      // Replace template variables
      Some(text_to_use.replace("{user}", &format!("<@{}>", user_id)).replace("{rank}", rank_name).replace("{name}", &display_name))
    }
    Some(_) => None, // Empty string means no description
    None => None,
  };

  // Create embed with title showing nickname + "joined the queue"
  let mut embed = CE::new().title(match sg_name {
      Some(name) => format!("{display_name} joined the {name} queue"),
      None => format!("{display_name} joined the queue"),
    }).color(settings.join_alert_color as u32);

  // Only add description if there is one
  if let Some(desc) = description {
    embed = embed.description(desc);
  }

  // Add custom footer
  if let Some(footer_text) = &settings.join_alert_footer {
    // Sanitize footer spam only for actual announcements (not previews)
    let footer_to_use = if guild_id.is_some() { sanitize_text(footer_text, FOOTER_SPAM_REPLACEMENT_MESSAGES) } else { footer_text.to_string() };

    let mut footer = CreateEmbedFooter::new(footer_to_use);
    if let Some(footer_icon) = &settings.join_alert_footer_img {
      footer = footer.icon_url(footer_icon);
    }
    embed = embed.footer(footer);
  }

  // Add thumbnail
  if let Some(thumbnail) = &settings.join_alert_img {
    embed = embed.thumbnail(thumbnail);
  }

  embed
}

/// Build a leave announcement embed (used for both actual announcements and previews)
pub async fn build_leave_alert_embed(ctx: &Context, user_id: UI, guild_id: Option<GI>, settings: &crate::db::repo::UserSettings, sg_name: Option<&str>) -> CE {
  // Get display name - try member nickname first, then user name, then user ID
  let display_name = if let Some(gid) = guild_id {
    // With guild context - try to get member for nickname
    let member = gid.member(&ctx.http, user_id).await.ok();
    if let Some(m) = member {
      m.display_name().to_string()
    } else {
      // Fallback to fetching user directly
      ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
    }
  } else {
    // For preview without guild context, fetch from HTTP API
    ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
  };

  // Build description with template support
  // If custom description is set (even if empty), use it; otherwise use default
  let description = match &settings.leave_alert_desc {
    Some(custom_desc) if !custom_desc.trim().is_empty() => {
      // Sanitize newline spam only for actual announcements (not previews)
      let text_to_use = if guild_id.is_some() { sanitize_text(custom_desc, SPAM_REPLACEMENT_MESSAGES) } else { custom_desc.to_string() };

      // Replace template variables (no rank for leave)
      Some(text_to_use.replace("{user}", &format!("<@{}>", user_id)).replace("{name}", &display_name))
    }
    Some(_) => None, // Empty string means no description
    None => None,
  };

  // Create embed with title showing nickname + "left the queue"
  let mut embed = CE::new().title(match sg_name {
      Some(name) => format!("{display_name} left the {name} queue"),
      None => format!("{display_name} left the queue"),
    }).color(settings.join_alert_color as u32);

  // Only add description if there is one
  if let Some(desc) = description {
    embed = embed.description(desc);
  }

  // Add custom footer if provided
  if let Some(footer_text) = &settings.leave_alert_footer {
    // Sanitize footer spam only for actual announcements (not previews)
    let footer_to_use = if guild_id.is_some() { sanitize_text(footer_text, FOOTER_SPAM_REPLACEMENT_MESSAGES) } else { footer_text.to_string() };

    let mut footer = CreateEmbedFooter::new(footer_to_use);
    if let Some(footer_icon) = &settings.leave_alert_footer_img {
      footer = footer.icon_url(footer_icon);
    }
    embed = embed.footer(footer);
  }

  // Add custom thumbnail if provided
  if let Some(thumbnail) = &settings.leave_alert_img {
    embed = embed.thumbnail(thumbnail);
  }

  embed
}

/// Server settings structure for display
pub struct ServerSettings {
  pub runner_role: Option<String>,
  pub admin_role: Option<String>,
  pub toggle_states: Vec<bool>,
  pub balance_method: String,
  pub post_game_timeout: u16,
}

/// Build server settings embed
pub fn build_server_settings_embed(settings: &ServerSettings, guild_name: &str) -> CE {
  use {AsSettingsMenu, ServerSettingsDisplay};
  let display = ServerSettingsDisplay {
    guild_name: guild_name.to_string(),
    runner_role: settings.runner_role.clone(),
    admin_role: settings.admin_role.clone(),
    toggle_states: settings.toggle_states.clone(),
    balance_method: settings.balance_method.clone(),
    post_game_timeout: settings.post_game_timeout,
  };
  display.as_settings_menu().build_embed()
}

/// Build server settings buttons and select menus
pub fn build_server_settings_buttons(settings: &ServerSettings, guild_name: &str) -> Vec<CAR> {
  use {AsSettingsMenu, ServerSettingsDisplay};
  let display = ServerSettingsDisplay {
    guild_name: guild_name.to_string(),
    runner_role: settings.runner_role.clone(),
    admin_role: settings.admin_role.clone(),
    toggle_states: settings.toggle_states.clone(),
    balance_method: settings.balance_method.clone(),
    post_game_timeout: settings.post_game_timeout,
  };
  display.as_settings_menu().build_components()
}

/// Build a CIR navigating back to the main server settings page
async fn nav_server_settings(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let settings = get_server_settings(db, guild_id).await?;
  let guild_name = guild_name(ctx, guild_id);
  let embed = build_server_settings_embed(&settings, &guild_name);
  let buttons = build_server_settings_buttons(&settings, &guild_name);
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR navigating back to the server configuration page
async fn nav_role_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_server_settings(db, guild_id).await?;
  let display = ServerConfigDisplay {
    guild_name,
    runner_role: settings.runner_role,
    admin_role: settings.admin_role,
    toggle_states: settings.toggle_states,
    balance_method: settings.balance_method,
    post_game_timeout: settings.post_game_timeout,
  };
  Ok(CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components())))
}

/// Build a CIR navigating back to the rank configuration page
async fn nav_rank_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let rank_roles = get_all_rank_roles(db, guild_id).await?;
  let (toggle_states, default_rank_role) = get_rank_settings(db, guild_id).await?;
  let display = RankConfigDisplay { guild_name, rank_roles, toggle_states, default_rank_role };
  Ok(CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components())))
}

/// Build a CIR navigating back to the category list page
async fn nav_category_list(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let categories = db.categories.get_categories_for_guild(guild_id).await?;
  let display = CategoryListDisplay { guild_name, categories };
  Ok(CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components())))
}

/// Handle server settings button interactions
pub async fn handle_server_settings_button(
  ctx: &Context,
  interaction: &ComponentInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let button_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("{} pressed {}", user_tag, button_id);

  match button_id.as_str() {
    // Generic handler for server-level config toggles (ELO-Rank linked, etc.)
    _ if SERVER_CONFIG_TOGGLES.iter().any(|t| t.button_id == button_id) => {
      let toggle = SERVER_CONFIG_TOGGLES.iter().find(|t| t.button_id == button_id).unwrap();

      let current = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
      db.config.set_bool(guild_id, toggle.column, !current).await?;
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    // Generic handler for all rank config toggles (dynamic ELO, ELO-Rank linked, etc.)
    _ if RANK_CONFIG_TOGGLES.iter().any(|t| t.button_id == button_id) => {
      let toggle = RANK_CONFIG_TOGGLES.iter().find(|t| t.button_id == button_id).unwrap();

      let current = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
      db.config.set_bool(guild_id, toggle.column, !current).await?;
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    "server_settings_roles" => {
      send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
    }
    "server_settings_roles_back" => {
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    "server_settings_runner_role" => {
      if let CIDK::RoleSelect { values } = &interaction.data.kind {
        if let Some(role_id) = values.first() {
          db.config.set_runner_role_id(guild_id, *role_id).await?;
        }
        send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
      }
    }
    "server_settings_admin_role" => {
      if let CIDK::RoleSelect { values } = &interaction.data.kind {
        if let Some(role_id) = values.first() {
          db.config.set_admin_role_id(guild_id, *role_id).await?;
        }
        send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
      }
    }
    "server_settings_ranks" => {
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    "server_settings_ranks_back" => {
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    "server_settings_rank_select" => {
      // Handle rank selection from dropdown (value is role ID)
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(role_id_str) = values.first() {
          let guild_name = guild_name(ctx, guild_id);

          if let Ok(role_id) = role_id_str.parse::<u64>() {
            let rid = RoleId::new(role_id);
            if let Ok(guild_rank) = db.ranks.rank_from_role_id(guild_id, rid).await {
              let display =
                RankRoleConfigDisplay { guild_name, rank_name: guild_rank.name.clone(), rank_key: guild_rank.name.clone(), elo: guild_rank.elo, role_id: guild_rank.role_id };

              let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
              interaction.create_response(&ctx.http, response).await?;
            }
          }
        }
      }
    }
    "server_settings_rank_link_role" => {
      // Handle role selection for linking existing rank
      let selected_role_id = if let CIDK::RoleSelect { values } = &interaction.data.kind {
        values.first().copied().ok_or_else(|| anyhow!("No role selected"))?
      } else {
        return Err(anyhow!("No role selected"));
      };

      // Get the role name to use as default
      let role_name = get_role_name_with_fallback(ctx, guild_id, selected_role_id).await;

      // Show modal to specify rank name and ELO for the selected role

      let modal = CreateModal::new(format!("server_settings_rank_modal_link_{}", selected_role_id.get()), "Link existing rank").components(vec![
        create_value_input_sh("Rank name", "name", "e.g., Bronze, Gold, Platinum", &role_name),
        create_input_sh_cap("ELO Threshold", "elo", "Minimum ELO for this rank", 1, 3),
      ]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_rank_back" => {
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    _ if button_id.starts_with("server_settings_rank_edit_") => {
      // Handle rank name/ELO edit button
      let rank_name = button_id.strip_prefix("server_settings_rank_edit_").unwrap();
      if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
        let modal = CreateModal::new(format!("server_settings_rank_modal_{}", rank_name), format!("Edit {} rank", guild_rank.name)).components(vec![
          create_value_input_sh("Rank name", "name", "e.g., Beginner, Expert, Champion", &guild_rank.name),
          create_value_input_sh_cap("ELO Threshold", "elo", "Minimum ELO for this rank", &guild_rank.elo.to_string(), 1, 3),
        ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "server_settings_rank_add" => {
      // Show modal to add a new rank

      let modal = CreateModal::new("server_settings_rank_modal_add", "Add new rank").components(vec![
        create_input_sh("Rank name", "name", "e.g., Champion, Legend, Elite"),
        create_input_sh_cap("ELO Threshold", "elo", "Minimum ELO for this rank", 1, 3),
      ]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_rank_link" => {
      // Show role selector for linking existing rank
      let response = CIR::UpdateMessage(
        CIRM::new().embed(
            CE::new().title("Link ranks").description("Select a Discord role to link to a new rank. The role will be used to assign this rank to players automatically.").color(0x5865F2),
          ).components(vec![
            CAR::SelectMenu(
              CSM::new("server_settings_rank_link_role", CSMK::Role { default_roles: None }).placeholder("Select a Discord role to link").min_values(1).max_values(1),
            ),
            CAR::Buttons(vec![CB::new("server_settings_ranks_back").label("Back to ranks").style(BS::Secondary)]),
          ]),
      );
      interaction.create_response(&ctx.http, response).await?;
    }
    _ if button_id.starts_with("server_settings_rank_delete_") => {
      let rank_name = button_id.strip_prefix("server_settings_rank_delete_").unwrap();
      db.ranks.delete_rank(guild_id, rank_name).await?;
      let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
      info!("{} deleted rank {}", user_tag, rank_name);
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    _ if button_id.starts_with("server_settings_rank_role_") => {
      // Handle role selector for linking Discord role to rank
      let rank_name = button_id.strip_prefix("server_settings_rank_role_").unwrap();

      // Get selected role from interaction
      let selected_role_id = if let CIDK::RoleSelect { values } = &interaction.data.kind {
        values.first().copied().ok_or_else(|| anyhow!("No role selected"))?
      } else {
        return Err(anyhow!("No role selected"));
      };

      // Update rank's linked role in DB
      db.ranks.update_rank_role(guild_id, rank_name, selected_role_id).await?;

      let role_display = format!("<@&{}>", selected_role_id.get());
      let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
      info!("{} linked rank {} to role {}", user_tag, rank_name, role_display);

      // Refresh the rank config display
      let guild_name = guild_name(ctx, guild_id);
      if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
        let display = RankRoleConfigDisplay { guild_name, rank_name: guild_rank.name.clone(), rank_key: rank_name.to_string(), elo: guild_rank.elo, role_id: guild_rank.role_id };

        let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "server_settings_default_rank_select" => {
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(role_id_str) = values.first() {
          // Parse role ID from string
          if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
            let role_id = RoleId::new(role_id_u64);

            // Set default rank as role ID
            db.config.set_default_rank_role_id(guild_id, role_id).await?;

            send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
          }
        }
      }
    }
    "server_settings_categories" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    "server_settings_categories_back" => {
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    "server_settings_edit_post_game_timeout" => {
      // Show modal to edit post-game timeout

      let current_timeout = db.config.get_post_game_timeout(guild_id).await.unwrap_or(120);

      let modal = CreateModal::new("server_settings_post_game_timeout_modal", "Edit Post-Game Timeout").components(vec![
        create_value_input_sh_cap("Post-game timeout (seconds)", "post_game_timeout_input", "Enter timeout in seconds (30-300)", &current_timeout.to_string(), 1, 3),
      ]);

      interaction.create_response(&ctx.http, CIR::Modal(modal)).await?;
    }
    "server_settings_create_roles" => {
      // Create runner, admin, and rank roles
      let guild_name = guild_name(ctx, guild_id);

      // Create Runner role if not configured
      let runner_role = db.config.get_runner_role_id(guild_id).await?;
      if runner_role.is_none() {
        match guild_id.create_role(&ctx.http, ER::new().name("PUG Runner").colour(crate::RUNNER).permissions(Permissions::empty())).await {
          Ok(role) => {
            if let Err(e) = db.config.set_runner_role_id(guild_id, role.id).await {
              warn!("Failed to save runner_role config: {e}");
            }
            info!("[{}] Created PUG Runner role", guild_name);
          }
          Err(e) => {
            warn!("[{}] Failed to create PUG Runner role: {}", guild_name, e);
          }
        }
      }

      // Create Admin role if not configured
      let admin_role = db.config.get_admin_role_id(guild_id).await?;
      if admin_role.is_none() {
        match guild_id.create_role(&ctx.http, ER::new().name("PUG Admin").colour(crate::ADMIN).permissions(Permissions::empty())).await {
          Ok(role) => {
            if let Err(e) = db.config.set_admin_role_id(guild_id, role.id).await {
              warn!("Failed to save admin_role config: {e}");
            }
            info!("[{}] Created PUG Admin role", guild_name);
          }
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

      send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
    }
    "server_settings_create_category" => {
      // Show modal to collect category settings before creating channels

      let modal = CreateModal::new("server_settings_modal_create_category", "Create a new category").components(vec![
        create_input_sh("Category name", "category_name", "e.g., NA PUGs, EU Competitive"),
        create_input_sh("Channel prefix", "channel_prefix", "e.g., pug, na, eu"),
        create_value_input_sh("Category name", "discord_category", "e.g., PUG Queue", "PUG Queue"),
        create_value_input_sh_cap("Quota (players per game)", "quota", "e.g., 12", &crate::DEFAULT_QUOTA.to_string(), 1, 3),
        create_paragraph_input_with_value("Bot-only dashboard (yes/no)", "bot_only_dashboard", "Set to 'yes' to restrict dashboard channel to bot-only messages", "yes"),
      ]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_link_category" => {
      // Show category selection dropdown to link existing category

      let guild_name = guild_name(ctx, guild_id);

      // Get all categories in the guild - extract data before any awaits
      let mut categories: Vec<(CI, String)> = {
        let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;
        guild.channels.iter().filter_map(|(id, channel)| if channel.kind == ChannelType::Category { Some((*id, channel.name.clone())) } else { None }).collect()
      };

      // Sort by name
      categories.sort_by(|a, b| a.1.cmp(&b.1));

      if categories.is_empty() {
        let response = CIR::Message(CIRM::new().content("No categories found in this server. Please create a category with the required channels first.").ephemeral(true));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      // Create dropdown with categories
      let options: Vec<CSMO> = categories.iter().take(25) // Discord limit
        .map(|(id, name)| CSMO::new(name.clone(), id.get().to_string()).description(format!("Category ID: {}", id.get())))
        .collect();

      let select_menu = CSM::new("server_settings_link_category_select", CSMK::String { options }).placeholder("Select a category to link");

      let embed = CE::new().title(format!("{} - Link Existing Category", guild_name)).description(
          "**Select a category to link as a category**\n\n\
                    The category must contain these channels:\n\
                    • `dashboard` - Text channel for the dashboard\n\
                    • `queue` - Text channel for queue chat\n\
                    • `queue-vc` - Voice channel for the queue\n\
                    • `red` - Voice channel for red team\n\
                    • `blue` - Voice channel for blue team\n\n\
                    Channel names must match exactly (case-insensitive).",
        ).color(0x5865F2);

      let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_link_cancel").label("Cancel").style(BS::Secondary)])];

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_link_category_select" => {
      // Handle category selection - verify channels and link
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(category_id_str) = values.first() {
          if let Ok(category_id_u64) = category_id_str.parse::<u64>() {
            let category_id = CI::new(category_id_u64);
            let guild_name = guild_name(ctx, guild_id);

            // Find channels in this category - extract data before any awaits
            let (dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel) = {
              let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;

              let mut dashboard_channel = None;
              let mut queue_channel = None;
              let mut queue_vc_channel = None;
              let mut red_channel = None;
              let mut blue_channel = None;

              for (channel_id, channel) in &guild.channels {
                if channel.parent_id == Some(category_id) {
                  let name_lower = channel.name.to_lowercase();

                  // Dashboard channel (text)
                  if dashboard_channel.is_none() && channel.kind == ChannelType::Text && (name_lower == "dashboard" || name_lower == "dash") {
                    dashboard_channel = Some(*channel_id);
                  }

                  // Queue chat channel (text) - try multiple variations
                  if queue_channel.is_none()
                    && channel.kind == ChannelType::Text
                    && (name_lower == "queue" || name_lower == "pug-chat" || name_lower == "chat" || name_lower == "queue-chat" || name_lower == "pug")
                  {
                    queue_channel = Some(*channel_id);
                  }

                  // Queue voice channel - try multiple variations
                  if queue_vc_channel.is_none() && channel.kind == ChannelType::Voice && name_lower == "queue-vc"
                    || name_lower == "queue"
                    || name_lower == "pug"
                    || name_lower == "queue vc"
                    || name_lower == "waiting"
                  {
                    queue_vc_channel = Some(*channel_id);
                  }

                  // Red team voice channel
                  if red_channel.is_none() && channel.kind == ChannelType::Voice {
                    if name_lower == "red" || name_lower == "red team" || name_lower == "team red" {
                      red_channel = Some(*channel_id);
                    }
                  }

                  // Blue team voice channel
                  if blue_channel.is_none() && channel.kind == ChannelType::Voice {
                    if name_lower == "blue" || name_lower == "blue team" || name_lower == "team blue" || name_lower == "blu" {
                      blue_channel = Some(*channel_id);
                    }
                  }
                }
              }

              (dashboard_channel, queue_channel, queue_vc_channel, red_channel, blue_channel)
            };

            // Check if any channels are missing
            let has_all_channels = dashboard_channel.is_some() && queue_channel.is_some() && queue_vc_channel.is_some() && red_channel.is_some() && blue_channel.is_some();

            if !has_all_channels {
              // Start manual channel selection flow

              // Get all text and voice channels in the guild
              let (text_channels, voice_channels) = {
                let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;
                let mut text_chans = Vec::new();
                let mut voice_chans = Vec::new();

                for (channel_id, channel) in &guild.channels {
                  if channel.parent_id == Some(category_id) {
                    match channel.kind {
                      ChannelType::Text => {
                        text_chans.push((*channel_id, channel.name.clone()));
                      }
                      ChannelType::Voice => {
                        voice_chans.push((*channel_id, channel.name.clone()));
                      }
                      _ => {}
                    }
                  }
                }
                (text_chans, voice_chans)
              };

              // Determine which channel to select first
              let (next_channel_type, next_channel_name, available_channels) = if dashboard_channel.is_none() {
                ("dashboard", "Dashboard (text)", text_channels)
              } else if queue_channel.is_none() {
                ("queue", "Queue chat (text)", text_channels)
              } else if queue_vc_channel.is_none() {
                ("queue_vc", "Queue voice channel", voice_channels)
              } else if red_channel.is_none() {
                ("red", "Red team voice channel", voice_channels)
              } else {
                ("blue", "Blue team voice channel", voice_channels)
              };

              if available_channels.is_empty() {
                let response = CIR::Message(
                  CIRM::new().content(format!(
                      "No suitable channels found in this category.\n\n\
                                            Please create the required channels first."
                    )).ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
              }

              // Create channel selection dropdown
              let options: Vec<CSMO> = available_channels.iter().map(|(id, name)| CSMO::new(name.clone(), id.get().to_string())).collect();

              // Encode state compactly: use hex for IDs and single char for type
              // Format: cat_d_q_qv_r_b_t where each is hex (or 0)
              let type_char = match next_channel_type {
                "dashboard" => "d",
                "queue" => "q",
                "queue_vc" => "v",
                "red" => "r",
                "blue" => "b",
                _ => "x",
              };
              let state = format!(
                "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}_{}",
                category_id.get(),
                dashboard_channel.map(|c| c.get()).unwrap_or(0),
                queue_channel.map(|c| c.get()).unwrap_or(0),
                queue_vc_channel.map(|c| c.get()).unwrap_or(0),
                red_channel.map(|c| c.get()).unwrap_or(0),
                blue_channel.map(|c| c.get()).unwrap_or(0),
                type_char
              );

              let select_menu = CSM::new(format!("link_ch_{}", state), CSMK::String { options }).placeholder(format!("Select {}", next_channel_name));

              // Build status message
              let mut status = String::from("**Channel Linking Progress:**\n\n");
              status.push_str(&format!("Dashboard: {}\n", if let Some(id) = dashboard_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Chat: {}\n", if let Some(id) = queue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Voice: {}\n", if let Some(id) = queue_vc_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Red Team: {}\n", if let Some(id) = red_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Blue Team: {}\n", if let Some(id) = blue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));

              let embed = CE::new().title(format!("{} - Link Channels", guild_name)).description(format!("{}\n\n**Next:** Select the {} channel from the dropdown below.", status, next_channel_name)).color(0x5865F2);

              let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_link_cancel").label("Cancel").style(BS::Secondary)])];

              let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
              interaction.create_response(&ctx.http, response).await?;
              return Ok(());
            }

            // All channels found - search for existing dashboard messages
            let dashboard_channel = dashboard_channel.unwrap();
            let queue_channel = queue_channel.unwrap();
            let queue_vc_channel = queue_vc_channel.unwrap();
            let red_channel = red_channel.unwrap();
            let blue_channel = blue_channel.unwrap();

            // Check for existing categories using these channels
            let existing_categories = db.categories.get_categories_for_guild(guild_id).await?;
            let duplicate_category = existing_categories.iter().find(|g| {
              g.channels.dashboard == dashboard_channel
                || g.channels.queue_chat == queue_channel
                || g.channels.queue_vc == queue_vc_channel
                || g.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
            });

            // Search for bot messages in dashboard channel
            let bot_user_id = ctx.cache.current_user().id;
            let mut existing_dashboard_msgs = Vec::new();

            match dashboard_channel.messages(&ctx.http, GM::new().limit(50)).await {
              Ok(messages) => {
                for msg in messages {
                  if msg.author.id == bot_user_id && !msg.embeds.is_empty() {
                    // Check if it looks like a dashboard (has embed with "Queue" in title/description)
                    if let Some(embed) = msg.embeds.first() {
                      let title = embed.title.as_deref().unwrap_or("");
                      let desc = embed.description.as_deref().unwrap_or("");
                      if title.contains("Queue") || desc.contains("Queue") || desc.contains("Join") {
                        existing_dashboard_msgs.push((msg.id, msg.timestamp));
                      }
                    }
                  }
                }
              }
              Err(e) => {
                warn!("[{}] Failed to fetch messages from dashboard channel: {}", guild_name, e);
              }
            }

            // Sort by timestamp (newest first)
            existing_dashboard_msgs.sort_by(|a, b| b.1.cmp(&a.1));

            // Build prompt based on what we found
            let mut description = String::new();
            let mut buttons = Vec::new();

            if let Some(dup_category) = duplicate_category {
              description.push_str(&format!(
                "⚠️ **Duplicate Category Detected**\n\n\
                                Category {} is already using one or more of these channels:\n\
                                • Dashboard: <#{}>\n\
                                • Queue Chat: <#{}>\n\
                                • Queue Voice: <#{}>\n\
                                • Red Team: <#{}>\n\
                                • Blue Team: <#{}>\n\n",
                dup_category.display_name(),
                dup_category.channels.dashboard.get(),
                dup_category.channels.queue_chat.get(),
                dup_category.channels.queue_vc.get(),
                dup_category.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                dup_category.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0)
              ));

              if !existing_dashboard_msgs.is_empty() {
                description.push_str(&format!(
                  "Found {} existing dashboard message(s) in <#{}>.\n\n\
                                    **Options:**\n\
                                    • Remove duplicate category and link to existing dashboard\n\
                                    • Create new dashboard (will create duplicate)\n\
                                    • Cancel",
                  existing_dashboard_msgs.len(),
                  dashboard_channel.get()
                ));

                // Encode state: channels + existing message ID
                let state = format!(
                  "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}",
                  dashboard_channel.get(),
                  queue_channel.get(),
                  queue_vc_channel.get(),
                  red_channel.get(),
                  blue_channel.get(),
                  existing_dashboard_msgs[0].0.get()
                );

                buttons.push(CB::new(format!("link_existing_remove_dup_{}", state)).label("Remove duplicate & link existing").style(BS::Success));
              } else {
                description.push_str(
                  "No existing dashboard messages found.\n\n\
                                    **Options:**\n\
                                    • Remove duplicate category and create new dashboard\n\
                                    • Provide message ID manually\n\
                                    • Cancel",
                );

                let state = format!("{:x}_{:x}_{:x}_{:x}_{:x}", dashboard_channel.get(), queue_channel.get(), queue_vc_channel.get(), red_channel.get(), blue_channel.get());

                buttons.push(CB::new(format!("link_remove_dup_new_{}", state)).label("Remove duplicate & create new").style(BS::Success));
                buttons.push(CB::new(format!("link_manual_msg_{}", state)).label("Provide message ID").style(BS::Primary));
              }
            } else if !existing_dashboard_msgs.is_empty() {
              description.push_str(&format!(
                "**Found {} existing dashboard message(s)**\n\n\
                                Found bot messages in <#{}> that appear to be dashboards.\n\
                                Most recent: <https://discord.com/channels/{}/{}/{}>\n\n\
                                **Options:**\n\
                                • Link to existing dashboard (recommended)\n\
                                • Create new dashboard\n\
                                • Provide different message ID\n\
                                • Cancel",
                existing_dashboard_msgs.len(),
                dashboard_channel.get(),
                guild_id.get(),
                dashboard_channel.get(),
                existing_dashboard_msgs[0].0.get()
              ));

              let state = format!(
                "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}",
                dashboard_channel.get(),
                queue_channel.get(),
                queue_vc_channel.get(),
                red_channel.get(),
                blue_channel.get(),
                existing_dashboard_msgs[0].0.get()
              );

              buttons.push(CB::new(format!("link_use_existing_{}", state)).label("Link to existing dashboard").style(BS::Success));
              buttons.push(CB::new(format!("link_create_new_{}", state)).label("Create new dashboard").style(BS::Primary));
              buttons.push(CB::new(format!("link_manual_msg_{}", state)).label("Provide message ID").style(BS::Secondary));
            } else {
              description.push_str(&format!(
                "ℹ️ **No existing dashboard messages found**\n\n\
                                Searched recent messages in <#{}> but didn't find any existing dashboards.\n\n\
                                **Options:**\n\
                                • Create new dashboard\n\
                                • Provide message ID manually (if you know it exists)\n\
                                • Cancel",
                dashboard_channel.get()
              ));

              let state = format!("{:x}_{:x}_{:x}_{:x}_{:x}", dashboard_channel.get(), queue_channel.get(), queue_vc_channel.get(), red_channel.get(), blue_channel.get());

              buttons.push(CB::new(format!("link_create_new_{}", state)).label("Create new dashboard").style(BS::Success));
              buttons.push(CB::new(format!("link_manual_msg_{}", state)).label("Provide message ID").style(BS::Secondary));
            }

            buttons.push(CB::new("server_settings_link_cancel").label("Cancel").style(BS::Danger));

            let embed = CE::new().title(format!("{} - Link Category Options", guild_name)).description(description).color(0x5865F2);

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(buttons)]));
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
          }
        }
      }
    }
    "server_settings_link_cancel" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    _ if button_id.starts_with("link_use_existing_") => {
      // Link to existing dashboard message
      let state_str = button_id.strip_prefix("link_use_existing_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() != 6 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel =     parse_cid(parts[1])?;
      let queue_vc_channel =  parse_cid(parts[2])?;
      let dashboard_msg_id =  parse_mid(parts[5])?;

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      // Create category with existing message ID
      let category_config = crate::db::repo::category::CategoryConfig {
        category_id: category_id.get(),
        dashboard_channel_id: dashboard_channel.get(),
        chat_channel_id: queue_channel.get(),
        queue_vc_id: queue_vc_channel.get(),
        quota: crate::DEFAULT_QUOTA,
      };

      let guild_name = guild_name(ctx, guild_id);

      match db.categories.create_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
        Ok(db_category) => {
          info!("[{}] Category {} linked to existing dashboard {}", guild_name, db_category.category_id, dashboard_msg_id);

          // Add category to in-memory server
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_server(guild_id) {
            if let Err(e) = server.add_category(db_category.clone()) {
              error!("Failed to add category to server: {e}");
            }
          }
          drop(manager_lock);

          // Show success and return to category list
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

          let response = CIR::UpdateMessage(
            CIRM::new().content(format!("Successfully linked category to existing dashboard!")).embed(display.build_embed()).components(display.build_components()),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          warn!("[{}] Failed to save linked category: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_create_new_") => {
      // Create new dashboard message
      let state_str = button_id.strip_prefix("link_create_new_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() < 5 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel =     parse_cid(parts[1])?;
      let queue_vc_channel =  parse_cid(parts[2])?;
      let red_channel =       parse_cid(parts[3])?;
      let blue_channel =      parse_cid(parts[4])?;

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      use crate::models::{Category, Channels, TeamChannel};

      let mut temp_category = Category::new(
        guild_id,
        None,
        0,
        None,
        crate::DEFAULT_QUOTA,
        crate::DEFAULT_HOT_JOIN_TIMEOUT,
        MI::new(1),
        Channels {
          category: category_id,
          queue_chat: queue_channel,
          queue_vc: queue_vc_channel,
          teams: vec![TeamChannel { red_vc: red_channel, blu_vc: blue_channel }],
          dashboard: dashboard_channel,
        },
        vec![],
      );

      let guild_name = guild_name(ctx, guild_id);

      // Publish the dashboard to get the actual message ID
      match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
        Ok(_) => {
          let dashboard_msg_id = temp_category.dashboard_msg.get();
          info!("[{}] Dashboard message created with ID {} (linked category)", guild_name, dashboard_msg_id);

          // Create the category in the database
          let category_config = crate::db::repo::category::CategoryConfig {
            category_id: category_id.get(),
            dashboard_channel_id: dashboard_channel.get(),
            chat_channel_id: queue_channel.get(),
            queue_vc_id: queue_vc_channel.get(),
            quota: crate::DEFAULT_QUOTA,
          };
          match db.categories.create_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
            Ok(db_category) => {
              info!("[{}] Category {} linked and saved to database", guild_name, db_category.category_id);

              // Add category to in-memory server
              let mut manager_lock = manager.lock().await;
              if let Ok(server) = manager_lock.get_server(guild_id) {
                if let Err(e) = server.add_category(db_category.clone()) {
                  error!("Failed to add category to server: {e}");
                }
              }
              drop(manager_lock);

              // Show success message and return to category list
              let categories = db.categories.get_categories_for_guild(guild_id).await?;
              let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

              let response =
                CIR::UpdateMessage(CIRM::new().content(format!("Successfully linked category from category!")).embed(display.build_embed()).components(display.build_components()));
              interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
              // Database save failed - clean up dashboard message
              let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;

              warn!("[{}] Failed to save linked category to database: {}", guild_name, e);
              send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
            }
          }
        }
        Err(e) => {
          warn!("[{}] Failed to create dashboard for linked category: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to create dashboard: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_ch_") => {
      // Handle channel selection in manual linking flow
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(selected_channel_str) = values.first() {
          if let Ok(selected_channel_id) = selected_channel_str.parse::<u64>() {
            let selected_channel = CI::new(selected_channel_id);

            // Decode state from custom_id: link_ch_{hex_state}
            let state_str = button_id.strip_prefix("link_ch_").unwrap();
            let parts: Vec<&str> = state_str.split('_').collect();

            if parts.len() != 7 {
              send_component_error_response(interaction, ctx, "Invalid state. Please start over.").await;
              return Ok(());
            }

            let category_id = parse_cid(parts[0])?;
            let mut dashboard_channel = parse_opt_cid(parts[1])?;
            let mut queue_channel =     parse_opt_cid(parts[2])?;
            let mut queue_vc_channel =  parse_opt_cid(parts[3])?;
            let mut red_channel =       parse_opt_cid(parts[4])?;
            let mut blue_channel =      parse_opt_cid(parts[5])?;
            let type_char = parts[6];
            let channel_type = match type_char {
              "d" => "dashboard",
              "q" => "queue",
              "v" => "queue_vc",
              "r" => "red",
              "b" => "blue",
              _ => "unknown",
            };

            // Update the appropriate channel based on type
            match channel_type {
              "dashboard" => dashboard_channel = Some(selected_channel),
              "queue" =>     queue_channel =     Some(selected_channel),
              "queue_vc" =>  queue_vc_channel =  Some(selected_channel),
              "red" =>       red_channel =       Some(selected_channel),
              "blue" =>      blue_channel =      Some(selected_channel),
              _ => {}
            }

            // Check if all channels are now selected
            if dashboard_channel.is_some() && queue_channel.is_some() && queue_vc_channel.is_some() && red_channel.is_some() && blue_channel.is_some() {
              // All channels selected - create the category
              let guild_name = guild_name(ctx, guild_id);

              use crate::models::{Category, Channels, TeamChannel};

              // Derive category from dashboard channel's parent
              let category_id = ctx.cache.channel(dashboard_channel.unwrap()).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

              let mut temp_category = Category::new(
                guild_id,
                None,
                0,
                None,
                crate::DEFAULT_QUOTA,
                crate::DEFAULT_HOT_JOIN_TIMEOUT,
                MI::new(1),
                Channels {
                  category: category_id,
                  queue_chat: queue_channel.unwrap(),
                  queue_vc: queue_vc_channel.unwrap(),
                  teams: vec![TeamChannel { red_vc: red_channel.unwrap(), blu_vc: blue_channel.unwrap() }],
                  dashboard: dashboard_channel.unwrap(),
                },
                vec![],
              );

              // Publish the dashboard
              match temp_category.dash_publish(ctx, dashboard_channel.unwrap(), db, guild_id).await {
                Ok(_) => {
                  let dashboard_msg_id = temp_category.dashboard_msg.get();
                  info!("[{}] Dashboard message created with ID {} (linked category)", guild_name, dashboard_msg_id);

                  // Create the category in the database
                  let category_config = crate::db::repo::category::CategoryConfig {
                    category_id: category_id.get(),
                    dashboard_channel_id: dashboard_channel.unwrap().get(),
                    chat_channel_id: queue_channel.unwrap().get(),
                    queue_vc_id: queue_vc_channel.unwrap().get(),
                    quota: crate::DEFAULT_QUOTA,
                  };

                  match db.categories.create_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
                    Ok(db_category) => {
                      info!("[{}] Category {} linked and saved to database", guild_name, db_category.category_id);

                      // Add category to in-memory server
                      let mut manager_lock = manager.lock().await;
                      if let Ok(server) = manager_lock.get_server(guild_id) {
                        if let Err(e) = server.add_category(db_category.clone()) {
                          error!("Failed to add category to server: {e}");
                        }
                      }
                      drop(manager_lock);

                      // Show success and return to category list
                      let categories = db.categories.get_categories_for_guild(guild_id).await?;
                      let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

                      let response =
                        CIR::UpdateMessage(CIRM::new().content("Successfully linked category from category!").embed(display.build_embed()).components(display.build_components()));
                      interaction.create_response(&ctx.http, response).await?;
                    }
                    Err(e) => {
                      let _ = dashboard_channel.unwrap().delete_message(&ctx.http, dashboard_msg_id).await;
                      warn!("[{}] Failed to save linked category to database: {}", guild_name, e);
                      send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
                    }
                  }
                }
                Err(e) => {
                  warn!("[{}] Failed to create dashboard for linked category: {}", guild_name, e);
                  send_component_error_response(interaction, ctx, &format!("Failed to create dashboard: {e}")).await;
                }
              }
            } else {
              // Continue to next channel - recursively trigger the same logic
              // by simulating a category selection with updated state
              let _fake_category_id_str = category_id.get().to_string();

              // Reuse the same logic by creating a fake interaction data
              // Actually, let's just redirect back to the category select handler
              // by crafting the state as if we just selected the category

              // Get guild name
              let guild_name = guild_name(ctx, guild_id);

              // Continue with manual channel selection flow (same code as above)

              let (text_channels, voice_channels) = {
                let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;
                let mut text_chans = Vec::new();
                let mut voice_chans = Vec::new();

                for (channel_id, channel) in &guild.channels {
                  if channel.parent_id == Some(category_id) {
                    match channel.kind {
                      ChannelType::Text => {
                        text_chans.push((*channel_id, channel.name.clone()));
                      }
                      ChannelType::Voice => {
                        voice_chans.push((*channel_id, channel.name.clone()));
                      }
                      _ => {}
                    }
                  }
                }
                (text_chans, voice_chans)
              };

              let (next_channel_type, next_channel_name, available_channels) = if dashboard_channel.is_none() {
                ("dashboard", "Dashboard (text)", text_channels)
              } else if queue_channel.is_none() {
                ("queue", "Queue chat (text)", text_channels)
              } else if queue_vc_channel.is_none() {
                ("queue_vc", "Queue voice channel", voice_channels)
              } else if red_channel.is_none() {
                ("red", "Red team voice channel", voice_channels)
              } else {
                ("blue", "Blue team voice channel", voice_channels)
              };

              let options: Vec<CSMO> = available_channels.iter().map(|(id, name)| CSMO::new(name.clone(), id.get().to_string())).collect();

              let type_char = match next_channel_type {
                "dashboard" => "d",
                "queue" => "q",
                "queue_vc" => "v",
                "red" => "r",
                "blue" => "b",
                _ => "x",
              };
              let state = format!(
                "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}_{}",
                category_id.get(),
                dashboard_channel.map(|c| c.get()).unwrap_or(0),
                queue_channel.map(|c| c.get()).unwrap_or(0),
                queue_vc_channel.map(|c| c.get()).unwrap_or(0),
                red_channel.map(|c| c.get()).unwrap_or(0),
                blue_channel.map(|c| c.get()).unwrap_or(0),
                type_char
              );

              let select_menu = CSM::new(format!("link_ch_{}", state), CSMK::String { options }).placeholder(format!("Select {}", next_channel_name));

              let mut status = String::from("**Channel Linking Progress:**\n\n");
              status.push_str(&format!("Dashboard: {}\n", if let Some(id) = dashboard_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Chat: {}\n", if let Some(id) = queue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Voice: {}\n", if let Some(id) = queue_vc_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Red Team: {}\n", if let Some(id) = red_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Blue Team: {}\n", if let Some(id) = blue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));

              let embed = CE::new().title(format!("{} - Link Channels", guild_name)).description(format!("{}\n\n**Next:** Select the {} channel from the dropdown below.", status, next_channel_name)).color(0x5865F2);

              let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_link_cancel").label("Cancel").style(BS::Secondary)])];

              let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
              interaction.create_response(&ctx.http, response).await?;
            }
          }
        }
      }
    }
    _ if button_id.starts_with("link_existing_remove_dup_") => {
      // Remove duplicate category and link to existing dashboard
      let state_str = button_id.strip_prefix("link_existing_remove_dup_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() != 6 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel = parse_cid(parts[1])?;
      let queue_vc_channel = parse_cid(parts[2])?;
      let red_channel = parse_cid(parts[3])?;
      let blue_channel = parse_cid(parts[4])?;
      let dashboard_msg_id = u64::from_str_radix(parts[5], 16)?;

      let guild_name = guild_name(ctx, guild_id);

      // Find and remove duplicate category
      let existing_categories = db.categories.get_categories_for_guild(guild_id).await?;
      let duplicate_category = existing_categories.iter().find(|g| {
        g.channels.dashboard == dashboard_channel
          || g.channels.queue_chat == queue_channel
          || g.channels.queue_vc == queue_vc_channel
          || g.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
      });

      if let Some(dup_category) = duplicate_category {
        let dup_category_id = dup_category.category_id;

        // Delete duplicate from database
        if let Err(e) = db.categories.delete_category(guild_id, dup_category_id).await {
          warn!("[{}] Failed to delete duplicate category {}: {}", guild_name, dup_category_id, e);
          send_component_error_response(interaction, ctx, &format!("Failed to remove duplicate category: {e}")).await;
          return Ok(());
        }

        // Remove from in-memory server
        let mut manager_lock = manager.lock().await;
        if let Ok(server) = manager_lock.get_server(guild_id) {
          server.categories.retain(|g| g.category_id != dup_category_id);
        }
        drop(manager_lock);

        info!("[{}] Removed duplicate category {} before linking", guild_name, dup_category_id);
      }

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      // Create new category with existing message ID
      let category_config = crate::db::repo::category::CategoryConfig {
        category_id: category_id.get(),
        dashboard_channel_id: dashboard_channel.get(),
        chat_channel_id: queue_channel.get(),
        queue_vc_id: queue_vc_channel.get(),
        quota: crate::DEFAULT_QUOTA,
      };

      match db.categories.create_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
        Ok(db_category) => {
          info!("[{}] Category {} linked to existing dashboard {} (duplicate removed)", guild_name, db_category.category_id, dashboard_msg_id);

          // Add category to in-memory server
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_server(guild_id) {
            if let Err(e) = server.add_category(db_category.clone()) {
              error!("Failed to add category to server: {e}");
            }
          }
          drop(manager_lock);

          // Show success and return to category list
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

          let response = CIR::UpdateMessage(
            CIRM::new().content(format!("Removed duplicate category and linked to existing dashboard!")).embed(display.build_embed()).components(display.build_components()),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          warn!("[{}] Failed to save linked category: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_remove_dup_new_") => {
      // Remove duplicate category and create new dashboard
      let state_str = button_id.strip_prefix("link_remove_dup_new_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() < 5 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel = parse_cid(parts[1])?;
      let queue_vc_channel = parse_cid(parts[2])?;
      let red_channel = parse_cid(parts[3])?;
      let blue_channel = parse_cid(parts[4])?;

      let guild_name = guild_name(ctx, guild_id);

      // Find and remove duplicate category
      let existing_categories = db.categories.get_categories_for_guild(guild_id).await?;
      let duplicate_category = existing_categories.iter().find(|g| {
        g.channels.dashboard == dashboard_channel
          || g.channels.queue_chat == queue_channel
          || g.channels.queue_vc == queue_vc_channel
          || g.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
      });

      if let Some(dup_category) = duplicate_category {
        let dup_category_id = dup_category.category_id;

        // Delete duplicate from database
        if let Err(e) = db.categories.delete_category(guild_id, dup_category_id).await {
          warn!("[{}] Failed to delete duplicate category {}: {}", guild_name, dup_category_id, e);
          send_component_error_response(interaction, ctx, &format!("Failed to remove duplicate category: {e}")).await;
          return Ok(());
        }

        // Remove from in-memory server
        let mut manager_lock = manager.lock().await;
        if let Ok(server) = manager_lock.get_server(guild_id) {
          server.categories.retain(|g| g.category_id != dup_category_id);
        }
        drop(manager_lock);

        info!("[{}] Removed duplicate category {} before creating new dashboard", guild_name, dup_category_id);
      }

      // Create new dashboard
      use crate::models::{Category, Channels, TeamChannel};

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      let mut temp_category = Category::new(
        guild_id,
        None,
        0,
        None,
        crate::DEFAULT_QUOTA,
        crate::DEFAULT_HOT_JOIN_TIMEOUT,
        MI::new(1),
        Channels {
          category: category_id,
          queue_chat: queue_channel,
          queue_vc: queue_vc_channel,
          teams: vec![TeamChannel { red_vc: red_channel, blu_vc: blue_channel }],
          dashboard: dashboard_channel,
        },
        vec![],
      );

      match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
        Ok(_) => {
          let dashboard_msg_id = temp_category.dashboard_msg.get();

          let category_config = crate::db::repo::category::CategoryConfig {
            category_id: category_id.get(),
            dashboard_channel_id: dashboard_channel.get(),
            chat_channel_id: queue_channel.get(),
            queue_vc_id: queue_vc_channel.get(),
            quota: crate::DEFAULT_QUOTA,
          };

          match db.categories.create_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
            Ok(db_category) => {
              info!("[{}] Category {} created with new dashboard (duplicate removed)", guild_name, db_category.category_id);

              let mut manager_lock = manager.lock().await;
              if let Ok(server) = manager_lock.get_server(guild_id) {
                if let Err(e) = server.add_category(db_category.clone()) {
                  error!("Failed to add category to server: {e}");
                }
              }
              drop(manager_lock);

              let categories = db.categories.get_categories_for_guild(guild_id).await?;
              let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

              let response = CIR::UpdateMessage(
                CIRM::new().content(format!("Removed duplicate category and created new dashboard!")).embed(display.build_embed()).components(display.build_components()),
              );
              interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
              let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;
              warn!("[{}] Failed to save category: {}", guild_name, e);
              send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
            }
          }
        }
        Err(e) => {
          warn!("[{}] Failed to create dashboard: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to create dashboard: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_manual_msg_") => {
      // Prompt user to provide message ID manually
      let response = CIR::Message(
                CIRM::new().content("**Manual Message ID Input**\n\nPlease provide the dashboard message ID.\n\nYou can get this by:\n1. Right-clicking the dashboard message\n2. Selecting \"Copy message link\"\n3. The ID is the last number in the URL\n\nExample: `https://discord.com/channels/123/456/789` → Message ID is `789`\n\n*Note: This feature requires a modal input which will be implemented in a future update. For now, please use the automatic detection or create a new dashboard.*").ephemeral(true)
            );
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_remove_category" => {
      // Show category selection dropdown for removal

      let guild_name = guild_name(ctx, guild_id);
      let categories = db.categories.get_categories_for_guild(guild_id).await?;

      if categories.is_empty() {
        let response = CIR::Message(CIRM::new().content("No categories to remove.").ephemeral(true));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      // Create dropdown with categories
      let options: Vec<CSMO> = categories.iter().map(|category| {
          let name = category.display_name();
          CSMO::new(name.clone(), category.category_id.to_string()).description(format!("Category ID: {}", category.category_id))
        }).collect();

      let select_menu = CSM::new("server_settings_remove_category_select", CSMK::String { options }).placeholder("Select a category to remove");

      let embed = CE::new().title(format!("{} - Remove Category", guild_name)).description(
          "**⚠️ Warning: This action cannot be undone!**\n\n\
                    Select a category to remove. This will:\n\
                    • Delete the category from the database\n\
                    • Remove it from the server manager\n\
                    • **NOT** delete the Discord channels\n\n\
                    You can manually delete the channels afterwards if needed.",
        ).color(0xFF0000);

      let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_remove_cancel").label("Cancel").style(BS::Secondary)])];

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_remove_category_select" => {
      // Show confirmation prompt asking about channel deletion
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(category_id_str) = values.first() {
          if let Ok(category_id) = category_id_str.parse::<u8>() {
            let guild_name = guild_name(ctx, guild_id);

            // Get category info and channel list
            let (category_name, channel_list) = {
              let mut manager_lock = manager.lock().await;
              if let Ok(server) = manager_lock.get_server(guild_id) {
                if let Some(category) = server.categories.iter().find(|g| g.category_id == category_id) {
                  let name = category.display_name();
                  let mut channels = Vec::new();
                  if category.channels.category.get() > 1 {
                    channels.push(format!("• <#{}> (category)", category.channels.category.get()));
                  }
                  channels.push(format!("• <#{}> (dashboard)", category.channels.dashboard.get()));
                  channels.push(format!("• <#{}> (queue chat)", category.channels.queue_chat.get()));
                  channels.push(format!("• <#{}> (queue voice)", category.channels.queue_vc.get()));
                  for team in &category.channels.teams {
                    channels.push(format!("• <#{}> (red team)", team.red_vc.get()));
                    channels.push(format!("• <#{}> (blue team)", team.blu_vc.get()));
                  }
                  (Some(name), channels.join("\n"))
                } else {
                  (None, String::new())
                }
              } else {
                (None, String::new())
              }
            };

            let display_name = category_name.unwrap_or_else(|| format!("Category {}", category_id));

            let embed = CE::new().title(format!("{} - Delete Channels?", guild_name)).description(format!(
                "**Removing category: {}**\n\n\
                                The following Discord channels are associated with this category:\n\n\
                                {}\n\n\
                                **Do you want to delete these Discord channels?**\n\n\
                                ⚠️ This action cannot be undone!",
                display_name, channel_list
              )).color(0xFF0000);

            let components = vec![CAR::Buttons(vec![
              CB::new(format!("server_settings_remove_confirm_delete_{}", category_id)).label("Yes, delete channels").style(BS::Danger),
              CB::new(format!("server_settings_remove_confirm_keep_{}", category_id)).label("No, keep channels").style(BS::Success),
              CB::new("server_settings_remove_cancel").label("Cancel").style(BS::Secondary),
            ])];

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
            interaction.create_response(&ctx.http, response).await?;
          }
        }
      }
    }
    _ if button_id.starts_with("server_settings_remove_confirm_delete_") => {
      // Confirm removal with channel deletion
      let category_id_str = button_id.strip_prefix("server_settings_remove_confirm_delete_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
        let guild_name = guild_name(ctx, guild_id);

        // Get category info and channels before deletion
        let (category_name, channels_to_delete) = {
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_server(guild_id) {
            if let Some(category) = server.categories.iter().find(|g| g.category_id == category_id) {
              let name = category.display_name();
              let category_channel = category.channels.category;
              let mut channels = vec![category.channels.dashboard, category.channels.queue_chat, category.channels.queue_vc];
              for team in &category.channels.teams {
                channels.push(team.red_vc);
                channels.push(team.blu_vc);
              }
              // Category last so children are deleted first
              if category_channel.get() > 1 {
                channels.push(category_channel);
              }
              (Some(name), channels)
            } else {
              (None, Vec::new())
            }
          } else {
            (None, Vec::new())
          }
        };

        // Delete from database first
        match db.categories.delete_category(guild_id, category_id).await {
          Ok(_) => {
            info!("[{}] Category {} deleted from database", guild_name, category_id);

            // Remove from in-memory server
            let mut manager_lock = manager.lock().await;
            if let Ok(server) = manager_lock.get_server(guild_id) {
              server.categories.retain(|g| g.category_id != category_id);
            }
            drop(manager_lock);

            // Delete Discord channels
            let mut deleted_count = 0;
            for channel_id in channels_to_delete {
              let channel_id: CI = channel_id;
              match channel_id.delete(&ctx.http).await {
                Ok(_) => {
                  deleted_count += 1;
                  info!("[{}] Deleted channel {}", guild_name, channel_id.get());
                }
                Err(e) => {
                  warn!("[{}] Failed to delete channel {}: {}", guild_name, channel_id.get(), e);
                }
              }
            }

            // Show success and return to category list
            let categories = db.categories.get_categories_for_guild(guild_id).await?;
            let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

            let success_msg = if let Some(name) = category_name {
              format!("Successfully removed category: {}\n🗑️ Deleted {} Discord channels", name, deleted_count)
            } else {
              format!("Successfully removed category {}\n🗑️ Deleted {} Discord channels", category_id, deleted_count)
            };

            let response = CIR::UpdateMessage(CIRM::new().content(success_msg).embed(display.build_embed()).components(display.build_components()));
            interaction.create_response(&ctx.http, response).await?;
          }
          Err(e) => {
            warn!("[{}] Failed to delete category {}: {}", guild_name, category_id, e);
            send_component_error_response(interaction, ctx, &format!("Failed to remove category: {e}")).await;
          }
        }
      }
    }
    _ if button_id.starts_with("server_settings_remove_confirm_keep_") => {
      // Confirm removal without channel deletion
      let category_id_str = button_id.strip_prefix("server_settings_remove_confirm_keep_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
        let guild_name = guild_name(ctx, guild_id);

        // Get category info before deletion
        let category_name = {
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_server(guild_id) {
            server.categories.iter().find(|g| g.category_id == category_id).map(|g| g.display_name())
          } else {
            None
          }
        };

        // Delete from database
        match db.categories.delete_category(guild_id, category_id).await {
          Ok(_) => {
            info!("[{}] Category {} deleted from database (channels kept)", guild_name, category_id);

            // Remove from in-memory server
            let mut manager_lock = manager.lock().await;
            if let Ok(server) = manager_lock.get_server(guild_id) {
              server.categories.retain(|g| g.category_id != category_id);
            }
            drop(manager_lock);

            // Show success and return to category list
            let categories = db.categories.get_categories_for_guild(guild_id).await?;
            let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

            let success_msg = if let Some(name) = category_name {
              format!("Successfully removed category: {}\n📁 Discord channels were kept", name)
            } else {
              format!("Successfully removed category {}\n📁 Discord channels were kept", category_id)
            };

            let response = CIR::UpdateMessage(CIRM::new().content(success_msg).embed(display.build_embed()).components(display.build_components()));
            interaction.create_response(&ctx.http, response).await?;
          }
          Err(e) => {
            warn!("[{}] Failed to delete category {}: {}", guild_name, category_id, e);
            send_component_error_response(interaction, ctx, &format!("Failed to remove category: {e}")).await;
          }
        }
      }
    }
    "server_settings_remove_cancel" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    "server_settings_category_select" => {
      // Handle category selection from dropdown - show modal with all settings
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(category_id_str) = values.first() {
          if let Ok(category_id) = category_id_str.parse::<u8>() {
            // Find the category
            let categories = db.categories.get_categories_for_guild(guild_id).await?;
            if let Some(category) = categories.iter().find(|g| g.category_id == category_id) {
              let modal = CreateModal::new(format!("server_settings_category_modal_{category_id}"), "Edit category settings").components(vec![
                create_short_input_opt("Name", "name", "e.g., NA PUGs, EU Competitive", &category.name.clone().unwrap_or_default()),
                create_value_input_sh_cap("Quota (2-100)", "quota", "Number of players required", &category.quota().to_string(), 1, 3),
                create_value_input_sh_cap("Ready check duration (seconds)", "timeout", "Seconds for missing players to join VC", &category.timeout.to_string(), 1, 3),
                create_paragraph_input_with_value("Connect info", "connect", "e.g., connect 192.168.1.1:27015; password secret", &category.connect_info().unwrap_or_default().to_string()),
              ]);

              let response = CIR::Modal(modal);
              interaction.create_response(&ctx.http, response).await?;
            }
          }
        }
      }
    }
    "server_settings_category_back" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    _ if button_id.starts_with("category_settings_link_message_") => {
      // Handle link message button - search for existing dashboard messages for this specific category
      let category_id_str = button_id.strip_prefix("category_settings_link_message_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
        let guild_name = guild_name(ctx, guild_id);

        // Get the category to find its dashboard channel
        let categories = db.categories.get_categories_for_guild(guild_id).await?;
        if let Some(category) = categories.iter().find(|g| g.category_id == category_id) {
          let dashboard_channel = category.channels.dashboard;

          // Search for bot messages in dashboard channel
          let bot_user_id = ctx.cache.current_user().id;
          let mut existing_dashboard_msgs = Vec::new();

          match dashboard_channel.messages(&ctx.http, GM::new().limit(50)).await {
            Ok(messages) => {
              for msg in messages {
                if msg.author.id == bot_user_id && !msg.embeds.is_empty() {
                  // Check if it looks like a dashboard
                  if let Some(embed) = msg.embeds.first() {
                    let title = embed.title.as_deref().unwrap_or("");
                    let desc = embed.description.as_deref().unwrap_or("");
                    if title.contains("Queue") || desc.contains("Queue") || desc.contains("Join") {
                      existing_dashboard_msgs.push((msg.id, msg.timestamp));
                    }
                  }
                }
              }
            }
            Err(e) => {
              warn!("[{}] Failed to fetch messages from dashboard channel: {}", guild_name, e);
            }
          }

          // Sort by timestamp (newest first)
          existing_dashboard_msgs.sort_by(|a, b| b.1.cmp(&a.1));

          let mut description = String::new();
          let mut buttons = Vec::new();

          if !existing_dashboard_msgs.is_empty() {
            description.push_str(&format!(
              "**Found {} existing dashboard message(s)**\n\n\
                            Found bot messages in <#{}> that appear to be dashboards.\n\
                            Most recent: <https://discord.com/channels/{}/{}/{}>\n\n\
                            **Select an option:**\n\
                            • Link to existing dashboard (will update category's dashboard_msg)\n\
                            • Cancel",
              existing_dashboard_msgs.len(),
              dashboard_channel.get(),
              guild_id.get(),
              dashboard_channel.get(),
              existing_dashboard_msgs[0].0.get()
            ));

            // Encode state: category_id + message_id
            let state = format!("{}_{:x}", category_id, existing_dashboard_msgs[0].0.get());

            buttons.push(CB::new(format!("category_link_msg_confirm_{}", state)).label("Link to this message").style(BS::Success));
          } else {
            description.push_str(&format!(
              "ℹ️ **No existing dashboard messages found**\n\n\
                            Searched recent messages in <#{}> but didn't find any existing dashboards.\n\n\
                            The bot will continue using the current dashboard message.",
              dashboard_channel.get()
            ));
          }

          buttons.push(CB::new(format!("category_settings_back_{}", category_id)).label("Back").style(BS::Secondary));

          let embed = CE::new().title(format!("{} - Link Dashboard Message", category.display_name())).description(description).color(0x5865F2);

          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(buttons)]));
          interaction.create_response(&ctx.http, response).await?;
        } else {
          warn!("Category {category_id} not found for guild {guild_id}");
        }
      }
    }
    _ if button_id.starts_with("category_link_msg_confirm_") => {
      // Confirm linking message to category
      let state_str = button_id.strip_prefix("category_link_msg_confirm_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() != 2 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let category_id = parts[0].parse::<u8>()?;
      let dashboard_msg_id = parse_mid(parts[1])?;

      let guild_name = guild_name(ctx, guild_id);

      // Update the category's dashboard_msg in database
      match db.categories.update_dashboard_msg_by_category_id(guild_id, category_id, dashboard_msg_id).await {
        Ok(_) => {
          info!("[{}] Updated category {} dashboard message to {}", guild_name, category_id, dashboard_msg_id);

          // Update in-memory category
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_server(guild_id) {
            if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
              category.dashboard_msg = MI::new(dashboard_msg_id);
            }
          }
          drop(manager_lock);

          // Return to category settings
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          if let Some(category) = categories.iter().find(|g| g.category_id == category_id) {
            let settings = CategorySettings::from_category(category);

            let embed = build_category_settings_embed(&settings);
            let buttons = build_category_settings_buttons(settings.category_id);

            let response = CIR::UpdateMessage(CIRM::new().content(format!("Successfully linked dashboard message!")).embed(embed).components(buttons));
            interaction.create_response(&ctx.http, response).await?;
          }
        }
        Err(e) => {
          warn!("[{}] Failed to update dashboard message for category {}: {}", guild_name, category_id, e);
          send_component_error_response(interaction, ctx, &format!("Failed to link message: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("category_settings_back_") => {
      // Return to category settings from link message screen
      let category_id_str = button_id.strip_prefix("category_settings_back_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
        let categories = db.categories.get_categories_for_guild(guild_id).await?;
        if let Some(category) = categories.iter().find(|g| g.category_id == category_id) {
          let settings = CategorySettings::from_category(category);

          let embed = build_category_settings_embed(&settings);
          let buttons = build_category_settings_buttons(settings.category_id);

          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    _ if button_id.starts_with("server_settings_category_select_") => {
      // Handle category selection from button - show settings screen with Link Message button
      let value_str = button_id.strip_prefix("server_settings_category_select_").unwrap();

      // Parse format "categoryid_queueid" to handle duplicate category_id values
      let parts: Vec<&str> = value_str.split('_').collect();
      if parts.len() == 2 {
        if let (Ok(category_id), Ok(queue_id)) = (parts[0].parse::<u8>(), parts[1].parse::<u64>()) {
          // Find the category by both category_id and queue channel ID
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          if let Some(category) = categories.iter().find(|g| g.category_id == category_id && g.channels.queue_vc.get() == queue_id) {
            // Show category settings screen with buttons including Link Message
            let settings = CategorySettings::from_category(category);

            let embed = build_category_settings_embed(&settings);
            let buttons = build_category_settings_buttons(settings.category_id);

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
            interaction.create_response(&ctx.http, response).await?;
          } else {
            warn!("Category {} not found for guild {}", category_id, guild_id);
          }
        } else {
          warn!("Invalid category ID format in button: {}", value_str);
        }
      } else {
        warn!("Invalid category ID format in button: {}", value_str);
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

  let result: Vec<(String, u16, RoleId)> = guild_ranks.into_iter().map(|gr| (gr.name, gr.elo, gr.role_id)).collect();

  Ok(result)
}

/// Get server settings from database
pub async fn get_server_settings(db: &Arc<Database>, guild_id: GI) -> Result<ServerSettings> {
  use SERVER_CONFIG_TOGGLES;

  let config_map = db.config.get_config_map(guild_id).await?;
  let runner_role = config_map.get("runner_id").cloned();
  let admin_role = config_map.get("admin_id").cloned();
  let balance_method = config_map.get("balance_method").cloned().unwrap_or_else(|| "bch".to_string());

  let mut toggle_states = Vec::with_capacity(SERVER_CONFIG_TOGGLES.len());
  for toggle in SERVER_CONFIG_TOGGLES {
    toggle_states.push(db.config.get_bool(guild_id, toggle.column, toggle.default).await?);
  }

  let post_game_timeout = db.config.get_post_game_timeout(guild_id).await.unwrap_or(120);

  Ok(ServerSettings { runner_role, admin_role, toggle_states, balance_method, post_game_timeout })
}

/// Get rank settings from database (for rank configuration menu)
pub async fn get_rank_settings(db: &Arc<Database>, guild_id: GI) -> Result<(Vec<bool>, Option<RoleId>)> {
  use RANK_CONFIG_TOGGLES;
  let mut toggle_states = Vec::with_capacity(RANK_CONFIG_TOGGLES.len());
  for toggle in RANK_CONFIG_TOGGLES {
    toggle_states.push(db.config.get_bool(guild_id, toggle.column, toggle.default).await?);
  }
  let default_rank_role = db.config.get_default_rank_role_id(guild_id).await?;
  Ok((toggle_states, default_rank_role))
}

/// Handle server settings modal submissions
pub async fn handle_server_settings_modal(
  ctx: &Context,
  interaction: &ModalInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("{} submitted modal {}", user_tag, modal_id);

  if modal_id == "server_settings_rank_modal_add" {
    // Handle add new rank modal
    let mut name_value = String::new();
    let mut elo_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
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
      send_modal_error_response(interaction, ctx, "Rank name cannot be empty.").await;
      return Ok(());
    }

    let elo: u16 = match elo_value.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Check if rank name already exists
    if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, name).await {
      send_modal_error_response(interaction, ctx, "A rank with this name already exists. Please choose a different name.").await;
      return Ok(());
    }

    // Create a new Discord role for this rank
    let guild_name = guild_name(ctx, guild_id);
    let role_name = name.to_string();

    let role_id = match guild_id.create_role(&ctx.http, ER::new().name(&role_name).colour(Color::from_rgb(128, 128, 128)).hoist(false).mentionable(true).permissions(Permissions::empty())).await
    {
      Ok(role) => {
        info!("[{}] Created new role {} for rank {}", guild_name, role.name, name);
        role.id
      }
      Err(e) => {
        warn!("[{}] Failed to create role for rank {}: {}", guild_name, name, e);
        send_modal_error_response(interaction, ctx, "Failed to create Discord role. Please check bot permissions.").await;
        return Ok(());
      }
    };

    // Add rank to DB with the created role ID
    db.ranks.add_rank(guild_id, name, elo, role_id).await?;
    let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
    info!("{} added rank '{}' with ELO {} and role {}", user_tag, name, elo, role_id.get());

    send_nav_modal!(interaction, ctx, db, nav_rank_config, guild_id)?;
  } else if modal_id.starts_with("server_settings_rank_modal_link_") {
    // Handle link existing rank modal
    let role_id_str = modal_id.strip_prefix("server_settings_rank_modal_link_").unwrap();
    let role_id = match role_id_str.parse::<u64>() {
      Ok(id) => RoleId::new(id),
      Err(_) => {
        send_modal_error_response(interaction, ctx, "Invalid role ID.").await;
        return Ok(());
      }
    };

    let mut name_value = String::new();
    let mut elo_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
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
      send_modal_error_response(interaction, ctx, "Rank name cannot be empty.").await;
      return Ok(());
    }

    let elo: u16 = match elo_value.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Check if rank name already exists
    if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, name).await {
      send_modal_error_response(interaction, ctx, "A rank with this name already exists. Please choose a different name.").await;
      return Ok(());
    }

    // Add rank to DB with the selected role ID
    db.ranks.add_rank(guild_id, name, elo, role_id).await?;
    let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
    info!("{} linked rank '{}' with ELO {} to role {}", user_tag, name, elo, role_id.get());

    send_nav_modal!(interaction, ctx, db, nav_rank_config, guild_id)?;
  } else if modal_id.starts_with("server_settings_rank_modal_") {
    // Handle rank name/ELO edit modal
    let old_rank_name = modal_id.strip_prefix("server_settings_rank_modal_").ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

    let mut name_value = String::new();
    let mut elo_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
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
      send_modal_error_response(interaction, ctx, "Rank name cannot be empty.").await;
      return Ok(());
    }

    let elo: u16 = match elo_value.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Check if new rank name already exists (and it's not the same rank being renamed)
    if new_name != old_rank_name {
      if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, new_name).await {
        send_modal_error_response(interaction, ctx, "A rank with this name already exists. Please choose a different name.").await;
        return Ok(());
      }
    }

    // Update rank in DB using name instead of position
    db.ranks.update_rank_name(guild_id, old_rank_name, new_name).await?;
    db.ranks.update_rank_elo(guild_id, new_name, elo).await?;

    send_nav_modal!(interaction, ctx, db, nav_rank_config, guild_id)?;
  } else if modal_id.starts_with("server_settings_category_modal_") {
    // Handle category settings modal submission
    let category_id: u8 =
      modal_id.strip_prefix("server_settings_category_modal_").and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

    // Extract all values from the modal
    let mut name_value = String::new();
    let mut quota_value = String::new();
    let mut timeout_value = String::new();
    let mut connect_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
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
        send_modal_error_response(interaction, ctx, "Invalid quota. Must be between 2 and 100.").await;
        return Ok(());
      }
    };

    // Parse and validate timeout
    let timeout: u16 = match timeout_value.trim().parse() {
      Ok(t) if t > 0 => t,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid timeout. Must be a positive number.").await;
        return Ok(());
      }
    };

    let name = if name_value.trim().is_empty() { None } else { Some(name_value.trim().to_string()) };
    let connect_info = if connect_value.trim().is_empty() { None } else { Some(connect_value.trim().to_string()) };

    // Update in database
    db.categories.update_name(guild_id, category_id, name.as_deref()).await?;
    db.categories.update_quota(guild_id, category_id, quota).await?;
    db.categories.update_timeout(guild_id, category_id, timeout).await?;
    if connect_info.is_some() || connect_value.trim().is_empty() {
      db.categories.update_connect_info(guild_id, category_id, connect_info.as_deref()).await?;
    }

    // Update in-memory category
    {
      let mut manager_lock = manager.lock().await;
      if let Ok(server) = manager_lock.get_server(guild_id) {
        if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
          category.name = name.clone();
          category.timeout = timeout;
          category.set_quota(quota);
          category.set_connect_info(connect_info.clone());
        }
      }
    }

    send_nav_modal!(interaction, ctx, db, nav_category_list, guild_id)?;
  } else if modal_id == "server_settings_modal_create_category" {
    // Extract modal fields
    let mut category_name = String::new();
    let mut channel_prefix = String::new();
    let mut guild_category_name = String::new();
    let mut quota_str = String::new();
    let mut bot_only_dashboard_str = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          match input.custom_id.as_str() {
            "category_name" => category_name = input.value.clone().unwrap_or_default(),
            "channel_prefix" => channel_prefix = input.value.clone().unwrap_or_default(),
            "discord_category" => guild_category_name = input.value.clone().unwrap_or_default(),
            "quota" => quota_str = input.value.clone().unwrap_or_default(),
            "bot_only_dashboard" => bot_only_dashboard_str = input.value.clone().unwrap_or_default(),
            _ => {}
          }
        }
      }
    }

    let category_name = category_name.trim().to_string();
    let channel_prefix = channel_prefix.trim().to_lowercase().replace(' ', "-");
    let guild_category_name = guild_category_name.trim().to_string();
    let bot_only_dashboard = bot_only_dashboard_str.trim().to_lowercase();

    if category_name.is_empty() || channel_prefix.is_empty() || guild_category_name.is_empty() {
      send_modal_error_response(interaction, ctx, "Category name, channel prefix, and category name cannot be empty.").await;
      return Ok(());
    }

    if !["yes", "no"].contains(&bot_only_dashboard.as_str()) {
      send_modal_error_response(interaction, ctx, "Bot-only dashboard must be 'yes' or 'no'.").await;
      return Ok(());
    }

    let quota: u8 = match quota_str.trim().parse() {
      Ok(q) if (2..=100).contains(&q) => q,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid quota. Must be between 2 and 100.").await;
        return Ok(());
      }
    };

    // Defer the response so we have time to create channels
    interaction.create_response(&ctx.http, CIR::Defer(CIRM::new().ephemeral(true))).await?;

    let guild_name = guild_name(ctx, guild_id);

    // Create channels
    match crate::handlers::admin::create_category_channels(ctx, guild_id, &guild_category_name, &channel_prefix, bot_only_dashboard.as_str() == "yes").await {
      Ok((category_id, dashboard_channel, queue_channel, queue_vc_channel)) => {
        use crate::models::{Category, Channels};

        let mut temp_category = Category::new(
          guild_id,
          Some(category_name.clone()),
          0,
          Some(category_name.clone()),
          quota,
          crate::DEFAULT_HOT_JOIN_TIMEOUT,
          MI::new(1),
          Channels { category: category_id, queue_chat: queue_channel, queue_vc: queue_vc_channel, teams: vec![], dashboard: dashboard_channel },
          vec![],
        );

        // Publish the dashboard
        match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
          Ok(_) => {
            let dashboard_msg_id = temp_category.dashboard_msg.get();
            info!("[{}] Dashboard message created with ID {}", guild_name, dashboard_msg_id);

            let category_config = crate::db::repo::category::CategoryConfig {
              category_id: category_id.get(),
              dashboard_channel_id: dashboard_channel.get(),
              chat_channel_id: queue_channel.get(),
              queue_vc_id: queue_vc_channel.get(),
              quota,
            };
            match db.categories.create_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
              Ok(db_category) => {
                info!("[{}] Category {} saved to database", guild_name, db_category.category_id);

                // Update category name in DB
                let _ = db.categories.update_name(guild_id, db_category.category_id, Some(&category_name)).await;

                // Add category to in-memory server
                let mut manager_lock = manager.lock().await;
                if let Ok(server) = manager_lock.get_server(guild_id) {
                  let mut category = db_category.clone();
                  category.name = Some(category_name.clone());
                  if let Err(e) = server.add_category(category) {
                    error!("Failed to add category to server: {e}");
                  }
                }
                drop(manager_lock);

                // Follow up with success
                let categories = db.categories.get_categories_for_guild(guild_id).await?;
                let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

                let followup = CIRF::new().embed(display.build_embed()).components(display.build_components()).ephemeral(true);
                interaction.create_followup(&ctx.http, followup).await?;
              }
              Err(e) => {
                let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;
                let _ = dashboard_channel.delete(&ctx.http).await;
                let _ = queue_channel.delete(&ctx.http).await;
                let _ = queue_vc_channel.delete(&ctx.http).await;
                let _ = category_id.delete(&ctx.http).await;

                warn!("[{}] Failed to save category to database: {}", guild_name, e);
                let followup = CIRF::new().content(format!("Failed to save category: {e}")).ephemeral(true);
                interaction.create_followup(&ctx.http, followup).await?;
              }
            }
          }
          Err(e) => {
            let _ = dashboard_channel.delete(&ctx.http).await;
            let _ = queue_channel.delete(&ctx.http).await;
            let _ = queue_vc_channel.delete(&ctx.http).await;
            let _ = category_id.delete(&ctx.http).await;

            warn!("[{}] Failed to create dashboard: {}", guild_name, e);
            let followup = CIRF::new().content(format!("Failed to create dashboard: {e}")).ephemeral(true);
            interaction.create_followup(&ctx.http, followup).await?;
          }
        }
      }
      Err(e) => {
        warn!("[{}] Failed to create channels: {}", guild_name, e);
        let followup = CIRF::new().content(format!("Failed to create channels: {e}")).ephemeral(true);
        interaction.create_followup(&ctx.http, followup).await?;
      }
    }
  } else if modal_id == "server_settings_post_game_timeout_modal" {
    // Handle post-game timeout modal
    let mut timeout_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          if input.custom_id == "post_game_timeout_input" {
            timeout_value = input.value.clone().unwrap_or_default();
          }
        }
      }
    }

    // Parse and validate timeout
    let timeout: u16 = match timeout_value.trim().parse() {
      Ok(t) if (30..=300).contains(&t) => t,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid timeout. Must be between 30 and 300 seconds.").await;
        return Ok(());
      }
    };

    // Update database
    db.config.set_post_game_timeout(guild_id, timeout).await?;
    let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
    info!("{} set post-game timeout to {} seconds", user_tag, timeout);

    send_nav_modal!(interaction, ctx, db, nav_role_config, guild_id)?;
  } else {
    warn!("Unknown server settings modal: {}", modal_id);
  }

  Ok(())
}

/// Handle player settings rank selection dropdown
pub async fn handle_player_settings_rank_select(
  ctx: &Context,
  interaction: &ComponentInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let custom_id = &interaction.data.custom_id;

  // Extract user_id from custom_id (format: player_settings_rank_select_<user_id>)
  let target_user_id: u64 = custom_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid select ID format: {}", custom_id))?;

  let target_uid = UI::new(target_user_id);
  let guild_id = interaction.guild_id.expect("Guild ID not found");

  // Get the selected role ID from the select menu
  let selected_role_id_str = match &interaction.data.kind {
    CIDK::StringSelect { values } => values.first().ok_or_else(|| anyhow::anyhow!("No rank selected"))?.clone(),
    _ => return Err(anyhow!("Invalid interaction type")),
  };

  let selected_role_id: u64 = selected_role_id_str.parse().map_err(|_| anyhow::anyhow!("Invalid role ID: {}", selected_role_id_str))?;
  let role_id = RoleId::new(selected_role_id);

  // Get current player data
  let player = db.users.check_user(target_uid, None).await?;
  let guild_elo = db.elo.get(target_uid, guild_id, db).await?;

  // Get the new rank from the selected role ID
  let new_rank = match db.ranks.rank_from_role_id(guild_id, role_id).await {
    Ok(rank) => crate::models::types::Rank { guild_id, role_id: rank.role_id, name: rank.name, elo: rank.elo },
    Err(e) => {
      warn!("Failed to find rank for role ID {}: {}", selected_role_id, e);

      // Send error message to user
      let error_embed = CE::new().title("Rank Not Found").description(format!("The rank for role <@&{}> was not found in the database. Please ensure ranks are properly configured in server settings.", selected_role_id)).color(RED);
      let response = CIR::Message(CIRM::new().embed(error_embed).ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }
  };

  let elo_ranks_linked = db.config.get_elo_ranks_linked(guild_id).await?;

  if elo_ranks_linked {
    // Linked: update both rank and ELO
    db.elo.set(target_uid, guild_id, new_rank.elo, new_rank.clone()).await?;

    // Validate ELO against player's Discord rank (if they have one)
    use crate::handlers::player::get_user_rank_from_discord_roles;
    if let Some(discord_rank_info) = get_user_rank_from_discord_roles(ctx, db, guild_id, target_uid).await {
      let discord_rank = crate::models::types::Rank { guild_id, role_id: discord_rank_info.role_id, name: discord_rank_info.name.clone(), elo: discord_rank_info.elo };

      // Validate and normalize the manually set rank's ELO
      if let Ok((normalized_elo, was_normalized)) = db.elo.validate_and_normalize_elo(target_uid, guild_id, &discord_rank, db).await {
        if was_normalized {
          info!("Admin set rank {} (ELO {}) for {}, but normalized to {} based on Discord rank {}", new_rank.name, new_rank.elo, target_uid, normalized_elo, discord_rank.name);
        }
      }
    }
  } else {
    // Independent: update rank only, keep existing ELO
    db.elo.set(target_uid, guild_id, guild_elo.elo, new_rank.clone()).await?;
  }

  if guild_elo.rank.name != new_rank.name {
    info!("Updated rank for {}: {} -> {}{}", target_uid, guild_elo.rank.name, new_rank.name, if elo_ranks_linked { "" } else { " (ELO unchanged, independent)" });
  }

  // Update Discord roles
  if let Ok(member) = guild_id.member(&ctx.http, target_uid).await {
    // Remove old rank role
    if member.roles.contains(&guild_elo.rank.role_id) {
      if let Err(e) = member.remove_role(&ctx.http, guild_elo.rank.role_id).await {
        info!("Failed to remove old rank role {} from user {}: {}", guild_elo.rank.role_id, target_uid, e);
      } else {
        info!("Removed rank role {} from user {}", guild_elo.rank.name, target_uid);
      }
    }

    // Add new rank role
    if !member.roles.contains(&new_rank.role_id) {
      if let Err(e) = member.add_role(&ctx.http, new_rank.role_id).await {
        info!("Failed to add new rank role {} to user {}: {}", new_rank.role_id, target_uid, e);
      } else {
        info!("Added rank role {} to user {}", new_rank.name, target_uid);
      }
    }
  }

  // Update dashboards where this player is queued
  {
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_server(guild_id) {
      let mut found_in_queue = false;
      for category in &server.categories {
        // Check if player is in any session in this category
        let player_in_queue = category.formats[0].sessions.iter().any(|session| session.pool.iter().any(|p| p.player.user_id == target_uid));

        if player_in_queue {
          found_in_queue = true;
          info!("Player {} rank changed, updating dashboard for category {}", target_uid, category.category_id);
          category.queue_dash_update(ctx, guild_id).await;
        }
      }
      if !found_in_queue {
        info!("Player {} rank changed but not found in any queue, no dashboard update needed", target_uid);
      }
    } else {
      warn!("Failed to get server for guild {} when checking if player {} is queued", guild_id, target_uid);
    }
  }

  // Refresh the settings menu
  let username = match ctx.http.get_user(target_uid).await {
    Ok(u) => u.name.clone(),
    Err(_) => target_user_id.to_string(),
  };

  let updated_guild_elo = db.elo.get(target_uid, guild_id, db).await?;
  let settings = PlayerSettings {
    user_id:  target_uid,
    username,
    steam_id: player.steam_id,
    elo:      updated_guild_elo.elo,
    rank:     updated_guild_elo.rank.name.clone(),
    games:    updated_guild_elo.games,
    wins:     updated_guild_elo.wins,
  };

  let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

// ============================================================================
// CATEGORY SETTINGS HANDLERS
// ============================================================================

/// Category settings structure for display
pub struct CategorySettings {
  pub category_id:  u8,
  pub name:         Option<String>,
  pub quota:        u8,
  pub timeout:      u16,
  pub connect_info: Option<String>,
  pub format_names: Vec<String>,
  pub vc_create:    String,
  pub vc_destroy:   String,
  pub vc_keep_min:  bool,
}

impl CategorySettings {
  pub fn from_category(category: &crate::models::Category) -> Self {
    Self {
      category_id:  category.category_id,
      name:         category.name.clone(),
      quota:        category.quota(),
      timeout:      category.timeout,
      connect_info: category.connect_info().map(|s| s.to_string()),
      format_names: category.formats.iter().map(|sg| sg.name.clone()).collect(),
      vc_create:    category.team_vc_settings.create_policy.to_string(),
      vc_destroy:   category.team_vc_settings.destroy_policy.to_string(),
      vc_keep_min:  category.team_vc_settings.keep_minimum,
    }
  }
}

/// Build category settings embed
pub fn build_category_settings_embed(settings: &CategorySettings) -> CE {
  use {AsSettingsMenu, CategorySettingsDisplay};
  let display = CategorySettingsDisplay {
    category_id:  settings.category_id,
    name:         settings.name.clone(),
    quota:        settings.quota,
    timeout:      settings.timeout,
    connect_info: settings.connect_info.clone(),
    format_names: settings.format_names.clone(),
    vc_create:    settings.vc_create.clone(),
    vc_destroy:   settings.vc_destroy.clone(),
    vc_keep_min:  settings.vc_keep_min,
  };
  display.as_settings_menu().build_embed()
}

/// Build category settings buttons with category_id embedded in custom_id
pub fn build_category_settings_buttons(category_id: u8) -> Vec<CAR> {
  use {AsSettingsMenu, CategorySettingsDisplay};
  let display = CategorySettingsDisplay {
    category_id,
    name: None,
    quota: 0,
    timeout: 0,
    connect_info: None,
    format_names: Vec::new(),
    vc_create: String::new(),
    vc_destroy: String::new(),
    vc_keep_min: true,
  };
  display.as_settings_menu().build_components()
}

/// Build category selector for choosing which category to configure
pub fn build_category_selector(categories: &[crate::models::Category]) -> CAR {
  let options: Vec<CSMO> = categories.iter().map(|g| {
      let label = g.display_name();
      let value = g.category_id.to_string();
      CSMO::new(label, value)
    }).collect();

  CAR::SelectMenu(CSM::new("category_settings_select", CSMK::String { options }).placeholder("Select a category...").min_values(1).max_values(1))
}

/// Handle category settings button interactions
pub async fn handle_category_settings_button(
  ctx: &Context,
  interaction: &ComponentInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let button_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Category Settings] {} pressed {}", user_tag, button_id);

  // Handle format remove confirmation (button: category_sg_confirm_remove_{gid}_{sgid}, select: category_sg_confirm_remove with value gid_sgid)
  if button_id == "category_sg_confirm_remove" || button_id.starts_with("category_sg_confirm_remove_") {
    let selected = if button_id == "category_sg_confirm_remove" {
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      button_id.strip_prefix("category_sg_confirm_remove_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.split('_').collect();
    if parts.len() != 2 {
      return Err(anyhow::anyhow!("Invalid remove selection format"));
    }
    let category_id: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("Invalid category_id"))?;
    let sg_id: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid format_id"))?;

    let mut manager_lock = manager.lock().await;
    let category = {
      let server = manager_lock.get_server(guild_id)?;
      server.categories.iter_mut().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
    };

    match category.remove_format(sg_id) {
      Ok(_) => {
        // Persist to DB
        db.categories.save_all_formats(guild_id, category_id, &category.formats).await?;

        // Update dashboard
        category.queue_dash_update(ctx, guild_id).await;

        let display =
          FormatListDisplay { category_id, category_name: category.display_name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
        drop(manager_lock);
        let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
        interaction.create_response(&ctx.http, response).await?;
      }
      Err(e) => {
        drop(manager_lock);
        send_component_error_response(interaction, ctx, &format!("Failed to remove format: {}", e)).await;
      }
    }
    return Ok(());
  }

  // Handle format edit (button: category_sg_edit_{gid}_{sgid}, select: category_sg_edit with value gid_sgid)
  if button_id == "category_sg_edit" || button_id.starts_with("category_sg_edit_") {
    let selected = if button_id == "category_sg_edit" {
      // Select menu variant
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      // Button variant: strip prefix to get "gid_sgid"
      button_id.strip_prefix("category_sg_edit_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.split('_').collect();
    if parts.len() != 2 {
      return Err(anyhow::anyhow!("Invalid edit selection format"));
    }
    let category_id: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("Invalid category_id"))?;
    let sg_id: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid format_id"))?;

    // Show modal to edit the format's name and quota

    let mut manager_lock = manager.lock().await;
    let sg_name;
    let sg_quota;
    {
      let server = manager_lock.get_server(guild_id)?;
      let category = server.categories.iter().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;
      let sg = category.formats.iter().find(|s| s.id == sg_id).ok_or_else(|| anyhow::anyhow!("Format {} not found", sg_id))?;
      sg_name = sg.name.clone();
      sg_quota = sg.quota.to_string();
    }
    drop(manager_lock);

    let modal = CreateModal::new(format!("category_sg_modal_edit_{}_{}", category_id, sg_id), format!("Edit format: {}", sg_name)).components(vec![
      create_value_input_sh("Format name", "name", "", &sg_name),
      create_value_input_sh("Quota (players per match)", "quota", "", &sg_quota),
    ]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  // Handle elo gate buttons (these parse their own category_id from the value)
  if button_id.starts_with("category_settings_elo_gate_") {
    let category_id_str = button_id.strip_prefix("category_settings_elo_gate_").unwrap();
    if let Ok(category_id) = category_id_str.parse::<u8>() {
      let ranks = db.ranks.get_ranks(guild_id).await?;
      if ranks.is_empty() {
        let embed = CE::new().title("No ranks configured").description("You need to configure ranks before setting up an ELO gate.\nGo to server settings and set up ranks first.").color(RED);
        let response = CIR::UpdateMessage(
          CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary)])]),
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      let embed = CE::new().title("ELO Gate - Select minimum rank").description("Select the **minimum** rank that can view this category's category.\nAll ranks from min to max (inclusive) will have access.").color(0x5865F2);

      let mut options: Vec<(String, String)> = Vec::new();
      options.push(("No minimum".to_string(), format!("{}_0", category_id)));
      for (i, r) in ranks.iter().enumerate() {
        options.push((format!("{} (ELO {})", r.name, r.elo), format!("{}_{}", category_id, i)));
      }

      let mut components = Vec::new();
      if let Some(menu) = create_selection_menu("elo_gate_min", "Select minimum rank", options) {
        components.push(menu);
      }
      components.push(CAR::Buttons(vec![
        CB::new(format!("elo_gate_clear_{category_id}")).label("Clear ELO gate").style(BS::Danger),
        CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary),
      ]));

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    return Ok(());
  }

  if button_id == "elo_gate_min" || button_id.starts_with("elo_gate_min_") {
    let selected = if button_id == "elo_gate_min" {
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      button_id.strip_prefix("elo_gate_min_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.splitn(2, '_').collect();
    if parts.len() == 2 {
      let category_id: u8 = parts[0].parse().unwrap_or(0);
      let min_idx: usize = parts[1].parse().unwrap_or(0);

      let ranks = db.ranks.get_ranks(guild_id).await?;
      let min_rank_name = if min_idx == 0 { "No minimum" } else { ranks.get(min_idx).map(|r| r.name.as_str()).unwrap_or("?") };

      let embed = CE::new().title("ELO Gate - Select maximum rank").description(format!("Minimum rank: **{}**\n\nNow select the **maximum** rank that can view this category's category.", min_rank_name)).color(0x5865F2);

      let mut options: Vec<(String, String)> =
        ranks.iter().enumerate().filter(|(i, _)| *i >= min_idx).map(|(i, r)| (format!("{} (ELO {})", r.name, r.elo), format!("{}_{}_{}", category_id, min_idx, i))).collect();
      options.push(("No maximum".to_string(), format!("{}_{}_{}", category_id, min_idx, ranks.len())));

      let mut components = Vec::new();
      if let Some(menu) = create_selection_menu("elo_gate_max", "Select maximum rank", options) {
        components.push(menu);
      }
      components.push(CAR::Buttons(vec![CB::new(format!("category_settings_elo_gate_{category_id}")).label("Back").style(BS::Secondary)]));

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    return Ok(());
  }

  if button_id == "elo_gate_max" || button_id.starts_with("elo_gate_max_") {
    let selected = if button_id == "elo_gate_max" {
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      button_id.strip_prefix("elo_gate_max_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.splitn(3, '_').collect();
    if parts.len() == 3 {
      let category_id: u8 = parts[0].parse().unwrap_or(0);
      let min_idx: usize = parts[1].parse().unwrap_or(0);
      let raw_max: usize = parts[2].parse().unwrap_or(0);

      let ranks = db.ranks.get_ranks(guild_id).await?;
      // Clamp: sentinel ranks.len() ("No maximum") maps to last valid index
      let max_idx = raw_max.min(ranks.len().saturating_sub(1));
      let category_id = {
        let mut manager_lock = manager.lock().await;
        let server = manager_lock.get_server(guild_id)?;
        let category = server.categories.iter().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;
        category.channels.category
      };

      match apply_elo_gate(ctx, guild_id, category_id, &ranks, min_idx, max_idx).await {
        Ok(count) => {
          let min_name = if min_idx == 0 { "No minimum" } else { ranks.get(min_idx).map(|r| r.name.as_str()).unwrap_or("?") };
          let max_name = if max_idx >= ranks.len().saturating_sub(1) { "No maximum" } else { ranks.get(max_idx).map(|r| r.name.as_str()).unwrap_or("?") };
          let embed = CE::new().title("ELO Gate Applied").description(format!("Category visibility restricted to ranks **{}** through **{}**.\n{} rank role(s) granted view access.", min_name, max_name, count)).color(crate::GREEN);

          let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![CB::new(format!("category_settings_back_{category_id}")).label("Back to category settings").style(BS::Secondary)])]),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          let hint = if e.to_string().contains("Missing Access") {
            "\n\nThe bot may lack **Manage Roles** or **Manage Channels** permission on this category. Check the bot's channel-level permissions."
          } else {
            ""
          };
          let embed = CE::new().title("ELO Gate Failed").description(format!("Failed to apply permissions: {}{}", e, hint)).color(RED);
          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![
            CB::new(format!("category_settings_elo_gate_{category_id}")).label("Retry").style(BS::Primary),
            CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary),
          ])]));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    return Ok(());
  }

  if button_id.starts_with("elo_gate_clear_") {
    let category_id_str = button_id.strip_prefix("elo_gate_clear_").unwrap();
    if let Ok(category_id) = category_id_str.parse::<u8>() {
      let category_id = {
        let mut manager_lock = manager.lock().await;
        let server = manager_lock.get_server(guild_id)?;
        let category = server.categories.iter().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;
        category.channels.category
      };

      match clear_elo_gate(ctx, guild_id, category_id).await {
        Ok(_) => {
          let embed = CE::new().title("ELO Gate Cleared").description("Category is now visible to everyone.").color(crate::GREEN);
          let response = CIR::UpdateMessage(
            CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![CB::new(format!("category_settings_back_{category_id}")).label("Back to category settings").style(BS::Secondary)])]),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          let embed = CE::new().title("Clear ELO Gate Failed").description(format!("Failed to clear permissions: {}", e)).color(RED);
          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![
            CB::new(format!("elo_gate_clear_{category_id}")).label("Retry").style(BS::Primary),
            CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary),
          ])]));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    return Ok(());
  }

  // Extract category_id from button custom_id (format: category_settings_edit_<action>_<category_id>)
  let category_id: u8 = button_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid button ID format: {}", button_id))?;

  // Get the category by ID
  let mut manager_lock = manager.lock().await;
  let category = {
    let server = manager_lock.get_server(guild_id)?;
    server.categories.iter().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?.clone()
  };
  let settings = CategorySettings::from_category(&category);
  drop(manager_lock);

  // Match button action (button_id format: category_settings_edit_<action>_<category_id>)
  if button_id.starts_with("category_settings_edit_name_") {
    let modal = CreateModal::new(format!("category_settings_modal_name_{category_id}"), "Set category name").components(vec![create_short_input_opt("Category name", "name", "e.g., NA PUGs, EU Competitive", &settings.name.unwrap_or_default())]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_quota_") {
    let modal = CreateModal::new(format!("category_settings_modal_quota_{category_id}"), "Set queue quota").components(vec![create_value_input_sh_cap("Quota (2-100)", "quota", "Number of players required", &settings.quota.to_string(), 1, 3)]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_timeout_") {
    let modal = CreateModal::new(format!("category_settings_modal_timeout_{category_id}"), "Set ready check duration").components(vec![create_value_input_sh_cap("Timeout (seconds)", "timeout", "Seconds for missing players to join VC when queue goes hot", &settings.timeout.to_string(), 1, 3)]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_connect_") {
    let modal = CreateModal::new(format!("category_settings_modal_connect_{category_id}"), "Set server connect info").components(vec![create_paragraph_input_with_value("Connect command", "connect_info", "e.g., connect 192.168.1.1:27015; password secret", &settings.connect_info.unwrap_or_default())]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_vc_create_") {
    // Cycle through create policies
    use crate::models::TeamVcCreatePolicy;
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_server(guild_id) {
      if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
        let next = match category.team_vc_settings.create_policy {
          TeamVcCreatePolicy::OnFirstJoin => TeamVcCreatePolicy::OnHot,
          TeamVcCreatePolicy::OnHot => TeamVcCreatePolicy::OnGameStart,
          TeamVcCreatePolicy::OnGameStart => TeamVcCreatePolicy::OnFirstJoin,
        };
        category.team_vc_settings.create_policy = next;
        let _ = db.categories.update_team_vc_settings(guild_id, category_id, &category.team_vc_settings).await;
        category.reconcile_team_vcs(ctx, guild_id, db).await;
        refresh_category_settings!(interaction, ctx, category)?;
      }
    }
    drop(manager_lock);
  } else if button_id.starts_with("category_settings_edit_vc_destroy_") {
    // Cycle through destroy policies
    use crate::models::TeamVcDestroyPolicy;
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_server(guild_id) {
      if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
        let next = match category.team_vc_settings.destroy_policy {
          TeamVcDestroyPolicy::OnLastLeave => TeamVcDestroyPolicy::AfterPull,
          TeamVcDestroyPolicy::AfterPull => TeamVcDestroyPolicy::AfterTimeout,
          TeamVcDestroyPolicy::AfterTimeout => TeamVcDestroyPolicy::OnLastLeave,
        };
        category.team_vc_settings.destroy_policy = next;
        let _ = db.categories.update_team_vc_settings(guild_id, category_id, &category.team_vc_settings).await;
        category.reconcile_team_vcs(ctx, guild_id, db).await;
        refresh_category_settings!(interaction, ctx, category)?;
      }
    }
    drop(manager_lock);
  } else if button_id.starts_with("category_settings_edit_vc_keepmin_") {
    // Toggle keep_minimum
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_server(guild_id) {
      if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
        category.team_vc_settings.keep_minimum = !category.team_vc_settings.keep_minimum;
        let _ = db.categories.update_team_vc_settings(guild_id, category_id, &category.team_vc_settings).await;
        category.reconcile_team_vcs(ctx, guild_id, db).await;
        refresh_category_settings!(interaction, ctx, category)?;
      }
    }
    drop(manager_lock);
  } else if button_id.starts_with("category_settings_link_message_") {
    // Handle link message button - search for existing dashboard messages
    let guild_name = guild_name(ctx, guild_id);

    // Get the category to find its dashboard channel
    let categories = db.categories.get_categories_for_guild(guild_id).await?;
    if let Some(category) = categories.iter().find(|g| g.category_id == category_id) {
      let dashboard_channel = category.channels.dashboard;

      // Search for bot messages in dashboard channel
      let bot_user_id = ctx.cache.current_user().id;
      let mut existing_dashboard_msgs = Vec::new();

      match dashboard_channel.messages(&ctx.http, GM::new().limit(50)).await {
        Ok(messages) => {
          for msg in messages {
            if msg.author.id == bot_user_id && !msg.embeds.is_empty() {
              // Check if it looks like a dashboard
              if let Some(embed) = msg.embeds.first() {
                let title = embed.title.as_deref().unwrap_or("");
                let desc = embed.description.as_deref().unwrap_or("");
                if title.contains("Queue") || desc.contains("Queue") || desc.contains("Join") {
                  existing_dashboard_msgs.push((msg.id, msg.timestamp));
                }
              }
            }
          }
        }
        Err(e) => {
          warn!("[{}] Failed to fetch messages from dashboard channel: {}", guild_name, e);
        }
      }

      // Sort by timestamp (newest first)
      existing_dashboard_msgs.sort_by(|a, b| b.1.cmp(&a.1));

      let mut description = String::new();
      let mut buttons = Vec::new();

      if !existing_dashboard_msgs.is_empty() {
        description.push_str(&format!(
          "**Found {} existing dashboard message(s)**\n\n\
                    Found bot messages in <#{}> that appear to be dashboards.\n\n\
                    **Select a message to link:**",
          existing_dashboard_msgs.len(),
          dashboard_channel.get()
        ));

        // Add a button for each found message (limit to 5 to avoid Discord limits)
        for (i, (msg_id, timestamp)) in existing_dashboard_msgs.iter().take(5).enumerate() {
          let state = format!("{}_{:x}", category_id, msg_id.get());
          let time_str = timestamp.unix_timestamp();
          let label = if i == 0 { format!("Most recent (<t:{}:f>)", time_str) } else { format!("Message {} (<t:{}:f>)", i + 1, time_str) };

          buttons.push(CB::new(format!("category_link_msg_confirm_{}", state)).label(label).style(BS::Success));
        }

        if existing_dashboard_msgs.len() > 5 {
          description.push_str(&format!("\n\n*Showing 5 of {} messages*", existing_dashboard_msgs.len()));
        }
      } else {
        description.push_str(&format!(
          "ℹ️ **No existing dashboard messages found**\n\n\
                    Searched recent messages in <#{}> but didn't find any existing dashboards.\n\n\
                    The bot will continue using the current dashboard message.",
          dashboard_channel.get()
        ));
      }

      // Add manual input button
      buttons.push(CB::new(format!("category_link_msg_manual_{}", category_id)).label("Enter message ID").style(BS::Primary));

      buttons.push(CB::new(format!("category_settings_back_{}", category_id)).label("Back").style(BS::Secondary));

      let embed = CE::new().title(format!("{} - Link Dashboard Message", category.display_name())).description(description).color(0x5865F2);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(buttons)]));
      interaction.create_response(&ctx.http, response).await?;
    } else {
      warn!("Category {category_id} not found for guild {guild_id}");
    }
  } else if button_id.starts_with("category_link_msg_confirm_") {
    // Confirm linking message to category
    let state_str = button_id.strip_prefix("category_link_msg_confirm_").unwrap();
    let parts: Vec<&str> = state_str.split('_').collect();

    if parts.len() != 2 {
      warn!("Invalid state format in category_link_msg_confirm: {}", state_str);
      return Ok(());
    }

    let category_id = parts[0].parse::<u8>().map_err(|e| anyhow::anyhow!("Invalid category_id: {}", e))?;
    let dashboard_msg_id = parse_mid(parts[1]).map_err(|e| anyhow::anyhow!("Invalid message_id: {}", e))?;

    // Update database
    match db.categories.update_dashboard_msg_by_category_id(guild_id, category_id, dashboard_msg_id).await {
      Ok(_) => {
        // Update in-memory category
        let mut manager_lock = manager.lock().await;
        if let Ok(server) = manager_lock.get_server(guild_id) {
          if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
            category.dashboard_msg = dashboard_msg_id.into();
            info!("Updated category {} dashboard_msg to {} in memory", category_id, dashboard_msg_id);
          }
        }
        drop(manager_lock);

        let embed = CE::new().title("Dashboard Message Linked").description(format!(
            "Successfully linked existing dashboard message to this category.\n\n\
                        Message ID: `{}`\n\n\
                        The bot will now update this message instead of creating a new one.",
            dashboard_msg_id
          )).color(0x57F287);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));
        interaction.create_response(&ctx.http, response).await?;
      }
      Err(e) => {
        error!("Failed to update dashboard_msg for category {}: {}", category_id, e);
        let embed = CE::new().title("Failed to link the message").description(format!("Database error: {}", e)).color(0xED4245);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
  } else if button_id.starts_with("category_link_msg_manual_") {
    // Manual message ID input - show modal

    let modal = CreateModal::new(format!("category_link_msg_modal_{}", category_id), "Enter dashboard message ID").components(vec![create_input_sh_cap("Message ID or link", "message_id", "e.g., 1467572971093885086 or https://discord.com/channels/.../...", 17, 200)]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_formats_") {
    // Show formats list screen
    let display =
      FormatListDisplay { category_id, category_name: category.display_name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
    let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_sg_back_") {
    // Back from formats list -> category settings
    let settings = CategorySettings::from_category(&category);
    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_sg_add_") {
    // Show modal to add a new format

    let modal = CreateModal::new(format!("category_sg_modal_add_{}", category_id), "Add format").components(vec![
      create_input_sh("Format name", "name", "e.g., Competitive, Casual"),
      create_input_sh("Quota (players per match)", "quota", "e.g., 12"),
    ]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_sg_remove_") {
    // Show select menu to pick which format to remove
    // Only non-default formats (id != 0) can be removed
    let removable: Vec<(String, String)> =
      category.formats.iter().filter(|sg| sg.id != 0).map(|sg| (format!("{} (quota: {})", sg.name, sg.quota), format!("{}_{}", category_id, sg.id))).collect();

    if removable.is_empty() {
      send_component_error_response(interaction, ctx, "No removable formats (the default format cannot be removed).").await;
    } else {
      use create_selection_menu;
      let mut components = Vec::new();
      if let Some(menu) = create_selection_menu("category_sg_confirm_remove", "Select format to remove", removable) {
        components.push(menu);
      }
      components.push(CAR::Buttons(vec![crate::models::embeds::Ephemeral::back(format!("category_sg_back_{}", category_id))]));
      let embed = CE::new().title("Remove format").description("Select a format to remove. The default format cannot be removed.").color(0xED4245);
      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
  } else if button_id.starts_with("category_settings_back_") {
    // Back button - return to category settings screen
    let category_id_str = button_id.strip_prefix("category_settings_back_").unwrap();
    if let Ok(category_id) = category_id_str.parse::<u8>() {
      let categories = db.categories.get_categories_for_guild(guild_id).await?;
      if let Some(category) = categories.iter().find(|g| g.category_id == category_id) {
        let settings = CategorySettings::from_category(category);
        let embed = build_category_settings_embed(&settings);
        let buttons = build_category_settings_buttons(settings.category_id);
        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
  } else {
    warn!("Unknown category settings button: {}", button_id);
  }

  Ok(())
}

/// Apply ELO gate permissions on a category channel.
/// Denies VIEW_CHANNEL for @everyone, allows VIEW_CHANNEL for rank roles in [min_idx..=max_idx].
/// Also ensures the bot can still see the category.
async fn apply_elo_gate(ctx: &Context, guild_id: GI, category_id: CI, ranks: &[crate::db::repo::rank::GuildRank], min_idx: usize, max_idx: usize) -> Result<usize> {
  let guild = guild_id.to_partial_guild(&ctx.http).await?;
  let bot_user_id = ctx.cache.current_user().id;

  // Find bot's integration role
  let bot_role = guild.roles.values().find(|r| r.managed && r.tags.bot_id == Some(bot_user_id)).map(|r| r.id);

  // Grant bot permissions FIRST so it doesn't lose access after denying @everyone.
  // Must include MANAGE_CHANNELS and MANAGE_ROLES so the bot can still edit
  // dashboard messages, delete team VCs, and modify permissions on channels
  // under this category after @everyone is denied.
  let bot_perms = Permissions::VIEW_CHANNEL
    | Permissions::SEND_MESSAGES
    | Permissions::EMBED_LINKS
    | Permissions::CONNECT
    | Permissions::MOVE_MEMBERS
    | Permissions::MANAGE_CHANNELS
    | Permissions::MANAGE_ROLES;

  category_id.create_permission(&ctx.http, PO { allow: bot_perms, deny: Permissions::empty(), kind: POT::Member(bot_user_id) }).await?;

  // Allow bot integration role if present
  if let Some(role_id) = bot_role {
    category_id.create_permission(&ctx.http, PO { allow: bot_perms, deny: Permissions::empty(), kind: POT::Role(role_id) }).await?;
  }

  // Deny @everyone VIEW_CHANNEL on the category
  category_id.create_permission(&ctx.http, PO { allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL, kind: POT::Role(guild_id.everyone_role()) }).await?;

  // Collect all rank role IDs so we can deny those outside the range
  let mut allowed_count = 0usize;
  for (i, rank) in ranks.iter().enumerate() {
    if i >= min_idx && i <= max_idx {
      // Allow this rank to view
      category_id.create_permission(&ctx.http, PO { allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty(), kind: POT::Role(rank.role_id) }).await?;
      allowed_count += 1;
    } else {
      // Explicitly deny this rank
      category_id.create_permission(&ctx.http, PO { allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL, kind: POT::Role(rank.role_id) }).await?;
    }
  }

  info!("Applied rank gate on category {} in guild {}: ranks {}..={} ({} roles allowed)", category_id, guild_id, min_idx, max_idx, allowed_count);

  Ok(allowed_count)
}

/// Clear ELO gate permissions from a category channel.
/// Removes the VIEW_CHANNEL deny from @everyone and removes all rank role overwrites.
async fn clear_elo_gate(ctx: &Context, guild_id: GI, category_id: CI) -> Result<()> {
  // Remove @everyone VIEW_CHANNEL deny by deleting the overwrite
  category_id.delete_permission(&ctx.http, POT::Role(guild_id.everyone_role())).await?;

  // Get the current channel to find existing overwrites
  let channel = ctx.http.get_channel(category_id).await?;
  if let Some(guild_channel) = channel.guild() {
    for overwrite in &guild_channel.permission_overwrites {
      // Remove role overwrites (but keep member overwrites like the bot's)
      if let POT::Role(role_id) = overwrite.kind {
        if role_id != guild_id.everyone_role() {
          let _ = category_id.delete_permission(&ctx.http, POT::Role(role_id)).await;
        }
      }
    }
  }

  info!("Cleared rank gate on category {} in guild {}", category_id, guild_id);
  Ok(())
}

/// Handle category selection from the selector menu
pub async fn handle_category_settings_select(
  ctx: &Context,
  interaction: &ComponentInteraction,
  _db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, _db).await;
  info!("[Category Settings] {} selected category", user_tag);

  // Extract selected category_id from the interaction
  let category_id: u8 = match &interaction.data.kind {
    CIDK::StringSelect { values } => values.first().and_then(|v| v.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid category selection"))?,
    _ => return Err(anyhow::anyhow!("Expected string select interaction")),
  };

  // Get the category by ID
  let mut manager_lock = manager.lock().await;
  let category = {
    let server = manager_lock.get_server(guild_id)?;
    server.categories.iter().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category not found"))?.clone()
  };
  drop(manager_lock);

  let settings = CategorySettings::from_category(&category);

  let embed = build_category_settings_embed(&settings);
  let buttons = build_category_settings_buttons(settings.category_id);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Handle manual dashboard message link modal submissions
pub async fn handle_category_link_msg_modal(
  ctx: &Context,
  interaction: &ModalInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  // Extract category_id from modal ID (format: category_link_msg_modal_{category_id})
  let category_id: u8 = modal_id.strip_prefix("category_link_msg_modal_").and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

  // Get the message ID input
  let message_input = interaction.data.components.first().and_then(|row| row.components.first()).and_then(|comp| if let ARC::InputText(input) = comp { input.value.as_ref() } else { None }).ok_or_else(|| anyhow::anyhow!("No message ID provided"))?;

  // Parse message ID from input (could be just ID or a Discord link)
  let dashboard_msg_id = if message_input.contains("discord.com/channels/") {
    // Extract message ID from Discord link
    // Format: https://discord.com/channels/{guild_id}/{channel_id}/{message_id}
    message_input.split('/').last().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| anyhow::anyhow!("Invalid Discord message link format"))?
  } else {
    // Parse as direct message ID
    message_input.trim().parse::<u64>().map_err(|_| anyhow::anyhow!("Invalid message ID: must be a number or Discord message link"))?
  };

  // Validate that the message exists in the dashboard channel
  let categories = db.categories.get_categories_for_guild(guild_id).await?;
  let category = categories.iter().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;

  let dashboard_channel = category.channels.dashboard;

  // Try to fetch the message to verify it exists
  match dashboard_channel.message(&ctx.http, dashboard_msg_id).await {
    Ok(_) => {
      // Message exists, update database
      match db.categories.update_dashboard_msg_by_category_id(guild_id, category_id, dashboard_msg_id).await {
        Ok(_) => {
          // Update in-memory category
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_server(guild_id) {
            if let Some(category) = server.categories.iter_mut().find(|g| g.category_id == category_id) {
              category.dashboard_msg = dashboard_msg_id.into();
              info!("Updated category {} dashboard_msg to {} in memory", category_id, dashboard_msg_id);
            }
          }
          drop(manager_lock);

          let embed = CE::new().title("Dashboard Message Linked").description(format!(
              "Successfully linked dashboard message to this category.\n\n\
                            Message ID: `{}`\n\
                            Channel: <#{}>\n\n\
                            The bot will now update this message instead of creating a new one.",
              dashboard_msg_id,
              dashboard_channel.get()
            )).color(0x57F287);

          let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          error!("Failed to update dashboard_msg for category {}: {}", category_id, e);
          let embed = CE::new().title("Failed to link the message").description(format!("Database error: {}", e)).color(0xED4245);

          let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    Err(e) => {
      warn!("Message {} not found in channel {}: {}", dashboard_msg_id, dashboard_channel, e);
      let embed = CE::new().title("Message Not Found").description(format!(
          "Could not find message `{}` in <#{}>.\n\n\
                    Please verify:\n\
                    • The message ID is correct\n\
                    • The message exists in the dashboard channel\n\
                    • The bot has permission to view the channel",
          dashboard_msg_id,
          dashboard_channel.get()
        )).color(0xED4245);

      let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
    }
  }

  Ok(())
}

/// Handle category settings modal submissions
pub async fn handle_category_settings_modal(
  ctx: &Context,
  interaction: &ModalInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Category Settings] {} submitted modal {}", user_tag, modal_id);

  // Handle format modals first (format: category_sg_modal_{action}_{category_id}_{sg_id})
  // These have two trailing IDs so they must be handled before the generic rsplit extraction.
  if modal_id.starts_with("category_sg_modal_edit_") || modal_id.starts_with("category_sg_modal_add_") {
    return handle_format_modal(ctx, interaction, db, manager, guild_id, modal_id).await;
  }

  // Extract category_id from modal custom_id (format: category_settings_modal_<action>_<category_id>)
  let category_id: u8 = modal_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

  // Get the category by ID
  let mut manager_lock = manager.lock().await;
  let category = {
    let server = manager_lock.get_server(guild_id)?;
    server.categories.iter_mut().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
  };

  if modal_id.starts_with("category_settings_modal_name_") {
    // Extract name value
    let name_str = get_modal_input!(interaction);

    let name = if name_str.trim().is_empty() { None } else { Some(name_str.trim().to_string()) };

    // Update in-memory and build settings while holding lock
    category.name = name.clone();
    let settings = CategorySettings::from_category(category);
    drop(manager_lock);

    // Update in database (after releasing lock)
    db.categories.update_name(guild_id, category_id, name.as_deref()).await?;

    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("category_settings_modal_quota_") {
    // Extract quota value
    let quota_str = get_modal_input!(interaction);

    let quota: u8 = match quota_str.trim().parse() {
      Ok(q) if (2..=100).contains(&q) => q,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid quota. Must be between 2 and 100.").await;
        return Ok(());
      }
    };

    // Update in-memory
    category.set_quota(quota);

    // Update in database
    db.set_category(guild_id, category.channels.category.get(), category.channels.queue_vc.get(), category.channels.dashboard.get(), category.channels.queue_chat.get(), quota).await?;

    // Update dashboard
    category.queue_dash_update(ctx, guild_id).await;

    // Get updated settings and refresh the menu
    refresh_category_settings_modal!(interaction, ctx, category)?;
  } else if modal_id.starts_with("category_settings_modal_timeout_") {
    // Extract timeout value
    let timeout_str = get_modal_input!(interaction);

    let timeout: u16 = match timeout_str.trim().parse() {
      Ok(t) if t > 0 => t,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid timeout. Must be a positive number.").await;
        return Ok(());
      }
    };

    // Update in-memory and persist to database
    category.timeout = timeout;
    db.categories.update_timeout(guild_id, category_id, timeout).await?;

    // Get updated settings and refresh the menu
    refresh_category_settings_modal!(interaction, ctx, category)?;
  } else if modal_id.starts_with("category_settings_modal_connect_") {
    // Extract connect info value
    let connect_str = get_modal_input!(interaction);

    let connect_info = if connect_str.trim().is_empty() { None } else { Some(connect_str.trim().to_string()) };

    // Update in-memory
    category.set_connect_info(connect_info);

    // Update dashboard
    category.queue_dash_update(ctx, guild_id).await;

    // Get updated settings and refresh the menu
    refresh_category_settings_modal!(interaction, ctx, category)?;
  } else {
    warn!("Unknown category settings modal: {}", modal_id);
  }

  Ok(())
}

/// Handle format modal submissions (add and edit)
/// Separated from handle_category_settings_modal because the modal ID format
/// (category_sg_modal_{action}_{category_id}_{sg_id}) has two trailing IDs,
/// which breaks the generic rsplit('_').next() category_id extraction.
async fn handle_format_modal(
  ctx: &Context,
  interaction: &ModalInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
  guild_id: GI,
  modal_id: &str,
) -> Result<()> {
  // Extract name and quota from modal fields
  let name_str = get_modal_input!(interaction, 0);
  let quota_str = get_modal_input!(interaction, 1);

  let name = name_str.trim().to_string();
  if name.is_empty() {
    send_modal_error_response(interaction, ctx, "Format name cannot be empty.").await;
    return Ok(());
  }

  let quota: u8 = match quota_str.trim().parse() {
    Ok(q) if q >= 2 => q,
    _ => {
      send_modal_error_response(interaction, ctx, "Invalid quota. Must be a number >= 2.").await;
      return Ok(());
    }
  };

  if modal_id.starts_with("category_sg_modal_edit_") {
    let suffix = modal_id.strip_prefix("category_sg_modal_edit_").unwrap();
    let parts: Vec<&str> = suffix.split('_').collect();
    if parts.len() != 2 {
      return Err(anyhow::anyhow!("Invalid edit modal ID format"));
    }
    let category_id: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("Invalid category_id"))?;
    let sg_id: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid format_id"))?;

    let mut manager_lock = manager.lock().await;
    let category = {
      let server = manager_lock.get_server(guild_id)?;
      server.categories.iter_mut().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
    };

    if let Some(sg) = category.formats.iter_mut().find(|s| s.id == sg_id) {
      sg.name = name;
      sg.quota = quota;
    } else {
      send_modal_error_response(interaction, ctx, &format!("Format {} not found.", sg_id)).await;
      return Ok(());
    }

    // Persist to DB
    db.categories.save_all_formats(guild_id, category_id, &category.formats).await?;

    // Update dashboard
    category.queue_dash_update(ctx, guild_id).await;

    // Show updated formats list
    let display =
      FormatListDisplay { category_id, category_name: category.display_name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
    let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("category_sg_modal_add_") {
    let category_id: u8 = modal_id.strip_prefix("category_sg_modal_add_").and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid add modal ID format"))?;

    let mut manager_lock = manager.lock().await;
    let category = {
      let server = manager_lock.get_server(guild_id)?;
      server.categories.iter_mut().find(|g| g.category_id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
    };

    match category.add_format(name, quota) {
      Ok(_) => {
        // Persist to DB
        db.categories.save_all_formats(guild_id, category_id, &category.formats).await?;

        // Update dashboard
        category.queue_dash_update(ctx, guild_id).await;

        // Show updated formats list
        let display =
          FormatListDisplay { category_id, category_name: category.display_name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
        let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
        interaction.create_response(&ctx.http, response).await?;
      }
      Err(e) => {
        send_modal_error_response(interaction, ctx, &format!("Failed to add format: {}", e)).await;
      }
    }
  }

  Ok(())
}

/// Handle server-level team balance method selection
pub async fn handle_server_settings_balance_select(
  ctx: &Context,
  interaction: &ComponentInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Server Settings] {} selected team balance method", user_tag);

  // Extract selected value
  let method_str = match &interaction.data.kind {
    CIDK::StringSelect { values } => values.first().ok_or_else(|| anyhow::anyhow!("No value selected"))?.clone(),
    _ => return Err(anyhow::anyhow!("Expected string select interaction")),
  };

  let method = crate::models::TeamBalanceMethod::from_str(&method_str);

  // Update all categories in-memory and persist to database
  let mut manager_lock = manager.lock().await;
  {
    let server = manager_lock.get_server(guild_id)?;
    for category in server.categories.iter_mut() {
      category.team_balance_method = method;
      if let Err(e) = db.categories.update_team_balance_method(guild_id, category.category_id, method).await {
        warn!("Failed to persist team_balance_method for category {}: {e}", category.category_id);
      }
    }
  }
  drop(manager_lock);

  // Return to server settings
  let settings = get_server_settings(db, guild_id).await?;
  let guild_name = guild_name(ctx, guild_id);
  let embed = build_server_settings_embed(&settings, &guild_name);
  let buttons = build_server_settings_buttons(&settings, &guild_name);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

// ============================================================================
// Player Settings (Admin editing of player info)
// ============================================================================

/// Player settings structure for admin editing
pub struct PlayerSettings {
  pub user_id: UI,
  pub username: String,
  pub steam_id: Option<u64>,
  pub elo: u16,
  pub rank: String,
  pub games: u32,
  pub wins: u32,
}

impl PlayerSettings {
  pub fn to_display(&self) -> PlayerSettingsDisplay {
    PlayerSettingsDisplay {
      user_id:  self.user_id,
      username: self.username.clone(),
      steam_id: self.steam_id,
      elo:      self.elo,
      rank:     self.rank.clone(),
      games:    self.games,
      wins:     self.wins,
    }
  }
}

/// Build player settings embed and components (with rank dropdown when ranks exist)
pub async fn nav_player_settings(settings: &PlayerSettings, db: &Arc<Database>, guild_id: GI) -> (CE, Vec<CAR>) {
  build_player_settings_menu(&settings.to_display(), db, guild_id).await
}

/// Handle player settings button interactions
pub async fn handle_player_settings_button(ctx: &Context, interaction: &ComponentInteraction, db: &Arc<Database>) -> Result<()> {
  let button_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Player Settings] {} pressed {}", user_tag, button_id);

  // Extract user_id from button custom_id (format: player_settings_edit_<action>_<user_id>)
  let target_user_id: u64 = button_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid button ID format: {}", button_id))?;

  let target_uid = UI::new(target_user_id);

  // Get current player data (ensure user exists)
  let player = db.users.check_user(target_uid, None).await?;
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let guild_elo = db.elo.get(target_uid, guild_id, db).await?;

  if button_id.starts_with("player_settings_edit_steam_") {
    let modal = CreateModal::new(format!("player_settings_modal_steam_{target_user_id}"), "Edit Steam ID").components(vec![create_short_input_opt("Steam ID (64-bit)", "steam_id", "e.g., 76561198012345678", &player.steam_id.map(|id| id.to_string()).unwrap_or_default())]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_elo_") {
    let modal = CreateModal::new(format!("player_settings_modal_elo_{target_user_id}"), "Edit ELO").components(vec![create_value_input_sh_cap("ELO", "elo", "e.g., 50", &guild_elo.elo.to_string(), 1, 3)]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_rank_") {
    let modal = CreateModal::new(format!("player_settings_modal_rank_{target_user_id}"), "Edit rank").components(vec![create_value_input_sh("Rank", "rank", "e.g., Gold, Silver, Bronze", &guild_elo.rank.name)]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("player_settings_edit_alerts_") {
    // Get target user's current alert settings
    let user_settings = db.users.get_prefs(target_uid).await?;

    let modal = CreateModal::new(format!("player_settings_modal_alerts_{target_user_id}"), "Edit player alerts").components(vec![
      create_short_input_opt("HEX color", "join_alert_color", "e.g., 3447003 or FF5733", &format!("{:06X}", user_settings.join_alert_color)),
      create_paragraph_input_with_value("Join alert message", "join_alert", "e.g., Kafri: defense", &user_settings.join_alert_desc.unwrap_or_default()),
      create_short_input_opt("Join alert footer", "join_alert_footer", "e.g., Good luck!", &user_settings.join_alert_footer.unwrap_or_default()),
      create_paragraph_input_with_value("Leave alert message", "leave_alert", "e.g., See you next time!", &user_settings.leave_alert_desc.unwrap_or_default()),
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
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Player Settings] {} submitted modal {}", user_tag, modal_id);

  // Extract user_id from modal custom_id (format: player_settings_modal_<action>_<user_id>)
  let target_user_id: u64 = modal_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

  let target_uid = UI::new(target_user_id);

  if modal_id.starts_with("player_settings_modal_steam_") {
    let steam_str = get_modal_input!(interaction);

    let steam_id: Option<u64> = if steam_str.trim().is_empty() {
      None
    } else {
      match steam_str.trim().parse::<u64>() {
        Ok(id) => Some(id),
        Err(_) => {
          send_modal_error_response(interaction, ctx, "Invalid Steam ID. Must be a 64-bit number.").await;
          return Ok(());
        }
      }
    };

    db.users.update_steam_id(&target_uid, steam_id).await?;

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_elo_") {
    let elo_str = get_modal_input!(interaction);

    let elo: u16 = match elo_str.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Get current rank and calculate new rank from ELO
    let guild_elo = db.elo.get(target_uid, guild_id, db).await?;
    let old_rank = guild_elo.rank.clone();
    let elo_ranks_linked = db.config.get_elo_ranks_linked(guild_id).await?;

    // Get user tag for logging
    let target_tag = crate::log::get_user_tag(ctx, target_uid, db).await;

    if elo_ranks_linked {
      let new_rank = crate::models::types::Rank::from_elo(db, guild_id, elo).await?;

      // Check if this ELO change would cause a rank change
      if old_rank.role_id != new_rank.role_id {
        // Rank would change - show confirmation prompt
        let username = ctx.http.get_user(target_uid).await.map(|u| u.name.clone()).unwrap_or_else(|_| target_user_id.to_string());

        let confirm_embed = CE::new().title("Rank Change Required").description(format!(
            "Setting **{}'s** ELO to **{}** will change their rank:\n\n\
                        **Current:** {} (ELO {})\n\
                        **New:** {} (ELO {})\n\n\
                        This will update their Discord role from <@&{}> to <@&{}>.\n\n\
                        Do you want to continue?",
            username, elo, old_rank.name, guild_elo.elo, new_rank.name, elo, old_rank.role_id, new_rank.role_id
          )).color(0xFFA500);

        let confirm_buttons = vec![CAR::Buttons(vec![
          CB::new(format!("confirm_elo_change_{}_{}", target_user_id, elo)).label("Confirm").style(BS::Success),
          CB::new(format!("cancel_elo_change_{}", target_user_id)).label("Cancel").style(BS::Danger),
        ])];

        let response = CIR::UpdateMessage(CIRM::new().embed(confirm_embed).components(confirm_buttons));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      // No rank change - proceed with ELO update (rank stays the same)
      db.elo.set(target_uid, guild_id, elo, new_rank.clone()).await?;
      info!("Updated ELO for {} to {} (rank: {})", target_tag, elo, new_rank.name);
    } else {
      // ELO-Rank independent: update ELO only, keep existing rank
      db.elo.set(target_uid, guild_id, elo, old_rank.clone()).await?;
      info!("Updated ELO for {} to {} (rank unchanged: {}, ELO-Rank independent)", target_tag, elo, old_rank.name);
    }

    // Update dashboards where this player is queued
    {
      let mut manager_lock = manager.lock().await;
      if let Ok(server) = manager_lock.get_server(guild_id) {
        let mut found_in_queue = false;
        for category in &server.categories {
          // Check if player is in any session in this category
          let player_in_queue = category.formats[0].sessions.iter().any(|session| session.pool.iter().any(|p| p.player.user_id == target_uid));

          if player_in_queue {
            found_in_queue = true;
            let prefix = crate::log::log_prefix_category(&crate::models::constants::guild_name(ctx, guild_id), &category.display_name());
            info!("{} Player {} ELO changed, updating dashboard", prefix, target_tag);
            category.queue_dash_update(ctx, guild_id).await;
          }
        }
        if !found_in_queue {
          info!("Player {} ELO changed but not found in any queue, no dashboard update needed", target_tag);
        }
      } else {
        let guild_name = crate::models::constants::guild_name(ctx, guild_id);
        warn!("[{}] Failed to get server when checking if player {} is queued", guild_name, target_tag);
      }
    }

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_rank_") {
    let rank_str = get_modal_input!(interaction);

    let new_rank = crate::models::types::Rank::from_name(db, guild_id, rank_str.trim()).await?;

    // Get current data
    let guild_elo = db.elo.get(target_uid, guild_id, db).await?;
    let old_rank = guild_elo.rank;
    let elo_ranks_linked = db.config.get_elo_ranks_linked(guild_id).await?;

    if elo_ranks_linked {
      // Linked: update both rank and ELO to the new rank's base
      db.elo.set(target_uid, guild_id, new_rank.elo, new_rank.clone()).await?;
    } else {
      // Independent: update rank only, keep existing ELO
      db.elo.set(target_uid, guild_id, guild_elo.elo, new_rank.clone()).await?;
    }

    if old_rank.name != new_rank.name {
      info!("Updated rank for {}: {} -> {}{}", target_uid, old_rank.name, new_rank.name, if elo_ranks_linked { "" } else { " (ELO unchanged, independent)" });
    }

    // Update Discord roles
    if let Ok(member) = guild_id.member(&ctx.http, target_uid).await {
      // Remove old rank role
      if member.roles.contains(&old_rank.role_id) {
        if let Err(e) = member.remove_role(&ctx.http, old_rank.role_id).await {
          info!("Failed to remove old rank role {} from user {}: {}", old_rank.role_id, target_uid, e);
        } else {
          info!("Removed rank role {} from user {}", old_rank.name, target_uid);
        }
      }

      // Add new rank role
      if !member.roles.contains(&new_rank.role_id) {
        if let Err(e) = member.add_role(&ctx.http, new_rank.role_id).await {
          info!("Failed to add new rank role {} to user {}: {}", new_rank.role_id, target_uid, e);
        } else {
          info!("Added rank role {} to user {}", new_rank.name, target_uid);
        }
      }
    }

    // Update dashboards where this player is queued
    {
      let mut manager_lock = manager.lock().await;
      if let Ok(server) = manager_lock.get_server(guild_id) {
        let mut found_in_queue = false;
        for category in &server.categories {
          let player_in_queue = category.formats[0].sessions.iter().any(|session| session.pool.iter().any(|p| p.player.user_id == target_uid));
          if player_in_queue {
            found_in_queue = true;
            info!("Player {} rank changed, updating dashboard for category {}", target_uid, category.category_id);
            category.queue_dash_update(ctx, guild_id).await;
          }
        }
        if !found_in_queue {
          info!("Player {} rank changed but not found in any queue, no dashboard update needed", target_uid);
        }
      } else {
        warn!("Failed to get server for guild {} when checking if player {} is queued", guild_id, target_uid);
      }
    }

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("player_settings_modal_alerts_") {
    // Extract values from modal components
    let mut user_settings = db.users.get_prefs(target_uid).await?;

    for (idx, action_row) in interaction.data.components.iter().enumerate() {
      if let Some(ARC::InputText(input)) = action_row.components.first() {
        if let Some(value) = &input.value {
          let trimmed = value.trim();
          match idx {
            0 => {
              // Color field
              if !trimmed.is_empty() {
                let hex_str = trimmed.trim_start_matches('#');
                if let Ok(color) = u32::from_str_radix(hex_str, 16) {
                  if (0..=0xFFFFFF).contains(&color) {
                    user_settings.join_alert_color = color;
                  }
                }
              }
            }
            1 => user_settings.join_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
            2 => user_settings.join_alert_footer = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
            3 => user_settings.leave_alert_desc = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) },
            _ => {}
          }
        }
      }
    }

    // Update target user's settings
    db.users.update_settings(target_uid, &user_settings).await?;

    // Refresh the settings menu
    let settings = get_player_settings!(db, ctx, target_uid, guild_id, target_user_id);

    let (embed, components) = nav_player_settings(&settings, db, guild_id).await;
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
    interaction.create_response(&ctx.http, response).await?;

    info!("[Player Settings] Updated alerts for user {}", target_uid);
  } else {
    warn!("Unknown player settings modal: {}", modal_id);
  }

  Ok(())
}
