//! Server Configuration Menu System (Unified)
//!
//! This module wraps the existing MenuSystem to use the unified menu framework.
//! It ensures all server config buttons are properly registered and handled.

pub use super::menu_system::{MenuPage, MenuSystem, MenuDefinition, MenuButton, MenuColor, ButtonStyle};
use std::sync::OnceLock;

/// Get the global server config menu system
pub fn get_server_config_menu_system() -> &'static MenuSystem {
    static SERVER_CONFIG_MENU_SYSTEM: OnceLock<MenuSystem> = OnceLock::new();
    SERVER_CONFIG_MENU_SYSTEM.get_or_init(MenuSystem::new)
}

/// Verify that all server config buttons have handlers
pub fn verify_server_config_handlers() -> Vec<&'static str> {
    let system = get_server_config_menu_system();
    let mut missing = Vec::new();

    for menu in system.menus.values() {
        for button in &menu.buttons {
            // Navigation buttons don't need handlers
            if button.target_page.is_none() {
                // This button should have a handler
                // For now, we just track it
                missing.push(button.id);
            }
        }
    }

    missing
}

/// Helper to get navigation info from a button ID
pub fn get_server_config_navigation_info(button_id: &str) -> Option<MenuPage> {
    // Check if it's a back button
    if is_server_config_back_button(button_id) {
        return get_server_config_parent_from_back_button(button_id);
    }

    // Check if it's a navigation button
    super::menu_system::get_target_page(button_id)
}

/// Check if a button ID is a back button
pub fn is_server_config_back_button(button_id: &str) -> bool {
    button_id.ends_with("_back") && button_id.starts_with("guild_config_")
}

/// Get the parent page from a back button ID
pub fn get_server_config_parent_from_back_button(button_id: &str) -> Option<MenuPage> {
    match button_id {
        "guild_config_back" => Some(MenuPage::GuildConfig),
        "guild_config_server_back" => Some(MenuPage::ServerConfig),
        "guild_config_roles_back" => Some(MenuPage::RolesConfig),
        "guild_config_elo_back" => Some(MenuPage::EloConfig),
        "guild_config_vc_back" => Some(MenuPage::VcConfig),
        "guild_config_general_back" => Some(MenuPage::GeneralConfig),
        "guild_config_rank_back" => Some(MenuPage::RankConfig),
        "guild_config_category_back" => Some(MenuPage::CategorySettings),
        _ => None,
    }
}
