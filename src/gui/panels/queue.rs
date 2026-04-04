//! Queue/Game view panel

use crate::gui::state::GuiSharedState;
use egui::{self, FontFamily, RichText, ScrollArea};
use egui_phosphor::regular;

pub fn show_queue_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
    // Read latest manager snapshot
    let manager_opt = if let Ok(latest) = state.latest_manager.try_read() {
        latest.clone()
    } else {
        None
    };

    ScrollArea::vertical()
        .max_height(600.0)
        .show(ui, |ui| {
            if let Some(manager) = manager_opt {
                if manager.qguilds.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);
                        ui.heading("No Guilds Connected");
                        ui.add_space(20.0);
                        ui.label("Waiting for the bot to connect to Discord servers...");
                        ui.add_space(10.0);
                        ui.label("Make sure the bot is running and has access to your servers.");
                        ui.add_space(50.0);
                    });
                } else {
                    for guild in &manager.qguilds {
                        let guild_id = guild.id;
                        let response = ui.collapsing(format!("{}", guild.name), |ui| {
                            if guild.categories.is_empty() {
                                ui.label("No categories configured - run `/setup` in Discord");
                            } else {
                                for category in &guild.categories {
                                    let cat_name = category.name.as_deref().unwrap_or("Unnamed");
                                    ui.collapsing(format!("{}", cat_name), |ui| {
                                        if category.formats.is_empty() {
                                            ui.label("No formats configured");
                                        } else {
                                            for format in &category.formats {
                                                ui.collapsing(format!("{} (quota: {})", format.name, format.quota), |ui| {
                                                    if format.sessions.is_empty() {
                                                        ui.label("No sessions initiated");
                                                    } else {
                                                        for (i, session) in format.sessions.iter().enumerate() {
                                                            ui.horizontal(|ui| {
                                                                ui.label(session.status.icon());
                                                                ui.label(format!("Session {} - {}", i, session.pool.len()));
                                                                ui.label(RichText::new(regular::USER).family(FontFamily::Name("phosphor".into())));
                                                            });

                                                            if !session.pool.is_empty() {
                                                                ui.indent(format!("players_{}", i), |ui: &mut egui::Ui| {
                                                                    for session_player in &session.pool {

                                                                        let team_str = match session_player.team {
                                                                            Some(crate::models::Team::Red) => "RED ",
                                                                            Some(crate::models::Team::Blu) => "BLU ",
                                                                            Some(crate::models::Team::Unassigned) => "",
                                                                            None => "",
                                                                        };
                                                                        ui.horizontal(|ui| {
                                                                            ui.label(session_player.vc_icon());
                                                                            ui.label(format!("{}{}‹{}›", team_str, session_player.player.tag, session_player.player.elo));
                                                                        });
                                                                    }
                                                                });
                                                            }
                                                        }
                                                    }
                                                });
                                            }
                                        }
                                    });
                                }
                            }
                        });

                        response.header_response.context_menu(|ui| {
                            let guild_id_str = format!("{}", guild_id);
                            if ui.button("Copy Guild ID").clicked() {
                                ui.ctx().copy_text(guild_id_str.clone());
                                ui.close_menu();
                            }
                        });
                    }
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    ui.heading("Waiting for Bot Data");
                    ui.add_space(20.0);
                    ui.label("The bot is starting up...");
                    ui.label("Please wait a moment for data to load.");
                    ui.add_space(50.0);
                });
            }
        });
}

