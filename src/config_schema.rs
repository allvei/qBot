//! Declarative Configuration Schema
//!
//! This module provides a single source of truth for all configuration options.
//! Using macros, it automatically generates:
//! - Database migrations
//! - Get/set methods for repositories
//! - UI display components
//! - Toggle buttons for boolean settings
//!
//! To add a new configuration option, simply add an entry to the appropriate
//! macro invocation. Everything else is generated automatically.
//!
//! ## Discord Limits
//! 
//! Discord has a limit of **5 buttons per action row**. The settings menu
//! automatically splits toggle buttons into multiple rows when needed.
//! You can add as many boolean toggles as required without worrying about this limit.

macro_rules! define_server_config {
    (
        $(
            $field:ident: $type:ty {
                column: $column:literal,
                default: $default:expr,
                display: $display_name:literal,
                $(button: $button_id:literal,)?
                $(labels: [$label_on:literal, $label_off:literal],)?
                description: $description:literal,
            }
        ),* $(,)?
    ) => {
        pub mod server_config {
            pub const COLUMNS: &[(&str, &str, &str)] = &[
                $(
                    ($column, stringify!($type), stringify!($default)),
                )*
            ];

            pub const TOGGLES: &[crate::handlers::settings::menu::ConfigToggle] = &[
                $(
                    $(
                        crate::handlers::settings::menu::ConfigToggle {
                            column: $column,
                            button_id: $button_id,
                            label_on: $label_on,
                            label_off: $label_off,
                            default: $default,
                        },
                    )?
                )*
            ];

            pub const DESCRIPTIONS: &[(&str, &str)] = &[
                $(
                    ($column, $description),
                )*
            ];
        }
    };
}

macro_rules! define_category_config {
    (
        $(
            $field:ident: $type:ty {
                column: $column:literal,
                default: $default:expr,
                display: $display_name:literal,
                $(button_prefix: $button_prefix:literal,)?
                $(labels: [$label_on:literal, $label_off:literal],)?
                description: $description:literal,
            }
        ),* $(,)?
    ) => {
        pub mod category_config {
            pub const COLUMNS: &[(&str, &str, &str)] = &[
                $(
                    ($column, stringify!($type), stringify!($default)),
                )*
            ];

            pub const TOGGLES: &[(&str, &str, &str, &str, bool)] = &[
                $(
                    $(
                        ($column, $button_prefix, $label_on, $label_off, $default),
                    )?
                )*
            ];
        }
    };
}

macro_rules! define_user_preferences {
    (
        $(
            $field:ident: $type:ty {
                column: $column:literal,
                global_table: $global_table:literal,
                override_table: $override_table:literal,
                default: $default:expr,
                display: $display_name:literal,
                $(button: $button_id:literal,)?
                $(labels: [$label_on:literal, $label_off:literal],)?
                description: $description:literal,
            }
        ),* $(,)?
    ) => {
        pub mod user_preferences {
            pub const COLUMNS: &[(&str, &str, &str, &str, &str)] = &[
                $(
                    ($column, stringify!($type), $global_table, $override_table, stringify!($default)),
                )*
            ];

            pub const TOGGLES: &[(&str, &str, &str, &str, bool)] = &[
                $(
                    $(
                        ($column, $button_id, $label_on, $label_off, $default),
                    )?
                )*
            ];
        }
    };
}

define_server_config! {
    elo_ranks_linked: bool {
        column: "elo_ranks_linked",
        default: true,
        display: "ELO-Rank Linking",
        button: "server_cfg_elo_ranks_linked",
        labels: ["ELO-Rank linked", "ELO-Rank independent"],
        description: "Link ELO to rank roles automatically",
    },
    active_elo: bool {
        column: "active_elo",
        default: false,
        display: "Dynamic ELO",
        button: "guild_config_dynamic_elo",
        labels: ["Dynamic ELO enabled", "Dynamic ELO disabled"],
        description: "Enable dynamic ELO calculations",
    },
    hide_elo: bool {
        column: "hide_elo",
        default: false,
        display: "Hide ELO",
        button: "server_cfg_hide_elo",
        labels: ["ELO is visible", "ELO is hidden"],
        description: "Hide ELO values from players",
    },
    post_game_auto_leave: bool {
        column: "post_game_auto_leave",
        default: true,
        display: "Post-game Auto-remove",
        button: "server_cfg_post_game_auto_leave",
        labels: ["Post-game auto-remove is enabled", "Post-game auto-remove is disabled"],
        description: "Automatically remove players from queue after game ends",
    },
    default_vc_auto_join: bool {
        column: "default_vc_auto_join",
        default: false,
        display: "Default VC auto-join",
        button: "server_cfg_default_vc_auto_join",
        labels: ["VC auto-join enabled by default", "VC auto-join disabled by default"],
        description: "Server default for automatically joining voice channel when queuing",
    },
    default_vc_auto_leave: bool {
        column: "default_vc_auto_leave",
        default: false,
        display: "Default VC auto-leave",
        button: "server_cfg_default_vc_auto_leave",
        labels: ["VC auto-leave enabled by default", "VC auto-leave disabled by default"],
        description: "Server default for automatically leaving voice channel when unqueuing",
    },
    default_vc_leave_queue: bool {
        column: "default_vc_leave_queue",
        default: false,
        display: "Default VC leave queue",
        button: "server_cfg_default_vc_leave_queue",
        labels: ["Leave queue on VC exit enabled by default", "Leave queue on VC exit disabled by default"],
        description: "Server default for leaving queue when exiting voice channel",
    },
    post_game_confirm_time: u16 {
        column: "post_game_confirm_time",
        default: 60,
        display: "Post-game Confirm Time",
        description: "Seconds to wait for post-game confirmation",
    },
    team_balance_method: String {
        column: "team_balance_method",
        default: "bch".to_string(),
        display: "Team balance method",
        description: "Algorithm used for team balancing",
    },
    gamemode: String {
        column: "gamemode",
        default: "".to_string(),
        display: "Gamemode",
        description: "Current gamemode setting",
    },
    ping_role: String {
        column: "ping_role",
        default: "".to_string(),
        display: "Ping role",
        description: "Role ID to ping instead of @here (empty for @here)",
    },
    ping_users_enabled: bool {
        column: "ping_users_enabled",
        default: true,
        display: "Ping users",
        button: "server_cfg_ping_users_enabled",
        labels: ["Users can ping", "Only runners can ping"],
        description: "Allow regular users to use the ping button",
    },
    ping_user_cooldown: u16 {
        column: "ping_user_cooldown",
        default: 30,
        display: "Ping user cooldown",
        description: "Cooldown in minutes for regular users to ping",
    },
    ping_runner_cooldown: u16 {
        column: "ping_runner_cooldown",
        default: 15,
        display: "Ping runner cooldown",
        description: "Cooldown in minutes for runners to ping",
    },
}

