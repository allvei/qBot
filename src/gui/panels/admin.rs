//! Admin panel for bot management commands

use crate::gui::state::GuiSharedState;
use egui::{self, ScrollArea};

pub fn show_admin_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
    ui.heading("Admin Commands");
    ui.label("Manage bot state and execute administrative actions");
    ui.separator();

    ScrollArea::vertical()
        .max_height(600.0)
        .show(ui, |ui| {
            ui.collapsing("Queue Management", |ui| {
                ui.label("Manage queues and sessions:");
                ui.separator();
                ui.label("• Force-end game - End a live game immediately");
                ui.label("• Clear queue - Remove all players from queue");
                ui.label("• Add player - Manually add a player to queue");
                ui.label("• Reorder queue - Move player to specific position");
                ui.label("• Remove player - Remove specific player from queue");
                ui.label("• Move player between sessions - For concurrent games");
                ui.label("• Force session state - Manually set session status");
                ui.label("• Reset session timer - Clear confirm timeout");
                ui.label("• Force team regeneration - Regenerate teams");
                ui.label("• Swap teams - Swap Red/Blu teams");
                ui.separator();
                ui.label("(TODO: Implement command UI with:");
                ui.label("  - Input fields for parameters");
                ui.label("  - Confirmation dialogs");
                ui.label("  - Result feedback");
            });

            ui.collapsing("Recovery from Bugs", |ui| {
                ui.label("Recover from stuck states:");
                ui.separator();
                ui.label("• Clear all team VCs - Force delete all team channels");
                ui.label("• Reset category state - Full category reset");
                ui.label("• Remove orphaned sessions - Delete sessions with no players");
                ui.label("• Fix player VC state - Reset stuck VC flags");
                ui.label("• Clear pending team switches - Remove uncommitted switches");
                ui.label("• Reset voice state tracking - Clear VC cache");
                ui.label("• Recover from database - Reload category from DB");
                ui.separator();
                ui.label("(TODO: Implement command UI)");
            });

            ui.collapsing("Voice Channel Management", |ui| {
                ui.label("Manage voice channels:");
                ui.separator();
                ui.label("• Move player to VC - Force move to specific channel");
                ui.label("• Kick from VC - Disconnect from voice");
                ui.label("• Sync VC state - Resync in_queue_vc flags");
                ui.separator();
                ui.label("(TODO: Implement command UI)");
            });

            ui.collapsing("Debugging/Development", |ui| {
                ui.label("Debug and development tools:");
                ui.separator();
                ui.label("• Dump state to log - Export full state");
                ui.label("• Toggle debug mode - Enable verbose logging");
                ui.label("• Test Discord API - Ping gateway");
                ui.label("• View session details - Raw JSON display");
                ui.separator();
                ui.label("(TODO: Implement command UI)");
            });

            ui.collapsing("Testing/Load Testing", |ui| {
                ui.label("Testing and load simulation:");
                ui.separator();
                ui.label("• Add dummy players - Add test accounts");
                ui.label("• Simulate game flow - Auto-run game states");
                ui.label("• Trigger concurrent games - Start multiple games");
                ui.label("• Test balance methods - Compare algorithms");
                ui.label("• Force quota met - Trigger hot_fmt regardless");
                ui.label("• Simulate VC timeout - Test confirm timeout");
                ui.separator();
                ui.label("(TODO: Implement command UI)");
            });
        });

    ui.separator();
    ui.label("Tip: Commands will execute on the bot thread and results will appear in the Logs tab");
}

