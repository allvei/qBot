use serenity::all::{
    CreateEmbed as CE, CreateActionRow as CAR, CreateButton as CB, ButtonStyle as BS,
    CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO,
    CreateEmbedFooter, RoleId,
};

use crate::Ephemeral as Eph;

const LIST_THRESHOLD: usize = 5;


type SF = SettingsField;
/// A field displayed in the settings embed
pub struct SettingsField {
    pub name:   String,
    pub value:  String,
    pub inline: bool,
}

impl SF {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: value.into(), inline: true }
    }

    pub fn inline(mut self, inline: bool) -> Self {
        self.inline = inline;
        self
    }
}

type SR = SettingsRow;
/// A row of components (buttons or select menu)
pub enum SettingsRow {
    Buttons(Vec<SB>),
    RoleSelect { id: String, placeholder: String, default: Option<RoleId> },
    StringSelect { id: String, placeholder: String, options: Vec<(String, String)> },
}


type SB = SettingsButton;
/// A button in the settings menu
pub struct SettingsButton {
    pub id:       String,
    pub label:    String,
    pub style:    SBS,
    pub disabled: bool,
}

impl SB {
    pub fn toggle(id: impl Into<String>, label: impl Into<String>, enabled: bool) -> Self {
        let label_str = label.into();
        Self {
            id:       id.into(),
            label:    if enabled { format!("{label_str} enabled") } else { format!("{label_str} disabled") },
            style:    if enabled { SBS::Success } else { SBS::Danger },
            disabled: false,
        }
    }

