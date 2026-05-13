//! Queue/Game view panel – plain egui widgets

use crate::gui::commands::GuiCommand;
use crate::gui::state::GuiSharedState;
use crate::models::{SessionStatus, Team};
use egui::{self, Layout, RichText, ScrollArea};

pub fn show_queue_panel(ui: &mut egui::Ui, state: &GuiSharedState) {
    let manager_opt = if let Ok(l) = state.latest_manager.try_read() { l.clone() } else { None };
    let Some(manager) = manager_opt else {
        ui.centered_and_justified(|ui| { ui.label("Waiting for bot data…"); });
        return;
    };
    if manager.qguilds.is_empty() {
        ui.centered_and_justified(|ui| { ui.label("No guilds connected."); });
        return;
    }

    let guild_key       = egui::Id::new("q_sel_guild");
    let dummy_count_key = egui::Id::new("q_dummy_count");
    let mut sel_guild:    usize = ui.ctx().data(|d| d.get_temp(guild_key)).unwrap_or(0);
    let mut dummy_count:  usize = ui.ctx().data(|d| d.get_temp(dummy_count_key)).unwrap_or(1);
    sel_guild = sel_guild.min(manager.qguilds.len().saturating_sub(1));

    // ── Sidebar: guild list ────────────────────────────────────────────────────
    egui::SidePanel::left("q_guild_panel")
        .resizable(false)
        .exact_width(160.0)
        .show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Guilds").strong());
            ui.separator();
            for (g_idx, guild) in manager.qguilds.iter().enumerate() {
                let selected = g_idx == sel_guild;
                let resp = ui.selectable_label(selected, &guild.name)
                    .on_hover_text(format!("ID: {}", guild.id.get()));
                if resp.clicked() {
                    sel_guild = g_idx;
                }
                resp.context_menu(|ui| {
                    ui.label(RichText::new(&guild.name).strong());
                    ui.separator();
                    if ui.button("Copy Guild ID").clicked() {
                        ui.ctx().copy_text(guild.id.get().to_string());
                        ui.close_menu();
                    }
                });
            }
        });

    // ── Main content ───────────────────────────────────────────────────────────
    let guild = &manager.qguilds[sel_guild];
    let guild_id = guild.id.get();
    let send     = |cmd: GuiCommand| { state.send_cmd(cmd); };

    ScrollArea::vertical().show(ui, |ui| {
        for category in &guild.categories {
            let cat_id = category.id;
            let cat_name = category.name.as_deref().unwrap_or("?");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(cat_name).size(20.0).strong());
            });
            ui.add_space(6.0);
            for format in &category.formats {
                let fid = format.id;
                ui.add_space(8.0);
                ui.group(|ui| {
                    // ── Format header + action buttons ─────────────────────────
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&format.name).heading());
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(egui::DragValue::new(&mut dummy_count).range(1..=32).speed(0.1));
                            ui.label("×");
                            if ui.small_button("+Dummies").clicked() {
                                send(GuiCommand::AddDummyPlayers { guild_id, category_id: cat_id, fmt_id: fid, count: dummy_count, role_id: None });
                            }
                            ui.separator();
                            if ui.small_button("Clear Queue").clicked() {
                                send(GuiCommand::ClearQueue { guild_id, category_id: cat_id, fmt_id: fid });
                            }
                            if ui.small_button("Force Hot").clicked() {
                                send(GuiCommand::ForceQuotaMet { guild_id, category_id: cat_id, fmt_id: fid });
                            }
                        });
                    });
                    ui.separator();

                    // ── Queue players (left) and active sessions (right) ────────
                    ui.columns(2, |cols| {
                        cols[0].label(RichText::new("Queue").strong());
                        let has_queue = format.sessions.iter().any(|s| s.is_idle() && !s.pool.is_empty());
                        if !has_queue {
                            cols[0].label("Empty");
                        } else {
                            for session in format.sessions.iter().filter(|s| s.is_idle()) {
                                for sp in &session.pool {
                                    player_row(&mut cols[0], sp, guild_id, cat_id, fid, state);
                                }
                            }
                        }

                        cols[1].label(RichText::new("Sessions").strong());
                        let has_active = format.sessions.iter().any(|s| !s.is_idle());
                        if !has_active {
                            cols[1].label("No active sessions");
                        } else {
                            // Pass the real index into format.sessions so session commands work correctly
                            for (sess_idx, session) in format.sessions.iter().enumerate() {
                                if !session.is_idle() {
                                    session_block(&mut cols[1], session, sess_idx, guild_id, cat_id, fid, state);
                                }
                            }
                        }
                    });
                });
                ui.add_space(6.0);
            }
        }
    });

    persist(ui, guild_key, sel_guild, dummy_count_key, dummy_count);
}

