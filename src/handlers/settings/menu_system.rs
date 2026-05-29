//! Centralized Menu System for Configuration
//!
//! This module provides a unified way to define and navigate config menus.
//! It ensures consistent styling, formatting, and navigation across all config pages.

use crate::config_schema::SERVER_CONFIG_DESCRIPTIONS;
use serenity::all::{ButtonStyle as BS, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use std::collections::HashMap;

/// Styling configuration for menus
#[derive(Debug, Clone, Copy)]
pub enum MenuColor {
    /// Discord blurple (0x5865F2) - standard config pages
    Standard = 0x5865F2,
    /// Green (0x57F287) - success/confirmation pages
    Success = 0x57F287,
    /// Red (0xED4245) - danger/warning pages
    Danger = 0xED4245,
    /// Gold (0xFEE75C) - warning/attention pages
    Warning = 0xFEE75C,
    /// Grey (0x99AAB5) - neutral pages
    Neutral = 0x99AAB5,
}

/// Button style configuration
#[derive(Debug, Clone, Copy)]
pub enum ButtonStyle {
    /// Primary action
    Primary,
    /// Secondary action
    Secondary,
    /// Success action
    Success,
    /// Danger action
    Danger,
}

impl ButtonStyle {
    pub fn to_discord_style(self) -> BS {
        match self {
            ButtonStyle::Primary => BS::Primary,
            ButtonStyle::Secondary => BS::Secondary,
            ButtonStyle::Success => BS::Success,
            ButtonStyle::Danger => BS::Danger,
        }
    }
}

/// Menu page identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuPage {
    /// Main guild config overview page (Server/Ranks/Categories buttons)
    GuildConfig,
    /// Server config page (General/Roles/VC/ELO buttons)
    ServerConfig,
    /// Roles configuration sub-menu
    RolesConfig,
    /// ELO configuration page
    EloConfig,
    /// VC configuration page
    VcConfig,
    /// General configuration page
    GeneralConfig,
    /// Rank configuration page
    RankConfig,
    /// Category list page
    CategoryList,
    /// Category settings page
    CategorySettings,
}

/// Menu button definition
#[derive(Debug, Clone)]
pub struct MenuButton {
    pub id: &'static str,
    pub label: &'static str,
    pub description: Option<&'static str>,
    pub target_page: Option<MenuPage>,
}

/// Dynamic field data callback
pub type DynamicFieldCallback = fn(&str) -> Option<String>;

/// Dynamic component callback (for RoleSelect, toggles, etc.)
pub type DynamicComponentCallback = fn(&str) -> Option<CAR>;

