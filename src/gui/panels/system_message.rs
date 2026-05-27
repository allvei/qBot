use egui::{ScrollArea, TextEdit, Ui};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::gui::state::GuiState;

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
    ui.add_sized([f32::INFINITY, 150.0], TextEdit::multiline(&mut self.message_content).desired_width(f32::INFINITY));
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
      let mut guilds: Vec<_> = state_guard.guilds.iter().map(|(id, name)| (*id, name.clone())).collect();
      let sys_msg_guilds = state_guard.system_message_channel_guilds.clone();
      drop(state_guard);

      // Sort by name for stable dropdown order
      guilds.sort_by(|a, b| a.1.cmp(&b.1));

      // Filter to only show guilds with configured system message channel
      let guilds_with_channel: Vec<_> = guilds
        .into_iter()
        .filter(|(guild_id, _)| sys_msg_guilds.contains(&guild_id.get()))
        .collect();

      info!("UI: sys_msg_guilds.len={}, guilds_with_channel.len={}, selected_guild_index={}", sys_msg_guilds.len(), guilds_with_channel.len(), self.selected_guild_index);

      // Reset index if out of bounds
      if self.selected_guild_index >= guilds_with_channel.len() && !guilds_with_channel.is_empty() {
        self.selected_guild_index = 0;
      }

      if !guilds_with_channel.is_empty() {
        egui::ComboBox::from_label("Select server").selected_text(guilds_with_channel.get(self.selected_guild_index).map(|(_, name)| name.as_str()).unwrap_or("Select...")).show_ui(ui, |ui| {
          for (idx, (_, name)) in guilds_with_channel.iter().enumerate() {
            ui.selectable_value(&mut self.selected_guild_index, idx, name);
          }
        });
      } else {
        ui.label("No servers with configured system message channel");
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
    let guilds: Vec<_> = state_guard.guilds.iter().map(|(id, name)| (*id, name.clone())).collect();
    let sys_msg_guilds = state_guard.system_message_channel_guilds.clone();
    drop(state_guard);

    // Sort by name for stable dropdown order (same as in ui())
    let mut sorted_guilds = guilds.clone();
    sorted_guilds.sort_by(|a, b| a.1.cmp(&b.1));

    // Filter to only show guilds with configured system message channel (same as in ui())
    let guilds_with_channel: Vec<_> = sorted_guilds
      .into_iter()
      .filter(|(guild_id, _)| sys_msg_guilds.contains(&guild_id.get()))
      .collect();

    info!("send_message: sys_msg_guilds.len={}, guilds.len={}, filtered_guilds.len={}, selected_guild_index={}", sys_msg_guilds.len(), guilds.len(), guilds_with_channel.len(), self.selected_guild_index);

    // Reset index if out of bounds
    if self.selected_guild_index >= guilds_with_channel.len() && !guilds_with_channel.is_empty() {
      info!("Resetting selected_guild_index from {} to 0 (out of bounds)", self.selected_guild_index);
      self.selected_guild_index = 0;
    }

    let guild_id = if self.send_to_all {
      None
    } else {
      guilds_with_channel.get(self.selected_guild_index).map(|(id, _)| id.get())
    };

    info!("Selected guild_id: {:?}", guild_id);

    // Send command to bot thread instead of spawning directly
    let state_guard = state.blocking_lock();
    if let Some(shared_state) = state_guard.shared_state.as_ref() {
      shared_state.send_cmd(crate::gui::commands::GuiCommand::SendSystemMessage {
        guild_id,
        message: message.clone(),
      });
      self.last_result = Some(if self.send_to_all {
        "Sending to all servers...".to_string()
      } else {
        format!("Sending to guild {}...", guild_id.unwrap_or(0))
      });
    } else {
      self.last_result = Some("Error: Bot not initialized".to_string());
    }
  }

  fn validate_channels(&mut self, state: Arc<Mutex<GuiState>>) {
    let state_guard = state.blocking_lock();
    if let Some(shared_state) = state_guard.shared_state.as_ref() {
      shared_state.send_cmd(crate::gui::commands::GuiCommand::ValidateSystemMessageChannels);
      self.last_result = Some("Validating channels... Check logs for results.".to_string());
    } else {
      self.last_result = Some("Error: Bot not initialized".to_string());
    }
  }
}
