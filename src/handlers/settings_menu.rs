use serenity::all::{
    CreateEmbed as CE, CreateActionRow as CAR, CreateButton as CB, ButtonStyle as BS,
    CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO,
    CreateEmbedFooter, RoleId,
};

/// A field displayed in the settings embed
pub struct SettingsField {
    pub name:   String,
    pub value:  String,
    pub inline: bool,
}

impl SettingsField {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: value.into(), inline: true }
    }

    pub fn inline(mut self, inline: bool) -> Self {
        self.inline = inline;
        self
    }
}

/// A row of components (buttons or select menu)
pub enum SettingsRow {
    Buttons(Vec<SettingsButton>),
    RoleSelect { id: String, placeholder: String, default: Option<RoleId> },
    StringSelect { id: String, placeholder: String, options: Vec<(String, String)> },
}

/// A button in the settings menu
pub struct SettingsButton {
    pub id:       String,
    pub label:    String,
    pub style:    SettingsButtonStyle,
    pub disabled: bool,
}

impl SettingsButton {
    pub fn toggle(id: impl Into<String>, label: impl Into<String>, enabled: bool) -> Self {
        let label_str = label.into();
        Self {
            id:       id.into(),
            label:    if enabled { format!("{label_str} enabled") } else { format!("{label_str} disabled") },
            style:    if enabled { SettingsButtonStyle::Success } else { SettingsButtonStyle::Danger },
            disabled: false,
        }
    }

    pub fn edit(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id:       id.into(),
            label:    label.into(),
            style:    SettingsButtonStyle::Primary,
            disabled: false,
        }
    }

    pub fn action(id: impl Into<String>, label: impl Into<String>, style: SettingsButtonStyle) -> Self {
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

#[derive(Clone, Copy)]
pub enum SettingsButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
}

impl From<SettingsButtonStyle> for BS {
    fn from(style: SettingsButtonStyle) -> Self {
        match style {
            SettingsButtonStyle::Primary   => BS::Primary,
            SettingsButtonStyle::Secondary => BS::Secondary,
            SettingsButtonStyle::Success   => BS::Success,
            SettingsButtonStyle::Danger    => BS::Danger,
        }
    }
}

/// Universal settings menu configuration
pub struct SettingsMenu {
    pub title:       String,
    pub description: Option<String>,
    pub color:       u32,
    pub fields:      Vec<SettingsField>,
    pub rows:        Vec<SettingsRow>,
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

    pub fn field(mut self, field: SettingsField) -> Self {
        self.fields.push(field);
        self
    }

    pub fn row(mut self, row: SettingsRow) -> Self {
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
            SettingsRow::Buttons(buttons) => {
                let btns: Vec<CB> = buttons.iter().map(|b| {
                    CB::new(&b.id)
                        .label(&b.label)
                        .style(b.style.into())
                        .disabled(b.disabled)
                }).collect();
                CAR::Buttons(btns)
            }
            SettingsRow::RoleSelect { id, placeholder, default } => {
                CAR::SelectMenu(
                    CSM::new(id, CSMK::Role { default_roles: default.map(|r| vec![r]) })
                        .placeholder(placeholder)
                        .min_values(0)
                        .max_values(1)
                )
            }
            SettingsRow::StringSelect { id, placeholder, options } => {
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

/// Trait for types that can be displayed as a settings menu
pub trait AsSettingsMenu {
    fn as_settings_menu(&self) -> SettingsMenu;
}

// ============================================================================
// UserSettings implementation
// ============================================================================

impl AsSettingsMenu for crate::database::repositories::UserSettings {
    fn as_settings_menu(&self) -> SettingsMenu {
        let minutes = self.expiry_duration.as_secs() / 60;
        let timeout_desc = format!(
            "**Timeout length:** {} minute{}",
            minutes,
            if minutes == 1 { "" } else { "s" }
        );

        SettingsMenu::new("qBot user settings")
            .description(timeout_desc)
            .color(self.announcement_color as u32)
            .footer("VC Kick - kicks you from the vc when you leave the queue.")
            .row(SettingsRow::Buttons(vec![
                SettingsButton::toggle("settings_toggle_dm", "DM alerts", self.dm_alerts),
                SettingsButton::toggle("settings_vc_disconnect", "VC kick", self.vc_kick),
            ]))
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit("settings_timeout", "Set timeout length"),
            ]))
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit("settings_edit_alert", "Edit join alert"),
                SettingsButton::edit("settings_edit_leave_alert", "Edit leave alert"),
            ]))
    }
}

