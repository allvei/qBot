use std::sync::Arc;
use crate::Database;
use crate::RED;
use crate::create_input_sh;
use crate::create_paragraph_input_with_value;
use crate::create_value_input_sh;
use crate::create_value_input_sh_cap;
use crate::create_short_input_opt;
use crate::create_input_sh_cap;
use crate::guild_name;
use crate::handlers::settings::apply_elo_gate;
use crate::handlers::settings::build_category_settings_buttons;
use crate::handlers::settings::build_category_settings_embed;
use crate::handlers::settings::clear_elo_gate;
use crate::handlers::settings::parse_mid;
use crate::handlers::settings::menu::FormatListDisplay;
use crate::handlers::settings::menu::create_selection_menu;
use crate::handlers::settings::utils::send_modal_error_response;
use crate::handlers::settings::utils::send_embed_button_response_modal;
use crate::send_component_error_response;
use anyhow::Result;
use serenity::all::{
  ActionRowComponent as ARC, Context, ComponentInteraction as CI, ModalInteraction as MI, CreateModal as CM, CreateInteractionResponse as CIR,
  CreateActionRow as CAR, CreateButton as CB, ComponentInteractionDataKind as CIDK,
  ButtonStyle as BS, CreateInteractionResponseMessage as CIRM, CreateEmbed as CE, GuildId as GI, GetMessages as GM,
};
use tracing::{info, warn, error};
use crate::{get_modal_input, refresh_category_settings};

/// Macro to refresh category settings and send response for modal interactions
macro_rules! refresh_category_settings_modal {
  ($interaction:expr, $ctx:expr, $category:expr) => {{
    let settings = CategorySettings::from_category($category);
    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);
    send_embed_button_response_modal($interaction, $ctx, embed, buttons).await
  }};
}

/// Category settings structure for display
pub struct CategorySettings {
  pub category_id: u8,
  pub name: Option<String>,
  pub quota: u8,
  pub confirm_time: u16,
  pub connect_info: Option<String>,
  pub format_names: Vec<String>,
  pub vc_create: String,
  pub vc_destroy: String,
  pub vc_keep_min: bool,
}

impl CategorySettings {
  pub fn from_category(category: &crate::models::Category) -> Self {
    Self {
      category_id: category.id,
      name: category.name.clone(),
      quota: category.quota(),
      confirm_time: category.confirm_time,
      connect_info: category.connect_info().map(|s| s.to_string()),
      format_names: category.formats.iter().map(|sg| sg.name.clone()).collect(),
      vc_create: category.team_vc_settings.create_policy.to_string(),
      vc_destroy: category.team_vc_settings.destroy_policy.to_string(),
      vc_keep_min: category.team_vc_settings.keep_minimum,
    }
  }
}