/// Menu page definition
#[derive(Debug, Clone)]
pub struct MenuDefinition {
    pub page: MenuPage,
    pub title: &'static str,
    pub description: &'static str,
    pub color: MenuColor,
    pub parent: Option<MenuPage>,
    pub buttons: Vec<MenuButton>,
    pub fields: Vec<(&'static str, &'static str, bool)>, // (name, value, inline)
    pub dynamic_fields: Vec<(&'static str, DynamicFieldCallback, bool)>, // (name, callback, inline)
    pub dynamic_components: Vec<DynamicComponentCallback>, // callbacks for dynamic components
}

/// Menu hierarchy and navigation map
pub struct MenuSystem {
    pub menus: HashMap<MenuPage, MenuDefinition>,
}

impl MenuSystem {
    /// Create a new menu system with all defined menus
    pub fn new() -> Self {
        let mut menus = HashMap::new();

        // Guild Config Page
        menus.insert(MenuPage::GuildConfig, MenuDefinition {
            page: MenuPage::GuildConfig,
            title: "Guild Configuration",
            description: "**Configuration Overview:**\n\n**Server-wide settings**\n• Roles (runner/admin permissions)\n• Team balance method\n• ELO & Rank linking\n\n**Rank management**\n• Add, remove & link ranks\n• Set default rank\n\n**Category management**\n• Queue channels & voice channels\n• Team channels & game settings",
            color: MenuColor::Standard,
            parent: None,
            buttons: vec![
                MenuButton {
                    id: "guild_config_roles",
                    label: "Server",
                    description: Some("Configure server-wide settings like roles, team balance, and ELO"),
                    target_page: Some(MenuPage::ServerConfig),
                },
                MenuButton {
                    id: "guild_config_ranks",
                    label: "Ranks",
                    description: Some("Manage rank roles and ELO thresholds"),
                    target_page: Some(MenuPage::RankConfig),
                },
                MenuButton {
                    id: "guild_config_categories",
                    label: "Categories",
                    description: Some("Manage queue categories and their settings"),
                    target_page: Some(MenuPage::CategoryList),
                },
            ],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Server Config Page
        menus.insert(MenuPage::ServerConfig, MenuDefinition {
            page: MenuPage::ServerConfig,
            title: "Server Configuration",
            description: "Configure server-wide settings",
            color: MenuColor::Standard,
            parent: Some(MenuPage::GuildConfig),
            buttons: vec![
                MenuButton {
                    id: "guild_config_general_menu",
                    label: "General",
                    description: Some("Configure general server settings like post-game confirm time and gamemode"),
                    target_page: Some(MenuPage::GeneralConfig),
                },
                MenuButton {
                    id: "guild_config_roles_menu",
                    label: "Roles",
                    description: Some("Configure runner, admin, and ping roles for permissions and notifications"),
                    target_page: Some(MenuPage::RolesConfig),
                },
                MenuButton {
                    id: "guild_config_elo_menu",
                    label: "ELO",
                    description: Some("Configure ELO calculations, rank linking, and dynamic ELO settings"),
                    target_page: Some(MenuPage::EloConfig),
                },
                MenuButton {
                    id: "guild_config_vc_menu",
                    label: "Voice Chat",
                    description: Some("Configure voice channel settings like auto-join, auto-leave, and VC policies"),
                    target_page: Some(MenuPage::VcConfig),
                },
            ],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Roles Config Page
        menus.insert(MenuPage::RolesConfig, MenuDefinition {
            page: MenuPage::RolesConfig,
            title: "Roles Configuration",
            description: "Configure runner, admin, and ping roles",
            color: MenuColor::Standard,
            parent: Some(MenuPage::ServerConfig),
            buttons: vec![
                MenuButton {
                    id: "guild_config_create_ping_role",
                    label: "Create ping role",
                    description: Some("Create a new Discord role that will be pinged instead of @here"),
                    target_page: None,
                },
            ],
            fields: vec![
                ("Runner Role", "Select runner role", true),
                ("Admin Role", "Select admin role", true),
                ("Ping Role", "Select ping role (empty for @here)", true),
            ],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // ELO Config Page
        menus.insert(MenuPage::EloConfig, MenuDefinition {
            page: MenuPage::EloConfig,
            title: "ELO Configuration",
            description: "Configure ELO and rank settings",
            color: MenuColor::Standard,
            parent: Some(MenuPage::ServerConfig),
            buttons: vec![],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // VC Config Page
        menus.insert(MenuPage::VcConfig, MenuDefinition {
            page: MenuPage::VcConfig,
            title: "Voice Chat Configuration",
            description: "Configure voice channel settings",
            color: MenuColor::Standard,
            parent: Some(MenuPage::ServerConfig),
            buttons: vec![],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // General Config Page
        menus.insert(MenuPage::GeneralConfig, MenuDefinition {
            page: MenuPage::GeneralConfig,
            title: "General Configuration",
            description: "Configure general server settings",
            color: MenuColor::Standard,
            parent: Some(MenuPage::ServerConfig),
            buttons: vec![
                MenuButton {
                    id: "guild_config_edit_post_game_confirm_time",
                    label: "Edit timeout",
                    description: Some("Change the time in seconds to wait for post-game confirmation"),
                    target_page: None,
                },
                MenuButton {
                    id: "guild_config_edit_gamemode",
                    label: "Edit gamemode",
                    description: Some("Set the current gamemode for the server"),
                    target_page: None,
                },
                MenuButton {
                    id: "guild_config_edit_system_msg_channel",
                    label: "Edit system msg channel",
                    description: Some("Configure the channel for system message broadcasts"),
                    target_page: None,
                },
                MenuButton {
                    id: "guild_config_edit_community_updates_channel",
                    label: "Edit community updates channel",
                    description: Some("Configure the channel for community update broadcasts"),
                    target_page: None,
                },
            ],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Rank Config Page
        menus.insert(MenuPage::RankConfig, MenuDefinition {
            page: MenuPage::RankConfig,
            title: "Rank Configuration",
            description: "Configure rank settings",
            color: MenuColor::Standard,
            parent: Some(MenuPage::EloConfig),
            buttons: vec![],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Category List Page
        menus.insert(MenuPage::CategoryList, MenuDefinition {
            page: MenuPage::CategoryList,
            title: "Category List",
            description: "Select a category to manage",
            color: MenuColor::Standard,
            parent: Some(MenuPage::ServerConfig),
            buttons: vec![],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Category Settings Page
        menus.insert(MenuPage::CategorySettings, MenuDefinition {
            page: MenuPage::CategorySettings,
            title: "Category Settings",
            description: "Configure category-specific settings",
            color: MenuColor::Standard,
            parent: Some(MenuPage::CategoryList),
            buttons: vec![],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        Self { menus }
    }

    /// Get the menu definition for a given page
    pub fn get_menu(&self, page: MenuPage) -> Option<&MenuDefinition> {
        self.menus.get(&page)
    }

    /// Get the parent page for a given page
    pub fn get_parent(&self, page: MenuPage) -> Option<MenuPage> {
        self.menus.get(&page).and_then(|m| m.parent)
    }

    /// Build an embed for a given menu page
    pub fn build_embed(&self, page: MenuPage, guild_name: &str) -> Option<CE> {
        self.build_embed_with_dynamic(page, guild_name, &[])
    }

    /// Build an embed for a given menu page with dynamic field data
    pub fn build_embed_with_dynamic(&self, page: MenuPage, guild_name: &str, dynamic_data: &[(&str, String)]) -> Option<CE> {
        let menu = self.get_menu(page)?;
        let mut embed = CE::new()
            .title(format!("{} - {}", guild_name, menu.title))
            .description(menu.description)
            .color(menu.color as u32);

        // Add static fields
        for (name, value, inline) in &menu.fields {
            embed = embed.field(*name, *value, *inline);
        }

        // Add dynamic fields
        for (name, callback, inline) in &menu.dynamic_fields {
            if let Some(value) = callback(guild_name) {
                embed = embed.field(*name, value, *inline);
            }
        }

        // Add help section with button descriptions
        let help_text: Vec<String> = menu.buttons.iter()
            .filter(|b| b.label != "Back")
            .filter_map(|b| b.description.map(|desc| format!("**{}**: {}", b.label, desc)))
            .collect();

        if !help_text.is_empty() {
            embed = embed.field("Help", help_text.join("\n"), false);
        }

        Some(embed)
    }

    /// Build components (buttons) for a given menu page
    pub fn build_components(&self, page: MenuPage) -> Option<Vec<CAR>> {
        let menu = self.get_menu(page)?;

        if menu.buttons.is_empty() {
            return None;
        }

        // Split buttons into rows of 5 (Discord limit)
        let mut rows = Vec::new();
        let mut current_row = Vec::new();

        for button in &menu.buttons {
            let style = if button.label == "Back" {
                BS::Secondary
            } else {
                BS::Primary
            };
            current_row.push(CB::new(button.id).label(button.label).style(style));

            if current_row.len() == 5 {
                rows.push(CAR::Buttons(current_row.clone()));
                current_row.clear();
            }
        }

        if !current_row.is_empty() {
            rows.push(CAR::Buttons(current_row));
        }

        Some(rows)
    }

    /// Build components with additional dynamic components
    pub fn build_components_with_extra(&self, page: MenuPage, extra_components: Vec<CAR>) -> Vec<CAR> {
        let mut components = self.build_components(page).unwrap_or_default();
        components.extend(extra_components);
        components
    }

    /// Build a complete interaction response for a given menu page
    pub fn build_response(&self, page: MenuPage, guild_name: &str) -> Option<CIR> {
        let embed = self.build_embed(page, guild_name)?;
        let components = self.build_components(page).unwrap_or_default();

        Some(CIR::UpdateMessage(CIRM::new().embed(embed).components(components)))
    }

    /// Build a complete interaction response with extra components
    pub fn build_response_with_extra(&self, page: MenuPage, guild_name: &str, extra_components: Vec<CAR>) -> Option<CIR> {
        let embed = self.build_embed(page, guild_name)?;
        let components = self.build_components_with_extra(page, extra_components);

        Some(CIR::UpdateMessage(CIRM::new().embed(embed).components(components)))
    }

    /// Populate button descriptions from config_schema
    pub fn populate_descriptions(&mut self) {
        for menu in self.menus.values_mut() {
            for button in &mut menu.buttons {
                // Try to find description in SERVER_CONFIG_DESCRIPTIONS
                for (column, desc) in SERVER_CONFIG_DESCRIPTIONS {
                    if button.id.contains(column) {
                        button.description = Some(desc);
                        break;
                    }
                }
            }

            // Auto-add back button if parent exists and not already present
            if let Some(parent) = menu.parent {
                let has_back = menu.buttons.iter().any(|b| b.label == "Back");
                if !has_back {
                    // Generate unique back button ID based on parent page
                    let back_id = match parent {
                        MenuPage::GuildConfig => "guild_config_back",
                        MenuPage::ServerConfig => "guild_config_server_back",
                        MenuPage::RolesConfig => "guild_config_roles_back",
                        MenuPage::EloConfig => "guild_config_elo_back",
                        MenuPage::VcConfig => "guild_config_vc_back",
                        MenuPage::GeneralConfig => "guild_config_general_back",
                        MenuPage::RankConfig => "guild_config_rank_back",
                        MenuPage::CategoryList => "guild_config_categories_back",
                        MenuPage::CategorySettings => "guild_config_category_back",
                    };
                    menu.buttons.push(MenuButton {
                        id: back_id,
                        label: "Back",
                        description: Some("Return to previous menu"),
                        target_page: Some(parent),
                    });
                }
            }
        }
    }
}

/// Global menu system instance
pub static MENU_SYSTEM: std::sync::OnceLock<MenuSystem> = std::sync::OnceLock::new();

/// Get the global menu system
pub fn get_menu_system() -> &'static MenuSystem {
    MENU_SYSTEM.get_or_init(|| {
        let mut system = MenuSystem::new();
        system.populate_descriptions();
        system
    })
}

/// Navigate to a menu page based on button ID
/// Returns the target page if the button ID is found in the menu system
pub fn get_target_page(button_id: &str) -> Option<MenuPage> {
    let system = get_menu_system();
    for menu in system.menus.values() {
        if let Some(button) = menu.buttons.iter().find(|b| b.id == button_id) {
            return button.target_page;
        }
    }
    None
}

/// Check if a button ID is a back button
pub fn is_back_button(button_id: &str) -> bool {
    button_id.ends_with("_back")
}

/// Get the back button ID for a given parent page
pub fn get_back_button_id(parent: MenuPage) -> &'static str {
    match parent {
        MenuPage::GuildConfig => "guild_config_back",
        MenuPage::ServerConfig => "guild_config_server_back",
        MenuPage::RolesConfig => "guild_config_roles_back",
        MenuPage::EloConfig => "guild_config_elo_back",
        MenuPage::VcConfig => "guild_config_vc_back",
        MenuPage::GeneralConfig => "guild_config_general_back",
        MenuPage::RankConfig => "guild_config_rank_back",
        MenuPage::CategoryList => "guild_config_categories_back",
        MenuPage::CategorySettings => "guild_config_category_back",
    }
}

/// Add a back button row to components
pub fn add_back_button(components: &mut Vec<CAR>, parent: MenuPage) {
    add_back_button_with_label(components, parent, "Back");
}

/// Add a back button row to components with custom label
pub fn add_back_button_with_label(components: &mut Vec<CAR>, parent: MenuPage, label: &str) {
    use serenity::all::{CreateButton as CB, ButtonStyle as BS};
    let back_id = get_back_button_id(parent);
    components.push(CAR::Buttons(vec![CB::new(back_id).label(label).style(BS::Secondary)]));
}

/// Handle navigation based on button ID using the menu system
/// Returns (nav_function_name, page_number) if the button should navigate
pub fn get_navigation_info(button_id: &str) -> Option<(&'static str, Option<usize>)> {
    // First check if it's a back button that's not in the menu system
    if is_back_button(button_id) {
        // Map back button IDs to the page they navigate TO
        let target = match button_id {
            "guild_config_back" => MenuPage::GuildConfig,
            "guild_config_server_back" => MenuPage::ServerConfig,
            "guild_config_roles_back" => MenuPage::RolesConfig,
            "guild_config_elo_back" => MenuPage::EloConfig,
            "guild_config_vc_back" => MenuPage::VcConfig,
            "guild_config_general_back" => MenuPage::GeneralConfig,
            "guild_config_rank_back" => MenuPage::RankConfig,
            "guild_config_categories_back" => MenuPage::CategoryList,
            "guild_config_category_back" => MenuPage::CategorySettings,
            _ => return None,
        };
        return match target {
            MenuPage::ServerConfig => Some(("nav_server_config", None)),
            MenuPage::RolesConfig => Some(("nav_roles_config", None)),
            MenuPage::EloConfig => Some(("nav_elo_config", Some(0))),
            MenuPage::VcConfig => Some(("nav_vc_config", Some(0))),
            MenuPage::GeneralConfig => Some(("nav_general_config", None)),
            MenuPage::RankConfig => Some(("nav_rank_config", None)),
            MenuPage::CategoryList => Some(("nav_category_list", None)),
            MenuPage::GuildConfig => Some(("nav_guild_config", None)),
            MenuPage::CategorySettings => None,
        };
    }

    // Then check menu system for other buttons
    let target_page = get_target_page(button_id)?;
    match target_page {
        MenuPage::ServerConfig => Some(("nav_server_config", None)),
        MenuPage::RolesConfig => Some(("nav_roles_config", None)),
        MenuPage::EloConfig => Some(("nav_elo_config", Some(0))),
        MenuPage::VcConfig => Some(("nav_vc_config", Some(0))),
        MenuPage::GeneralConfig => Some(("nav_general_config", None)),
        MenuPage::RankConfig => Some(("nav_rank_config", None)),
        MenuPage::CategoryList => Some(("nav_category_list", None)),
        MenuPage::GuildConfig => Some(("nav_guild_config", None)),
        MenuPage::CategorySettings => None,
    }
}
