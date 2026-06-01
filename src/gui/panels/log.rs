//! Log panel for viewing bot logs

use crate::gui::state::GuiSharedState;
use egui::{self, ScrollArea};

pub fn show_log_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
  // Read log buffer
  let logs = if let Ok(buffer) = state.log_buffer.try_lock() { buffer.iter().cloned().collect::<Vec<_>>() } else { vec!["Error: Could not acquire log buffer lock".to_string()] };

  // Check if user is selecting text and near edges for auto-scroll
  let is_selecting = ui.input(|i| i.pointer.any_down());
  let scroll_delta = if is_selecting {
    if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
      let viewport = ui.clip_rect();
      let edge_threshold = 50.0;
      let scroll_speed = 10.0;

      // Auto-scroll when near bottom edge
      if pointer_pos.y > viewport.max.y - edge_threshold {
        ui.ctx().request_repaint();
        scroll_speed
      }
      // Auto-scroll when near top edge
      else if pointer_pos.y < viewport.min.y + edge_threshold {
        ui.ctx().request_repaint();
        -scroll_speed
      } else {
        0.0
      }
    } else {
      0.0
    }
  } else {
    0.0
  };

  let scroll_id = ui.id().with("log_scroll");
  let stick_to_bottom = !is_selecting;

  // Apply scroll delta if needed
  if scroll_delta != 0.0 {
    let mut scroll_state = egui::scroll_area::State::load(ui.ctx(), scroll_id).unwrap_or_default();
    scroll_state.offset.y += scroll_delta;
    scroll_state.store(ui.ctx(), scroll_id);
  }

  ScrollArea::vertical().id_source(scroll_id).stick_to_bottom(stick_to_bottom).show(ui, |ui: &mut egui::Ui| {
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
