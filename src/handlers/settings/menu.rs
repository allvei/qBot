use serenity::all::{
  ButtonStyle as BS, CreateActionRow as CAR, CreateButton as CB, CreateEmbed as CE, CreateEmbedFooter, CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK,
  CreateSelectMenuOption as CSMO, RoleId,
};

use crate::Ephemeral as Eph;

// ============================================================================
// Core Types and Constants
// ============================================================================

const LIST_THRESHOLD: usize = 5;

// ============================================================================
// Settings Menu Components
// ============================================================================

/// A field displayed in the settings embed
pub struct SettingsField {
  pub name: String,
  pub value: String,
  pub inline: bool,
}

type SF = SettingsField;

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

type SR = SettingsRow;

/// A button in the settings menu
pub struct SettingsButton {
  pub id: String,
  pub label: String,
  pub style: SettingsButtonStyle,
  pub disabled: bool,
}

type SB = SettingsButton;

impl SettingsButton {
  pub fn toggle(id: impl Into<String>, label: impl Into<String>, enabled: bool) -> Self {
    Self {
      id: id.into(),
      label: label.into(),
      style: if enabled { SettingsButtonStyle::Success } else { SettingsButtonStyle::Danger },
      disabled: false,
    }
  }

  pub fn edit(id: impl Into<String>, label: impl Into<String>) -> Self {
    Self { id: id.into(), label: label.into(), style: SettingsButtonStyle::Primary, disabled: false }
  }

  pub fn action(id: impl Into<String>, label: impl Into<String>, style: SettingsButtonStyle) -> Self {
    Self { id: id.into(), label: label.into(), style, disabled: false }
  }

  pub fn disabled(mut self, disabled: bool) -> Self {
    self.disabled = disabled;
    self
  }

  /// Create an edit button scoped to a category: `category_settings_{action}_{category_id}`
  pub fn category_edit(action: &str, label: impl Into<String>, category_id: u8) -> Self {
    Self::edit(format!("category_settings_{action}_{category_id}"), label)
  }

  /// Create an action button scoped to a category: `category_settings_{action}_{category_id}`
  pub fn category_action(action: &str, label: impl Into<String>, style: SettingsButtonStyle, category_id: u8) -> Self {
    Self::action(format!("category_settings_{action}_{category_id}"), label, style)
  }
}

#[derive(Clone, Copy)]
pub enum SettingsButtonStyle {
  Primary,
  Secondary,
  Success,
  Danger,
}

type Sbs = SettingsButtonStyle;

impl From<SettingsButtonStyle> for BS {
  fn from(style: SettingsButtonStyle) -> Self {
    match style {
      SettingsButtonStyle::Primary => BS::Primary,
      SettingsButtonStyle::Secondary => BS::Secondary,
      SettingsButtonStyle::Success => BS::Success,
      SettingsButtonStyle::Danger => BS::Danger,
    }
  }
}

/// Universal settings menu configuration
pub struct SettingsMenu {
  pub title: String,
  pub description: Option<String>,
  pub color: u32,
  pub fields: Vec<SF>,
  pub rows: Vec<SR>,
  pub footer: Option<String>,
}

impl SettingsMenu {
  pub fn new(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      description: None,
      color: 0x5865F2, // Discord blurple
      fields: Vec::new(),
      rows: Vec::new(),
      footer: None,
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
    let mut embed = CE::new().title(&self.title).color(self.color);

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
    self
      .rows
      .iter()
      .map(|row| match row {
        SR::Buttons(buttons) => {
          let btns: Vec<CB> = buttons.iter().map(|b| CB::new(&b.id).label(&b.label).style(b.style.into()).disabled(b.disabled)).collect();
          CAR::Buttons(btns)
        }
        SR::RoleSelect { id, placeholder, default } => {
          CAR::SelectMenu(CSM::new(id, CSMK::Role { default_roles: default.map(|r| vec![r]) }).placeholder(placeholder).min_values(0).max_values(1))
        }
        SR::StringSelect { id, placeholder, options } => {
          let opts: Vec<CSMO> = options.iter().map(|(label, value)| CSMO::new(label, value)).collect();
          CAR::SelectMenu(CSM::new(id, CSMK::String { options: opts }).placeholder(placeholder).min_values(1).max_values(1))
        }
      })
      .collect()
  }
}

