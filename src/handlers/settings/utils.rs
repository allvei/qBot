use anyhow::Result;
use serenity::all::{
  ComponentInteraction as CI, Context, CreateActionRow as CAR, CreateEmbed as CE, CreateInputText as CIT, CreateInteractionResponse as CIR,
  CreateInteractionResponseMessage as CIRM, GuildId as GI, InputTextStyle as ITS, ModalInteraction as MI, RoleId, UserId as UI,
};
use tracing::error;

#[macro_export]
/// Macro to fetch player data and create PlayerSettings struct
macro_rules! get_player_settings {
  ($db:expr, $ctx:expr, $target_uid:expr, $guild_id:expr, $target_user_id:expr) => {{
    let player = $db.players.check_user($target_uid, None).await?;
    let guild_elo = $db.elo.get($target_uid, $guild_id, $db).await?;
    let username = $ctx.http.get_user($target_uid).await.map(|u| u.name.clone()).unwrap_or_else(|_| $target_user_id.to_string());

    PlayerSettings {
      user_id: $target_uid,
      username,
      steam_id: player.steam_id,
      elo: guild_elo.elo,
      dynamic_elo: guild_elo.dynamic_elo,
      rank: guild_elo.rank.name.clone(),
      games: guild_elo.games,
      wins: guild_elo.wins,
    }
  }};
}

#[macro_export]
/// Macro to extract modal input text value
macro_rules! get_modal_input {
  ($interaction:expr, $index:expr) => {{
    $interaction
      .data
      .components
      .get($index)
      .and_then(|row| row.components.first())
      .and_then(|c| if let ARC::InputText(input) = c { input.value.clone() } else { None })
      .unwrap_or_default()
  }};
  ($interaction:expr) => {{
    get_modal_input!($interaction, 0)
  }};
}

#[macro_export]
/// Macro to refresh category settings and send response for component interactions
macro_rules! refresh_category_settings {
  ($interaction:expr, $ctx:expr, $category:expr) => {{
    let settings = CategorySettings::from_category($category);
    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);
    $crate::send_embed_button_response($interaction, $ctx, embed, buttons).await
  }};
}

#[macro_export]
/// Macro to send navigation response with specific nav function
macro_rules! send_nav {
  ($interaction:expr, $ctx:expr, $db:expr, $nav_fn:expr, $($arg:expr),*) => {{
    send_nav_response($interaction, $ctx, $nav_fn($ctx, $db, $($arg),*).await).await
  }};
}

#[macro_export]
/// Macro to send navigation response for modals
macro_rules! send_nav_modal {
  ($interaction:expr, $ctx:expr, $db:expr, $nav_fn:expr, $($arg:expr),*) => {{
    send_nav_response_modal($interaction, $ctx, $nav_fn($ctx, $db, $($arg),*).await).await
  }};
}

/// Helper function to send navigation response
pub async fn send_nav_response(interaction: &CI, ctx: &Context, response: Result<CIR>) -> Result<()> {
  interaction.create_response(&ctx.http, response?).await?;
  Ok(())
}

/// Helper function to send navigation response for modals
pub async fn send_nav_response_modal(interaction: &MI, ctx: &Context, response: Result<CIR>) -> Result<()> {
  interaction.create_response(&ctx.http, response?).await?;
  Ok(())
}

/// Helper function for sending error responses in component interactions
pub async fn send_component_error_response(interaction: &CI, ctx: &Context, message: &str) {
  let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
  if let Err(e) = interaction.create_response(&ctx.http, response).await {
    error!("Failed to send error response: {e}");
  }
}

/// Helper function for sending error responses in modal interactions
pub async fn send_modal_error_response(interaction: &MI, ctx: &Context, message: &str) {
  let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
  if let Err(e) = interaction.create_response(&ctx.http, response).await {
    error!("Failed to send error response: {e}");
  }
}

/// Helper function to create and send embed/button response
pub async fn send_embed_button_response(interaction: &CI, ctx: &Context, embed: CE, components: Vec<CAR>) -> Result<()> {
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

/// Helper function for modal interactions
pub async fn send_embed_button_response_modal(interaction: &MI, ctx: &Context, embed: CE, components: Vec<CAR>) -> Result<()> {
  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

/// Helper function to get role name with fallback to role ID
pub async fn get_role_name_with_fallback(ctx: &Context, guild_id: GI, role_id: RoleId) -> String {
  guild_id.roles(&ctx.http).await.ok().and_then(|roles| roles.get(&role_id).map(|role| role.name.clone())).unwrap_or_else(|| role_id.get().to_string())
}

/// Track DM activity for cleanup
pub async fn track_dm_activity(ctx: &Context, user_id: UI) {
  if let Some(dm_tracker) = ctx.data.read().await.get::<crate::models::DmTrackerKey>() {
    dm_tracker.update_activity(user_id).await;
  }
}

/// Create a short text input field for modals
pub fn create_input_sh(label: &str, id: &str, placeholder: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).required(true))
}

/// Create a short text input field with value for modals
pub fn create_value_input_sh(label: &str, id: &str, placeholder: &str, value: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).value(value).required(true))
}

/// Create a short text input field with optional value for modals
pub fn create_short_input_opt(label: &str, id: &str, placeholder: &str, value: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).value(value).required(false))
}

/// Create a short text input field with constraints for modals
pub fn create_input_sh_cap(label: &str, id: &str, placeholder: &str, min_len: u16, max_len: u16) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).required(true).min_length(min_len).max_length(max_len))
}

/// Create a short text input field with value and constraints for modals
pub fn create_value_input_sh_cap(label: &str, id: &str, placeholder: &str, value: &str, min_len: u16, max_len: u16) -> CAR {
  CAR::InputText(CIT::new(ITS::Short, label, id).placeholder(placeholder).value(value).required(true).min_length(min_len).max_length(max_len))
}

/// Create a paragraph text input field for modals
pub fn create_paragraph_input(label: &str, id: &str, placeholder: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Paragraph, label, id).placeholder(placeholder).required(false))
}

/// Create a paragraph text input field with value for modals
pub fn create_paragraph_input_with_value(label: &str, id: &str, placeholder: &str, value: &str) -> CAR {
  CAR::InputText(CIT::new(ITS::Paragraph, label, id).placeholder(placeholder).value(value).required(false))
}

/// Create a paragraph text input field with constraints for modals
pub fn create_paragraph_input_constrained(label: &str, id: &str, placeholder: &str, max_len: u16) -> CAR {
  CAR::InputText(CIT::new(ITS::Paragraph, label, id).placeholder(placeholder).required(false).max_length(max_len))
}
