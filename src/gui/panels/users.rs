//! Users panel – search and edit users across all servers

use crate::db::repo::GuildElo;
use crate::gui::commands::GuiCommand;
use crate::gui::state::GuiSharedState;
use crate::models::Player;
use egui::{self, RichText, ScrollArea};

pub fn show_users_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
  let search_key = egui::Id::new("usr_search_term");
  let modal_open_key = egui::Id::new("usr_modal_open");
  let modal_user_key = egui::Id::new("usr_modal_user");

  let mut search_term: String = ui.ctx().data(|d| d.get_temp(search_key)).unwrap_or_default();
  let mut modal_open: bool = ui.ctx().data(|d| d.get_temp(modal_open_key)).unwrap_or(false);
  let mut modal_user_id: Option<u64> = ui.ctx().data(|d| d.get_temp(modal_user_key));

  // ── Search bar ───────────────────────────────────────────────────────────
  ui.horizontal(|ui| {
    ui.label(RichText::new("Search:").strong());
    ui.add(egui::TextEdit::singleline(&mut search_term).hint_text("tag or user ID").desired_width(200.0));
    if ui.button("Search").clicked() && !search_term.trim().is_empty() {
      state.send_cmd(GuiCommand::QueryUsers { search_term: search_term.trim().to_string() });
    }
    if ui.button("Clear").clicked() {
      search_term.clear();
      modal_open = false;
      modal_user_id = None;
    }
  });

  ui.separator();

  // ── Results table ────────────────────────────────────────────────────────
  let results: Vec<Player> = if let Ok(lock) = state.user_search_results.try_read() { lock.clone() } else { Vec::new() };

  if results.is_empty() {
    ui.label("No results. Enter a search term above.");
  } else {
    ui.label(format!("{} result(s)", results.len()));
    ui.add_space(4.0);

    // Aligned table using Grid
    egui::Grid::new("usr_results_grid").num_columns(5).spacing([12.0, 4.0]).min_col_width(0.0).show(ui, |ui| {
      // Header
      ui.label(RichText::new("").strong()); // Edit column
      ui.label(RichText::new("User ID").strong().monospace());
      ui.label(RichText::new("Tag").strong());
      ui.label(RichText::new("Steam ID").strong().monospace());
      ui.label(RichText::new("Queue Exp").strong());
      ui.end_row();
      ui.separator();
      ui.end_row();

      for player in &results {
        let uid = player.user_id.get();

        // Edit button
        if ui.button("Edit").clicked() {
          modal_open = true;
          modal_user_id = Some(uid);
          state.send_cmd(GuiCommand::GetUserGuildData { user_id: uid });
        }

        // Data cells as plain labels (non-interactive)
        ui.label(format!("{}", uid)).on_hover_text(format!("Discord ID: {}", uid));
        let tag_text = if player.tag.is_empty() { "<no tag>".to_string() } else { player.tag.clone() };
        ui.label(tag_text);
        let steam_text = player.steam_id.map(|s| s.to_string()).unwrap_or_else(|| "—".to_string());
        ui.label(steam_text);
        ui.label(format!("{}", player.queue_expiration));
        ui.end_row();
      }
    });
  }

  // ── Edit modal window ────────────────────────────────────────────────────
  if modal_open {
    if let Some(uid) = modal_user_id {
      if let Some(player) = results.iter().find(|p| p.user_id.get() == uid) {
        let title = format!("Edit User — {} ({})", player.tag, uid);
        let mut window_open = modal_open;

        egui::Window::new(title).open(&mut window_open).resizable(true).default_size([500.0, 400.0]).show(ui.ctx(), |ui| {
          ScrollArea::vertical().show(ui, |ui| {
            // ── Global fields ──────────────────────────────────────────────
            ui.heading("Global");
            ui.separator();

            let global_edit_id = egui::Id::new(("usr_global_edit", uid));
            let mut edit_tag: String = ui.ctx().data(|d| d.get_temp(global_edit_id.with("tag"))).unwrap_or_else(|| player.tag.clone());
            let mut edit_steam: String = ui.ctx().data(|d| d.get_temp(global_edit_id.with("steam"))).unwrap_or_else(|| player.steam_id.map(|s| s.to_string()).unwrap_or_default());
            let mut edit_expiry: u8 = ui.ctx().data(|d| d.get_temp(global_edit_id.with("expiry"))).unwrap_or(player.queue_expiration);

            egui::Grid::new(("usr_modal_global", uid)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
              ui.label("Tag:");
              ui.add(egui::TextEdit::singleline(&mut edit_tag).desired_width(200.0));
              ui.end_row();

              ui.label("Steam ID:");
              ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut edit_steam).hint_text("blank = none").desired_width(200.0));
              });
              ui.end_row();

              ui.label("Queue Expiration:");
              ui.add(egui::DragValue::new(&mut edit_expiry).range(0..=255));
              ui.end_row();
            });

            ui.horizontal(|ui| {
              if ui.button("Save Global").clicked() {
                let mut updated = false;
                if edit_tag != player.tag {
                  state.send_cmd(GuiCommand::UpdateUserTag { user_id: uid, tag: edit_tag.clone() });
                  updated = true;
                }
                let steam_id = if edit_steam.trim().is_empty() { None } else { edit_steam.trim().parse::<u64>().ok() };
                if steam_id != player.steam_id {
                  state.send_cmd(GuiCommand::UpdateUserSteamId { user_id: uid, steam_id });
                  updated = true;
                }
                if edit_expiry != player.queue_expiration {
                  state.send_cmd(GuiCommand::UpdateUserQueueExpiration { user_id: uid, queue_expiration: edit_expiry });
                  updated = true;
                }
                // Refresh
                if !search_term.trim().is_empty() {
                  state.send_cmd(GuiCommand::QueryUsers { search_term: search_term.trim().to_string() });
                }
                if updated {
                  ui.ctx().data_mut(|d| {
                    d.remove_temp::<String>(global_edit_id.with("tag"));
                    d.remove_temp::<String>(global_edit_id.with("steam"));
                    d.remove_temp::<u8>(global_edit_id.with("expiry"));
                  });
                }
              }
            });

            // Persist global edit values
            ui.ctx().data_mut(|d| {
              d.insert_temp(global_edit_id.with("tag"), edit_tag);
              d.insert_temp(global_edit_id.with("steam"), edit_steam);
              d.insert_temp(global_edit_id.with("expiry"), edit_expiry);
            });

            ui.add_space(12.0);

            // ── Per-guild data ─────────────────────────────────────────────
            ui.heading("Per-Server");
            ui.separator();

            let guild_data: Vec<(u64, GuildElo)> = if let Ok(lock) = state.user_guild_data.try_read() { lock.clone() } else { Vec::new() };

            if guild_data.is_empty() {
              ui.label("No server data found for this user.");
            } else {
              for (guild_id, ge) in &guild_data {
                let guild_edit_id = egui::Id::new(("usr_guild_edit", uid, *guild_id));
                let mut edit_elo: u16 = ui.ctx().data(|d| d.get_temp(guild_edit_id.with("elo"))).unwrap_or(ge.elo);
                let mut edit_dyn: String = ui.ctx().data(|d| d.get_temp(guild_edit_id.with("dyn"))).unwrap_or_else(|| ge.dynamic_elo.map(|d| d.to_string()).unwrap_or_default());

                ui.group(|ui| {
                  ui.label(RichText::new(format!("Server {}", guild_id)).strong());

                  egui::Grid::new(("usr_guild_grid", uid, *guild_id)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("ELO:");
                    ui.add(egui::DragValue::new(&mut edit_elo).range(0..=u16::MAX));
                    ui.end_row();

                    ui.label("Dynamic ELO:");
                    ui.horizontal(|ui| {
                      ui.add(egui::TextEdit::singleline(&mut edit_dyn).hint_text("blank = none").desired_width(100.0));
                    });
                    ui.end_row();

                    ui.label("Rank:");
                    ui.label(format!("{} ({})", ge.rank.name, ge.rank.elo));
                    ui.end_row();

                    ui.label("Games / Wins:");
                    ui.label(format!("{} / {}", ge.games, ge.wins));
                    ui.end_row();
                  });

                  ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                      let mut updated = false;
                      if edit_elo != ge.elo {
                        state.send_cmd(GuiCommand::UpdateUserElo { user_id: uid, guild_id: *guild_id, elo: edit_elo });
                        updated = true;
                      }
                      let dynamic_elo = if edit_dyn.trim().is_empty() { None } else { edit_dyn.trim().parse::<u16>().ok() };
                      if dynamic_elo != ge.dynamic_elo {
                        state.send_cmd(GuiCommand::UpdateUserDynamicElo { user_id: uid, guild_id: *guild_id, dynamic_elo });
                        updated = true;
                      }
                      // Refresh guild data and clear temp values so next render uses DB data
                      state.send_cmd(GuiCommand::GetUserGuildData { user_id: uid });
                      if updated {
                        ui.ctx().data_mut(|d| {
                          d.remove_temp::<u16>(guild_edit_id.with("elo"));
                          d.remove_temp::<String>(guild_edit_id.with("dyn"));
                        });
                      }
                    }
                  });

                  // Persist guild edit values
                  ui.ctx().data_mut(|d| {
                    d.insert_temp(guild_edit_id.with("elo"), edit_elo);
                    d.insert_temp(guild_edit_id.with("dyn"), edit_dyn.clone());
                  });
                });
                ui.add_space(4.0);
              }
            }
          });
        });

        modal_open = window_open;
        if !window_open {
          modal_user_id = None;
        }
      } else {
        // Player not in current results, close modal
        modal_open = false;
        modal_user_id = None;
      }
    }
  }

  // Persist state
  ui.ctx().data_mut(|d| {
    d.insert_temp(search_key, search_term);
    d.insert_temp(modal_open_key, modal_open);
    if let Some(id) = modal_user_id {
      d.insert_temp(modal_user_key, id);
    } else {
      d.remove_temp::<u64>(modal_user_key);
    }
  });
}