/// Create an intelligent selection menu that adapts based on the number of options
pub fn create_selection_menu(menu_id: &str, placeholder: &str, options: Vec<(String, String)>) -> Option<CAR> {
  if options.is_empty() {
    return None;
  }

  // Always create a button for single option
  if options.len() == 1 {
    let (label, value) = options.into_iter().next().unwrap();
    let button = CB::new(format!("{}_{}", menu_id, value)).label(label.as_str()).style(BS::Primary);

    return Some(CAR::Buttons(vec![button]));
  }

  // Use buttons if below threshold, otherwise use select menu
  if options.len() < LIST_THRESHOLD {
    let buttons: Vec<CB> = options.into_iter().map(|(label, value)| CB::new(format!("{}_{}", menu_id, value)).label(label.as_str()).style(BS::Secondary)).collect();

    Some(CAR::Buttons(buttons))
  } else {
    let select_options: Vec<CSMO> = options.into_iter().map(|(label, value)| CSMO::new(label, value)).collect();

    Some(CAR::SelectMenu(CSM::new(menu_id, CSMK::String { options: select_options }).placeholder(placeholder).min_values(1).max_values(1)))
  }
}

/// Trait for types that can be displayed as a settings menu
pub trait AsSettingsMenu {
  fn as_settings_menu(&self) -> SettingsMenu;
}

// ============================================================================
// UserPreferences implementation
// ============================================================================

impl AsSettingsMenu for crate::db::repo::UserPreferences {
  fn as_settings_menu(&self) -> SettingsMenu {
    // Format expiration: hours for full hours, minutes for partial
    let queue_expiration_text = if self.queue_expiration >= 60 && self.queue_expiration.is_multiple_of(60) {
      let hours = self.queue_expiration / 60;
      format!("{}h", hours)
    } else {
      format!("{}m", self.queue_expiration)
    };

    SettingsMenu::new("qBot preferences")
      .description("Configure your queue preferences")
      .color(self.join_alert_color)
      .row(SR::Buttons(vec![SB::edit("settings_queue_expiration", format!("Timeout: {}", queue_expiration_text.as_str()))]))
      .row(SR::Buttons(vec![SB::toggle("settings_toggle_dm", "DM alerts", self.pm_hot_alert)]))
      .row(SR::Buttons(vec![SB::toggle("settings_vc_auto_join", "VC auto-join", self.vc_auto_join), SB::toggle("settings_vc_auto_leave", "VC auto-leave", self.vc_auto_leave)]))
      .row(SR::Buttons(vec![SB::edit("settings_edit_alert", "Edit join alert"), SB::edit("settings_edit_leave_alert", "Edit leave alert")]))
  }
}

// ============================================================================
// ServerSettings implementation
// ============================================================================

/// All boolean toggles shown on the main server settings page.
/// To add a new toggle: add a DB column, migration, and an entry here.
pub const SERVER_CONFIG_TOGGLES: &[ConfigToggle] = &[
  ConfigToggle { column: "elo_ranks_linked", button_id: "server_cfg_elo_ranks_linked", label_on: "ELO-Rank linked", label_off: "ELO-Rank independent", default: true },
  ConfigToggle { column: "active_elo", button_id: "server_settings_dynamic_elo", label_on: "Dynamic ELO enabled", label_off: "Dynamic ELO disabled", default: false },
  ConfigToggle {
    column: "post_game_auto_leave",
    button_id: "server_cfg_post_game_auto_leave",
    label_on: "Post-game auto-remove is enabled",
    label_off: "Post-game auto-remove is disabled",
    default: true,
  },
  ConfigToggle {
    column: "hide_elo",
    button_id: "server_cfg_hide_elo",
    label_on: "ELO is visible",
    label_off: "ELO is hidden",
    default: false,
  },
];

/// Server settings with guild name for display
pub struct ServerSettingsDisplay {
  pub guild_name: String,
  pub runner_role: Option<String>,
  pub admin_role: Option<String>,
  pub toggle_states: Vec<bool>,
  pub balance_method: String,
  pub post_game_confirm_time: u16,
}

impl AsSettingsMenu for ServerSettingsDisplay {
  fn as_settings_menu(&self) -> SettingsMenu {
    SettingsMenu::new(format!("{} - Server settings", self.guild_name))
            .color(0x5865F2)
            .description("**Configuration Overview:**\n\n**Server-wide settings**\n• Roles (runner/admin permissions)\n• Team balance method\n• ELO & Rank linking\n\n**Rank management**\n• Add, remove & link ranks\n• Set default rank\n\n**Category management**\n• Queue channels & voice channels\n• Team channels & game settings")
            .row(SR::Buttons(vec![
                SB::action("server_settings_roles",  "Server", Sbs::Secondary),
                SB::action("server_settings_ranks",  "Ranks", Sbs::Secondary),
                SB::action("server_settings_categories", "Categories", Sbs::Secondary),
            ]))
            .footer("Select a category to manage:")
  }
}

