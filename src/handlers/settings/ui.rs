use serenity::all::{
  Context, CreateActionRow as CAR, CreateEmbed as CE,
  CreateInputText as CIT, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK,
  CreateSelectMenuOption as CSMO, GuildId as GI, InputTextStyle as ITS,
};

use crate::handlers::CategorySettings;
use crate::handlers::settings::{get_all_rank_roles, get_rank_settings, get_server_settings, ServerSettings};

use crate::handlers::settings::menu::{AsSettingsMenu, CategoryListDisplay, CategorySettingsDisplay, RankConfigDisplay, ServerConfigDisplay, ServerSettingsDisplay};
use crate::{guild_name, Database};
use anyhow::Result;
use std::sync::Arc;

/// Build settings embed
pub fn build_settings_embed(settings: &crate::db::repo::UserPreferences) -> CE {
  use AsSettingsMenu;
  settings.as_settings_menu().build_embed()
}

/// Build settings buttons
pub fn build_settings_buttons(settings: &crate::db::repo::UserPreferences) -> Vec<CAR> {
  use AsSettingsMenu;
  settings.as_settings_menu().build_components()
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

/// Build category settings embed
pub fn build_category_settings_embed(settings: &CategorySettings) -> CE {
  use {AsSettingsMenu, CategorySettingsDisplay};
  let display = CategorySettingsDisplay {
    category_id: settings.category_id,
    name: settings.name.clone(),
    quota: settings.quota,
    timeout: settings.timeout,
    connect_info: settings.connect_info.clone(),
    format_names: settings.format_names.clone(),
    vc_create: settings.vc_create.clone(),
    vc_destroy: settings.vc_destroy.clone(),
    vc_keep_min: settings.vc_keep_min,
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
  let options: Vec<CSMO> = categories
    .iter()
    .map(|g| {
      let label = g.display_name();
      let value = g.ctg_id.to_string();
      CSMO::new(label, value)
    })
    .collect();

  CAR::SelectMenu(CSM::new("category_settings_select", CSMK::String { options }).placeholder("Select a category...").min_values(1).max_values(1))
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

/// Build a CIR navigating back to the main server settings page
pub async fn nav_server_settings(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let settings = get_server_settings(db, guild_id).await?;
  let guild_name = guild_name(ctx, guild_id);
  let embed = build_server_settings_embed(&settings, &guild_name);
  let buttons = build_server_settings_buttons(&settings, &guild_name);
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR navigating back to the server configuration page
pub async fn nav_role_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_server_settings(db, guild_id).await?;
  let display = ServerConfigDisplay {
    guild_name: guild_name.clone(),
    runner_role: settings.runner_role.clone(),
    admin_role: settings.admin_role.clone(),
    toggle_states: settings.toggle_states.clone(),
    balance_method: settings.balance_method.clone(),
    post_game_timeout: settings.post_game_timeout,
  };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.as_settings_menu().build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR navigating back to the rank configuration page
pub async fn nav_rank_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let rank_roles = get_all_rank_roles(db, guild_id).await?;
  let (toggle_states, default_rank_role) = get_rank_settings(db, guild_id).await?;
  let display = RankConfigDisplay { guild_name: guild_name.clone(), rank_roles, toggle_states, default_rank_role };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.as_settings_menu().build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR navigating back to the category list page
pub async fn nav_category_list(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let categories = db.categories.get_categories_for_guild(guild_id).await?;
  let display = CategoryListDisplay { guild_name, categories };
  let embed = display.build_embed();
  let buttons = display.build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}
