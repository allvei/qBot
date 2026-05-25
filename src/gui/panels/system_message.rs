use egui::{ScrollArea, TextEdit, Ui};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::gui::state::GuiState;
use crate::services;

/// System message panel for sending custom updates to guilds
pub struct SystemMessagePanel {
  message_content: String,
  send_to_all: bool,
  selected_guild_index: usize,
  last_result: Option<String>,
}

impl Default for SystemMessagePanel {
  fn default() -> Self {
    Self { message_content: String::new(), send_to_all: true, selected_guild_index: 0, last_result: None }
  }
}

impl SystemMessagePanel {
  pub fn ui(&mut self, ui: &mut Ui, state: Arc<Mutex<GuiState>>) {
    ui.heading("System Message Broadcast");
    ui.separator();

    ui.label("Compose a system message to send to configured channels:");
    ui.add_space(10.0);

    // Message input
    ui.label("Message:");
    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
      ui.add(TextEdit::multiline(&mut self.message_content).desired_width(f32::INFINITY).desired_rows(8));
    });
    ui.add_space(10.0);

    // Target selection
    ui.horizontal(|ui| {
      ui.label("Send to:");
      if ui.radio(self.send_to_all, "All servers").clicked() {
        self.send_to_all = true;
      }
      if ui.radio(!self.send_to_all, "Specific server").clicked() {
        self.send_to_all = false;
      }
    });

    // Guild selector (only shown if not sending to all)
    if !self.send_to_all {
      ui.add_space(5.0);
      let state_guard = state.blocking_lock();
      let guilds: Vec<_> = state_guard.guilds.iter().map(|(id, name)| (*id, name.clone())).collect();
      drop(state_guard);

      if !guilds.is_empty() {
        egui::ComboBox::from_label("Select server").selected_text(guilds.get(self.selected_guild_index).map(|(_, name)| name.as_str()).unwrap_or("Select...")).show_ui(ui, |ui| {
          for (idx, (_, name)) in guilds.iter().enumerate() {
            ui.selectable_value(&mut self.selected_guild_index, idx, name);
          }
        });
      } else {
        ui.label("No servers available");
      }
    }

    ui.add_space(10.0);

    // Send button
    let can_send = !self.message_content.trim().is_empty();
    if ui.add_enabled(can_send, egui::Button::new("Send Message")).clicked() {
      self.send_message(state.clone());
    }

    // Display last result
    if let Some(ref result) = self.last_result {
      ui.add_space(10.0);
      ui.separator();
      ui.label("Last Result:");
      ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
        ui.label(result);
      });
    }

    ui.add_space(10.0);
    ui.separator();

    // Validation section
    if ui.button("Validate System Message Channels").clicked() {
      self.validate_channels(state.clone());
    }
  }

  fn send_message(&mut self, state: Arc<Mutex<GuiState>>) {
    let message = self.message_content.trim().to_string();
    if message.is_empty() {
      self.last_result = Some("Error: Message cannot be empty".to_string());
      return;
    }

    let state_guard = state.blocking_lock();
    let ctx = state_guard.ctx.clone();
    let db = state_guard.db.clone();
    let guilds: Vec<_> = state_guard.guilds.iter().map(|(id, _)| *id).collect();
    drop(state_guard);

    if let (Some(ctx), Some(db)) = (ctx, db) {
      if self.send_to_all {
        // Send to all guilds
        tokio::spawn(async move {
          match services::broadcast_system_message(&ctx, &db, &message).await {
            Ok(results) => {
              let mut success_count = 0;
              let mut error_messages = Vec::new();

              for (guild_id, result) in results {
                match result {
                  Ok(_) => success_count += 1,
                  Err(e) => error_messages.push(format!("Guild {}: {}", guild_id, e)),
                }
              }

              let summary = if error_messages.is_empty() {
                format!("Successfully sent to {} server(s)", success_count)
              } else {
                format!("Sent to {} server(s), {} failed:\n{}", success_count, error_messages.len(), error_messages.join("\n"))
              };

              info!("{}", summary);
            }
            Err(e) => {
              error!("Failed to broadcast system message: {}", e);
            }
          }
        });
        self.last_result = Some("Sending to all servers...".to_string());
      } else {
        // Send to specific guild
        if let Some(&guild_id) = guilds.get(self.selected_guild_index) {
          let message_clone = message.clone();
          tokio::spawn(async move {
            match services::send_system_message(&ctx, &db, guild_id, &message_clone).await {
              Ok(_) => info!("System message sent to guild {}", guild_id),
              Err(e) => error!("Failed to send system message to guild {}: {}", guild_id, e),
            }
          });
          self.last_result = Some(format!("Sending to guild {}...", guild_id));
        } else {
          self.last_result = Some("Error: No guild selected".to_string());
        }
      }

      // Clear message after sending
      self.message_content.clear();
    } else {
      self.last_result = Some("Error: Bot not initialized".to_string());
    }
  }

  fn validate_channels(&mut self, state: Arc<Mutex<GuiState>>) {
    let state_guard = state.blocking_lock();
    let ctx = state_guard.ctx.clone();
    let db = state_guard.db.clone();
    drop(state_guard);

    if let (Some(ctx), Some(db)) = (ctx, db) {
      tokio::spawn(async move {
        let errors = services::validate_system_message_channels(&ctx, &db).await;

        if errors.is_empty() {
          info!("All system message channels are valid");
        } else {
          error!("Found {} guild(s) with invalid system message channels:", errors.len());
          for (guild_id, guild_name, error) in errors {
            error!("[{}] {}: {}", guild_id, guild_name, error);
          }
        }
      });
      self.last_result = Some("Validating channels... Check logs for results.".to_string());
    } else {
      self.last_result = Some("Error: Bot not initialized".to_string());
    }
  }
}