/// Server configuration display for server settings sub-menu (roles, balance, ELO)
pub struct ServerConfigDisplay {
  pub guild_name: String,
  pub runner_role: Option<String>,
  pub admin_role: Option<String>,
  pub toggle_states: Vec<bool>,
  pub balance_method: String,
  pub post_game_confirm_time: u16,
}

impl AsSettingsMenu for ServerConfigDisplay {
  fn as_settings_menu(&self) -> SettingsMenu {
    let runner_default = self.runner_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);
    let admin_default = self.admin_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);

    let mut menu = SettingsMenu::new(format!("{} - Server Settings", self.guild_name))
      .description("Configure server-wide settings including roles, team balance method, and ELO settings")
      .field(SF::new("Post-game confirm time", format!("{} seconds", self.post_game_confirm_time)))
      .color(0x5865F2)
      .footer("Configure server-wide settings below");

    // Add role selects
    menu = menu.row(SR::RoleSelect {
      id: "server_settings_runner_role".to_string(),
      placeholder: "Select runner role".to_string(),
      default: runner_default,
    });

    menu = menu.row(SR::RoleSelect {
      id: "server_settings_admin_role".to_string(),
      placeholder: "Select admin role".to_string(),
      default: admin_default,
    });

    // Add balance method select
    menu = menu.row(SR::StringSelect {
      id: "server_settings_balance".to_string(),
      placeholder: "Team balance method...".to_string(),
      options: vec![
        ("Custom distribution algorithm".to_string(), "bch".to_string()),
        ("Average distribution".to_string(), "average".to_string()),
      ],
    });

    // Add ELO toggles
    if !self.toggle_states.is_empty() {
      let toggle_buttons: Vec<SB> = SERVER_CONFIG_TOGGLES
        .iter()
        .zip(self.toggle_states.iter())
        .map(|(toggle, &state)| SB::toggle(toggle.button_id, if state { toggle.label_on } else { toggle.label_off }, state))
        .collect();
      menu = menu.row(SR::Buttons(toggle_buttons));
    }

    // Add action buttons
    menu = menu.row(SR::Buttons(vec![
      SB::action("server_settings_edit_post_game_confirm_time", "Edit post-game timeout", Sbs::Secondary),
      SB::action("server_settings_create_roles", "Create roles", Sbs::Primary),
      SB::action("server_settings_roles_back", "Back", Sbs::Secondary),
    ]));

    menu
  }
}

impl ServerConfigDisplay {
  pub fn build_embed(&self) -> CE {
    self.as_settings_menu().build_embed()
  }

  pub fn build_components(&self) -> Vec<CAR> {
    let runner_default = self.runner_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);
    let admin_default = self.admin_role.as_ref().and_then(|s| s.parse::<u64>().ok()).map(RoleId::new);

    let mut components = vec![
      CAR::SelectMenu(CSM::new("server_settings_runner_role", CSMK::Role { default_roles: runner_default.map(|r| vec![r]) }).placeholder("Select runner role")),
      CAR::SelectMenu(CSM::new("server_settings_admin_role", CSMK::Role { default_roles: admin_default.map(|r| vec![r]) }).placeholder("Select admin role")),
    ];

    // Add team balance method
    components.push(CAR::SelectMenu(
      CSM::new("server_settings_balance", CSMK::String { options: vec![CSMO::new("Custom distribution algorithm", "bch"), CSMO::new("Average distribution", "average")] })
        .placeholder("Team balance method..."),
    ));

    // Add ELO toggles
    let toggle_buttons: Vec<CB> = SERVER_CONFIG_TOGGLES
      .iter()
      .zip(self.toggle_states.iter())
      .map(
        |(toggle, &state)| {
          if state {
            CB::new(toggle.button_id).label(toggle.label_on).style(BS::Success)
          } else {
            CB::new(toggle.button_id).label(toggle.label_off).style(BS::Danger)
          }
        },
      )
      .collect();

    if !toggle_buttons.is_empty() {
      components.push(CAR::Buttons(toggle_buttons));
    }