    pub fn edit(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id:       id.into(),
            label:    label.into(),
            style:    SBS::Primary,
            disabled: false,
        }
    }

    pub fn action(id: impl Into<String>, label: impl Into<String>, style: SBS) -> Self {
        Self {
            id:       id.into(),
            label:    label.into(),
            style,
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

type SBS = SettingsButtonStyle;
#[derive(Clone, Copy)]
pub enum SettingsButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
}

impl From<SBS> for BS {
    fn from(style: SBS) -> Self {
        match style {
            SBS::Primary   => BS::Primary,
            SBS::Secondary => BS::Secondary,
            SBS::Success   => BS::Success,
            SBS::Danger    => BS::Danger,
        }
    }
}

/// Universal settings menu configuration
pub struct SettingsMenu {
    pub title:       String,
    pub description: Option<String>,
    pub color:       u32,
    pub fields:      Vec<SF>,
    pub rows:        Vec<SR>,
    pub footer:      Option<String>,
}

impl SettingsMenu {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title:       title.into(),
            description: None,
            color:       0x5865F2, // Discord blurple
            fields:      Vec::new(),
            rows:        Vec::new(),
            footer:      None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    pub fn field(mut self, field: SF) -> Self {
        self.fields.push(field);
        self
    }

    pub fn row(mut self, row: SR) -> Self {
        self.rows.push(row);
        self
    }

    pub fn footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Build the embed for this settings menu
    pub fn build_embed(&self) -> CE {
        let mut embed = CE::new()
            .title(&self.title)
            .color(self.color);

        if let Some(desc) = &self.description {
            embed = embed.description(desc);
        }

        for field in &self.fields {
            embed = embed.field(&field.name, &field.value, field.inline);
        }

        if let Some(footer) = &self.footer {
            embed = embed.footer(CreateEmbedFooter::new(footer));
        }

        embed
    }

    /// Build the component rows for this settings menu
    pub fn build_components(&self) -> Vec<CAR> {
        self.rows.iter().map(|row| match row {
            SR::Buttons(buttons) => {
                let btns: Vec<CB> = buttons.iter().map(|b| {
                    CB::new(&b.id)
                        .label(&b.label)
                        .style(b.style.into())
                        .disabled(b.disabled)
                }).collect();
                CAR::Buttons(btns)
            }
            SR::RoleSelect { id, placeholder, default } => {
                CAR::SelectMenu(
                    CSM::new(id, CSMK::Role { default_roles: default.map(|r| vec![r]) })
                        .placeholder(placeholder)
                        .min_values(0)
                        .max_values(1)
                )
            }
            SR::StringSelect { id, placeholder, options } => {
                let opts: Vec<CSMO> = options.iter()
                    .map(|(label, value)| CSMO::new(label, value))
                    .collect();
                CAR::SelectMenu(
                    CSM::new(id, CSMK::String { options: opts })
                        .placeholder(placeholder)
                        .min_values(1)
                        .max_values(1)
                )
            }
        }).collect()
    }
}

/// Create an intelligent selection menu that adapts based on the number of options
pub fn create_selection_menu(
    menu_id: &str,
    placeholder: &str,
    options: Vec<(String, String)>,
) -> Option<CAR> {
    if options.is_empty() {
        return None;
    }

    // Always create a button for single option
    if options.len() == 1 {
        let (label, value) = options.into_iter().next().unwrap();
        let button = CB::new(&format!("{}_{}", menu_id, value))
            .label(label)
            .style(BS::Primary);
        
        return Some(CAR::Buttons(vec![button]));
    }

    // Use buttons if below threshold, otherwise use select menu
    if options.len() < LIST_THRESHOLD {
        let buttons: Vec<CB> = options.into_iter()
            .map(|(label, value)| {
                CB::new(&format!("{}_{}", menu_id, value))
                    .label(label)
                    .style(BS::Secondary)
            })
            .collect();
        
        Some(CAR::Buttons(buttons))
    } else {
        let select_options: Vec<CSMO> = options.into_iter()
            .map(|(label, value)| CSMO::new(label, value))
            .collect();
        
        Some(CAR::SelectMenu(
            CSM::new(menu_id, CSMK::String { options: select_options })
                .placeholder(placeholder)
                .min_values(1)
                .max_values(1)
        ))
    }
}

/// Trait for types that can be displayed as a settings menu
pub trait AsSettingsMenu {
    fn as_settings_menu(&self) -> SettingsMenu;
}

// ============================================================================
// UserSettings implementation
// ============================================================================

impl AsSettingsMenu for crate::database::repositories::UserSettings {
    fn as_settings_menu(&self) -> SettingsMenu {
        let minutes = self.timeout;
        let timeout_desc = format!(
            "**Timeout length:** {} minute{}",
            minutes,
            if minutes == 1 { "" } else { "s" }
        );

        SettingsMenu::new("qBot preferences")
            .description(format!("{}", timeout_desc))
            .color(self.join_alert_color as u32)
            .row(SR::Buttons(vec![
                SB::edit("settings_timeout", "Set timeout length"),
            ]))
            .row(SR::Buttons(vec![
                SB::toggle("settings_toggle_dm", "DM alerts", self.pm_hot_alert),
            ]))
            .row(SR::Buttons(vec![
                SB::toggle("settings_vc_auto_join", "VC auto-join", self.vc_auto_join),
                SB::toggle("settings_vc_auto_leave", "VC auto-leave", self.vc_auto_leave),
            ]))
            .row(SR::Buttons(vec![
                SB::edit("settings_edit_alert", "Edit join alert"),
                SB::edit("settings_edit_leave_alert", "Edit leave alert"),
            ]))
    }
}

// ============================================================================
// ServerSettings implementation
// ============================================================================

/// Server settings with guild name for display
pub struct ServerSettingsDisplay {
    pub guild_name:  String,
    pub runner_role: Option<String>,
    pub admin_role:  Option<String>,
}

impl AsSettingsMenu for ServerSettingsDisplay {
    fn as_settings_menu(&self) -> SettingsMenu {
        let runner_display = self.runner_role.as_ref()
            .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "*Not configured*".to_string());
        
        let admin_display = self.admin_role.as_ref()
            .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "*Not configured*".to_string());

        let _runner_default = self.runner_role.as_ref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(RoleId::new);
        let _admin_default = self.admin_role.as_ref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(RoleId::new);

        SettingsMenu::new(format!("{} Server Settings", self.guild_name))
            .color(0x5865F2)
            .row(SR::Buttons(vec![
                SB::action("server_settings_roles",  "Roles", SBS::Secondary),
                SB::action("server_settings_ranks",  "Ranks", SBS::Secondary),
                SB::action("server_settings_groups", "Groups", SBS::Secondary),
            ]))
            .footer("Select a category to manage:")
    }
}

/// Role configuration display for server settings sub-menu (runner/admin roles)
pub struct RoleConfigDisplay {
    pub guild_name:  String,
    pub runner_role: Option<String>,
    pub admin_role:  Option<String>,
}

