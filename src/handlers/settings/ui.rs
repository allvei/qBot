use serenity::all::{
  Context, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE, CreateInputText as CIT, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateSelectMenu as CSM,
  CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO, GuildId as GI, InputTextStyle as ITS, RoleId, ButtonStyle as BS, UserId as UI,
};

use crate::handlers::settings::{get_all_rank_roles, get_rank_settings, get_guild_config, ServerSettings};
use crate::handlers::CategorySettings;
use crate::handlers::settings::menu_system::{get_menu_system, MenuPage};
use crate::handlers::settings::menu::{AsSettingsMenu, CategoryListDisplay, CategorySettingsDisplay, RankConfigDisplay, ServerSettingsDisplay};
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

/// Build settings buttons with guild-specific ping notification toggle
pub async fn build_settings_buttons_with_ping(
  settings: &crate::db::repo::UserPreferences,
  _ctx: &Context,
  db: &Arc<Database>,
  guild_id: GI,
  user_id: UI,
) -> Vec<CAR> {
  use AsSettingsMenu;
  let mut components = settings.as_settings_menu().build_components();

  // Get ping notification state for this server
  let ping_enabled = db.user_server_prefs.get_ping_notification_enabled(user_id, guild_id).await.unwrap_or(None);
  let is_enabled = ping_enabled.unwrap_or(true); // Default to enabled

  // Add ping notification toggle button to the 4th row (index 3)
  if let Some(CAR::Buttons(buttons)) = components.get_mut(3) {
    let ping_button = CB::new("settings_ping_notifications")
      .label(if is_enabled { "Pings enabled" } else { "Pings disabled" })
      .style(if is_enabled { BS::Success } else { BS::Danger });
    buttons.push(ping_button);
  }

  components
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
pub async fn nav_server_config(ctx: &Context, _db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let system = get_menu_system();
  system.build_response(MenuPage::ServerConfig, &guild_name)
    .ok_or_else(|| anyhow::anyhow!("Failed to build server config response"))
}

/// Build a CIR for the roles configuration sub-menu
pub async fn nav_roles_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_guild_config(db, guild_id).await?;
  let system = get_menu_system();

  // Build role select components
  let runner_default = settings.runner_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);
  let admin_default = settings.admin_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);
  let ping_default = settings.ping_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);

  let extra_components = vec![
    CAR::SelectMenu(CSM::new("guild_config_runner_role", CSMK::Role {
      default_roles: runner_default.map(|r| vec![r]),
    }).placeholder("Select runner role")),
    CAR::SelectMenu(CSM::new("guild_config_admin_role", CSMK::Role {
      default_roles: admin_default.map(|r| vec![r]),
    }).placeholder("Select admin role")),
    CAR::SelectMenu(CSM::new("guild_config_ping_role", CSMK::Role {
      default_roles: ping_default.map(|r| vec![r]),
    }).placeholder("Select ping role (empty for @here)").min_values(0).max_values(1)),
  ];

  system.build_response_with_extra(MenuPage::RolesConfig, &guild_name, extra_components)
    .ok_or_else(|| anyhow::anyhow!("Failed to build roles config response"))
}

/// Build a CIR for the ELO configuration sub-menu
pub async fn nav_elo_config(ctx: &Context, db: &Arc<Database>, guild_id: GI, _page: usize) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let system = get_menu_system();

  // Get toggle states for ELO settings (filter SERVER_CONFIG_TOGGLES for ELO-related columns)
  let elo_toggles: Vec<&crate::handlers::settings::menu::ConfigToggle> = crate::handlers::settings::menu::SERVER_CONFIG_TOGGLES
    .iter()
    .filter(|t| t.column.contains("elo"))
    .collect();

  // Build embed using menu system
  let embed = system.build_embed(MenuPage::EloConfig, &guild_name)
    .ok_or_else(|| anyhow::anyhow!("Failed to build ELO config embed"))?;

  // Get base components from menu system (includes "Manage ranks" button and back button)
  let mut components = system.build_components(MenuPage::EloConfig).unwrap_or_default();

  // Add toggle buttons before the back button
  let mut toggle_buttons = Vec::new();
  for toggle in &elo_toggles {
    let mut state = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
    // Invert hide_elo since the column is named "hide_elo" but button shows "ELO is visible" when true
    if toggle.column == "hide_elo" {
      state = !state;
    }
    let label = if state { toggle.label_on } else { toggle.label_off };
    let style = if state { BS::Success } else { BS::Secondary };
    toggle_buttons.push(CB::new(toggle.button_id).label(label).style(style));
  }

  if !toggle_buttons.is_empty() {
    // Insert toggle buttons before the last component (back button)
    if !components.is_empty() {
      components.insert(components.len() - 1, CAR::Buttons(toggle_buttons));
    } else {
      components.push(CAR::Buttons(toggle_buttons));
    }
  }

  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(components)))
}