    // Add action buttons
    components.push(CAR::Buttons(vec![
      CB::new("server_settings_edit_post_game_confirm_time").label("Edit post-game timeout").style(BS::Secondary),
      CB::new("server_settings_create_roles").label("Create roles").style(BS::Primary),
      Eph::back("server_settings_roles_back"),
    ]));

    components
  }
}

/// Describes a boolean toggle option in a config menu.
/// Adding a new toggle only requires:
/// 1. A DB column + migration
/// 2. An entry in the relevant TOGGLES array
/// 3. A handler match arm (or use the generic `handle_config_toggle`)
pub struct ConfigToggle {
  /// DB column name in the config table
  pub column: &'static str,
  /// Button custom_id prefix (e.g. "server_settings_dynamic_elo")
  pub button_id: &'static str,
  /// Label shown when enabled
  pub label_on: &'static str,
  /// Label shown when disabled
  pub label_off: &'static str,
  /// Default value when not set in DB
  pub default: bool,
}

/// All boolean toggles shown in the Rank configuration menu.
/// To add a new toggle: add a DB column, migration, and an entry here.
pub const RANK_CONFIG_TOGGLES: &[ConfigToggle] = &[
    // Note: ELO-Rank linking moved to Server menu
    // Note: Dynamic ELO moved to Server menu
];

/// Rank configuration display for server settings sub-menu
pub struct RankConfigDisplay {
  pub guild_name: String,
  pub rank_roles: Vec<(String, u16, RoleId)>, // (rank_name, elo, role_id)
  pub toggle_states: Vec<bool>,
  pub default_rank_role: Option<RoleId>, // Discord role ID of default rank
}

impl AsSettingsMenu for RankConfigDisplay {
  fn as_settings_menu(&self) -> SettingsMenu {
    // Build compact rank list: ELO rank1, rank2, rank3 <@&role_id1>, <@&role_id2>, <@&role_id3> (default)
    let description = if self.rank_roles.is_empty() {
      "No ranks configured yet. Click 'Add Rank' to create your first rank.".to_string()
    } else {
      // Category ranks by ELO
      use std::collections::HashMap;
      let mut elo_categories: HashMap<u16, Vec<(String, RoleId)>> = HashMap::new();

      for (rank_name, elo, role_id) in &self.rank_roles {
        elo_categories.entry(*elo).or_default().push((rank_name.clone(), *role_id));
      }

      // Sort ELO values
      let mut sorted_elos: Vec<u16> = elo_categories.keys().cloned().collect();
      sorted_elos.sort();

      let mut desc = String::new();
      for elo in sorted_elos {
        if let Some(ranks) = elo_categories.get(&elo) {
          let role_displays: Vec<String> = ranks.iter().map(|(_, role_id)| format!("<@&{}>", role_id.get())).collect();

          // Check if any of these ranks is the default
          let is_default = ranks.iter().any(|(_, role_id)| self.default_rank_role.map(|r| r == *role_id).unwrap_or(false));
          let default_marker = if is_default { " (default)" } else { "" };

          desc.push_str(&format!("‹**{elo}**› {}{}\n", role_displays.join(", "), default_marker));
        }
      }
      desc
    };

    let mut menu = SettingsMenu::new(format!("{} - Manage Ranks", self.guild_name))
      .description(description)
      .color(0x5865F2)
      .footer(if self.rank_roles.is_empty() {
        "Configure ranks by adding new ones below"
      } else {
        "Select a rank below to edit its name, ELO, or linked role"
      });

    // Add toggle buttons if any
    if !self.toggle_states.is_empty() {
      let toggle_buttons: Vec<SB> = RANK_CONFIG_TOGGLES
        .iter()
        .zip(self.toggle_states.iter())
        .map(|(toggle, &state)| SB::toggle(toggle.button_id, toggle.label_on, state))
        .collect();
      menu = menu.row(SR::Buttons(toggle_buttons));
    }

    // Add rank selection if there are ranks
    if !self.rank_roles.is_empty() {
      // Detect duplicate rank names for default rank select
      let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
      for (name, _, _) in &self.rank_roles {
        *name_counts.entry(name.as_str()).or_insert(0) += 1;
      }

      let default_options: Vec<(String, String)> = self
        .rank_roles
        .iter()
        .map(|(name, _, role_id)| {
          let is_default = self.default_rank_role.map(|r| r == *role_id).unwrap_or(false);
          let has_duplicate = name_counts.get(name.as_str()).copied().unwrap_or(0) > 1;
          let label = match (is_default, has_duplicate) {
            (true, true) => format!("{} (ID {}, current default)", name, role_id.get()),
            (true, false) => format!("{} (current default)", name),
            (false, true) => format!("{} (ID {})", name, role_id.get()),
            (false, false) => name.clone(),
          };
          (label, role_id.to_string())
        })
        .collect();

      menu = menu.row(SR::StringSelect {
        id: "server_settings_default_rank_select".to_string(),
        placeholder: "Set default rank".to_string(),
        options: default_options,
      });

      // Add rank edit select
      let edit_options: Vec<(String, String)> = self
        .rank_roles
        .iter()
        .map(|(name, _, role_id)| {
          let label = if name_counts.get(name.as_str()).copied().unwrap_or(0) > 1 {
            format!("{} (ID {})", name, role_id.get())
          } else {
            name.clone()
          };
          (label, role_id.to_string())
        })
        .collect();

      menu = menu.row(SR::StringSelect {
        id: "server_settings_rank_select".to_string(),
        placeholder: "Select rank to configure".to_string(),
        options: edit_options,
      });
    }

    // Add action buttons
    menu = menu.row(SR::Buttons(vec![
      SB::action("server_settings_rank_add", "Add rank", Sbs::Success),
      SB::action("server_settings_rank_link", "Link ranks", Sbs::Primary),
      SB::action("server_settings_ranks_back", "Back", Sbs::Secondary),
    ]));

    menu
  }
}