impl RoleConfigDisplay {
    pub fn build_embed(&self) -> CE {
        let runner_display = self.runner_role.as_ref()
            .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "*Not configured*".to_string());
        
        let admin_display = self.admin_role.as_ref()
            .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "*Not configured*".to_string());

        CE::new()
            .title(format!("{} - Manage Roles", self.guild_name))
            .field("Runner Role", runner_display, true)
            .field("Admin Role", admin_display, true)
            .color(0x5865F2)
            .footer(CreateEmbedFooter::new("Select roles below or use Create Roles to auto-generate"))
    }

    pub fn build_components(&self) -> Vec<CAR> {
        let runner_default = self.runner_role.as_ref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(RoleId::new);
        let admin_default = self.admin_role.as_ref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(RoleId::new);

        vec![
            CAR::SelectMenu(
                CSM::new("server_settings_runner_role", CSMK::Role { default_roles: runner_default.map(|r| vec![r]) })
                    .placeholder("Select Runner Role")
            ),
            CAR::SelectMenu(
                CSM::new("server_settings_admin_role", CSMK::Role { default_roles: admin_default.map(|r| vec![r]) })
                    .placeholder("Select Admin Role")
            ),
            CAR::Buttons(vec![
                CB::new("server_settings_create_roles")
                    .label("Create roles")
                    .style(BS::Primary),
                Eph::back("server_settings_roles_back"),
            ]),
        ]
    }
}

/// Rank configuration display for server settings sub-menu
pub struct RankConfigDisplay {
    pub guild_name:        String,
    pub rank_roles:        Vec<(String, u16, RoleId)>, // (rank_name, elo, role_id)
    pub dynamic_elo:       bool,
    pub default_rank_role: Option<RoleId>, // Discord role ID of default rank
}

impl RankConfigDisplay {
    pub fn build_embed(&self) -> CE {
        // Build compact rank list: ELO rank1, rank2, rank3 <@&role_id1>, <@&role_id2>, <@&role_id3> (default)
        let description = if self.rank_roles.is_empty() {
            "No ranks configured yet. Click 'Add Rank' to create your first rank.".to_string()
        } else {
            // Group ranks by ELO
            use std::collections::HashMap;
            let mut elo_groups: HashMap<u16, Vec<(String, RoleId)>> = HashMap::new();
            
            for (rank_name, elo, role_id) in &self.rank_roles {
                elo_groups.entry(*elo).or_insert_with(Vec::new).push((rank_name.clone(), *role_id));
            }
            
            // Sort ELO values
            let mut sorted_elos: Vec<u16> = elo_groups.keys().cloned().collect();
            sorted_elos.sort();
            
            let mut desc = String::new();
            for elo in sorted_elos {
                if let Some(ranks) = elo_groups.get(&elo) {
                    let role_displays: Vec<String> = ranks.iter()
                        .map(|(_, role_id)| format!("<@&{}>", role_id.get()))
                        .collect();
                    
                    // Check if any of these ranks is the default
                    let is_default = ranks.iter().any(|(_, role_id)| self.default_rank_role.map(|r| r == *role_id).unwrap_or(false));
                    let default_marker = if is_default { " (default)" } else { "" };
                    
                    desc.push_str(&format!("‹**{elo}**› {}{}\n", role_displays.join(", "), default_marker));
                }
            }
            desc
        };

        CE::new()
            .title(format!("{} - Manage Ranks", self.guild_name))
            .description(description)
            .color(0x5865F2)
            .footer(CreateEmbedFooter::new(if self.rank_roles.is_empty() {
                "Configure ranks by adding new ones below"
            } else {
                "Select a rank below to edit its name, ELO, or linked role"
            }))
    }

