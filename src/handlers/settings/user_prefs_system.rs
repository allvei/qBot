//! User Preferences Menu System (Unified)
//!
//! This module uses the unified menu system to define user preference menus
//! with compile-time guarantees that all buttons are properly handled.

use crate::handlers::settings::unified_menu::ButtonType;
use crate::db::repo::UserPreferences;
use serenity::all::{ButtonStyle as BS, CreateActionRow as CAR, CreateButton as CB};

/// User preference page identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserPrefsPage {
    /// Main user preferences overview page
    Main,
    /// Queue settings page (timeout, auto-join, auto-leave, etc.)
    QueueSettings,
    /// Queue timeout selection page
    QueueTimeoutSettings,
    /// Alert settings page (join/leave alerts, DM alerts)
    AlertSettings,
    /// Ping notifications page (per-server ping preferences)
    PingSettings,
}

// Dynamic component callbacks
fn dm_toggle_component(prefs: &UserPreferences) -> Option<CAR> {
    Some(CAR::Buttons(vec![
        CB::new("settings_toggle_dm")
            .label("DM Alerts")
            .style(if prefs.pm_hot_alert { BS::Success } else { BS::Danger }),
    ]))
}

fn vc_auto_join_component(prefs: &UserPreferences) -> Option<CAR> {
    Some(CAR::Buttons(vec![
        CB::new("settings_vc_auto_join")
            .label("VC Auto-join")
            .style(if prefs.vc_auto_join { BS::Success } else { BS::Danger }),
    ]))
}

fn vc_auto_leave_component(prefs: &UserPreferences) -> Option<CAR> {
    Some(CAR::Buttons(vec![
        CB::new("settings_vc_auto_leave")
            .label("VC Auto-leave")
            .style(if prefs.vc_auto_leave { BS::Success } else { BS::Danger }),
    ]))
}

fn vc_leave_queue_component(prefs: &UserPreferences) -> Option<CAR> {
    Some(CAR::Buttons(vec![
        CB::new("settings_vc_leave_queue")
            .label("Leave Queue on VC Disconnect")
            .style(if prefs.vc_leave_queue { BS::Success } else { BS::Danger }),
    ]))
}

fn queue_timeout_buttons_component(_prefs: &UserPreferences) -> Option<CAR> {
    Some(CAR::Buttons(vec![
        CB::new("settings_queue_expiration:30m").label("30 minutes").style(BS::Primary),
        CB::new("settings_queue_expiration:1h").label("1 hour").style(BS::Primary),
        CB::new("settings_queue_expiration:2h").label("2 hours").style(BS::Primary),
    ]))
}

fn queue_timeout_cancel_component(_prefs: &UserPreferences) -> Option<CAR> {
    Some(CAR::Buttons(vec![
        CB::new("settings_queue_expiration:3h").label("3 hours").style(BS::Primary),
        CB::new("settings_queue_expiration:4h").label("4 hours").style(BS::Primary),
        CB::new("settings_queue_expiration:cancel").label("Cancel").style(BS::Secondary),
    ]))
}

// Dynamic field callbacks
fn queue_timeout_field(prefs: &UserPreferences) -> Option<String> {
    let text = if prefs.queue_expiration >= 60 && prefs.queue_expiration % 60 == 0 {
        format!("{}h", prefs.queue_expiration / 60)
    } else {
        format!("{}m", prefs.queue_expiration)
    };
    Some(text)
}

// Placeholder for menu system - will be populated by macro
// The macro ensures all buttons are defined and have handlers
pub struct UserPrefsMenuSystem {
    pub inner: crate::handlers::settings::unified_menu::MenuSystem<UserPrefsPage, UserPreferences>,
}