/// Handle category settings button interactions
pub async fn handle_category_settings_button(
  ctx: &Context,
  interaction: &CI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let button_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Category Settings] {} pressed {}", user_tag, button_id);

  // Handle format remove confirmation (button: category_fmt_confirm_remove_{gid}_{sgid}, select: category_fmt_confirm_remove with value gid_fmtid)
  if button_id == "category_fmt_confirm_remove" || button_id.starts_with("category_fmt_confirm_remove_") {
    let selected = if button_id == "category_fmt_confirm_remove" {
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      button_id.strip_prefix("category_fmt_confirm_remove_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.split('_').collect();
    if parts.len() != 2 {
      return Err(anyhow::anyhow!("Invalid remove selection format"));
    }
    let category_id: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("Invalid category_id"))?;
    let fmt_id: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid format_id"))?;

    let mut manager_lock = manager.lock().await;
    let category = {
      let server = manager_lock.get_qguild(guild_id)?;
      server.categories.iter_mut().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
    };

    match category.remove_format(fmt_id) {
      Ok(_) => {
        // Persist to DB
        db.categories.save_all_formats(guild_id, category_id, &category.formats).await?;

        // Update dashboard
        category.queue_dash_update(ctx, guild_id).await;

        let display =
          FormatListDisplay { category_id, category_name: category.name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
        drop(manager_lock);
        let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
        interaction.create_response(&ctx.http, response).await?;
      }
      Err(e) => {
        drop(manager_lock);
        send_component_error_response(interaction, ctx, &format!("Failed to remove format: {}", e)).await;
      }
    }
    return Ok(());
  }

  // Handle format edit (button: category_fmt_edit_{gid}_{sgid}, select: category_fmt_edit with value gid_fmtid)
  if button_id == "category_fmt_edit" || button_id.starts_with("category_fmt_edit_") {
    let selected = if button_id == "category_fmt_edit" {
      // Select menu variant
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      // Button variant: strip prefix to get "gid_fmtid"
      button_id.strip_prefix("category_fmt_edit_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.split('_').collect();
    if parts.len() != 2 {
      return Err(anyhow::anyhow!("Invalid edit selection format"));
    }
    let category_id: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("Invalid category_id"))?;
    let fmt_id: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid format_id"))?;

    // Show modal to edit the format's name and quota

    let mut manager_lock = manager.lock().await;
    let fmt_name;
    let fmt_quota;
    {
      let server = manager_lock.get_qguild(guild_id)?;
      let category = server.categories.iter().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;
      let sg = category.formats.iter().find(|s| s.id == fmt_id).ok_or_else(|| anyhow::anyhow!("Format {} not found", fmt_id))?;
      fmt_name = sg.name.clone();
      fmt_quota = sg.quota.to_string();
    }
    drop(manager_lock);

    let modal = CM::new(format!("category_fmt_modal_edit_{}_{}", category_id, fmt_id), format!("Edit format: {}", fmt_name))
      .components(vec![create_value_input_sh("Format name", "name", "", &fmt_name), create_value_input_sh("Quota (players per match)", "quota", "", &fmt_quota)]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
    return Ok(());
  }

  // Handle elo gate buttons (these parse their own category_id from the value)
  if button_id.starts_with("category_settings_elo_gate_") {
    let category_id_str = button_id.strip_prefix("category_settings_elo_gate_").unwrap();
    if let Ok(category_id) = category_id_str.parse::<u8>() {
      let ranks = db.ranks.get_ranks(guild_id).await?;
      if ranks.is_empty() {
        let embed = CE::new()
          .title("No ranks configured")
          .description("You need to configure ranks before setting up an ELO gate.\nGo to server settings and set up ranks first.")
          .color(RED);
        let response = CIR::UpdateMessage(
          CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary)])]),
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      let embed = CE::new()
        .title("ELO Gate - Select minimum rank")
        .description("Select the **minimum** rank that can view this category's category.\nAll ranks from min to max (inclusive) will have access.")
        .color(0x5865F2);

      let mut options: Vec<(String, String)> = Vec::new();
      options.push(("No minimum".to_string(), format!("{}_0", category_id)));
      for (i, r) in ranks.iter().enumerate() {
        options.push((format!("{} (ELO {})", r.name, r.elo), format!("{}_{}", category_id, i)));
      }

      let mut components = Vec::new();
      if let Some(menu) = create_selection_menu("elo_gate_min", "Select minimum rank", options) {
        components.push(menu);
      }
      components.push(CAR::Buttons(vec![
        CB::new(format!("elo_gate_clear_{category_id}")).label("Clear ELO gate").style(BS::Danger),
        CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary),
      ]));

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    return Ok(());
  }

  if button_id == "elo_gate_min" || button_id.starts_with("elo_gate_min_") {
    let selected = if button_id == "elo_gate_min" {
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      button_id.strip_prefix("elo_gate_min_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.splitn(2, '_').collect();
    if parts.len() == 2 {
      let category_id: u8 = parts[0].parse().unwrap_or(0);
      let min_idx: usize = parts[1].parse().unwrap_or(0);

      let ranks = db.ranks.get_ranks(guild_id).await?;
      let min_rank_name = if min_idx == 0 { "No minimum" } else { ranks.get(min_idx).map(|r| r.name.as_str()).unwrap_or("?") };

      let embed = CE::new()
        .title("ELO Gate - Select maximum rank")
        .description(format!("Minimum rank: **{}**\n\nNow select the **maximum** rank that can view this category's category.", min_rank_name))
        .color(0x5865F2);

      let mut options: Vec<(String, String)> =
        ranks.iter().enumerate().filter(|(i, _)| *i >= min_idx).map(|(i, r)| (format!("{} (ELO {})", r.name, r.elo), format!("{}_{}_{}", category_id, min_idx, i))).collect();
      options.push(("No maximum".to_string(), format!("{}_{}_{}", category_id, min_idx, ranks.len())));

      let mut components = Vec::new();
      if let Some(menu) = create_selection_menu("elo_gate_max", "Select maximum rank", options) {
        components.push(menu);
      }
      components.push(CAR::Buttons(vec![CB::new(format!("category_settings_elo_gate_{category_id}")).label("Back").style(BS::Secondary)]));

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    return Ok(());
  }

  if button_id == "elo_gate_max" || button_id.starts_with("elo_gate_max_") {
    let selected = if button_id == "elo_gate_max" {
      match &interaction.data.kind {
        CIDK::StringSelect { values } => values.first().cloned().unwrap_or_default(),
        _ => return Err(anyhow::anyhow!("Expected string select")),
      }
    } else {
      button_id.strip_prefix("elo_gate_max_").unwrap().to_string()
    };
    let parts: Vec<&str> = selected.splitn(3, '_').collect();
    if parts.len() == 3 {
      let category_id: u8 = parts[0].parse().unwrap_or(0);
      let min_idx: usize = parts[1].parse().unwrap_or(0);
      let raw_max: usize = parts[2].parse().unwrap_or(0);

      let ranks = db.ranks.get_ranks(guild_id).await?;
      // Clamp: sentinel ranks.len() ("No maximum") maps to last valid index
      let max_idx = raw_max.min(ranks.len().saturating_sub(1));
      let category_id = {
        let mut manager_lock = manager.lock().await;
        let server = manager_lock.get_qguild(guild_id)?;
        let category = server.categories.iter().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;
        category.channels.category
      };

      match apply_elo_gate(ctx, guild_id, category_id, &ranks, min_idx, max_idx).await {
        Ok(count) => {
          let min_name = if min_idx == 0 { "No minimum" } else { ranks.get(min_idx).map(|r| r.name.as_str()).unwrap_or("?") };
          let max_name = if max_idx >= ranks.len().saturating_sub(1) { "No maximum" } else { ranks.get(max_idx).map(|r| r.name.as_str()).unwrap_or("?") };
          let embed = CE::new()
            .title("ELO Gate Applied")
            .description(format!("Category visibility restricted to ranks **{}** through **{}**.\n{} rank role(s) granted view access.", min_name, max_name, count))
            .color(crate::GREEN);

          let response = CIR::UpdateMessage(
            CIRM::new()
              .embed(embed)
              .components(vec![CAR::Buttons(vec![CB::new(format!("category_settings_back_{category_id}")).label("Back to category settings").style(BS::Secondary)])]),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          let hint = if e.to_string().contains("Missing Access") {
            "\n\nThe bot may lack **Manage Roles** or **Manage Channels** permission on this category. Check the bot's channel-level permissions."
          } else {
            ""
          };
          let embed = CE::new().title("ELO Gate Failed").description(format!("Failed to apply permissions: {}{}", e, hint)).color(RED);
          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![
            CB::new(format!("category_settings_elo_gate_{category_id}")).label("Retry").style(BS::Primary),
            CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary),
          ])]));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    return Ok(());
  }

  if button_id.starts_with("elo_gate_clear_") {
    let category_id_str = button_id.strip_prefix("elo_gate_clear_").unwrap();
    if let Ok(category_id) = category_id_str.parse::<u8>() {
      let category_id = {
        let mut manager_lock = manager.lock().await;
        let server = manager_lock.get_qguild(guild_id)?;
        let category = server.categories.iter().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;
        category.channels.category
      };

      match clear_elo_gate(ctx, guild_id, category_id).await {
        Ok(_) => {
          let embed = CE::new().title("ELO Gate Cleared").description("Category is now visible to everyone.").color(crate::GREEN);
          let response = CIR::UpdateMessage(
            CIRM::new()
              .embed(embed)
              .components(vec![CAR::Buttons(vec![CB::new(format!("category_settings_back_{category_id}")).label("Back to category settings").style(BS::Secondary)])]),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          let embed = CE::new().title("Clear ELO Gate Failed").description(format!("Failed to clear permissions: {}", e)).color(RED);
          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(vec![
            CB::new(format!("elo_gate_clear_{category_id}")).label("Retry").style(BS::Primary),
            CB::new(format!("category_settings_back_{category_id}")).label("Back").style(BS::Secondary),
          ])]));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    return Ok(());
  }

  // Extract category_id from button custom_id (format: category_settings_edit_<action>_<category_id>)
  let category_id: u8 = button_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid button ID format: {}", button_id))?;

  // Get the category by ID
  let mut manager_lock = manager.lock().await;
  let category = {
    let server = manager_lock.get_qguild(guild_id)?;
    server.categories.iter().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?.clone()
  };
  let settings = CategorySettings::from_category(&category);
  drop(manager_lock);

  // Match button action (button_id format: category_settings_edit_<action>_<category_id>)
  if button_id.starts_with("category_settings_edit_name_") {
    let modal = CM::new(format!("category_settings_modal_name_{category_id}"), "Set category name").components(vec![create_short_input_opt(
      "Category name",
      "name",
      "e.g., NA PUGs, EU Competitive",
      &settings.name.unwrap_or_default(),
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_quota_") {
    let modal = CM::new(format!("category_settings_modal_quota_{category_id}"), "Set queue quota").components(vec![create_value_input_sh_cap(
      "Quota (2-100)",
      "quota",
      "Number of players required",
      &settings.quota.to_string(),
      1,
      3,
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_confirm_time_") {
    let modal = CM::new(format!("category_settings_modal_confirm_time_{category_id}"), "Set ready check duration").components(vec![create_value_input_sh_cap(
      "Confirm time (seconds)",
      "confirm_time",
      "Seconds for missing players to join VC when queue goes hot",
      &settings.confirm_time.to_string(),
      1,
      3,
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_connect_") {
    let modal = CM::new(format!("category_settings_modal_connect_{category_id}"), "Set server connect info").components(vec![create_paragraph_input_with_value(
      "Connect command",
      "connect_info",
      "e.g., connect 192.168.1.1:27015; password secret",
      &settings.connect_info.unwrap_or_default(),
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_edit_vc_create_") {
    // Cycle through create policies
    use crate::models::TeamVcCreatePolicy;
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_qguild(guild_id) {
      if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
        let next = match category.team_vc_settings.create_policy {
          TeamVcCreatePolicy::OnFirstJoin => TeamVcCreatePolicy::OnHot,
          TeamVcCreatePolicy::OnHot => TeamVcCreatePolicy::OnGameStart,
          TeamVcCreatePolicy::OnGameStart => TeamVcCreatePolicy::OnFirstJoin,
        };
        category.team_vc_settings.create_policy = next;
        let _ = db.categories.update_team_vc_settings(guild_id, category_id, &category.team_vc_settings).await;
        category.reconcile_team_vcs(ctx, guild_id, db).await;
        refresh_category_settings!(interaction, ctx, category)?;
      }
    }
    drop(manager_lock);
  } else if button_id.starts_with("category_settings_edit_vc_destroy_") {
    // Cycle through destroy policies
    use crate::models::TeamVcDestroyPolicy;
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_qguild(guild_id) {
      if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
        let next = match category.team_vc_settings.destroy_policy {
          TeamVcDestroyPolicy::OnLastLeave => TeamVcDestroyPolicy::AfterPull,
          TeamVcDestroyPolicy::AfterPull => TeamVcDestroyPolicy::AfterExpiration,
          TeamVcDestroyPolicy::AfterExpiration => TeamVcDestroyPolicy::OnLastLeave,
        };
        category.team_vc_settings.destroy_policy = next;
        let _ = db.categories.update_team_vc_settings(guild_id, category_id, &category.team_vc_settings).await;
        category.reconcile_team_vcs(ctx, guild_id, db).await;
        refresh_category_settings!(interaction, ctx, category)?;
      }
    }
    drop(manager_lock);
  } else if button_id.starts_with("category_settings_edit_vc_keepmin_") {
    // Toggle keep_minimum
    let mut manager_lock = manager.lock().await;
    if let Ok(server) = manager_lock.get_qguild(guild_id) {
      if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
        category.team_vc_settings.keep_minimum = !category.team_vc_settings.keep_minimum;
        let _ = db.categories.update_team_vc_settings(guild_id, category_id, &category.team_vc_settings).await;
        category.reconcile_team_vcs(ctx, guild_id, db).await;
        refresh_category_settings!(interaction, ctx, category)?;
      }
    }
    drop(manager_lock);
  } else if button_id.starts_with("category_settings_link_message_") {
    // Handle link message button - search for existing dashboard messages
    let guild_name = guild_name(ctx, guild_id);

    // Get the category to find its dashboard channel
    let categories = db.categories.get_categories_for_guild(guild_id).await?;
    if let Some(category) = categories.iter().find(|g| g.id == category_id) {
      let dashboard_channel = category.channels.dashboard;

      // Search for bot messages in dashboard channel
      let bot_user_id = ctx.cache.current_user().id;
      let mut existing_dashboard_msgs = Vec::new();

      match dashboard_channel.messages(&ctx.http, GM::new().limit(50)).await {
        Ok(messages) => {
          for msg in messages {
            if msg.author.id == bot_user_id && !msg.embeds.is_empty() {
              // Check if it looks like a dashboard
              if let Some(embed) = msg.embeds.first() {
                let title = embed.title.as_deref().unwrap_or("");
                let desc = embed.description.as_deref().unwrap_or("");
                if title.contains("Queue") || desc.contains("Queue") || desc.contains("Join") {
                  existing_dashboard_msgs.push((msg.id, msg.timestamp));
                }
              }
            }
          }
        }
        Err(e) => {
          warn!("[{}] Failed to fetch messages from dashboard channel: {}", guild_name, e);
        }
      }

      // Sort by timestamp (newest first)
      existing_dashboard_msgs.sort_by(|a, b| b.1.cmp(&a.1));

      let mut description = String::new();
      let mut buttons = Vec::new();

      if !existing_dashboard_msgs.is_empty() {
        description.push_str(&format!(
          "**Found {} existing dashboard message(s)**\n\n\
                    Found bot messages in <#{}> that appear to be dashboards.\n\n\
                    **Select a message to link:**",
          existing_dashboard_msgs.len(),
          dashboard_channel.get()
        ));

        // Add a button for each found message (limit to 5 to avoid Discord limits)
        for (i, (msg_id, timestamp)) in existing_dashboard_msgs.iter().take(5).enumerate() {
          let state = format!("{}_{:x}", category_id, msg_id.get());
          let time_str = timestamp.unix_timestamp();
          let label = if i == 0 { 
        format!("Most recent ({})", crate::timestamp_from_unix(time_str, crate::Style::ShortDateTime)) 
      } else { 
        format!("Message {} ({})", i + 1, crate::timestamp_from_unix(time_str, crate::Style::ShortDateTime)) 
      };

          buttons.push(CB::new(format!("category_link_msg_confirm_{}", state)).label(label).style(BS::Success));
        }

        if existing_dashboard_msgs.len() > 5 {
          description.push_str(&format!("\n\n*Showing 5 of {} messages*", existing_dashboard_msgs.len()));
        }
      } else {
        description.push_str(&format!(
          "ℹ️ **No existing dashboard messages found**\n\n\
                    Searched recent messages in <#{}> but didn't find any existing dashboards.\n\n\
                    The bot will continue using the current dashboard message.",
          dashboard_channel.get()
        ));
      }

      // Add manual input button
      buttons.push(CB::new(format!("category_link_msg_manual_{}", category_id)).label("Enter message ID").style(BS::Primary));

      buttons.push(CB::new(format!("category_settings_back_{}", category_id)).label("Back").style(BS::Secondary));

      let embed = CE::new().title(format!("{} - Link Dashboard Message", category.name())).description(description).color(0x5865F2);

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(buttons)]));
      interaction.create_response(&ctx.http, response).await?;
    } else {
      warn!("Category {category_id} not found for guild {guild_id}");
    }
  } else if button_id.starts_with("category_link_msg_confirm_") {
    // Confirm linking message to category
    let state_str = button_id.strip_prefix("category_link_msg_confirm_").unwrap();
    let parts: Vec<&str> = state_str.split('_').collect();

    if parts.len() != 2 {
      warn!("Invalid state format in category_link_msg_confirm: {}", state_str);
      return Ok(());
    }

    let category_id = parts[0].parse::<u8>().map_err(|e| anyhow::anyhow!("Invalid category_id: {}", e))?;
    let dashboard_msg_id = parse_mid(parts[1]).map_err(|e| anyhow::anyhow!("Invalid message_id: {}", e))?;

    // Update database
    match db.categories.update_dashboard_msg_by_category_id(guild_id, category_id, dashboard_msg_id).await {
      Ok(_) => {
        // Update in-memory category
        let mut manager_lock = manager.lock().await;
        if let Ok(server) = manager_lock.get_qguild(guild_id) {
          if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
            category.dashboard_msg = dashboard_msg_id.into();
            info!("Updated category {} dashboard_msg to {} in memory", category_id, dashboard_msg_id);
          }
        }
        drop(manager_lock);

        let embed = CE::new()
          .title("Dashboard Message Linked")
          .description(format!(
            "Successfully linked existing dashboard message to this category.\n\n\
                        Message ID: `{}`\n\n\
                        The bot will now update this message instead of creating a new one.",
            dashboard_msg_id
          ))
          .color(0x57F287);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));
        interaction.create_response(&ctx.http, response).await?;
      }
      Err(e) => {
        error!("Failed to update dashboard_msg for category {}: {}", category_id, e);
        let embed = CE::new().title("Failed to link the message").description(format!("Database error: {}", e)).color(0xED4245);

        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
  } else if button_id.starts_with("category_link_msg_manual_") {
    // Manual message ID input - show modal

    let modal = CM::new(format!("category_link_msg_modal_{}", category_id), "Enter dashboard message ID").components(vec![create_input_sh_cap(
      "Message ID or link",
      "message_id",
      "e.g., 1467572971093885086 or https://discord.com/channels/.../...",
      17,
      200,
    )]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_settings_formats_") {
    // Show formats list screen
    let display =
      FormatListDisplay { category_id, category_name: category.name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
    let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_fmt_back_") {
    // Back from formats list -> category settings
    let settings = CategorySettings::from_category(&category);
    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);
    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_fmt_add_") {
    // Show modal to add a new format

    let modal = CM::new(format!("category_fmt_modal_add_{}", category_id), "Add format")
      .components(vec![create_input_sh("Format name", "name", "e.g., Competitive, Casual"), create_input_sh("Quota (players per match)", "quota", "e.g., 12")]);

    let response = CIR::Modal(modal);
    interaction.create_response(&ctx.http, response).await?;
  } else if button_id.starts_with("category_fmt_remove_") {
    // Show select menu to pick which format to remove
    // Only non-default formats (id != 0) can be removed
    let removable: Vec<(String, String)> =
      category.formats.iter().filter(|sg| sg.id != 0).map(|sg| (format!("{} (quota: {})", sg.name, sg.quota), format!("{}_{}", category_id, sg.id))).collect();

    if removable.is_empty() {
      send_component_error_response(interaction, ctx, "No removable formats (the default format cannot be removed).").await;
    } else {
      use create_selection_menu;
      let mut components = Vec::new();
      if let Some(menu) = create_selection_menu("category_fmt_confirm_remove", "Select format to remove", removable) {
        components.push(menu);
      }
      components.push(CAR::Buttons(vec![crate::models::embeds::Ephemeral::back(format!("category_fmt_back_{}", category_id))]));
      let embed = CE::new().title("Remove format").description("Select a format to remove. The default format cannot be removed.").color(0xED4245);
      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
  } else if button_id.starts_with("category_settings_back_") {
    // Back button - return to category settings screen
    let category_id_str = button_id.strip_prefix("category_settings_back_").unwrap();
    if let Ok(category_id) = category_id_str.parse::<u8>() {
      let categories = db.categories.get_categories_for_guild(guild_id).await?;
      if let Some(category) = categories.iter().find(|g| g.id == category_id) {
        let settings = CategorySettings::from_category(category);
        let embed = build_category_settings_embed(&settings);
        let buttons = build_category_settings_buttons(settings.category_id);
        let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
  } else {
    warn!("Unknown category settings button: {}", button_id);
  }

  Ok(())
}

/// Handle category selection from the selector menu
pub async fn handle_category_settings_select(
  ctx: &Context,
  interaction: &CI,
  _db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, _db).await;
  info!("[Category Settings] {} selected category", user_tag);

  // Extract selected category_id from the interaction
  let category_id: u8 = match &interaction.data.kind {
    CIDK::StringSelect { values } => values.first().and_then(|v| v.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid category selection"))?,
    _ => return Err(anyhow::anyhow!("Expected string select interaction")),
  };

  // Get the category by ID
  let mut manager_lock = manager.lock().await;
  let category = {
    let server = manager_lock.get_qguild(guild_id)?;
    server.categories.iter().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category not found"))?.clone()
  };
  drop(manager_lock);

  let settings = CategorySettings::from_category(&category);

  let embed = build_category_settings_embed(&settings);
  let buttons = build_category_settings_buttons(settings.category_id);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Handle manual dashboard message link modal submissions
pub async fn handle_category_link_msg_modal(
  ctx: &Context,
  interaction: &MI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  // Extract category_id from modal ID (format: category_link_msg_modal_{category_id})
  let category_id: u8 = modal_id.strip_prefix("category_link_msg_modal_").and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

  // Get the message ID input
  let message_input = interaction
    .data
    .components
    .first()
    .and_then(|row| row.components.first())
    .and_then(|comp| if let ARC::InputText(input) = comp { input.value.as_ref() } else { None })
    .ok_or_else(|| anyhow::anyhow!("No message ID provided"))?;

  // Parse message ID from input (could be just ID or a Discord link)
  let dashboard_msg_id = if message_input.contains("discord.com/channels/") {
    // Extract message ID from Discord link
    // Format: https://discord.com/channels/{guild_id}/{channel_id}/{message_id}
    message_input.split('/').next_back().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| anyhow::anyhow!("Invalid Discord message link format"))?
  } else {
    // Parse as direct message ID
    message_input.trim().parse::<u64>().map_err(|_| anyhow::anyhow!("Invalid message ID: must be a number or Discord message link"))?
  };

  // Validate that the message exists in the dashboard channel
  let categories = db.categories.get_categories_for_guild(guild_id).await?;
  let category = categories.iter().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?;

  let dashboard_channel = category.channels.dashboard;

  // Try to fetch the message to verify it exists
  match dashboard_channel.message(&ctx.http, dashboard_msg_id).await {
    Ok(_) => {
      // Message exists, update database
      match db.categories.update_dashboard_msg_by_category_id(guild_id, category_id, dashboard_msg_id).await {
        Ok(_) => {
          // Update in-memory category
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_qguild(guild_id) {
            if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
              category.dashboard_msg = dashboard_msg_id.into();
              info!("Updated category {} dashboard_msg to {} in memory", category_id, dashboard_msg_id);
            }
          }
          drop(manager_lock);

          let embed = CE::new()
            .title("Dashboard Message Linked")
            .description(format!(
              "Successfully linked dashboard message to this category.\n\n\
                            Message ID: `{}`\n\
                            Channel: <#{}>\n\n\
                            The bot will now update this message instead of creating a new one.",
              dashboard_msg_id,
              dashboard_channel.get()
            ))
            .color(0x57F287);

          let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          error!("Failed to update dashboard_msg for category {}: {}", category_id, e);
          let embed = CE::new().title("Failed to link the message").description(format!("Database error: {}", e)).color(0xED4245);

          let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
          interaction.create_response(&ctx.http, response).await?;
        }
      }
    }
    Err(e) => {
      warn!("Message {} not found in channel {}: {}", dashboard_msg_id, dashboard_channel, e);
      let embed = CE::new()
        .title("Message Not Found")
        .description(format!(
          "Could not find message `{}` in <#{}>.\n\n\
                    Please verify:\n\
                    • The message ID is correct\n\
                    • The message exists in the dashboard channel\n\
                    • The bot has permission to view the channel",
          dashboard_msg_id,
          dashboard_channel.get()
        ))
        .color(0xED4245);

      let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
    }
  }

  Ok(())
}

/// Handle category settings modal submissions
pub async fn handle_category_settings_modal(
  ctx: &Context,
  interaction: &MI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Category Settings] {} submitted modal {}", user_tag, modal_id);

  // Handle format modals first (format: category_fmt_modal_{action}_{category_id}_{fmt_id})
  // These have two trailing IDs so they must be handled before the generic rsplit extraction.
  if modal_id.starts_with("category_fmt_modal_edit_") || modal_id.starts_with("category_fmt_modal_add_") {
    return handle_format_modal(ctx, interaction, db, manager, guild_id, modal_id).await;
  }

  // Extract category_id from modal custom_id (format: category_settings_modal_<action>_<category_id>)
  let category_id: u8 = modal_id.rsplit('_').next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

  // Get the category by ID
  let mut manager_lock = manager.lock().await;
  let category = {
    let server = manager_lock.get_qguild(guild_id)?;
    server.categories.iter_mut().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
  };

  if modal_id.starts_with("category_settings_modal_name_") {
    // Extract name value
    let name_str = get_modal_input!(interaction);

    let name = if name_str.trim().is_empty() { None } else { Some(name_str.trim().to_string()) };

    // Update in-memory and build settings while holding lock
    category.name = name.clone();
    let settings = CategorySettings::from_category(category);
    drop(manager_lock);

    // Update in database (after releasing lock)
    db.categories.update_name(guild_id, category_id, name.as_deref()).await?;

    let embed = build_category_settings_embed(&settings);
    let buttons = build_category_settings_buttons(settings.category_id);

    let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("category_settings_modal_quota_") {
    // Extract quota value
    let quota_str = get_modal_input!(interaction);

    let quota: u8 = match quota_str.trim().parse() {
      Ok(q) if (2..=100).contains(&q) => q,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid quota. Must be between 2 and 100.").await;
        return Ok(());
      }
    };

    // Update in-memory
    category.set_quota(quota);

    // Update in database
    db.set_category(guild_id, category.channels.category.get(), category.channels.queue_vc.get(), category.channels.dashboard.get(), category.channels.queue_chat.get(), quota)
      .await?;

    // Update dashboard
    category.queue_dash_update(ctx, guild_id).await;

    // Get updated settings and refresh the menu
    refresh_category_settings_modal!(interaction, ctx, category)?;
  } else if modal_id.starts_with("category_settings_modal_confirm_time_") {
    let confirm_time_str = get_modal_input!(interaction);

    let confirm_time: u16 = match confirm_time_str.trim().parse() {
      Ok(t) if t > 0 => t,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid time. Must be a positive number.").await;
        return Ok(());
      }
    };

    // Update in-memory and persist to database
    category.confirm_time = confirm_time;
    db.categories.update_confirm_time(guild_id, category_id, confirm_time).await?;

    // Get updated settings and refresh the menu
    refresh_category_settings_modal!(interaction, ctx, category)?;
  } else if modal_id.starts_with("category_settings_modal_connect_") {
    // Extract connect info value
    let connect_str = get_modal_input!(interaction);

    let connect_info = if connect_str.trim().is_empty() { None } else { Some(connect_str.trim().to_string()) };

    // Update in-memory
    category.set_connect_info(connect_info);

    // Update dashboard
    category.queue_dash_update(ctx, guild_id).await;

    // Get updated settings and refresh the menu
    refresh_category_settings_modal!(interaction, ctx, category)?;
  } else {
    warn!("Unknown category settings modal: {}", modal_id);
  }

  Ok(())
}

/// Handle format modal submissions (add and edit)
/// Separated from handle_category_settings_modal because the modal ID format
/// (category_fmt_modal_{action}_{category_id}_{fmt_id}) has two trailing IDs,
/// which breaks the generic rsplit('_').next() category_id extraction.
async fn handle_format_modal(
  ctx: &Context,
  interaction: &MI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
  guild_id: GI,
  modal_id: &str,
) -> Result<()> {
  // Extract name and quota from modal fields
  let name_str = get_modal_input!(interaction, 0);
  let quota_str = get_modal_input!(interaction, 1);

  let name = name_str.trim().to_string();
  if name.is_empty() {
    send_modal_error_response(interaction, ctx, "Format name cannot be empty.").await;
    return Ok(());
  }

  let quota: u8 = match quota_str.trim().parse() {
    Ok(q) if q >= 2 => q,
    _ => {
      send_modal_error_response(interaction, ctx, "Invalid quota. Must be a number >= 2.").await;
      return Ok(());
    }
  };

  if modal_id.starts_with("category_fmt_modal_edit_") {
    let suffix = modal_id.strip_prefix("category_fmt_modal_edit_").unwrap();
    let parts: Vec<&str> = suffix.split('_').collect();
    if parts.len() != 2 {
      return Err(anyhow::anyhow!("Invalid edit modal ID format"));
    }
    let category_id: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("Invalid category_id"))?;
    let fmt_id: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("Invalid format_id"))?;

    let mut manager_lock = manager.lock().await;
    let category = {
      let server = manager_lock.get_qguild(guild_id)?;
      server.categories.iter_mut().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
    };

    if let Some(sg) = category.formats.iter_mut().find(|s| s.id == fmt_id) {
      sg.name = name;
      sg.quota = quota;
    } else {
      send_modal_error_response(interaction, ctx, &format!("Format {} not found.", fmt_id)).await;
      return Ok(());
    }

    // Persist to DB
    db.categories.save_all_formats(guild_id, category_id, &category.formats).await?;

    // Update dashboard
    category.queue_dash_update(ctx, guild_id).await;

    // Show updated formats list
    let display =
      FormatListDisplay { category_id, category_name: category.name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
    let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
    interaction.create_response(&ctx.http, response).await?;
  } else if modal_id.starts_with("category_fmt_modal_add_") {
    let category_id: u8 = modal_id.strip_prefix("category_fmt_modal_add_").and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid add modal ID format"))?;

    let mut manager_lock = manager.lock().await;
    let category = {
      let server = manager_lock.get_qguild(guild_id)?;
      server.categories.iter_mut().find(|g| g.id == category_id).ok_or_else(|| anyhow::anyhow!("Category {} not found", category_id))?
    };

    match category.add_format(name, quota) {
      Ok(_) => {
        // Persist to DB
        db.categories.save_all_formats(guild_id, category_id, &category.formats).await?;

        // Update dashboard
        category.queue_dash_update(ctx, guild_id).await;

        // Show updated formats list
        let display =
          FormatListDisplay { category_id, category_name: category.name(), formats: category.formats.iter().map(|sg| (sg.id, sg.name.clone(), sg.quota)).collect() };
        let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
        interaction.create_response(&ctx.http, response).await?;
      }
      Err(e) => {
        send_modal_error_response(interaction, ctx, &format!("Failed to add format: {}", e)).await;
      }
    }
  }

  Ok(())
}