    pub fn build_components(&self) -> Vec<CAR> {
        let mut components = vec![
            CAR::Buttons(vec![
                CB::new("server_settings_dynamic_elo")
                    .label(if self.dynamic_elo { "Dynamic ELO enabled" } else { "Dynamic ELO disabled" })
                    .style(if self.dynamic_elo { BS::Success } else { BS::Danger }),
            ]),
        ];

        // Only add rank selection menus if there are valid ranks
        if !self.rank_roles.is_empty() {
            components.push(
                CAR::SelectMenu(
                    CSM::new("server_settings_default_rank_select", CSMK::String {
                        options: self.rank_roles.iter()
                            .map(|(name, _, role_id)| {
                                let is_default = self.default_rank_role.map(|r| r == *role_id).unwrap_or(false);
                                let label = if is_default {
                                    format!("{} (current default)", name)
                                } else {
                                    name.clone()
                                };
                                CSMO::new(label, role_id.to_string())
                            })
                            .collect()
                    })
                    .placeholder("Set default rank")
                )
            );
            
            components.push(
                CAR::SelectMenu(
                    CSM::new("server_settings_rank_select", CSMK::String {
                        options: self.rank_roles.iter()
                            .map(|(name, _, _)| CSMO::new(name, name.clone()))
                            .collect()
                    })
                    .placeholder("Select rank to configure")
                )
            );
        }

        components.push(
            CAR::Buttons(vec![
                CB::new("server_settings_rank_add")
                    .label("Add rank")
                    .style(BS::Success),
                CB::new("server_settings_rank_link")
                    .label("Link ranks")
                    .style(BS::Primary),
                Eph::back("server_settings_ranks_back"),
            ])
        );

        components
    }
}

/// Single rank configuration with Discord role linking
pub struct RankRoleConfigDisplay {
    pub guild_name: String,
    pub rank_name:  String,
    pub rank_key:   String,
    pub elo:        u16,
    pub role_id:    RoleId,
}

impl RankRoleConfigDisplay {
    pub fn build_embed(&self) -> CE {
        let role_display = format!("<@&{}>", self.role_id.get());

        CE::new()
            .title(format!("{} - {} Rank", self.guild_name, self.rank_name))
            .field("Name", &self.rank_name, true)
            .field("ELO Threshold", self.elo.to_string(), true)
            .field("Discord Role", role_display, true)
            .color(0x5865F2)
            .footer(CreateEmbedFooter::new("Edit name, ELO, or link a Discord role"))
    }

    pub fn build_components(&self) -> Vec<CAR> {
        vec![
            CAR::SelectMenu(
                CSM::new(
                    format!("server_settings_rank_role_{}", self.rank_key),
                    CSMK::Role { default_roles: Some(vec![self.role_id]) }
                )
                .placeholder("Link Discord Role")
                .min_values(0)
                .max_values(1)
            ),
            CAR::Buttons(vec![
                CB::new(format!("server_settings_rank_edit_{}", self.rank_key))
                    .label("Edit Name & ELO")
                    .style(BS::Primary),
                CB::new(format!("server_settings_rank_delete_{}", self.rank_key))
                    .label("Remove Rank")
                    .style(BS::Danger),
                CB::new("server_settings_rank_back")
                    .label("Back to Ranks")
                    .style(BS::Secondary),
            ]),
        ]
    }
}

// ============================================================================
// GroupList implementation
// ============================================================================

/// Group list display for server settings sub-menu
pub struct GroupListDisplay {
    pub guild_name: String,
    pub groups:     Vec<crate::models::Group>,
}

impl GroupListDisplay {
    pub fn build_embed(&self) -> CE {
        let mut description = String::new();
        description.push_str("**Active groups:**\n");

        if !self.groups.is_empty() {
            for group in &self.groups {
                let name = group.display_name();
                let quota = group.quota;
                let sessions = group.sessions.len();
                description.push_str(&format!("- {}\n", name));
            }
        }

        CE::new()
            .title(format!("{} - Manage Groups", self.guild_name))
            .description(description)
            .color(0x5865F2)
            .footer(CreateEmbedFooter::new("Select a group below to edit"))
    }

    pub fn build_components(&self) -> Vec<CAR> {
        let mut components = Vec::new();

        // Add group selector using intelligent selection menu
        if !self.groups.is_empty() {
            // Use queue channel ID to ensure uniqueness even if group_id is duplicated
            let options: Vec<(String, String)> = self.groups.iter()
                .map(|g| {
                    let label = g.display_name();
                    // Use format "groupid_queueid" to ensure uniqueness (each group has unique queue channel)
                    let value = format!("{}_{}", g.group_id, g.channels.queue_vc.get());
                    (label, value)
                })
                .collect();

            if let Some(selection_menu) = create_selection_menu(
                "server_settings_group_select",
                "Select group to configure",
                options,
            ) {
                components.push(selection_menu);
            }
        }

        // Add create group, remove group, and back buttons
        let mut buttons = vec![
            CB::new("server_settings_create_group")
                .label("Create New Group")
                .style(BS::Primary),
        ];
        
        // Only show remove button if there are groups to remove
        if !self.groups.is_empty() {
            buttons.push(
                CB::new("server_settings_remove_group")
                    .label("Remove Group")
                    .style(BS::Danger)
            );
        }
        
        buttons.push(Eph::back("server_settings_groups_back"));
        components.push(CAR::Buttons(buttons));

        components
    }
}

