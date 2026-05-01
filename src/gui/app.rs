//! Main egui application

use std::sync::Arc;
use std::time::SystemTime;
use eframe::egui;

use crate::gui::panels::{admin, log, queue, settings};
use crate::gui::state::GuiSharedState;

/// Main egui application struct
pub struct MyApp {
    state: Arc<GuiSharedState>,
    selected_tab: PanelTab,
    should_quit: bool,
    next_clock_tick: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelTab {
    Logs,
    Queue,
    Admin,
    Settings,
}

impl MyApp {
    pub fn new(state: Arc<GuiSharedState>) -> Self {
        Self { state, selected_tab: PanelTab::Queue, should_quit: false, next_clock_tick: Self::next_second_instant() }
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
        let _ = self.state.send_cmd(crate::gui::commands::GuiCommand::RefreshSnapshot);
    }
}

impl eframe::App for MyApp {
    fn on_exit(&mut self) {
        // Trigger shutdown when GUI closes
        self.trigger_shutdown();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx();
        
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
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("qBot Host Management Panel");
                ui.separator();

                // Connection status indicator
                let manager_opt = if let Ok(latest) = self.state.latest_manager.try_read() {
                    latest.clone()
                } else {
                    None
                };

                if let Some(manager) = manager_opt {
                    if !manager.qguilds.is_empty() {
                        let plural = if manager.qguilds.len() == 1 { "" } else { "s" };
                        ui.label(format!("{} guild{} and counting!", manager.qguilds.len(), plural));
                    }
                } else {
                    ui.label("Waiting for data...");
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let now = SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(now, 0)
                        .unwrap()
                        .format("%H:%M:%S")
                        .to_string();
                    ui.label(datetime);
                });
            });
        });

        // Tab navigation with action buttons
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, PanelTab::Logs, "Logs");
                ui.selectable_value(&mut self.selected_tab, PanelTab::Queue, "Queues");
                ui.selectable_value(&mut self.selected_tab, PanelTab::Admin, "Admin");
                ui.selectable_value(&mut self.selected_tab, PanelTab::Settings, "Settings");
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        self.trigger_shutdown();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh_data();
                    }
                });
            });
        });

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                PanelTab::Logs => log::show_log_panel(ui, &self.state),
                PanelTab::Queue => queue::show_queue_panel(ui, &self.state),
                PanelTab::Admin => admin::show_admin_panel(ui, &self.state),
                PanelTab::Settings => settings::show_settings_panel(ui, &self.state),
            }
        });

        if self.should_quit {
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
