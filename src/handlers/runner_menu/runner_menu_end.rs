use anyhow::Result;
use serenity::all::{
  ComponentInteraction as CI, Context, CreateActionRow as CAR, CreateEmbed as CE,
  CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, GuildId as GI,
};
use std::sync::Arc;
use tracing::{error, info};

use crate::db::Database;
use crate::handlers::player::check_component_role;
use crate::models::embeds::Ephemeral as Eph;
use crate::models::{ComponentContext as CC, Role};
use crate::{guild_name, log_prefix_category, Manager};

/// Handle "End without score" action from runner menu
/// Finds the runner's active match and ends it without requiring score reporting
pub async fn handle_end_without_score(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<Manager>>,
  guild_id: GI,
) -> Result<()> {
  let cc = CC { ctx, component: interaction, db: db.clone(), manager };

  if !check_component_role(&cc, &Role::Runner).await? {
    let embed = CE::new().title("Only runners can use this action.").color(0xFF0000);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
      CAR::Buttons(vec![Eph::back("runner_menu_back")])
    ]));
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  let user_id = interaction.user.id;
  
  // Find the runner's active match
  let (found_match, guild_name_str, category_name, format_name) = {
    let mut mgr = manager.lock().await;
    let server = mgr.get_server(guild_id)?;
    
    let mut found_match = None;
    
    // Strategy 1: Find the only active match in any category
    let mut active_matches = Vec::new();
    for (cat_idx, category) in server.categories.iter().enumerate() {
      for (fmt_idx, format) in category.formats.iter().enumerate() {
        for session in &format.sessions {
          if session.is_active() {
            active_matches.push((cat_idx, fmt_idx, category.ctg_id, format.id));
          }
        }
      }
    }
    
    if active_matches.len() == 1 {
      // Only one active match - use it
      found_match = Some(active_matches[0]);
    } else if active_matches.len() > 1 {
      // Strategy 2: Check if runner is in any team VC
      if let Some(guild) = ctx.cache.guild(guild_id) {
        if let Some(voice_state) = guild.voice_states.get(&user_id) {
          if let Some(channel_id) = voice_state.channel_id {
            // Check which category this channel belongs to
            for (cat_idx, category) in server.categories.iter().enumerate() {
              if category.channels.teams.iter().any(|tc| tc.red_vc == channel_id || tc.blu_vc == channel_id) {
                // Found the category - now find the active session
                for (fmt_idx, format) in category.formats.iter().enumerate() {
                  if format.sessions.iter().any(|s| s.is_active()) {
                    found_match = Some((cat_idx, fmt_idx, category.ctg_id, format.id));
                    break;
                  }
                }
                break;
              }
            }
          }
        }
      }
    }
    
    let (cat_idx, fmt_idx, _category_id, format_id) = match found_match {
      Some(m) => m,
      None => {
        let embed = CE::new()
          .title("No active match found")
          .description("Could not find an active match to end. Either there are no active matches, or you need to be in a team voice channel when multiple matches are running.")
          .color(0xFFAA00);
        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![
          CAR::Buttons(vec![Eph::back("runner_menu_back")])
        ]));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
    };
    
    let category = &server.categories[cat_idx];
    let guild_name_str = guild_name(ctx, guild_id);
    let category_name = category.name.as_deref().unwrap_or("Unknown").to_string();
    let format_name = category.formats[fmt_idx].name.clone();
    
    (found_match, guild_name_str, category_name, format_name)
  };
  
  let (_cat_idx, _fmt_idx, _category_id, format_id) = found_match.unwrap();
  
  info!("{} Runner {} used 'End without score'", 
    log_prefix_category(&guild_name_str, &category_name), 
    interaction.user.tag());
  
  // Defer the response since we're about to do async work
  interaction.create_response(&ctx.http, CIR::Defer(CIRM::new().ephemeral(true))).await?;
  
  // End the match using the category's pull method
  {
    let mut mgr = manager.lock().await;
    let server = mgr.get_server(guild_id)?;
    let (cat_idx, _fmt_idx, _category_id, _format_id) = found_match.unwrap();
    let category = &mut server.categories[cat_idx];
    
    match category.pull_fmt(format_id, ctx, guild_id, db, Some(manager.clone())).await {
      Ok(_) => {
        info!("{} Match ended without score report", log_prefix_category(&guild_name_str, &category_name));
        
        // Update all dashboards
        category.queue_dash_update_all(ctx).await;
        
        let embed = CE::new()
          .title("Match ended")
          .description(format!("Ended {} match without reporting score.", format_name))
          .color(0x00FF00);
        
        interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
      }
      Err(e) => {
        error!("Failed to end match: {e}");
        
        let embed = CE::new()
          .title("Failed to end match")
          .description(format!("Error: {}", e))
          .color(0xFF0000);
        
        interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(embed)).await?;
      }
    }
  }
  
  Ok(())
}
