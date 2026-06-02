//! Main egui application

use eframe::egui;
use std::sync::Arc;
use std::time::SystemTime;

use crate::gui::panels::{admin, broadcast, log, queue, settings, users};
use crate::gui::state::GuiSharedState;

/// Main egui application struct
pub struct MyApp {
  state: Arc<GuiSharedState>,
  selected_tab: PanelTab,
  should_quit: bool,
  next_clock_tick: std::time::Instant,
  broadcast_panel: broadcast::BroadcastPanel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelTab {
  Logs,
  Queue,
  Users,
  Admin,
  Settings,
  Broadcast,
}

impl MyApp {
  pub fn new(state: Arc<GuiSharedState>) -> Self {
    Self {
      state,
      selected_tab: PanelTab::Logs,
      should_quit: false,
      next_clock_tick: Self::next_second_instant(),
      broadcast_panel: broadcast::BroadcastPanel::default(),
    }
  }

  fn next_second_instant() -> std::time::Instant {
    let now_sys = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    let millis_until_next = 1000 - (now_sys.as_millis() % 1000);
    std::time::Instant::now() + std::time::Duration::from_millis(millis_until_next as u64)
  }

  fn trigger_shutdown(&mut self) {
    if let Ok(mut tx_lock) = self.state.shutdown_tx.try_lock() {
      if let Some(shutdown_tx) = tx_lock.take() {
        let _ = shutdown_tx.send(());
      }
    }
    // Drop cmd_tx so the command receiver gets None and exits
    if let Ok(mut tx_lock) = self.state.cmd_tx.try_lock() {
      tx_lock.take();
    }
    self.should_quit = true;
  }

  fn refresh_data(&self) {
    // Trigger a manual refresh by sending a command
    self.state.send_cmd(crate::gui::commands::GuiCommand::RefreshSnapshot);
  }
}

impl eframe::App for MyApp {
  fn on_exit(&mut self) {
    // Trigger shutdown when GUI closes
    self.trigger_shutdown();
  }

  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    let ctx = ui.ctx();

    // Apply theme and font settings
    if let Ok(settings) = self.state.gui_settings.try_read() {
      // Apply theme
      let visuals = match settings.theme {
        crate::gui::state::ThemeChoice::Dark => egui::Visuals::dark(),
        crate::gui::state::ThemeChoice::Light => egui::Visuals::light(),
      };
      ctx.set_visuals(visuals);

      // Apply font size
      let mut style = (*ctx.style()).clone();
      style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(settings.font_size, egui::FontFamily::Proportional),
      );
      style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(settings.font_size, egui::FontFamily::Proportional),
      );
      style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(settings.font_size * 1.3, egui::FontFamily::Proportional),
      );
      ctx.set_style(style);
    }

    // Handle keyboard shortcuts
    ctx.input(|i| {
      if i.modifiers.ctrl && i.key_pressed(egui::Key::Q) {
        self.trigger_shutdown();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
      }
      if i.modifiers.ctrl && i.key_pressed(egui::Key::R) {
        self.refresh_data();
      }
    });

    // Top bar with title and status
    egui::Panel::top("top_bar").show(ctx, |ui| {
      ui.horizontal(|ui| {
        ui.heading("qBot Host Management Panel");
        ui.separator();

        // Connection status indicator
        let manager_opt = if let Ok(latest) = self.state.latest_manager.try_read() { latest.clone() } else { None };

        if let Some(manager) = manager_opt {
          if !manager.qguilds.is_empty() {
            let plural = if manager.qguilds.len() == 1 { "" } else { "s" };
            ui.label(format!("{} guild{} and counting!", manager.qguilds.len(), plural));
          }
        } else {
          ui.label("Waiting for data...");
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
          let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(now, 0).unwrap().format("%H:%M:%S").to_string();
          ui.label(datetime);
        });
      });
    });

    // Tab navigation with action buttons
    egui::Panel::top("tab_bar").show(ctx, |ui| {
      ui.horizontal(|ui| {
        ui.selectable_value(&mut self.selected_tab, PanelTab::Logs, "Logs");
        ui.selectable_value(&mut self.selected_tab, PanelTab::Queue, "Queues");
        ui.selectable_value(&mut self.selected_tab, PanelTab::Users, "Users");
        ui.selectable_value(&mut self.selected_tab, PanelTab::Admin, "Admin");
        ui.selectable_value(&mut self.selected_tab, PanelTab::Settings, "Settings");
        ui.selectable_value(&mut self.selected_tab, PanelTab::Broadcast, "Broadcast");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          if ui.button("Quit").clicked() {
            self.state.send_cmd(crate::gui::commands::GuiCommand::GracefulShutdown);
          }
          if ui.button("Restart").clicked() {
            self.state.send_cmd(crate::gui::commands::GuiCommand::GracefulRestart);
          }
          if ui.button("Refresh").clicked() {
            self.refresh_data();
          }
        });
      });
    });

    // Main content area
    egui::CentralPanel::default().show(ctx, |ui| match self.selected_tab {
      PanelTab::Logs => log::show_log_panel(ui, &self.state),
      PanelTab::Queue => queue::show_queue_panel(ui, &self.state),
      PanelTab::Users => users::show_users_panel(ui, &self.state),
      PanelTab::Admin => admin::show_admin_panel(ui, &self.state),
      PanelTab::Settings => settings::show_settings_panel(ui, &self.state),
      PanelTab::Broadcast => {
        let mut gui_state = crate::gui::state::GuiState::from_shared(&self.state);
        gui_state.shared_state = Some(self.state.clone());
        let gui_state = Arc::new(tokio::sync::Mutex::new(gui_state));
        self.broadcast_panel.ui(ui, gui_state);
      }
    });

    if self.should_quit {
      ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // Check if graceful shutdown is complete, then close window
    if self.state.shutdown_complete.load(std::sync::atomic::Ordering::Relaxed) {
      ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // Request repaint at the next whole second boundary to keep clock synchronized.
    // next_clock_tick is stored in the struct so multiple repaints within the same tick
    // reuse the same Instant without recalculating.
    let now = std::time::Instant::now();
    if now >= self.next_clock_tick {
      self.next_clock_tick = Self::next_second_instant();
    }
    ctx.request_repaint_after(self.next_clock_tick.saturating_duration_since(now));
  }
}
