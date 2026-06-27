//! Admin panel – actual working command buttons

use crate::gui::commands::GuiCommand;
use crate::gui::state::GuiSharedState;
use crate::models::SessionStatus;
use egui::{self, ScrollArea};

pub fn show_admin_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
  let manager_opt = if let Ok(l) = state.latest_manager.try_read() { l.clone() } else { None };

  // ── Target selectors ───────────────────────────────────────────────────────
  let guild_key = egui::Id::new("adm_guild");
  let cat_key = egui::Id::new("adm_cat");
  let fmt_key = egui::Id::new("adm_fmt");
  let session_key = egui::Id::new("adm_session");

  let mut sel_guild: usize = ui.ctx().data(|d| d.get_temp(guild_key)).unwrap_or(0);
  let mut sel_cat: usize = ui.ctx().data(|d| d.get_temp(cat_key)).unwrap_or(0);
  let mut sel_fmt: usize = ui.ctx().data(|d| d.get_temp(fmt_key)).unwrap_or(0);
  let mut sel_session: usize = ui.ctx().data(|d| d.get_temp(session_key)).unwrap_or(0);

  // Scratchpad values stored in temp memory
  let dummy_count_key = egui::Id::new("adm_dummy_count");
  let user_id_key = egui::Id::new("adm_user_id");
  let new_pos_key = egui::Id::new("adm_new_pos");

  let mut dummy_count: usize = ui.ctx().data(|d| d.get_temp(dummy_count_key)).unwrap_or(1);
  let mut user_id_str: String = ui.ctx().data(|d| d.get_temp(user_id_key)).unwrap_or_default();
  let mut new_pos: usize = ui.ctx().data(|d| d.get_temp(new_pos_key)).unwrap_or(0);

  // Resolve current selections into IDs
  let (guild_id, cat_id, fmt_id, session_index) = if let Some(ref m) = manager_opt {
    sel_guild = sel_guild.min(m.qguilds.len().saturating_sub(1));
    let gid = m.qguilds.get(sel_guild).map(|g| g.id.get()).unwrap_or(0);
    let cid = m
      .qguilds
      .get(sel_guild)
      .and_then(|g| {
        sel_cat = sel_cat.min(g.categories.len().saturating_sub(1));
        g.categories.get(sel_cat)
      })
      .map(|c| c.id)
      .unwrap_or(0);
    let fid = m
      .qguilds
      .get(sel_guild)
      .and_then(|g| g.categories.get(sel_cat))
      .and_then(|c| {
        sel_fmt = sel_fmt.min(c.formats.len().saturating_sub(1));
        c.formats.get(sel_fmt)
      })
      .map(|f| f.id)
      .unwrap_or(0);
    let slen = m.qguilds.get(sel_guild).and_then(|g| g.categories.get(sel_cat)).and_then(|c| c.formats.get(sel_fmt)).map(|f| f.sessions.len()).unwrap_or(1);
    sel_session = sel_session.min(slen.saturating_sub(1));
    (gid, cid, fid, sel_session)
  } else {
    (0, 0, 0, 0)
  };

  // Send helper
  let send = |cmd: GuiCommand| {
    state.send_cmd(cmd);
  };

  // ── Target selector UI ─────────────────────────────────────────────────────
  ui.heading("Admin commands");
  ui.separator();

  egui::Grid::new("adm_selectors").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
    if let Some(ref m) = manager_opt {
      // Guild
      ui.label("Guild:");
      egui::ComboBox::from_id_salt("adm_guild_combo").selected_text(m.qguilds.get(sel_guild).map(|g| g.name.as_str()).unwrap_or("—")).show_ui(ui, |ui| {
        for (i, g) in m.qguilds.iter().enumerate() {
          if ui.selectable_label(i == sel_guild, &g.name).clicked() {
            sel_guild = i;
            sel_cat = 0;
            sel_fmt = 0;
            sel_session = 0;
          }
        }
      });
      ui.end_row();

      // Category
      if let Some(guild) = m.qguilds.get(sel_guild) {
        ui.label("Category:");
        egui::ComboBox::from_id_salt("adm_cat_combo").selected_text(guild.categories.get(sel_cat).and_then(|c| c.name.as_deref()).unwrap_or("—")).show_ui(ui, |ui| {
          for (i, c) in guild.categories.iter().enumerate() {
            let name = c.name.as_deref().unwrap_or("?");
            if ui.selectable_label(i == sel_cat, name).clicked() {
              sel_cat = i;
              sel_fmt = 0;
              sel_session = 0;
            }
          }
        });
        ui.end_row();

        // Format
        if let Some(cat) = guild.categories.get(sel_cat) {
          ui.label("Format:");
          egui::ComboBox::from_id_salt("adm_fmt_combo").selected_text(cat.formats.get(sel_fmt).map(|f| f.name.as_str()).unwrap_or("—")).show_ui(ui, |ui| {
            for (i, f) in cat.formats.iter().enumerate() {
              if ui.selectable_label(i == sel_fmt, &f.name).clicked() {
                sel_fmt = i;
                sel_session = 0;
              }
            }
          });
          ui.end_row();

          // Session
          if let Some(fmt) = cat.formats.get(sel_fmt) {
            ui.label("Session:");
            egui::ComboBox::from_id_salt("adm_session_combo")
              .selected_text(format!("Session {} ({:?})", sel_session + 1, fmt.sessions.get(sel_session).map(|s| &s.status).unwrap_or(&SessionStatus::Idle)))
              .show_ui(ui, |ui| {
                for (i, s) in fmt.sessions.iter().enumerate() {
                  let label = format!("Session {} ({:?})", i + 1, s.status);
                  if ui.selectable_label(i == sel_session, &label).clicked() {
                    sel_session = i;
                  }
                }
              });
            ui.end_row();
          }
        }
      }
    } else {
      ui.label("Status:");
      ui.label("Waiting for bot data…");
      ui.end_row();
    }
  });

  ui.separator();

  if manager_opt.is_none() {
    // Persist and bail — no commands without data
    persist(ui, guild_key, sel_guild, cat_key, sel_cat, fmt_key, sel_fmt, session_key, sel_session);
    return;
  }

  ScrollArea::vertical().show(ui, |ui| {
    // ── Queue Management ───────────────────────────────────────────────────
    ui.collapsing("Queue management", |ui| {
      egui::Grid::new("adm_qm").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        // Clear Queue
        ui.label("Clear all idle players:");
        if ui.button("Clear queue").clicked() {
          send(GuiCommand::ClearQueue { guild_id, category_id: cat_id, fmt_id });
        }
        ui.end_row();

        // Force End Game
        ui.label("End live game immediately:");
        if ui.button("Force end game").clicked() {
          send(GuiCommand::ForceEndGame { guild_id, category_id: cat_id, fmt_id, session_index });
        }
        ui.end_row();

        // Force Team Regeneration
        ui.label("Regenerate teams:");
        if ui.button("Regen teams").clicked() {
          send(GuiCommand::ForceTeamRegeneration { guild_id, category_id: cat_id, fmt_id, session_index });
        }
        ui.end_row();

        // Swap Teams
        ui.label("Swap Red ↔ Blue:");
        if ui.button("Swap teams").clicked() {
          send(GuiCommand::SwapTeams { guild_id, category_id: cat_id, fmt_id, session_index });
        }
        ui.end_row();

        // Reset Session Timer
        ui.label("Reset confirm timer:");
        if ui.button("Reset timer").clicked() {
          send(GuiCommand::ResetSessionTimer { guild_id, category_id: cat_id, fmt_id, session_index });
        }
        ui.end_row();

        // Force Session State
        ui.label("Force session state:");
        ui.horizontal(|ui| {
          for (label, status) in [("Idle", SessionStatus::Idle), ("Hot", SessionStatus::Hot), ("Live", SessionStatus::Live)] {
            if ui.button(label).clicked() {
              send(GuiCommand::ForceSessionState { guild_id, category_id: cat_id, fmt_id, session_index, new_state: status });
            }
          }
        });
        ui.end_row();

        // Force Quota Met
        ui.label("Force hot (bypass quota):");
        if ui.button("Force hot").clicked() {
          send(GuiCommand::ForceQuotaMet { guild_id, category_id: cat_id, fmt_id });
        }
        ui.end_row();

        // Add Player by ID
        ui.label("Add player by ID:");
        ui.horizontal(|ui| {
          ui.add(egui::TextEdit::singleline(&mut user_id_str).hint_text("user_id").desired_width(120.0));
          if ui.button("Add").clicked() {
            if let Ok(uid) = user_id_str.trim().parse::<u64>() {
              send(GuiCommand::AddPlayer { guild_id, category_id: cat_id, fmt_id, user_id: uid });
            }
          }
        });
        ui.end_row();

        // Reorder player
        ui.label("Move player to position:");
        ui.horizontal(|ui| {
          ui.add(egui::TextEdit::singleline(&mut user_id_str).hint_text("user_id").desired_width(120.0));
          ui.add(egui::DragValue::new(&mut new_pos).range(0..=64).prefix("pos "));
          if ui.button("Move").clicked() {
            if let Ok(uid) = user_id_str.trim().parse::<u64>() {
              send(GuiCommand::ReorderQueue { guild_id, category_id: cat_id, fmt_id, user_id: uid, new_position: new_pos });
            }
          }
        });
        ui.end_row();
      });
    });

    ui.add_space(4.0);

    // ── Testing / Load Testing ─────────────────────────────────────────────
    ui.collapsing("Testing", |ui| {
      egui::Grid::new("adm_test").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        // Add Dummy Players
        ui.label("Add dummy players:");
        ui.horizontal(|ui| {
          ui.add(egui::DragValue::new(&mut dummy_count).range(1..=32).suffix(" players"));
          if ui.button("Add dummies").clicked() {
            send(GuiCommand::AddDummyPlayers { guild_id, category_id: cat_id, fmt_id, count: dummy_count, role_id: None });
          }
        });
        ui.end_row();

        // Simulate Game Flow
        ui.label("Run full game cycle:");
        if ui.button("Simulate game").clicked() {
          send(GuiCommand::SimulateGameFlow { guild_id, category_id: cat_id, fmt_id });
        }
        ui.end_row();

        // Trigger Concurrent Games
        ui.label("Start concurrent games:");
        ui.horizontal(|ui| {
          ui.add(egui::DragValue::new(&mut dummy_count).range(1..=8).suffix(" games"));
          if ui.button("Trigger").clicked() {
            send(GuiCommand::TriggerConcurrentGames { guild_id, category_id: cat_id, fmt_id, count: dummy_count });
          }
        });
        ui.end_row();

        // Test Balance Methods
        ui.label("Compare balance algorithms:");
        if ui.button("Test balance").clicked() {
          send(GuiCommand::TestBalanceMethods { guild_id, category_id: cat_id, fmt_id });
        }
        ui.end_row();

        // Simulate VC Timeout
        ui.label("Simulate VC confirm timeout:");
        if ui.button("Simulate timeout").clicked() {
          send(GuiCommand::SimulateVCTimeout { guild_id, category_id: cat_id, fmt_id });
        }
        ui.end_row();
      });
    });

    ui.add_space(4.0);

    // ── Recovery ──────────────────────────────────────────────────────────
    ui.collapsing("Recovery from bugs", |ui| {
      egui::Grid::new("adm_rec").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Delete all team VCs:");
        if ui.button("Clear team VCs").clicked() {
          send(GuiCommand::ClearAllTeamVCs { guild_id, category_id: cat_id });
        }
        ui.end_row();

        ui.label("Full category reset:");
        if ui.button("Reset category").clicked() {
          send(GuiCommand::ResetCategoryState { guild_id, category_id: cat_id });
        }
        ui.end_row();

        ui.label("Delete sessions with no players:");
        if ui.button("Remove orphaned sessions").clicked() {
          send(GuiCommand::RemoveOrphanedSessions { guild_id, category_id: cat_id });
        }
        ui.end_row();

        ui.label("Clear pending team switches:");
        if ui.button("Clear pending Switches").clicked() {
          send(GuiCommand::ClearPendingTeamSwitches { guild_id, category_id: cat_id, fmt_id });
        }
        ui.end_row();

        ui.label("Reset voice state tracking:");
        if ui.button("Reset VC tracking").clicked() {
          send(GuiCommand::ResetVoiceStateTracking { guild_id });
        }
        ui.end_row();

        ui.label("Reload category from DB:");
        if ui.button("Recover from DB").clicked() {
          send(GuiCommand::RecoverFromDatabase { guild_id, category_id: cat_id });
        }
        ui.end_row();

        ui.label("Fix player VC state:");
        ui.horizontal(|ui| {
          ui.add(egui::TextEdit::singleline(&mut user_id_str).hint_text("user_id").desired_width(120.0));
          if ui.button("Fix").clicked() {
            if let Ok(uid) = user_id_str.trim().parse::<u64>() {
              send(GuiCommand::FixPlayerVCState { guild_id, category_id: cat_id, user_id: uid });
            }
          }
        });
        ui.end_row();
      });
    });

    ui.add_space(4.0);

    // ── Voice Channel Management ───────────────────────────────────────────
    ui.collapsing("Voice channel management", |ui| {
      egui::Grid::new("adm_vc").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Resync VC flags:");
        if ui.button("Sync VC state").clicked() {
          send(GuiCommand::SyncVCState { guild_id, category_id: cat_id });
        }
        ui.end_row();

        ui.label("Kick player from VC:");
        ui.horizontal(|ui| {
          ui.add(egui::TextEdit::singleline(&mut user_id_str).hint_text("user_id").desired_width(120.0));
          if ui.button("Kick").clicked() {
            if let Ok(uid) = user_id_str.trim().parse::<u64>() {
              send(GuiCommand::KickFromVC { guild_id, user_id: uid });
            }
          }
        });
        ui.end_row();
      });
    });

    ui.add_space(4.0);

    // ── System Control ────────────────────────────────────────────────────
    ui.collapsing("System control", |ui| {
      egui::Grid::new("adm_sys").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Restart bot gracefully:");
        if ui.button("🔄 Graceful Restart").clicked() {
          send(GuiCommand::GracefulRestart);
        }
        ui.end_row();
        ui.label("");
        ui.label("⚠️ State will be preserved. Games in progress will be saved.");
        ui.end_row();
      });
    });

    ui.add_space(4.0);

    // ── Debugging ─────────────────────────────────────────────────────────
    ui.collapsing("Debugging", |ui| {
      egui::Grid::new("adm_dbg").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Export full state to log:");
        if ui.button("Dump state").clicked() {
          send(GuiCommand::DumpStateToLog { guild_id });
        }
        ui.end_row();

        ui.label("Ping Discord gateway:");
        if ui.button("Test Discord API").clicked() {
          send(GuiCommand::TestDiscordApi);
        }
        ui.end_row();

        ui.label("View session details (log):");
        if ui.button("View session").clicked() {
          send(GuiCommand::ViewSessionDetails { guild_id, category_id: cat_id, fmt_id, session_index });
        }
        ui.end_row();
      });
    });

    ui.add_space(4.0);

    // ── Global ────────────────────────────────────────────────────────────
    ui.collapsing("Global", |ui| {
      if ui.button("Refresh snapshot (Ctrl+R)").clicked() {
        send(GuiCommand::RefreshSnapshot);
      }
    });
  });

  // Persist scratch values
  ui.ctx().data_mut(|d| {
    d.insert_temp(dummy_count_key, dummy_count);
    d.insert_temp(user_id_key, user_id_str);
    d.insert_temp(new_pos_key, new_pos);
  });

  persist(ui, guild_key, sel_guild, cat_key, sel_cat, fmt_key, sel_fmt, session_key, sel_session);
}

fn persist(ui: &egui::Ui, gk: egui::Id, sg: usize, ck: egui::Id, sc: usize, fk: egui::Id, sf: usize, sk: egui::Id, ss: usize) {
  ui.ctx().data_mut(|d| {
    d.insert_temp(gk, sg);
    d.insert_temp(ck, sc);
    d.insert_temp(fk, sf);
    d.insert_temp(sk, ss);
  });
}