// ============================================================================
// ServerSettings implementation
// ============================================================================

/// Server settings with guild name for display
pub struct ServerSettingsDisplay {
    pub guild_name:   String,
    pub runner_role:  Option<String>,
    pub admin_role:   Option<String>,
    pub dynamic_elo:  bool,
    pub default_elo:  u16,
    pub default_rank: String,
}

impl AsSettingsMenu for ServerSettingsDisplay {
    fn as_settings_menu(&self) -> SettingsMenu {
        let runner_display = self.runner_role.as_ref()
            .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "*Not configured*".to_string());
        
        let admin_display = self.admin_role.as_ref()
            .map(|ids| ids.split(',').map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "*Not configured*".to_string());

        let runner_default = self.runner_role.as_ref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(RoleId::new);
        let admin_default = self.admin_role.as_ref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(RoleId::new);

        SettingsMenu::new(format!("{} Server Settings", self.guild_name))
            .field(SettingsField::new("Runner Role", runner_display).inline(false))
            .field(SettingsField::new("Admin Role", admin_display).inline(false))
            .field(SettingsField::new("Default ELO", self.default_elo.to_string()).inline(true))
            .field(SettingsField::new("Default Rank", &self.default_rank).inline(true))
            .color(0x5865F2)
            .row(SettingsRow::Buttons(vec![
                SettingsButton::toggle("server_settings_dynamic_elo", "Dynamic ELO", self.dynamic_elo),
                SettingsButton::action("server_settings_ranks", "Rank Configuration", SettingsButtonStyle::Secondary),
            ]))
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit("server_settings_edit_default_elo", "Edit Default ELO"),
                SettingsButton::edit("server_settings_edit_default_rank", "Edit Default Rank"),
            ]))
            .row(SettingsRow::Buttons(vec![
                SettingsButton::action("server_settings_create_roles", "Create Roles", SettingsButtonStyle::Primary),
                SettingsButton::action("server_settings_create_group", "Create Group", SettingsButtonStyle::Primary),
            ]))
            .row(SettingsRow::RoleSelect {
                id: "server_settings_runner_role".to_string(),
                placeholder: "Select Runner Role".to_string(),
                default: runner_default,
            })
            .row(SettingsRow::RoleSelect {
                id: "server_settings_admin_role".to_string(),
                placeholder: "Select Admin Role".to_string(),
                default: admin_default,
            })
    }
}

/// Rank configuration display for server settings sub-menu
pub struct RankConfigDisplay {
    pub guild_name:  String,
    pub rank_roles:  Vec<(String, Option<String>)>, // (rank_name, role_ids_csv)
}

impl RankConfigDisplay {
    pub fn build_embed(&self) -> CE {
        let mut embed = CE::new()
            .title(format!("{} Rank Configuration", self.guild_name))
            .color(0x5865F2);

        for (rank_name, role_ids) in &self.rank_roles {
            let display = role_ids.as_ref()
                .map(|ids| ids.split(',').filter(|s| !s.is_empty()).map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "*Not configured*".to_string());
            embed = embed.field(rank_name, display, false);
        }

        embed.footer(CreateEmbedFooter::new("Select a rank below to configure its Discord role"))
    }