define_category_config! {
    quota: u8 {
        column: "quota",
        default: 8,
        display: "Player quota",
        description: "Number of players required to start a game",
    },
    confirm_time: u16 {
        column: "confirm_time",
        default: 60,
        display: "Confirm time",
        description: "Seconds to wait for player confirmation",
    },
    require_score_report: bool {
        column: "require_score_report",
        default: false,
        display: "Require score report",
        button_prefix: "category_cfg_require_score",
        labels: ["Score reporting required", "Score reporting optional"],
        description: "Require score reporting when ending matches",
    },
    dm_alert_enabled: bool {
        column: "dm_alert_enabled",
        default: false,
        display: "DM alert enabled",
        button_prefix: "category_cfg_dm_alert",
        labels: ["DM alerts enabled", "DM alerts disabled"],
        description: "Send DM notifications when threshold is met",
    },
    team_vc_keep_minimum: bool {
        column: "team_vc_keep_minimum",
        default: true,
        display: "Keep minimum VCs",
        button_prefix: "category_cfg_keep_min_vcs",
        labels: ["Keep minimum VCs enabled", "Keep minimum VCs disabled"],
        description: "Keep at least one set of team VCs even when empty",
    },
}

define_user_preferences! {
    vc_auto_join: bool {
        column: "vc_auto_join",
        global_table: "users",
        override_table: "user_server_prefs",
        default: false,
        display: "VC auto-join",
        button: "settings_vc_auto_join",
        labels: ["VC auto-join enabled", "VC auto-join disabled"],
        description: "Automatically join voice channel when queuing",
    },
    vc_auto_leave: bool {
        column: "vc_auto_leave",
        global_table: "users",
        override_table: "user_server_prefs",
        default: false,
        display: "VC auto-leave",
        button: "settings_vc_auto_leave",
        labels: ["VC auto-leave enabled", "VC auto-leave disabled"],
        description: "Automatically leave voice channel when unqueuing",
    },
    vc_leave_queue: bool {
        column: "vc_leave_queue",
        global_table: "users",
        override_table: "user_server_prefs",
        default: false,
        display: "Leave queue on VC exit",
        button: "settings_vc_leave_queue",
        labels: ["Leave queue on VC exit enabled", "Leave queue on VC exit disabled"],
        description: "Leave queue when exiting voice channel",
    },
    pm_hot_alert: bool {
        column: "pm_hot_alert",
        global_table: "users",
        override_table: "",
        default: false,
        display: "DM alerts",
        button: "settings_toggle_dm",
        labels: ["DM alerts enabled", "DM alerts disabled"],
        description: "Receive DM notifications when queue is ready",
    },
    queue_expiration: u16 {
        column: "queue_expiration",
        global_table: "users",
        override_table: "",
        default: 120,
        display: "Queue timeout",
        description: "Minutes before auto-removal from queue",
    },
}

pub use server_config::COLUMNS as SERVER_CONFIG_COLUMNS;
pub use server_config::TOGGLES as SERVER_CONFIG_TOGGLES;
pub use server_config::DESCRIPTIONS as SERVER_CONFIG_DESCRIPTIONS;
pub use category_config::COLUMNS as CATEGORY_CONFIG_COLUMNS;
pub use category_config::TOGGLES as CATEGORY_CONFIG_TOGGLES;
pub use user_preferences::COLUMNS as USER_PREFERENCES_COLUMNS;
pub use user_preferences::TOGGLES as USER_PREFERENCES_TOGGLES;

pub fn sql_type_for_rust_type(type_str: &str) -> &'static str {
    match type_str {
        "bool" => "INTEGER",
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" => "INTEGER",
        "String" | "&str" => "TEXT",
        _ => "TEXT",
    }
}

pub fn sql_default_for_value(value_str: &str, type_str: &str) -> String {
    match type_str {
        "bool" => if value_str == "true" { "1".to_string() } else { "0".to_string() },
        "String" | "&str" => {
            if value_str.is_empty() || value_str == "\"\"" || value_str.contains("to_string()") {
                "NULL".to_string()
            } else {
                format!("'{}'", value_str.trim_matches('"'))
            }
        }
        _ => value_str.to_string(),
    }
}