fn persist(ui: &egui::Ui, gk: egui::Id, sg: usize, dk: egui::Id, dc: usize) {
    ui.ctx().data_mut(|d| {
        d.insert_temp(gk, sg);
        d.insert_temp(dk, dc);
    });
}

fn session_block(
    ui: &mut egui::Ui, session: &crate::models::Session, sess_idx: usize,
    guild_id: u64, cat_id: u8, fmt_id: u8, state: &GuiSharedState,
) {
    let blu: Vec<&crate::models::SessionPlayer> =
        session.pool.iter().filter(|p| p.team == Some(Team::Blu)).collect();
    let red: Vec<&crate::models::SessionPlayer> =
        session.pool.iter().filter(|p| p.team == Some(Team::Red)).collect();
    let has_teams = !blu.is_empty() || !red.is_empty();
    let send = |cmd: GuiCommand| { state.send_cmd(cmd); };

    ui.group(|ui| {
        // ── Session header + action buttons ────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(session.status.icon());
            ui.label(format!("Session {}", sess_idx + 1));
            ui.separator();
            if ui.small_button("End").on_hover_text("Force end — clears pool, resets to Idle").clicked() {
                send(GuiCommand::ForceEndGame { guild_id, category_id: cat_id, fmt_id, session_index: sess_idx });
            }
            if ui.small_button("Regen").on_hover_text("Regenerate teams (BCH)").clicked() {
                send(GuiCommand::ForceTeamRegeneration { guild_id, category_id: cat_id, fmt_id, session_index: sess_idx });
            }
            if ui.small_button("Swap").on_hover_text("Swap Red ↔ Blue").clicked() {
                send(GuiCommand::SwapTeams { guild_id, category_id: cat_id, fmt_id, session_index: sess_idx });
            }
            ui.separator();
            if ui.small_button("→Idle").on_hover_text("Force session to Idle").clicked() {
                send(GuiCommand::ForceSessionState { guild_id, category_id: cat_id, fmt_id, session_index: sess_idx, new_state: SessionStatus::Idle });
            }
            if ui.small_button("→Live").on_hover_text("Force session to Live").clicked() {
                send(GuiCommand::ForceSessionState { guild_id, category_id: cat_id, fmt_id, session_index: sess_idx, new_state: SessionStatus::Live });
            }
            if ui.small_button("⏱").on_hover_text("Reset confirm timer").clicked() {
                send(GuiCommand::ResetSessionTimer { guild_id, category_id: cat_id, fmt_id, session_index: sess_idx });
            }
        });

        // ── Player list ────────────────────────────────────────────────────
        if !has_teams {
            for sp in &session.pool {
                player_row(ui, sp, guild_id, cat_id, fmt_id, state);
            }
            return;
        }

        ui.columns(2, |cols| {
            cols[0].label(RichText::new("Blue").strong());
            for sp in &blu { player_row(&mut cols[0], sp, guild_id, cat_id, fmt_id, state); }
            cols[1].label(RichText::new("Red").strong());
            for sp in &red { player_row(&mut cols[1], sp, guild_id, cat_id, fmt_id, state); }
        });
    });
}

fn player_row(
    ui: &mut egui::Ui, sp: &crate::models::SessionPlayer,
    guild_id: u64, cat_id: u8, fmt_id: u8, state: &GuiSharedState,
) {
    let uid = sp.player.user_id.get();
    let text = format!("{} ‹{}›", sp.player.tag, sp.player.elo);
    let resp = ui.horizontal(|ui| {
        ui.label(sp.vc_icon());
        ui.label(&text).on_hover_text(format!("Discord ID: {}", uid))
    }).inner;
    resp.context_menu(|ui| {
        ui.label(RichText::new(&sp.player.tag).strong());
        ui.label(RichText::new(format!("ID: {}", uid)).weak());
        ui.separator();
        if ui.button("Copy Player ID").clicked() {
            ui.ctx().copy_text(uid.to_string());
            ui.close_menu();
        }
        if ui.button("Copy tag").clicked() {
            ui.ctx().copy_text(sp.player.tag.clone());
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Remove").clicked() {
            state.send_cmd(GuiCommand::RemovePlayer { guild_id, category_id: cat_id, fmt_id, user_id: uid });
            ui.close_menu();
        }
        if ui.button("Buffer (move to front)").clicked() {
            state.send_cmd(GuiCommand::BufferPlayer { guild_id, category_id: cat_id, fmt_id, user_id: uid });
            ui.close_menu();
        }
        if ui.button("Fatkid (move to end)").clicked() {
            state.send_cmd(GuiCommand::FatkidPlayer { guild_id, category_id: cat_id, fmt_id, user_id: uid });
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Fix VC State").on_hover_text("Reset stuck in_vc flag for this player").clicked() {
            state.send_cmd(GuiCommand::FixPlayerVCState { guild_id, category_id: cat_id, user_id: uid });
            ui.close_menu();
        }
    });
}