impl RankConfigDisplay {
  pub fn build_embed(&self) -> CE {
    self.as_settings_menu().build_embed()
  }

  pub fn build_components(&self) -> Vec<CAR> {
    // Build toggle buttons from RANK_CONFIG_TOGGLES
    let toggle_buttons: Vec<CB> = RANK_CONFIG_TOGGLES
      .iter()
      .zip(self.toggle_states.iter())
      .map(|(toggle, &state)| CB::new(toggle.button_id).label(if state { toggle.label_on } else { toggle.label_off }).style(if state { BS::Success } else { BS::Danger }))
      .collect();

    let mut components = Vec::new();

    // Only add toggle buttons row if there are toggle buttons
    if !toggle_buttons.is_empty() {
      components.push(CAR::Buttons(toggle_buttons));
    }

    // Only add rank selection menus if there are valid ranks
    if !self.rank_roles.is_empty() {
      components.push({
        // Detect duplicate rank names for labeling
        let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for (name, _, _) in &self.rank_roles {
          *name_counts.entry(name.as_str()).or_insert(0) += 1;
        }

        CAR::SelectMenu(
          CSM::new(
            "server_settings_default_rank_select",
            CSMK::String {
              options: self
                .rank_roles
                .iter()
                .map(|(name, _, role_id)| {
                  let is_default = self.default_rank_role.map(|r| r == *role_id).unwrap_or(false);
                  let has_duplicate = name_counts.get(name.as_str()).copied().unwrap_or(0) > 1;
                  let label = match (is_default, has_duplicate) {
                    (true, true) => format!("{} (ID {}, current default)", name, role_id.get()),
                    (true, false) => format!("{} (current default)", name),
                    (false, true) => format!("{} (ID {})", name, role_id.get()),
                    (false, false) => name.clone(),
                  };
                  CSMO::new(label, role_id.to_string())
                })
                .collect(),
            },
          )
          .placeholder("Set default rank"),
        )
      });

      components.push({
        // Detect duplicate rank names
        let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for (name, _, _) in &self.rank_roles {
          *name_counts.entry(name.as_str()).or_insert(0) += 1;
        }

        CAR::SelectMenu(
          CSM::new(
            "server_settings_rank_select",
            CSMK::String {
              options: self
                .rank_roles
                .iter()
                .map(|(name, _, role_id)| {
                  let label = if name_counts.get(name.as_str()).copied().unwrap_or(0) > 1 {
                    format!("{} (ID {})", name, role_id.get())
                  } else {
                    name.clone()
                  };
                  CSMO::new(label, role_id.to_string())
                })
                .collect(),
            },
          )
          .placeholder("Select rank to configure"),
        )
      });
    }

    components.push(CAR::Buttons(vec![
      CB::new("server_settings_rank_add").label("Add rank").style(BS::Success),
      CB::new("server_settings_rank_link").label("Link ranks").style(BS::Primary),
      Eph::back("server_settings_ranks_back"),
    ]));

    components
  }
}

