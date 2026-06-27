//! Settings panel with theme, font size, connection status, and config editor

use crate::gui::state::{GuiSharedState, ThemeChoice};
use egui::{self, Color32, RichText};

pub fn show_settings_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
  egui::ScrollArea::vertical().show(ui, |ui| {
    ui.heading("Settings");
    ui.separator();

    // Two-column layout for better organization
    ui.columns(2, |cols| {
      // Left column: Appearance & Preferences
      cols[0].vertical(|ui| {
        show_appearance_settings(ui, state);
        ui.add_space(10.0);
        show_connection_status(ui, state);
      });

      // Right column: Config Editor & Actions
      cols[1].vertical(|ui| {
        show_config_editor(ui, state);
        ui.add_space(10.0);
        show_actions(ui, state);
      });
    });
  });
}

fn show_appearance_settings(ui: &mut egui::Ui, state: &GuiSharedState) {
  ui.group(|ui| {
    ui.heading("Appearance");
    ui.separator();

    if let Ok(mut settings) = state.gui_settings.try_write() {
      // Theme selection
      ui.horizontal(|ui| {
        ui.label("Theme:");
        let mut changed = false;
        if ui.selectable_label(settings.theme == ThemeChoice::Dark, "Dark").clicked() {
          settings.theme = ThemeChoice::Dark;
          changed = true;
        }
        if ui.selectable_label(settings.theme == ThemeChoice::Light, "Light").clicked() {
          settings.theme = ThemeChoice::Light;
          changed = true;
        }
        if changed {
          settings.save();
        }
      });

      ui.add_space(5.0);

      // Font size adjustment
      ui.horizontal(|ui| {
        ui.label("Font size:");
        let mut font_size = settings.font_size;
        if ui.add(egui::Slider::new(&mut font_size, 10.0..=20.0).suffix(" px")).changed() {
          settings.font_size = font_size;
          settings.save();
        }
      });

      ui.add_space(5.0);

      // Log buffer size
      ui.horizontal(|ui| {
        ui.label("Log buffer:");
        let mut buffer_size = settings.log_buffer_size;
        if ui.add(egui::Slider::new(&mut buffer_size, 100..=5000).suffix(" lines")).changed() {
          settings.log_buffer_size = buffer_size;
          settings.save();
        }
      });

      ui.add_space(5.0);

      // Show current settings file location
      ui.label(RichText::new("Settings saved to: gui_settings.json").small().italics());
    } else {
      ui.label("Settings locked");
    }
  });
}

fn show_connection_status(ui: &mut egui::Ui, state: &GuiSharedState) {
  ui.group(|ui| {
    ui.heading("Connection status");
    ui.separator();

    // Database connection status
    ui.horizontal(|ui| {
      ui.label("Database:");
      let is_connected = state.db.pool.size() > 0;
      if is_connected {
        ui.label(RichText::new("● Connected").color(Color32::GREEN));
      } else {
        ui.label(RichText::new("● Disconnected").color(Color32::RED));
      }
    });

    ui.horizontal(|ui| {
      ui.label("Pool size:");
      ui.label(format!("{} connections", state.db.pool.size()));
    });

    ui.add_space(5.0);

    // Discord gateway status (check if context is available)
    ui.horizontal(|ui| {
      ui.label("Discord gateway:");
      let ctx_available = state.ctx.lock().map(|ctx| ctx.is_some()).unwrap_or(false);
      if ctx_available {
        ui.label(RichText::new("● Connected").color(Color32::GREEN));
      } else {
        ui.label(RichText::new("● Connecting...").color(Color32::YELLOW));
      }
    });

    if let Ok(ctx_guard) = state.ctx.lock() {
      if let Some(ctx) = &*ctx_guard {
        ui.horizontal(|ui| {
          ui.label("Guilds:");
          ui.label(format!("{} loaded", state.guilds.len()));
        });

        ui.horizontal(|ui| {
          ui.label("Shard:");
          ui.label(format!("{}", ctx.shard_id));
        });
      }
    }
  });
}

