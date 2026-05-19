//! Log panel for viewing bot logs

use crate::gui::state::GuiSharedState;
use egui::{self, ScrollArea};

pub fn show_log_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
  // Read log buffer
  let logs = if let Ok(buffer) = state.log_buffer.try_lock() { buffer.iter().cloned().collect::<Vec<_>>() } else { vec!["Error: Could not acquire log buffer lock".to_string()] };

  ScrollArea::vertical().stick_to_bottom(true).max_height(600.0).show(ui, |ui: &mut egui::Ui| {
    if logs.is_empty() {
      ui.vertical_centered(|ui| {
        ui.add_space(50.0);
        ui.heading("No Logs Yet");
        ui.add_space(20.0);
        ui.label("Waiting for the bot to generate log messages...");
        ui.add_space(10.0);
        ui.label("Logs will appear here as the bot operates.");
        ui.add_space(50.0);
      });
    } else {
      for log in logs.iter() {
        ui.label(log);
      }
    }
  });

  // TODO: Add these features:
  // - Filter by log level (INFO, WARN, ERROR)
  // - Search/filter text input
  // - Copy to clipboard button
  // - Clear buffer button
  // - Auto-scroll to bottom toggle
}