/// Single rank configuration with Discord role linking
pub struct RankRoleConfigDisplay {
  pub guild_name: String,
  pub rank_name: String,
  pub rank_key: String,
  pub elo: u16,
  pub role_id: RoleId,
}

impl RankRoleConfigDisplay {
  pub fn build_embed(&self) -> CE {
    let role_display = format!("<@&{}>", self.role_id.get());

    CE::new()
      .title(format!("{} - {} Rank", self.guild_name, self.rank_name))
      .field("Name", &self.rank_name, true)
      .field("ELO Threshold", self.elo.to_string(), true)
      .field("Discord role", role_display, true)
      .color(0x5865F2)
      .footer(CreateEmbedFooter::new("Edit name, ELO, or link a Discord role"))
  }

  pub fn build_components(&self) -> Vec<CAR> {
    vec![
      CAR::SelectMenu(
        CSM::new(format!("server_settings_rank_role_{}", self.rank_key), CSMK::Role { default_roles: Some(vec![self.role_id]) })
          .placeholder("Link discord Role")
          .min_values(0)
          .max_values(1),
      ),
      CAR::Buttons(vec![
        CB::new(format!("server_settings_rank_edit_{}", self.rank_key)).label("Edit name & ELO").style(BS::Primary),
        CB::new(format!("server_settings_rank_delete_{}", self.rank_key)).label("Remove rank").style(BS::Danger),
        CB::new("server_settings_rank_back").label("Back to ranks").style(BS::Secondary),
      ]),
    ]
  }
}

// ============================================================================
// CategoryList implementation
// ============================================================================

/// Category list display for server settings sub-menu
pub struct CategoryListDisplay {
  pub guild_name: String,
  pub categories: Vec<crate::models::Category>,
}

impl AsSettingsMenu for CategoryListDisplay {
  fn as_settings_menu(&self) -> SettingsMenu {
    let mut description = String::new();
    description.push_str("**Active categories:**\n");

    if !self.categories.is_empty() {
      for category in &self.categories {
        let name = category.name();
        let _quota = category.quota();
        let _sessions = category.formats[0].sessions.len();
        description.push_str(&format!("- {}\n", name));
      }
    }

    let mut menu = SettingsMenu::new(format!("{} - Manage Categories", self.guild_name))
      .description(description)
      .color(0x5865F2)
      .footer("Select a category below to edit");

    // Add category selector if there are categories
    if !self.categories.is_empty() {
      let options: Vec<(String, String)> = self
        .categories
        .iter()
        .map(|g| {
          let label = g.name();
          let value = format!("{}_{}", g.id, g.channels.queue_vc.get());
          (label, value)
        })
        .collect();

      menu = menu.row(SR::StringSelect {
        id: "server_settings_category_select".to_string(),
        placeholder: "Select category to configure".to_string(),
        options,
      });
    }

    // Add action buttons
    let mut buttons = vec![SB::action("server_settings_create_category", "Create a category", Sbs::Primary)];
    if !self.categories.is_empty() {
      buttons.push(SB::action("server_settings_remove_category", "Remove a category", Sbs::Danger));
    }
    buttons.push(SB::action("server_settings_categories_back", "Back", Sbs::Secondary));
    menu = menu.row(SR::Buttons(buttons));

    menu
  }
}

impl CategoryListDisplay {
  pub fn build_embed(&self) -> CE {
    self.as_settings_menu().build_embed()
  }

  pub fn build_components(&self) -> Vec<CAR> {
    let mut components = Vec::new();

    // Add category selector using intelligent selection menu
    if !self.categories.is_empty() {
      // Use queue channel ID to ensure uniqueness even if category_id is duplicated
      let options: Vec<(String, String)> = self
        .categories
        .iter()
        .map(|g| {
          let label = g.name();
          // Use format "categoryid_queueid" to ensure uniqueness (each category has unique queue channel)
          let value = format!("{}_{}", g.id, g.channels.queue_vc.get());
          (label, value)
        })
        .collect();

      if let Some(selection_menu) = create_selection_menu("server_settings_category_select", "Select category to configure", options) {
        components.push(selection_menu);
      }
    }

    // Add create category, remove category, and back buttons
    let mut buttons = vec![CB::new("server_settings_create_category").label("Create a category").style(BS::Primary)];

    // Only show remove button if there are categories to remove
    if !self.categories.is_empty() {
      buttons.push(CB::new("server_settings_remove_category").label("Remove a category").style(BS::Danger));
    }

    buttons.push(Eph::back("server_settings_categories_back"));
    components.push(CAR::Buttons(buttons));

    components
  }
}

