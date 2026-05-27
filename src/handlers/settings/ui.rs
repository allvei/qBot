use serenity::all::{
  Context, CreateActionRow as CAR, CreateEmbed as CE, CreateInputText as CIT, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateSelectMenu as CSM,
  CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO, GuildId as GI, InputTextStyle as ITS,
};

use crate::handlers::settings::{get_all_rank_roles, get_rank_settings, get_guild_config, ServerSettings};
use crate::handlers::CategorySettings;

use crate::handlers::settings::menu::{AsSettingsMenu, CategoryListDisplay, CategorySettingsDisplay, EloConfigDisplay, GeneralConfigDisplay, RankConfigDisplay, RolesConfigDisplay, ServerConfigDisplay, ServerSettingsDisplay, VcConfigDisplay};
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

/// Build guild config embed
pub fn build_guild_config_embed(settings: &ServerSettings, guild_name: &str) -> CE {
  use {AsSettingsMenu, ServerSettingsDisplay};
  let display = ServerSettingsDisplay {
    guild_name: guild_name.to_string(),
    runner_role: settings.runner_role.clone(),
    admin_role: settings.admin_role.clone(),
    toggle_states: settings.toggle_states.clone(),
    balance_method: settings.balance_method.clone(),
    post_game_confirm_time: settings.post_game_confirm_time,
  };
  display.as_settings_menu().build_embed()
}

/// Build guild config buttons and select menus
pub fn build_guild_config_buttons(settings: &ServerSettings, guild_name: &str) -> Vec<CAR> {
  use {AsSettingsMenu, ServerSettingsDisplay};
  let display = ServerSettingsDisplay {
    guild_name: guild_name.to_string(),
    runner_role: settings.runner_role.clone(),
    admin_role: settings.admin_role.clone(),
    toggle_states: settings.toggle_states.clone(),
    balance_method: settings.balance_method.clone(),
    post_game_confirm_time: settings.post_game_confirm_time,
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
    confirm_time: settings.confirm_time,
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
    confirm_time: 0,
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
      let label = g.name();
      let value = g.id.to_string();
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

/// Build a CIR navigating back to the main guild config page
pub async fn nav_guild_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let settings = get_guild_config(db, guild_id).await?;
  let guild_name = guild_name(ctx, guild_id);
  let embed = build_guild_config_embed(&settings, &guild_name);
  let buttons = build_guild_config_buttons(&settings, &guild_name);
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR navigating back to the server configuration page
pub async fn nav_role_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let display = ServerConfigDisplay { guild_name };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.as_settings_menu().build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR for the roles configuration sub-menu
pub async fn nav_roles_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_guild_config(db, guild_id).await?;
  let display = RolesConfigDisplay {
    guild_name: guild_name.clone(),
    runner_role: settings.runner_role.clone(),
    admin_role: settings.admin_role.clone(),
  };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.as_settings_menu().build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR for the ELO configuration sub-menu
pub async fn nav_elo_config(ctx: &Context, db: &Arc<Database>, guild_id: GI, page: usize) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_guild_config(db, guild_id).await?;

  // Get toggle states for ELO settings (filter SERVER_CONFIG_TOGGLES for ELO-related columns)
  let elo_toggles: Vec<&crate::handlers::settings::menu::ConfigToggle> = crate::handlers::settings::menu::SERVER_CONFIG_TOGGLES
    .iter()
    .filter(|t| t.column.contains("elo"))
    .collect();

  let mut toggle_states = Vec::new();
  for toggle in &elo_toggles {
    let state = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
    toggle_states.push(state);
  }

  let display = EloConfigDisplay {
    guild_name: guild_name.clone(),
    toggle_states,
    balance_method: settings.balance_method.clone(),
    page,
  };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.as_settings_menu().build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR for the general configuration sub-menu
pub async fn nav_general_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_guild_config(db, guild_id).await?;
  let system_message_channel = db.config.get_system_message_channel(guild_id).await?.map(|id| id.to_string());
  let community_updates_channel = db.config.get_community_updates_channel(guild_id).await?.map(|id| id.to_string());
  let display = GeneralConfigDisplay {
    guild_name: guild_name.clone(),
    post_game_confirm_time: settings.post_game_confirm_time,
    system_message_channel,
    community_updates_channel,
  };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.as_settings_menu().build_components();
  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons)))
}

/// Build a CIR for the VC configuration sub-menu
pub async fn nav_vc_config(ctx: &Context, db: &Arc<Database>, guild_id: GI, page: usize) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_guild_config(db, guild_id).await?;

  // Get toggle states for VC settings (filter SERVER_CONFIG_TOGGLES for VC-related columns)
  let vc_toggles: Vec<&crate::handlers::settings::menu::ConfigToggle> = crate::handlers::settings::menu::SERVER_CONFIG_TOGGLES
    .iter()
    .filter(|t| t.column.starts_with("default_vc_"))
    .collect();

  let mut toggle_states = Vec::new();
  for toggle in &vc_toggles {
    let state = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
    toggle_states.push(state);
  }

  let display = VcConfigDisplay {
    guild_name: guild_name.clone(),
    toggle_states,
    page,
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
