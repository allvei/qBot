//! Queue/Game view panel

use crate::gui::state::GuiSharedState;
use egui::{self, FontFamily, RichText, ScrollArea};
use egui_phosphor::regular;

pub fn show_queue_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
  // Read latest manager snapshot
  let manager_opt = if let Ok(latest) = state.latest_manager.try_read() { latest.clone() } else { None };

  ScrollArea::vertical().max_height(600.0).show(ui, |ui| {
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
          let guild_response = ui.collapsing(format!("{}", guild.name), |ui| {
            if guild.categories.is_empty() {
              ui.label("No categories configured - run `/setup` in Discord");
            } else {
              for category in &guild.categories {
                let cat_id = category.id;
                let cat_name = category.name.as_deref().unwrap_or("Unnamed");
                let cat_response = ui.collapsing(cat_name.to_string(), |ui| {
                  if category.formats.is_empty() {
                    ui.label("No formats configured");
                  } else {
                    for format in &category.formats {
                      let fmt_id = format.id;
                      let fmt_name = &format.name;
                      let fmt_response = ui.collapsing(format!("{} (quota: {})", fmt_name, format.quota), |ui| {
                        if format.sessions.is_empty() {
                          ui.label("No sessions initiated");
                        } else {
                          for (session_idx, session) in format.sessions.iter().enumerate() {
                            let session_response = ui.horizontal(|ui| {
                              ui.label(session.status.icon());
                              ui.label(format!("Session {} - {}", session_idx, session.pool.len()));
                              ui.label(RichText::new(regular::USER).family(FontFamily::Name("phosphor".into())));
                            });

                            // Session context menu (placeholder) - attached to session header
                            session_response.response.context_menu(|ui| {
                              ui.label(format!("Session {}", session_idx));
                              ui.separator();
                              ui.label("(Session actions - TODO)");
                              ui.label("- Force session state");
                              ui.label("- Reset timer");
                              ui.label("- Regenerate teams");
                              ui.label("- Swap teams");
                            });

                            if !session.pool.is_empty() {
                              ui.indent(format!("players_{}", session_idx), |ui: &mut egui::Ui| {
                                for session_player in &session.pool {
                                  let user_id = session_player.player.user_id.get();
                                  let team_str = match session_player.team {
                                    Some(crate::models::Team::Red) => "RED ",
                                    Some(crate::models::Team::Blu) => "BLU ",
                                    Some(crate::models::Team::Unassigned) => "",
                                    None => "",
                                  };
                                  let player_response = ui.horizontal(|ui| {
                                    ui.label(session_player.vc_icon());
                                    ui.label(format!("{}{}‹{}›", team_str, session_player.player.tag, session_player.player.elo));
                                  });

                                  // Player context menu with Remove, Buffer, Fatkid
                                  player_response.response.context_menu(|ui| {
                                    ui.label(format!("Player: {}", session_player.player.tag));
                                    ui.separator();

                                    if ui.button("Remove from Queue").clicked() {
                                      let _ = state.cmd_tx.try_send(crate::gui::commands::GuiCommand::RemovePlayer {
                                        guild_id: guild_id.get(),
                                        category_id: cat_id,
                                        fmt_id,
                                        user_id,
                                      });
                                      ui.close_menu();
                                    }

                                    if ui.button("Buffer (Move to Front)").clicked() {
                                      let _ = state.cmd_tx.try_send(crate::gui::commands::GuiCommand::BufferPlayer {
                                        guild_id: guild_id.get(),
                                        category_id: cat_id,
                                        fmt_id,
                                        user_id,
                                      });
                                      ui.close_menu();
                                    }

                                    if ui.button("Fatkid (Move to End)").clicked() {
                                      let _ = state.cmd_tx.try_send(crate::gui::commands::GuiCommand::FatkidPlayer {
                                        guild_id: guild_id.get(),
                                        category_id: cat_id,
                                        fmt_id,
                                        user_id,
                                      });
                                      ui.close_menu();
                                    }
                                  });
                                }
                              });
                            }
                          }
                        }
                      });

                      // Format context menu (placeholder) - attached to format header
                      fmt_response.header_response.context_menu(|ui| {
                        ui.label(format!("Format: {}", fmt_name));
                        ui.separator();
                        ui.label("(Format actions - TODO)");
                        ui.label("- Clear queue");
                        ui.label("- Force quota met");
                        ui.label("- Add dummy players");
                        ui.label("- Simulate game flow");
                      });
                    }
                  }
                });

                // Category context menu (placeholder) - attached to category header
                cat_response.header_response.context_menu(|ui| {
                  ui.label(format!("Category: {}", cat_name));
                  ui.separator();
                  ui.label("(Category actions - TODO)");
                  ui.label("- Clear all team VCs");
                  ui.label("- Reset category state");
                  ui.label("- Remove orphaned sessions");
                  ui.label("- Sync VC state");
                  ui.label("- Recover from database");
                });
              }
            }
          });

          // Guild context menu - attached to guild header
          guild_response.header_response.context_menu(|ui| {
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