fn show_config_editor(ui: &mut egui::Ui, state: &GuiSharedState) {
  ui.group(|ui| {
    ui.heading("Live config editor");
    ui.separator();

    ui.label(RichText::new("Edit server configuration in real-time").small());
    ui.add_space(5.0);

    // Guild selector with persistent state
    use std::sync::Mutex;
    use lazy_static::lazy_static;
    
    lazy_static! {
      static ref SELECTED_GUILD: Mutex<Option<u64>> = Mutex::new(None);
      static ref CONFIG_CACHE: Mutex<std::collections::HashMap<String, bool>> = Mutex::new(std::collections::HashMap::new());
      static ref CONFIG_LOADED: Mutex<bool> = Mutex::new(false);
    }
    
    // Derive guilds from latest_manager snapshot
    let guilds: Vec<(u64, String)> = if let Ok(lock) = state.latest_manager.try_read() {
      lock.as_ref().map(|m| m.qguilds.iter().map(|g| (g.id.get(), g.name.clone())).collect()).unwrap_or_default()
    } else {
      Vec::new()
    };

    ui.horizontal(|ui| {
      ui.label("Guild:");
      let current_guild_id = *SELECTED_GUILD.lock().unwrap();
      let selected_text = current_guild_id
        .and_then(|id| guilds.iter().find(|(gid, _)| *gid == id).map(|(_, name)| name.as_str()))
        .unwrap_or("Select guild...");

      egui::ComboBox::from_id_source("guild_selector")
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
          for (id, guild_name) in &guilds {
            if ui.selectable_label(current_guild_id == Some(*id), guild_name).clicked() {
              *SELECTED_GUILD.lock().unwrap() = Some(*id);
              *CONFIG_LOADED.lock().unwrap() = false;
              state.send_cmd(crate::gui::commands::GuiCommand::LoadGuildConfig { guild_id: *id });
            }
          }
        });
    });

    ui.add_space(5.0);

    // Show config options for selected guild
    let selected_guild_id = *SELECTED_GUILD.lock().unwrap();
    
    if let Some(guild_id) = selected_guild_id {
      // Try to load config from cache
      if let Ok(cache) = state.guild_config_cache.try_read() {
        if let Some(config_map) = cache.get(&guild_id) {
          if !*CONFIG_LOADED.lock().unwrap() {
            // Populate local cache from shared state
            let mut local_cache = CONFIG_CACHE.lock().unwrap();
            local_cache.clear();
            for (key, value) in config_map {
              if value == "1" || value == "true" {
                local_cache.insert(key.clone(), true);
              } else if value == "0" || value == "false" {
                local_cache.insert(key.clone(), false);
              }
            }
            *CONFIG_LOADED.lock().unwrap() = true;
          }
        }
      }

      ui.label(RichText::new("Server config options:").strong());
      ui.add_space(3.0);

      // Display toggles from config_schema
      use crate::config_schema::server_config::TOGGLES;
      
      let mut changed_values = Vec::new();
      let mut local_cache = CONFIG_CACHE.lock().unwrap();
      
      for toggle in TOGGLES {
        let current_value = local_cache.get(toggle.column).copied().unwrap_or(toggle.default);
        let mut new_value = current_value;
        
        let label = if current_value { toggle.label_on } else { toggle.label_off };
        if ui.checkbox(&mut new_value, label).changed() {
          local_cache.insert(toggle.column.to_string(), new_value);
          changed_values.push((toggle.column.to_string(), new_value));
        }
      }

      // Send updates for changed values
      for (column, value) in changed_values {
        state.send_cmd(crate::gui::commands::GuiCommand::UpdateGuildConfigBool {
          guild_id,
          column,
          value,
        });
      }

      ui.add_space(5.0);
      
      if ui.button("� Reload Config").clicked() {
        *CONFIG_LOADED.lock().unwrap() = false;
        state.send_cmd(crate::gui::commands::GuiCommand::LoadGuildConfig { guild_id });
      }

      ui.add_space(3.0);
      ui.label(RichText::new("✓ Changes save automatically").small().color(Color32::from_rgb(100, 200, 100)));
    } else {
      ui.label(RichText::new("Select a guild to edit configuration").weak());
    }
  });
}

fn show_actions(ui: &mut egui::Ui, state: &GuiSharedState) {
  ui.group(|ui| {
    ui.heading("Actions");
    ui.separator();

    if ui.button("🔄 Refresh Data").clicked() {
      state.send_cmd(crate::gui::commands::GuiCommand::RefreshSnapshot);
    }

    ui.add_space(3.0);

    if ui.button("💾 Save GUI Settings").clicked() {
      if let Ok(settings) = state.gui_settings.try_read() {
        settings.save();
      }
    }

    ui.add_space(3.0);

    if ui.button("🔄 Reload GUI Settings").clicked() {
      if let Ok(mut settings) = state.gui_settings.try_write() {
        *settings = crate::gui::state::GuiSettings::load();
      }
    }

    ui.add_space(3.0);

    if ui.button("🗑️ Reset to Defaults").clicked() {
      if let Ok(mut settings) = state.gui_settings.try_write() {
        *settings = crate::gui::state::GuiSettings::default();
        settings.save();
      }
    }
  });

  ui.add_space(10.0);

  // System info panel
  ui.group(|ui| {
    ui.heading("System info");
    ui.separator();

    if let Ok(manager_opt) = state.latest_manager.try_read() {
      if let Some(manager) = manager_opt.as_ref() {
        ui.label(format!("Active guilds: {}", manager.qguilds.len()));
        
        let session_count: usize = manager.qguilds.iter()
          .flat_map(|g| &g.categories)
          .flat_map(|c| &c.formats)
          .map(|f| f.sessions.len())
          .sum();
        ui.label(format!("Total sessions: {}", session_count));

        let active_sessions: usize = manager.qguilds.iter()
          .flat_map(|g| &g.categories)
          .flat_map(|c| &c.formats)
          .flat_map(|f| &f.sessions)
          .filter(|s| s.is_active())
          .count();
        ui.label(format!("Active games: {}", active_sessions));
      } else {
        ui.label("Waiting for data...");
      }
    } else {
      ui.label("Manager locked");
    }
  });
}