    pub fn build_components(&self) -> Vec<CAR> {
        let rank_options: Vec<(String, String)> = self.rank_roles.iter()
            .map(|(name, _)| (name.clone(), name.to_lowercase().replace(" ", "_")))
            .collect();

        vec![
            CAR::SelectMenu(
                CSM::new("server_settings_rank_select", CSMK::String {
                    options: rank_options.iter()
                        .map(|(label, value)| CSMO::new(label, value))
                        .collect()
                })
                .placeholder("Select rank to configure")
            ),
            CAR::Buttons(vec![
                CB::new("server_settings_ranks_back")
                    .label("Back to Server Settings")
                    .style(BS::Secondary),
            ]),
        ]
    }
}

/// Single rank role configuration
pub struct RankRoleConfigDisplay {
    pub guild_name: String,
    pub rank_name:  String,
    pub rank_key:   String,
    pub role_ids:   Option<String>,
}

impl RankRoleConfigDisplay {
    pub fn build_embed(&self) -> CE {
        let display = self.role_ids.as_ref()
            .map(|ids| ids.split(',').filter(|s| !s.is_empty()).map(|id| format!("<@&{id}>")).collect::<Vec<_>>().join(", "))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "*Not configured*".to_string());

        CE::new()
            .title(format!("{} - {} Rank", self.guild_name, self.rank_name))
            .field("Current Role(s)", display, false)
            .color(0x5865F2)
            .footer(CreateEmbedFooter::new("Select a role below to set for this rank"))
    }

    pub fn build_components(&self) -> Vec<CAR> {
        vec![
            CAR::SelectMenu(
                CSM::new(
                    format!("server_settings_rank_role_{}", self.rank_key),
                    CSMK::Role { default_roles: None }
                )
                .placeholder(format!("Select role for {}", self.rank_name))
            ),
            CAR::Buttons(vec![
                CB::new(format!("server_settings_rank_clear_{}", self.rank_key))
                    .label("Clear Role")
                    .style(BS::Danger),
                CB::new("server_settings_rank_back")
                    .label("Back to Ranks")
                    .style(BS::Secondary),
            ]),
        ]
    }
}

// ============================================================================
// GroupSettings implementation
// ============================================================================

/// Group settings for display
pub struct GroupSettingsDisplay {
    pub group_id:     u8,
    pub name:         Option<String>,
    pub quota:        u8,
    pub timeout:      u16,
    pub connect_info: Option<String>,
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
            .field(SettingsField::new("Name", name_display.clone()))
            .field(SettingsField::new("Quota", format!("{} players", self.quota)))
            .field(SettingsField::new("Timeout", format!("{} minutes", self.timeout)))
            .field(SettingsField::new("Connect Info", connect_display).inline(false))
            .color(0x5865F2)
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit(format!("group_settings_edit_name_{gid}"), "Edit Name"),
                SettingsButton::edit(format!("group_settings_edit_quota_{gid}"), "Edit Quota"),
                SettingsButton::edit(format!("group_settings_edit_timeout_{gid}"), "Edit Timeout"),
            ]))
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit(format!("group_settings_edit_connect_{gid}"), "Edit Connect Info"),
            ]))
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
    pub division: String,
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
            .field(SettingsField::new("Steam ID", steam_display))
            .field(SettingsField::new("ELO", format!("{}", self.elo)))
            .field(SettingsField::new("Rank", &self.division))
            .field(SettingsField::new("Games", format!("{}", self.games)))
            .field(SettingsField::new("Wins", format!("{}", self.wins)))
            .field(SettingsField::new("Winrate", winrate))
            .color(0x5865F2)
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit(format!("player_settings_edit_steam_{uid}"), "Edit Steam ID"),
                SettingsButton::edit(format!("player_settings_edit_elo_{uid}"), "Edit ELO"),
            ]))
            .row(SettingsRow::Buttons(vec![
                SettingsButton::edit(format!("player_settings_edit_division_{uid}"), "Edit Rank"),
            ]))
    }
}