impl UserPrefsMenuSystem {
    pub fn new() -> Self {
        let mut inner = crate::handlers::settings::unified_menu::MenuSystem::new();

        // Register Main page
        inner.add_page(crate::handlers::settings::unified_menu::MenuDefinition {
            page: UserPrefsPage::Main,
            title: "qBot Preferences",
            description: "Configure your queue preferences and notification settings",
            color: 0x5865F2,
            parent: None,
            buttons: vec![
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "user_prefs_queue_settings",
                    label: "Queue Settings",
                    description: Some("Configure queue timeout, auto-join, and auto-leave settings"),
                    target_page: Some(UserPrefsPage::QueueSettings),
                    button_type: ButtonType::Nav,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "user_prefs_alert_settings",
                    label: "Alert Settings",
                    description: Some("Configure join/leave alerts and DM notifications"),
                    target_page: Some(UserPrefsPage::AlertSettings),
                    button_type: ButtonType::Nav,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "user_prefs_ping_settings",
                    label: "Ping Settings",
                    description: Some("Configure ping notifications for each server"),
                    target_page: Some(UserPrefsPage::PingSettings),
                    button_type: ButtonType::Nav,
                },
            ],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Register QueueSettings page
        inner.add_page(crate::handlers::settings::unified_menu::MenuDefinition {
            page: UserPrefsPage::QueueSettings,
            title: "Queue Settings",
            description: "Configure queue behavior and voice channel settings",
            color: 0x5865F2,
            parent: Some(UserPrefsPage::Main),
            buttons: vec![
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_queue_expiration",
                    label: "Queue Timeout",
                    description: Some("Set how long before you're automatically removed from the queue"),
                    target_page: Some(UserPrefsPage::QueueTimeoutSettings),
                    button_type: ButtonType::Nav,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_vc_auto_join",
                    label: "VC Auto-join",
                    description: Some("Automatically join voice channel when joining queue"),
                    target_page: None,
                    button_type: ButtonType::Toggle,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_vc_auto_leave",
                    label: "VC Auto-leave",
                    description: Some("Automatically leave voice channel when leaving queue"),
                    target_page: None,
                    button_type: ButtonType::Toggle,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_vc_leave_queue",
                    label: "Leave Queue on VC Disconnect",
                    description: Some("Automatically leave queue when disconnecting from voice channel"),
                    target_page: None,
                    button_type: ButtonType::Toggle,
                },
            ],
            fields: vec![],
            dynamic_fields: vec![
                ("Queue Timeout", queue_timeout_field, true),
            ],
            dynamic_components: vec![
                vc_auto_join_component,
                vc_auto_leave_component,
                vc_leave_queue_component,
            ],
        });

        // Register QueueTimeoutSettings page
        inner.add_page(crate::handlers::settings::unified_menu::MenuDefinition {
            page: UserPrefsPage::QueueTimeoutSettings,
            title: "Queue Timeout",
            description: "Select how long before you're automatically removed from the queue",
            color: 0x5865F2,
            parent: Some(UserPrefsPage::QueueSettings),
            buttons: vec![],
            fields: vec![],
            dynamic_fields: vec![
                ("Current Value", queue_timeout_field, true),
            ],
            dynamic_components: vec![
                queue_timeout_buttons_component,
                queue_timeout_cancel_component,
            ],
        });

        // Register AlertSettings page
        inner.add_page(crate::handlers::settings::unified_menu::MenuDefinition {
            page: UserPrefsPage::AlertSettings,
            title: "Alert Settings",
            description: "Configure custom alerts and notification preferences",
            color: 0x5865F2,
            parent: Some(UserPrefsPage::Main),
            buttons: vec![
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_toggle_dm",
                    label: "DM Alerts",
                    description: Some("Receive DM notifications when games are ready"),
                    target_page: None,
                    button_type: ButtonType::Toggle,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_edit_alert",
                    label: "Edit Join Alert",
                    description: Some("Customize your join announcement embed"),
                    target_page: None,
                    button_type: ButtonType::Edit,
                },
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_edit_leave_alert",
                    label: "Edit Leave Alert",
                    description: Some("Customize your leave announcement embed"),
                    target_page: None,
                    button_type: ButtonType::Edit,
                },
            ],
            fields: vec![],
            dynamic_fields: vec![],
            dynamic_components: vec![
                dm_toggle_component,
            ],
        });

        // Register PingSettings page
        inner.add_page(crate::handlers::settings::unified_menu::MenuDefinition {
            page: UserPrefsPage::PingSettings,
            title: "Ping Settings",
            description: "Configure ping notifications for each server",
            color: 0x5865F2,
            parent: Some(UserPrefsPage::Main),
            buttons: vec![
                crate::handlers::settings::unified_menu::MenuButton {
                    id: "settings_ping_notifications",
                    label: "Ping Notifications",
                    description: Some("Toggle ping notifications for this server"),
                    target_page: None,
                    button_type: ButtonType::Toggle,
                },
            ],
            fields: vec![
                ("Note", "Ping settings are configured per-server. This toggle works when accessed from a server context (dashboard).", true),
            ],
            dynamic_fields: vec![],
            dynamic_components: vec![],
        });

        // Register handlers for non-nav buttons
        inner.register_handler("settings_toggle_dm", "handle_toggle_dm");
        inner.register_handler("settings_edit_alert", "handle_edit_alert");
        inner.register_handler("settings_edit_leave_alert", "handle_edit_leave_alert");
        inner.register_handler("settings_vc_auto_join", "handle_vc_auto_join");
        inner.register_handler("settings_vc_auto_leave", "handle_vc_auto_leave");
        inner.register_handler("settings_vc_leave_queue", "handle_vc_leave_queue");
        inner.register_handler("settings_queue_expiration:*", "handle_queue_expiration");
        inner.register_handler("settings_ping_notifications", "handle_ping_notifications");

        Self { inner }
    }

    pub fn get_menu(&self, page: UserPrefsPage) -> Option<&crate::handlers::settings::unified_menu::MenuDefinition<UserPrefsPage, UserPreferences>> {
        self.inner.get_menu(page)
    }

    pub fn get_parent(&self, page: UserPrefsPage) -> Option<UserPrefsPage> {
        self.inner.get_parent(page)
    }

    pub fn get_target_page(&self, button_id: &str) -> Option<UserPrefsPage> {
        self.inner.get_target_page(button_id)
    }

    pub fn build_embed(&self, page: UserPrefsPage, prefs: &UserPreferences) -> Option<CE> {
        self.inner.build_embed(page, prefs)
    }

    pub fn build_components(&self, page: UserPrefsPage, prefs: &UserPreferences) -> Option<Vec<CAR>> {
        self.inner.build_components(page, prefs)
    }

    pub fn build_response(&self, page: UserPrefsPage, prefs: &UserPreferences) -> Option<CIR> {
        self.inner.build_response(page, prefs)
    }

    pub fn get_back_button_id(&self, parent: UserPrefsPage) -> &'static str {
        match parent {
            UserPrefsPage::Main => "user_prefs_main_back",
            UserPrefsPage::QueueSettings => "user_prefs_queue_back",
            UserPrefsPage::QueueTimeoutSettings => "user_prefs_queue_back",
            UserPrefsPage::AlertSettings => "user_prefs_alert_back",
            UserPrefsPage::PingSettings => "user_prefs_ping_back",
        }
    }

    pub fn verify_handlers(&self) -> Vec<&'static str> {
        self.inner.verify_handlers()
    }
}

