//! Settings panel (optional)

use crate::gui::state::GuiSharedState;
use egui::{self};

pub fn show_settings_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
    ui.heading("Settings");
    ui.separator();

    ui.collapsing("Connection Status", |ui| {
        ui.label("📊 Bot Status: Running");
        ui.label("🔌 Discord Gateway: Connected");
        ui.label("💾 Database: Connected");
    });

    ui.collapsing("GUI Settings", |ui| {
        ui.label("📋 Log Buffer Size: 1000 lines");
        ui.label("🔄 Snapshot Interval: 100ms");
        ui.label("🎨 Theme: System Default");
    });

    ui.separator();

    ui.collapsing("Actions", |ui| {
        if ui.button("🔄 Refresh Data").clicked() {
            // TODO: Trigger manual snapshot refresh
        }

        if ui.button("⏹️ Shutdown Bot").clicked() {
            // TODO: Implement shutdown trigger
            // state.shutdown_tx.take()?.send(())?;
            ui.label("⚠️ Shutdown not yet implemented");
        }
    });

    ui.separator();

    ui.label("(TODO: Implement:");
    ui.label("  - Edit config values live");
    ui.label("  - View database connection status");
    ui.label("  - View active Discord gateway connection status");
    ui.label("  - Theme selection");
    ui.label("  - Font size adjustment");
    ui.label("  - Panel layout preferences");
}
