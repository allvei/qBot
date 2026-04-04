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
        Self { state, selected_tab: PanelTab::Queue }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                    if manager.qguilds.is_empty() {
                        ui.label("🔴 No guilds connected");
                    } else {
                        ui.label(format!("🟢 Connected to {} guild(s)", manager.qguilds.len()));
                    }
                } else {
                    ui.label("🟡 Waiting for data...");
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

        // Tab navigation
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, PanelTab::Logs, "📋 Logs");
                ui.selectable_value(&mut self.selected_tab, PanelTab::Queue, "🎮 Queue / Games");
                ui.selectable_value(&mut self.selected_tab, PanelTab::Admin, "⚙️ Admin");
                ui.selectable_value(&mut self.selected_tab, PanelTab::Settings, "🔧 Settings");
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
    }
}