// ============================================================================
// CategorySettings implementation
// ============================================================================

/// Category settings for display
pub struct CategorySettingsDisplay {
  pub category_id: u8,
  pub name: Option<String>,
  pub quota: u8,
  pub confirm_time: u16,
  pub connect_info: Option<String>,
  pub format_names: Vec<String>,
  pub vc_create: String,
  pub vc_destroy: String,
  pub vc_keep_min: bool,
}

impl AsSettingsMenu for CategorySettingsDisplay {
  fn as_settings_menu(&self) -> SettingsMenu {
    let name_display = self.name.as_ref().cloned().unwrap_or_else(|| format!("Category {}", self.category_id));
    let connect_display = self.connect_info.as_ref().filter(|s| !s.trim().is_empty()).map(|s| format!("`{s}`")).unwrap_or_else(|| "-".to_string());

    let gid = self.category_id;

    SettingsMenu::new(format!("{name_display} Settings"))
      .field(SF::new("Name", name_display.clone()))
      .field(SF::new("Quota", format!("{} players", self.quota)))
      .field(SF::new("Confirm expiry", format!("{} seconds", self.confirm_time)))
      .field(SF::new("Connect info", connect_display).inline(false))
      .field(
        SF::new("Formats", if self.format_names.is_empty() { "None".to_string() } else { self.format_names.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n") })
          .inline(false),
      )
      .field(SF::new("Team VC create", &self.vc_create))
      .field(SF::new("Team VC destroy", &self.vc_destroy))
      .field(SF::new("Keep minimum VCs", if self.vc_keep_min { "Yes" } else { "No" }))
      .color(0x5865F2)
      .row(SR::Buttons(vec![SB::category_edit("edit_name", "Name", gid), SB::category_edit("edit_quota", "Quota", gid), SB::category_edit("edit_confirm_time", "Confirm expiry", gid)]))
      .row(SR::Buttons(vec![
        SB::category_edit("edit_connect", "Connect info", gid),
        SB::category_action("formats", "Formats", Sbs::Primary, gid),
        SB::category_action("link_message", "Re-link dashboard", Sbs::Success, gid),
      ]))
      .row(SR::Buttons(vec![
        SB::category_edit("edit_vc_create", "VC create", gid),
        SB::category_edit("edit_vc_destroy", "VC destroy", gid),
        SB::category_edit("edit_vc_keepmin", "Keep min VCs", gid),
      ]))
      .row(SR::Buttons(vec![SB::category_action("elo_gate", "ELO gate", Sbs::Primary, gid), SB::action("server_settings_categories", "Back", Sbs::Secondary)]))
  }
}

// ============================================================================
// FormatList implementation
// ============================================================================

/// Format list display for category settings sub-menu
pub struct FormatListDisplay {
  pub category_id: u8,
  pub category_name: String,
  pub formats: Vec<(u8, String, u8)>, // (id, name, quota)
}

impl FormatListDisplay {
  pub fn build_embed(&self) -> CE {
    let mut description = String::new();

    for (id, name, quota) in &self.formats {
      description.push_str(&format!("- **{}** (quota: {}, id: {})\n", name, quota, id));
    }

    if description.is_empty() {
      description.push_str("*No formats configured.*\n");
    }

    CE::new().title(format!("{} - Formats", self.category_name)).description(description).color(0x5865F2).footer(CreateEmbedFooter::new("Manage formats for this category"))
  }

  pub fn build_components(&self) -> Vec<CAR> {
    let gid = self.category_id;
    let can_add = self.formats.len() < 3;

    let mut buttons = Vec::new();

    if can_add {
      buttons.push(CB::new(format!("category_fmt_add_{gid}")).label("Add format").style(BS::Primary));
    }

    if self.formats.len() > 1 {
      buttons.push(CB::new(format!("category_fmt_remove_{gid}")).label("Remove format").style(BS::Danger));
    }

    buttons.push(Eph::back(format!("category_fmt_back_{gid}")));

    let mut components = vec![CAR::Buttons(buttons)];

    // Add select menu for editing if there are formats
    if !self.formats.is_empty() {
      let options: Vec<(String, String)> = self.formats.iter().map(|(id, name, quota)| (format!("{} (quota: {})", name, quota), format!("{}_{}", gid, id))).collect();

      if let Some(menu) = create_selection_menu("category_fmt_edit", "Select format to edit", options) {
        components.push(menu);
      }
    }

    components
  }
}

// ============================================================================
// PlayerSettings implementation
// ============================================================================

/// Player settings for admin editing
pub struct PlayerSettingsDisplay {
  pub user_id: serenity::all::UserId,
  pub username: String,
  pub steam_id: Option<u64>,
  pub elo: u16,
  pub rank: String,
  pub games: u32,
  pub wins: u32,
}

impl AsSettingsMenu for PlayerSettingsDisplay {
  fn as_settings_menu(&self) -> SettingsMenu {
    let steam_display = self.steam_id.map(|id| format!("`{id}`")).unwrap_or_else(|| "*Not linked*".to_string());

    let winrate = if self.games > 0 { format!("{:.1}%", (self.wins as f64 / self.games as f64) * 100.0) } else { "N/A".to_string() };

    let uid = self.user_id.get();

    SettingsMenu::new(format!("{} - Player Settings", self.username))
      .field(SF::new("Steam ID", steam_display))
      .field(SF::new("ELO", format!("{}", self.elo)))
      .field(SF::new("Rank", &self.rank))
      .field(SF::new("Games", format!("{}", self.games)))
      .field(SF::new("Wins", format!("{}", self.wins)))
      .field(SF::new("Winrate", winrate))
      .color(0x5865F2)
      .row(SR::Buttons(vec![SB::edit(format!("player_settings_edit_steam_{uid}"), "Edit Steam ID"), SB::edit(format!("player_settings_edit_elo_{uid}"), "Edit ELO")]))
      .row(SR::Buttons(vec![SB::edit(format!("player_settings_edit_alerts_{uid}"), "Edit alerts")]))
  }
}

/// Build the player settings embed and components, including rank dropdown if ranks exist.
pub async fn build_player_settings_menu(settings: &PlayerSettingsDisplay, db: &crate::Database, guild_id: serenity::all::GuildId) -> (CE, Vec<CAR>) {
  use crate::db::repo::rank::GuildRank;

  let uid = settings.user_id.get();
  let steam_display = settings.steam_id.filter(|&id| id != 0).map(|id| format!("https://steamcommunity.com/profiles/{}", id)).unwrap_or_else(|| "Not set".to_string());

  let winrate = if settings.games > 0 { format!("{:.1}%", (settings.wins as f64 / settings.games as f64) * 100.0) } else { "0%".to_string() };

  let ranks = db.ranks.get_ranks(guild_id).await.unwrap_or_default();

  let mut menu = SettingsMenu::new(format!("{} - Player Settings", settings.username))
    .field(SF::new("Steam ID", steam_display))
    .field(SF::new("ELO", format!("{}", settings.elo)))
    .field(SF::new("Rank", &settings.rank))
    .field(SF::new("Games", format!("{}", settings.games)))
    .field(SF::new("Wins", format!("{}", settings.wins)))
    .field(SF::new("Winrate", winrate))
    .color(0x5865F2)
    .row(SR::Buttons(vec![SB::edit(format!("player_settings_edit_steam_{uid}"), "Edit Steam ID"), SB::edit(format!("player_settings_edit_elo_{uid}"), "Edit ELO")]));

  if !ranks.is_empty() {
    // Detect duplicate rank names
    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for rank in ranks.iter().take(25) {
      *name_counts.entry(&rank.name).or_insert(0) += 1;
    }

    menu = menu.row(SR::StringSelect {
      id: format!("player_settings_rank_select_{uid}"),
      placeholder: "Edit rank...".to_string(),
      options: ranks
        .iter()
        .take(25)
        .map(|rank: &GuildRank| {
          let label = if name_counts.get(rank.name.as_str()).copied().unwrap_or(0) > 1 {
            format!("{} (ELO: {}, ID {})", rank.name, rank.elo, rank.role_id.get())
          } else {
            format!("{} (ELO: {})", rank.name, rank.elo)
          };
          (label, rank.role_id.get().to_string())
        })
        .collect(),
    });
  }

  menu = menu.row(SR::Buttons(vec![SB::edit(format!("player_settings_edit_alerts_{uid}"), "Edit alerts")]));

  (menu.build_embed(), menu.build_components())
}