/// Build a CIR for the general configuration sub-menu
pub async fn nav_general_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let settings = get_guild_config(db, guild_id).await?;
  let system = get_menu_system();

  let system_message_channel = db.config.get_system_message_channel(guild_id).await?.map(|id| id.to_string());
  let community_updates_channel = db.config.get_community_updates_channel(guild_id).await?.map(|id| id.to_string());
  let ping_user_cooldown = db.config.get_ping_user_cooldown(guild_id).await.unwrap_or(30);
  let ping_runner_cooldown = db.config.get_ping_runner_cooldown(guild_id).await.unwrap_or(15);

  let dynamic_data = vec![
    ("Post-game confirm time", format!("{} seconds", settings.post_game_confirm_time)),
    ("Ping user cooldown", format!("{} minutes", ping_user_cooldown)),
    ("Ping runner cooldown", format!("{} minutes", ping_runner_cooldown)),
    ("System message channel", system_message_channel.as_ref().map(|id| format!("<#{}>", id)).unwrap_or_else(|| "Not configured".to_string())),
    ("Community updates channel", community_updates_channel.as_ref().map(|id| format!("<#{}>", id)).unwrap_or_else(|| "Not configured".to_string())),
  ];

  let embed = system.build_embed_with_dynamic(MenuPage::GeneralConfig, &guild_name, &dynamic_data)
    .ok_or_else(|| anyhow::anyhow!("Failed to build general config embed"))?;

  let mut components = system.build_components(MenuPage::GeneralConfig).unwrap_or_default();

  // Add ping_users_enabled toggle button before the back button
  let ping_users_enabled = db.config.get_ping_users_enabled(guild_id).await.unwrap_or(true);
  let ping_toggle_label = if ping_users_enabled { "Users can ping" } else { "Only runners can ping" };
  let ping_toggle_style = if ping_users_enabled { BS::Success } else { BS::Secondary };
  let ping_toggle_button = CB::new("server_cfg_ping_users_enabled").label(ping_toggle_label).style(ping_toggle_style);
  
  // Insert before the last component (back button)
  if !components.is_empty() {
    components.insert(components.len() - 1, CAR::Buttons(vec![ping_toggle_button]));
  } else {
    components.push(CAR::Buttons(vec![ping_toggle_button]));
  }

  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(components)))
}

/// Build a CIR for the VC configuration sub-menu
pub async fn nav_vc_config(ctx: &Context, db: &Arc<Database>, guild_id: GI, _page: usize) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let system = get_menu_system();

  // Get toggle states for VC settings (filter SERVER_CONFIG_TOGGLES for VC-related columns)
  let vc_toggles: Vec<&crate::handlers::settings::menu::ConfigToggle> = crate::handlers::settings::menu::SERVER_CONFIG_TOGGLES
    .iter()
    .filter(|t| t.column.starts_with("default_vc_") || t.column == "post_game_auto_leave")
    .collect();

  // Build embed using menu system
  let embed = system.build_embed(MenuPage::VcConfig, &guild_name)
    .ok_or_else(|| anyhow::anyhow!("Failed to build VC config embed"))?;

  // Get base components from menu system (includes back button)
  let mut components = system.build_components(MenuPage::VcConfig).unwrap_or_default();

  // Add toggle buttons before the back button
  let mut toggle_buttons = Vec::new();
  for toggle in &vc_toggles {
    let state = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
    let label = if state { toggle.label_on } else { toggle.label_off };
    let style = if state { BS::Success } else { BS::Secondary };
    toggle_buttons.push(CB::new(toggle.button_id).label(label).style(style));
  }

  if !toggle_buttons.is_empty() {
    // Insert toggle buttons before the last component (back button)
    if !components.is_empty() {
      components.insert(components.len() - 1, CAR::Buttons(toggle_buttons));
    } else {
      components.push(CAR::Buttons(toggle_buttons));
    }
  }

  Ok(CIR::UpdateMessage(CIRM::new().embed(embed).components(components)))
}

/// Build a CIR navigating back to the rank configuration page
pub async fn nav_rank_config(ctx: &Context, db: &Arc<Database>, guild_id: GI) -> Result<CIR> {
  let guild_name = guild_name(ctx, guild_id);
  let rank_roles = get_all_rank_roles(db, guild_id).await?;
  let (toggle_states, default_rank_role) = get_rank_settings(db, guild_id).await?;
  let display = RankConfigDisplay { guild_name: guild_name.clone(), rank_roles, toggle_states, default_rank_role };
  let embed = display.as_settings_menu().build_embed();
  let buttons = display.build_components();
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
