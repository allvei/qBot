//! Settings panel (optional)

use crate::gui::state::GuiSharedState;
use egui::{self};

pub fn show_settings_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
    ui.heading("Settings");
    ui.separator();

    // Connection Status Panel (left side)
    ui.columns(2, |cols| {
        cols[0].group(|ui| {
            ui.heading("GUI Settings");
            ui.separator();
            ui.label("Log Buffer Size: 1000 lines");
            ui.label("Snapshot Interval: 100ms");
            ui.label("Theme: System Default");
        });
    });

    ui.separator();

    // Actions Panel
    ui.group(|ui| {
        ui.heading("Actions");
        ui.separator();
        if ui.button("Refresh Data").clicked() {
            let _ = state.cmd_tx.try_send(crate::gui::commands::GuiCommand::RefreshSnapshot);
        }
    });

    ui.separator();

    // TODO list panel
    ui.group(|ui| {
        ui.heading("Planned Features");
        ui.separator();
        ui.label("- Edit config values live");
        ui.label("- View database connection status");
        ui.label("- View active Discord gateway connection status");
        ui.label("- Theme selection");
        ui.label("- Font size adjustment");
        ui.label("- Panel layout preferences");
    });
}
