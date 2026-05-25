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
        ui.label("Font Size:");
        let mut font_size = settings.font_size;
        if ui.add(egui::Slider::new(&mut font_size, 10.0..=20.0).suffix(" px")).changed() {
          settings.font_size = font_size;
          settings.save();
        }
      });

      ui.add_space(5.0);

      // Log buffer size
      ui.horizontal(|ui| {
        ui.label("Log Buffer:");
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
    ui.heading("Connection Status");
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
      ui.label("Pool Size:");
      ui.label(format!("{} connections", state.db.pool.size()));
    });

    ui.add_space(5.0);

    // Discord gateway status (check if context is available)
    ui.horizontal(|ui| {
      ui.label("Discord Gateway:");
      if state.ctx.is_some() {
        ui.label(RichText::new("● Connected").color(Color32::GREEN));
      } else {
        ui.label(RichText::new("● Connecting...").color(Color32::YELLOW));
      }
    });

    if let Some(ctx) = &state.ctx {
      ui.horizontal(|ui| {
        ui.label("Guilds:");
        ui.label(format!("{} loaded", state.guilds.len()));
      });

      ui.horizontal(|ui| {
        ui.label("Shard:");
        ui.label(format!("{}", ctx.shard_id));
      });
    }
  });
}

fn show_config_editor(ui: &mut egui::Ui, state: &GuiSharedState) {
  ui.group(|ui| {
    ui.heading("Live Config Editor");
    ui.separator();

    ui.label(RichText::new("Edit server configuration in real-time").small());
    ui.add_space(5.0);

    // Guild selector
    static mut SELECTED_GUILD: Option<u64> = None;
    
    ui.horizontal(|ui| {
      ui.label("Guild:");
      egui::ComboBox::from_id_source("guild_selector")
        .selected_text(unsafe {
          SELECTED_GUILD
            .and_then(|id| state.guilds.get(&serenity::all::GuildId::new(id)))
            .map(|s| s.as_str())
            .unwrap_or("Select guild...")
        })
        .show_ui(ui, |ui| {
          for (guild_id, guild_name) in &state.guilds {
            let id = guild_id.get();
            if ui.selectable_label(unsafe { SELECTED_GUILD } == Some(id), guild_name).clicked() {
              unsafe { SELECTED_GUILD = Some(id); }
            }
          }
        });
    });

    ui.add_space(5.0);

    // Show config options for selected guild
    unsafe {
      if let Some(guild_id) = SELECTED_GUILD {
        ui.label(RichText::new("Server Config Options:").strong());
        ui.add_space(3.0);

        // Example config toggles (can be expanded)
        ui.checkbox(&mut false, "ELO-Rank Linked");
        ui.checkbox(&mut false, "Dynamic ELO Enabled");
        ui.checkbox(&mut false, "Post-game Auto-remove");
        ui.checkbox(&mut false, "Hide ELO");

        ui.add_space(5.0);
        if ui.button("💾 Save Changes").clicked() {
          // TODO: Send command to update config
          ui.ctx().debug_text(format!("Save config for guild {}", guild_id));
        }

        ui.label(RichText::new("Note: Live config editing coming soon").small().italics());
      } else {
        ui.label(RichText::new("Select a guild to edit configuration").weak());
      }
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
    ui.heading("System Info");
    ui.separator();

    if let Ok(manager_opt) = state.latest_manager.try_read() {
      if let Some(manager) = manager_opt.as_ref() {
        ui.label(format!("Active Guilds: {}", manager.qguilds.len()));
        
        let session_count: usize = manager.qguilds.iter()
          .flat_map(|g| &g.categories)
          .flat_map(|c| &c.formats)
          .map(|f| f.sessions.len())
          .sum();
        ui.label(format!("Total Sessions: {}", session_count));

        let active_sessions: usize = manager.qguilds.iter()
          .flat_map(|g| &g.categories)
          .flat_map(|c| &c.formats)
          .flat_map(|f| &f.sessions)
          .filter(|s| s.is_active())
          .count();
        ui.label(format!("Active Games: {}", active_sessions));
      } else {
        ui.label("Waiting for data...");
      }
    } else {
      ui.label("Manager locked");
    }
  });
}