use serenity::all::{CreateInteractionResponse as CIR, CreateEmbed as CE};

/// Helper functions for navigation
pub fn get_user_prefs_target_page(button_id: &str) -> Option<UserPrefsPage> {
    get_user_prefs_menu_system().inner.get_target_page(button_id)
}

pub fn is_user_prefs_back_button(button_id: &str) -> bool {
    button_id.ends_with("_back") && button_id.starts_with("user_prefs_")
}

pub fn get_user_prefs_parent_from_back_button(button_id: &str) -> Option<UserPrefsPage> {
    match button_id {
        "user_prefs_main_back" => Some(UserPrefsPage::Main),
        "user_prefs_queue_back" => Some(UserPrefsPage::QueueSettings),
        "user_prefs_alert_back" => Some(UserPrefsPage::AlertSettings),
        "user_prefs_ping_back" => Some(UserPrefsPage::PingSettings),
        _ => None,
    }
}

pub fn get_user_prefs_navigation_info(button_id: &str) -> Option<UserPrefsPage> {
    if is_user_prefs_back_button(button_id) {
        get_user_prefs_parent_from_back_button(button_id)
    } else {
        get_user_prefs_target_page(button_id)
    }
}

use std::sync::OnceLock;

static USER_PREFS_MENU_SYSTEM: OnceLock<UserPrefsMenuSystem> = OnceLock::new();

pub fn get_user_prefs_menu_system() -> &'static UserPrefsMenuSystem {
    USER_PREFS_MENU_SYSTEM.get_or_init(UserPrefsMenuSystem::new)
}