// ============================================================================
// GroupSettings implementation
// ============================================================================

/// Group settings for display
pub struct GroupSettingsDisplay {
    pub group_id:            u8,
    pub name:                Option<String>,
    pub quota:               u8,
    pub timeout:             u16,
    pub connect_info:        Option<String>,
    pub team_balance_method: crate::models::TeamBalanceMethod,
}

impl AsSettingsMenu for GroupSettingsDisplay {
    fn as_settings_menu(&self) -> SettingsMenu {
        let name_display = self.name.as_ref()
            .cloned()
            .unwrap_or_else(|| format!("Group {}", self.group_id));
        let connect_display = self.connect_info.as_ref()
            .map(|s| format!("`{s}`"))
            .unwrap_or_else(|| "*Not configured*".to_string());

        let gid = self.group_id;

        SettingsMenu::new(format!("{name_display} Settings"))
            .field(SF::new("Name", name_display.clone()))
            .field(SF::new("Quota", format!("{} players", self.quota)))
            .field(SF::new("Hot Join Timeout", format!("{} seconds", self.timeout)))
            .field(SF::new("Connect Info", connect_display).inline(false))
            .field(SF::new("Team Balance", self.team_balance_method.as_str()))
            .color(0x5865F2)
            .row(SR::Buttons(vec![
                SB::edit(format!("group_settings_edit_name_{gid}"), "Edit Name"),
                SB::edit(format!("group_settings_edit_quota_{gid}"), "Edit Quota"),
                SB::edit(format!("group_settings_edit_timeout_{gid}"), "Edit Timeout"),
            ]))
            .row(SR::Buttons(vec![
                SB::edit(format!("group_settings_edit_connect_{gid}"), "Edit Connect Info"),
                SB::action(format!("group_settings_link_message_{gid}"), "Link Message", SBS::Success),
            ]))
            .row(SR::StringSelect {
                id: format!("group_settings_balance_{gid}"),
                placeholder: "Select team balance method...".to_string(),
                options: vec![
                    ("BCH".to_string(), "bch".to_string()),
                    ("Average".to_string(), "average".to_string()),
                ],
            })
    }
}

// ============================================================================
// PlayerSettings implementation
// ============================================================================

/// Player settings for admin editing
pub struct PlayerSettingsDisplay {
    pub user_id:  serenity::all::UserId,
    pub username: String,
    pub steam_id: Option<u64>,
    pub elo:      u16,
    pub rank: String,
    pub games:    u32,
    pub wins:     u32,
}

impl AsSettingsMenu for PlayerSettingsDisplay {
    fn as_settings_menu(&self) -> SettingsMenu {
        let steam_display = self.steam_id
            .map(|id| format!("`{id}`"))
            .unwrap_or_else(|| "*Not linked*".to_string());
        
        let winrate = if self.games > 0 {
            format!("{:.1}%", (self.wins as f64 / self.games as f64) * 100.0)
        } else {
            "N/A".to_string()
        };

        let uid = self.user_id.get();

        SettingsMenu::new(format!("{} - Player Settings", self.username))
            .field(SF::new("Steam ID", steam_display))
            .field(SF::new("ELO", format!("{}", self.elo)))
            .field(SF::new("Rank", &self.rank))
            .field(SF::new("Games", format!("{}", self.games)))
            .field(SF::new("Wins", format!("{}", self.wins)))
            .field(SF::new("Winrate", winrate))
            .color(0x5865F2)
            .row(SR::Buttons(vec![
                SB::edit(format!("player_settings_edit_steam_{uid}"), "Edit Steam ID"),
                SB::edit(format!("player_settings_edit_elo_{uid}"), "Edit ELO"),
            ]))
            .row(SR::Buttons(vec![
                SB::edit(format!("player_settings_edit_rank_{uid}"), "Edit Rank"),
                SB::edit(format!("player_settings_edit_alerts_{uid}"), "Edit Alerts"),
            ]))
    }
}
