use serenity::all::{
  Context, ComponentInteraction as CoI, ChannelId as CI, ModalInteraction, MessageId as MI, CreateActionRow as CAR, CreateInteractionResponse as CIR,
  CreateButton as CB, ButtonStyle as BS, CreateInteractionResponseMessage as CIRM,
  CreateEmbed as CE, CreateModal as CM,
  CreateSelectMenu as CSM, CreateSelectMenuOption as CSMO, CreateSelectMenuKind as CSMK,
  ComponentInteractionDataKind as CIDK, GuildId as GI, RoleId as RoleId,
  ActionRowComponent as ARC, ChannelType, GetMessages as GM, Permissions, Color,
  CreateInteractionResponseFollowup as CIRF, EditRole as ER,
};
use tracing::{info, warn, error};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use crate::Database;
use crate::handlers::settings::utils::{send_nav_response, send_nav_response_modal, send_component_error_response, send_modal_error_response, create_input_sh, create_input_sh_cap, create_value_input_sh, create_value_input_sh_cap, create_paragraph_input_with_value, get_role_name_with_fallback};
use crate::handlers::settings::core::{parse_cid, parse_opt_cid, parse_mid};
use crate::handlers::settings::menu::{SERVER_CONFIG_TOGGLES, RANK_CONFIG_TOGGLES, RankRoleConfigDisplay, CategoryListDisplay};
use crate::handlers::settings::{CategorySettings, build_category_settings_buttons, build_category_settings_embed, build_server_settings_buttons, build_server_settings_embed, nav_category_list, nav_rank_config, nav_role_config, nav_server_settings};
use crate::models::guild_name;

// Import macros from utils
use crate::{send_nav, send_nav_modal};

/// Server settings structure for display
pub struct ServerSettings {
  pub runner_role: Option<String>,
  pub admin_role: Option<String>,
  pub toggle_states: Vec<bool>,
  pub balance_method: String,
  pub post_game_confirm_time: u16,
}

/// Handle server settings button interactions
pub async fn handle_server_settings_button(
  ctx: &Context,
  interaction: &CoI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let button_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("{} pressed {}", user_tag, button_id);

  match button_id.as_str() {
    // Generic handler for server-level config toggles (ELO-Rank linked, etc.)
    _ if SERVER_CONFIG_TOGGLES.iter().any(|t| t.button_id == button_id) => {
      let toggle = SERVER_CONFIG_TOGGLES.iter().find(|t| t.button_id == button_id).unwrap();

      let current = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
      db.config.set_bool(guild_id, toggle.column, !current).await?;
      send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
    }
    // Generic handler for all rank config toggles (dynamic ELO, ELO-Rank linked, etc.)
    _ if RANK_CONFIG_TOGGLES.iter().any(|t| t.button_id == button_id) => {
      let toggle = RANK_CONFIG_TOGGLES.iter().find(|t| t.button_id == button_id).unwrap();

      let current = db.config.get_bool(guild_id, toggle.column, toggle.default).await?;
      db.config.set_bool(guild_id, toggle.column, !current).await?;
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    "server_settings_roles" => {
      send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
    }
    "server_settings_roles_back" => {
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    "server_settings_runner_role" => {
      if let CIDK::RoleSelect { values } = &interaction.data.kind {
        if let Some(role_id) = values.first() {
          db.config.set_runner_role_id(guild_id, *role_id).await?;
        }
        send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
      }
    }
    "server_settings_admin_role" => {
      if let CIDK::RoleSelect { values } = &interaction.data.kind {
        if let Some(role_id) = values.first() {
          db.config.set_admin_role_id(guild_id, *role_id).await?;
        }
        send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
      }
    }
    "server_settings_ranks" => {
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    "server_settings_ranks_back" => {
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    "server_settings_rank_select" => {
      // Handle rank selection from dropdown (value is role ID)
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(role_id_str) = values.first() {
          let guild_name = guild_name(ctx, guild_id);

          if let Ok(role_id) = role_id_str.parse::<u64>() {
            let rid = RoleId::new(role_id);
            if let Ok(guild_rank) = db.ranks.rank_from_role_id(guild_id, rid).await {
              let display =
                RankRoleConfigDisplay { guild_name, rank_name: guild_rank.name.clone(), rank_key: guild_rank.name.clone(), elo: guild_rank.elo, role_id: guild_rank.role_id };

              let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
              interaction.create_response(&ctx.http, response).await?;
            }
          }
        }
      }
    }
    "server_settings_rank_link_role" => {
      // Handle role selection for linking existing rank
      let selected_role_id = if let CIDK::RoleSelect { values } = &interaction.data.kind {
        values.first().copied().ok_or_else(|| anyhow!("No role selected"))?
      } else {
        return Err(anyhow!("No role selected"));
      };

      // Get the role name to use as default
      let role_name = get_role_name_with_fallback(ctx, guild_id, selected_role_id).await;

      // Show modal to specify rank name and ELO for the selected role

      let modal = CM::new(format!("server_settings_rank_modal_link_{}", selected_role_id.get()), "Link existing rank").components(vec![
        create_value_input_sh("Rank name", "name", "e.g., Bronze, Gold, Platinum", &role_name),
        create_input_sh_cap("ELO Threshold", "elo", "Minimum ELO for this rank", 1, 3),
      ]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_rank_back" => {
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    _ if button_id.starts_with("server_settings_rank_edit_") => {
      // Handle rank name/ELO edit button
      let rank_name = button_id.strip_prefix("server_settings_rank_edit_").unwrap();
      if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
        let modal = CM::new(format!("server_settings_rank_modal_{}", rank_name), format!("Edit {} rank", guild_rank.name)).components(vec![
          create_value_input_sh("Rank name", "name", "e.g., Beginner, Expert, Champion", &guild_rank.name),
          create_value_input_sh_cap("ELO Threshold", "elo", "Minimum ELO for this rank", &guild_rank.elo.to_string(), 1, 3),
        ]);

        let response = CIR::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "server_settings_rank_add" => {
      // Show modal to add a new rank

      let modal = CM::new("server_settings_rank_modal_add", "Add new rank")
        .components(vec![create_input_sh("Rank name", "name", "e.g., Champion, Legend, Elite"), create_input_sh_cap("ELO Threshold", "elo", "Minimum ELO for this rank", 1, 3)]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_rank_link" => {
      // Show role selector for linking existing rank
      let response = CIR::UpdateMessage(
        CIRM::new()
          .embed(
            CE::new()
              .title("Link ranks")
              .description("Select a Discord role to link to a new rank. The role will be used to assign this rank to players automatically.")
              .color(0x5865F2),
          )
          .components(vec![
            CAR::SelectMenu(
              CSM::new("server_settings_rank_link_role", CSMK::Role { default_roles: None }).placeholder("Select a Discord role to link").min_values(1).max_values(1),
            ),
            CAR::Buttons(vec![CB::new("server_settings_ranks_back").label("Back to ranks").style(BS::Secondary)]),
          ]),
      );
      interaction.create_response(&ctx.http, response).await?;
    }
    _ if button_id.starts_with("server_settings_rank_delete_") => {
      let rank_name = button_id.strip_prefix("server_settings_rank_delete_").unwrap();
      db.ranks.delete_rank(guild_id, rank_name).await?;
      let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
      info!("{} deleted rank {}", user_tag, rank_name);
      send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
    }
    _ if button_id.starts_with("server_settings_rank_role_") => {
      // Handle role selector for linking Discord role to rank
      let rank_name = button_id.strip_prefix("server_settings_rank_role_").unwrap();

      // Get selected role from interaction
      let selected_role_id = if let CIDK::RoleSelect { values } = &interaction.data.kind {
        values.first().copied().ok_or_else(|| anyhow!("No role selected"))?
      } else {
        return Err(anyhow!("No role selected"));
      };

      // Update rank's linked role in DB
      db.ranks.update_rank_role(guild_id, rank_name, selected_role_id).await?;

      let role_display = format!("<@&{}>", selected_role_id.get());
      let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
      info!("{} linked rank {} to role {}", user_tag, rank_name, role_display);

      // Refresh the rank config display
      let guild_name = guild_name(ctx, guild_id);
      if let Ok(Some(guild_rank)) = db.ranks.get_rank_by_name(guild_id, rank_name).await {
        let display = RankRoleConfigDisplay { guild_name, rank_name: guild_rank.name.clone(), rank_key: rank_name.to_string(), elo: guild_rank.elo, role_id: guild_rank.role_id };

        let response = CIR::UpdateMessage(CIRM::new().embed(display.build_embed()).components(display.build_components()));
        interaction.create_response(&ctx.http, response).await?;
      }
    }
    "server_settings_default_rank_select" => {
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(role_id_str) = values.first() {
          // Parse role ID from string
          if let Ok(role_id_u64) = role_id_str.parse::<u64>() {
            let role_id = RoleId::new(role_id_u64);

            // Set default rank as role ID
            db.config.set_default_rank_role_id(guild_id, role_id).await?;

            send_nav!(interaction, ctx, db, nav_rank_config, guild_id)?;
          }
        }
      }
    }
    "server_settings_categories" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    "server_settings_categories_back" => {
      send_nav!(interaction, ctx, db, nav_server_settings, guild_id)?;
    }
    "server_settings_edit_post_game_confirm_time" => {
      // Show modal to edit post-game timeout

      let current_confirm_time = db.config.get_post_game_confirm_time(guild_id).await.unwrap_or(120);

      let modal = CM::new("server_settings_post_game_confirm_time_modal", "Edit post-game confirm time").components(vec![create_value_input_sh_cap(
        "Post-game confirm time (seconds)",
        "post_game_confirm_time_input",
        "Enter time in seconds (30-300)",
        &current_confirm_time.to_string(),
        1,
        3,
      )]);

      interaction.create_response(&ctx.http, CIR::Modal(modal)).await?;
    }
    "server_settings_create_roles" => {
      // Create runner, admin, and rank roles
      let guild_name = guild_name(ctx, guild_id);

      // Create Runner role if not configured
      let runner_role = db.config.get_runner_role_id(guild_id).await?;
      if runner_role.is_none() {
        match guild_id.create_role(&ctx.http, ER::new().name("PUG Runner").colour(crate::RUNNER).permissions(Permissions::empty())).await {
          Ok(role) => {
            if let Err(e) = db.config.set_runner_role_id(guild_id, role.id).await {
              warn!("Failed to save runner_role config: {e}");
            }
            info!("[{}] Created PUG Runner role", guild_name);
          }
          Err(e) => {
            warn!("[{}] Failed to create PUG Runner role: {}", guild_name, e);
          }
        }
      }

      // Create Admin role if not configured
      let admin_role = db.config.get_admin_role_id(guild_id).await?;
      if admin_role.is_none() {
        match guild_id.create_role(&ctx.http, ER::new().name("PUG Admin").colour(crate::ADMIN).permissions(Permissions::empty())).await {
          Ok(role) => {
            if let Err(e) = db.config.set_admin_role_id(guild_id, role.id).await {
              warn!("Failed to save admin_role config: {e}");
            }
            info!("[{}] Created PUG Admin role", guild_name);
          }
          Err(e) => {
            warn!("[{}] Failed to create PUG Admin role: {}", guild_name, e);
          }
        }
      }

      // Initialize default ranks in database
      if let Err(e) = db.ranks.init_default_ranks(guild_id).await {
        warn!("[{}] Failed to initialize default ranks: {}", guild_name, e);
      } else {
        info!("[{}] Initialized default ranks", guild_name);
      }

      send_nav!(interaction, ctx, db, nav_role_config, guild_id)?;
    }
    "server_settings_create_category" => {
      // Show modal to collect category settings before creating channels

      let modal = CM::new("server_settings_modal_create_category", "Create a new category").components(vec![
        create_input_sh("Category name", "category_name", "e.g., NA PUGs, EU Competitive"),
        create_input_sh("Channel prefix", "channel_prefix", "e.g., pug, na, eu"),
        create_value_input_sh("Category name", "discord_category", "e.g., PUG Queue", "PUG Queue"),
        create_value_input_sh_cap("Quota (players per game)", "quota", "e.g., 12", &crate::DEFAULT_QUOTA.to_string(), 1, 3),
        create_paragraph_input_with_value("Bot-only dashboard (yes/no)", "bot_only_dashboard", "Set to 'yes' to restrict dashboard channel to bot-only messages", "yes"),
      ]);

      let response = CIR::Modal(modal);
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_link_category" => {
      // Show category selection dropdown to link existing category

      let guild_name = guild_name(ctx, guild_id);

      // Get all categories in the guild - extract data before any awaits
      let mut categories: Vec<(CI, String)> = {
        let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;
        guild.channels.iter().filter_map(|(id, channel)| if channel.kind == ChannelType::Category { Some((*id, channel.name.clone())) } else { None }).collect()
      };

      // Sort by name
      categories.sort_by(|a, b| a.1.cmp(&b.1));

      if categories.is_empty() {
        let response = CIR::Message(CIRM::new().content("No categories found in this server. Please create a category with the required channels first.").ephemeral(true));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      // Create dropdown with categories
      let options: Vec<CSMO> = categories
        .iter()
        .take(25) // Discord limit
        .map(|(id, name)| CSMO::new(name.clone(), id.get().to_string()).description(format!("Category ID: {}", id.get())))
        .collect();

      let select_menu = CSM::new("server_settings_link_category_select", CSMK::String { options }).placeholder("Select a category to link");

      let embed = CE::new()
        .title(format!("{} - Link Existing Category", guild_name))
        .description(
          "**Select a category to link as a category**\n\n\
                    The category must contain these channels:\n\
                    • `dashboard` - Text channel for the dashboard\n\
                    • `queue` - Text channel for queue chat\n\
                    • `ping` - Text channel for ping notifications\n\
                    • `queue-vc` - Voice channel for the queue\n\
                    • `red` - Voice channel for red team\n\
                    • `blue` - Voice channel for blue team\n\n\
                    Channel names must match exactly (case-insensitive).",
        )
        .color(0x5865F2);

      let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_link_cancel").label("Cancel").style(BS::Secondary)])];

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_link_category_select" => {
      // Handle category selection - verify channels and link
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(category_id_str) = values.first() {
          if let Ok(category_id_u64) = category_id_str.parse::<u64>() {
            let category_id = CI::new(category_id_u64);
            let guild_name = guild_name(ctx, guild_id);

            // Find channels in this category - extract data before any awaits
            let (dashboard_channel, queue_channel, ping_channel, queue_vc_channel, red_channel, blue_channel) = {
              let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;

              let mut dashboard_channel = None;
              let mut queue_channel = None;
              let mut ping_channel = None;
              let mut queue_vc_channel = None;
              let mut red_channel = None;
              let mut blue_channel = None;

              for (channel_id, channel) in &guild.channels {
                if channel.parent_id == Some(category_id) {
                  let name_lower = channel.name.to_lowercase();

                  // Dashboard channel (text)
                  if dashboard_channel.is_none() && channel.kind == ChannelType::Text && (name_lower == "dashboard" || name_lower == "dash") {
                    dashboard_channel = Some(*channel_id);
                  }

                  // Queue chat channel (text) - try multiple variations
                  if queue_channel.is_none()
                    && channel.kind == ChannelType::Text
                    && (name_lower == "queue" || name_lower == "pug-chat" || name_lower == "chat" || name_lower == "queue-chat" || name_lower == "pug")
                  {
                    queue_channel = Some(*channel_id);
                  }

                  // Ping channel (text)
                  if ping_channel.is_none() && channel.kind == ChannelType::Text && name_lower == "ping" {
                    ping_channel = Some(*channel_id);
                  }

                  // Queue voice channel - try multiple variations
                  if queue_vc_channel.is_none() && channel.kind == ChannelType::Voice && name_lower == "queue-vc"
                    || name_lower == "queue"
                    || name_lower == "pug"
                    || name_lower == "queue vc"
                    || name_lower == "waiting"
                  {
                    queue_vc_channel = Some(*channel_id);
                  }

                  // Red team voice channel
                  if red_channel.is_none() && channel.kind == ChannelType::Voice && (name_lower == "red" || name_lower == "red team" || name_lower == "team red") {
                      red_channel = Some(*channel_id);
                    }

                  // Blue team voice channel
                  if blue_channel.is_none() && channel.kind == ChannelType::Voice && (name_lower == "blue" || name_lower == "blue team" || name_lower == "team blue" || name_lower == "blu") {
                      blue_channel = Some(*channel_id);
                    }
                }
              }

              (dashboard_channel, queue_channel, ping_channel, queue_vc_channel, red_channel, blue_channel)
            };

            // Check if any channels are missing
            let has_all_channels = dashboard_channel.is_some() && queue_channel.is_some() && ping_channel.is_some() && queue_vc_channel.is_some() && red_channel.is_some() && blue_channel.is_some();

            if !has_all_channels {
              // Start manual channel selection flow

              // Get all text and voice channels in the guild
              let (text_channels, voice_channels) = {
                let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;
                let mut text_chans = Vec::new();
                let mut voice_chans = Vec::new();

                for (channel_id, channel) in &guild.channels {
                  if channel.parent_id == Some(category_id) {
                    match channel.kind {
                      ChannelType::Text => {
                        text_chans.push((*channel_id, channel.name.clone()));
                      }
                      ChannelType::Voice => {
                        voice_chans.push((*channel_id, channel.name.clone()));
                      }
                      _ => {}
                    }
                  }
                }
                (text_chans, voice_chans)
              };

              // Determine which channel to select first
              let (next_channel_type, next_channel_name, available_channels) = if dashboard_channel.is_none() {
                ("dashboard", "Dashboard (text)", text_channels.clone())
              } else if queue_channel.is_none() {
                ("queue", "Queue chat (text)", text_channels.clone())
              } else if ping_channel.is_none() {
                ("ping", "Ping channel (text)", text_channels)
              } else if queue_vc_channel.is_none() {
                ("queue_vc", "Queue voice channel", voice_channels)
              } else if red_channel.is_none() {
                ("red", "Red team voice channel", voice_channels.clone())
              } else {
                ("blue", "Blue team voice channel", voice_channels)
              };

              if available_channels.is_empty() {
                let response = CIR::Message(
                  CIRM::new()
                    .content(
                      "No suitable channels found in this category.\n\n\
                                            Please create the required channels first."
                    )
                    .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
              }

              // Create channel selection dropdown
              let options: Vec<CSMO> = available_channels.iter().map(|(id, name)| CSMO::new(name.clone(), id.get().to_string())).collect();

              // Encode state compactly: use hex for IDs and single char for type
              // Format: cat_d_q_p_qv_r_b_t where each is hex (or 0)
              let type_char = match next_channel_type {
                "dashboard" => "d",
                "queue" => "q",
                "ping" => "p",
                "queue_vc" => "v",
                "red" => "r",
                "blue" => "b",
                _ => "x",
              };
              let state = format!(
                "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}_{:x}_{}",
                category_id.get(),
                dashboard_channel.map(|c| c.get()).unwrap_or(0),
                queue_channel.map(|c| c.get()).unwrap_or(0),
                ping_channel.map(|c| c.get()).unwrap_or(0),
                queue_vc_channel.map(|c| c.get()).unwrap_or(0),
                red_channel.map(|c| c.get()).unwrap_or(0),
                blue_channel.map(|c| c.get()).unwrap_or(0),
                type_char
              );

              let select_menu = CSM::new(format!("link_ch_{}", state), CSMK::String { options }).placeholder(format!("Select {}", next_channel_name));

              // Build status message
              let mut status = String::from("**Channel Linking Progress:**\n\n");
              status.push_str(&format!("Dashboard: {}\n", if let Some(id) = dashboard_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Chat: {}\n", if let Some(id) = queue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Ping Channel: {}\n", if let Some(id) = ping_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Voice: {}\n", if let Some(id) = queue_vc_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Red Team: {}\n", if let Some(id) = red_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Blue Team: {}\n", if let Some(id) = blue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));

              let embed = CE::new()
                .title(format!("{} - Link Channels", guild_name))
                .description(format!("{}\n\n**Next:** Select the {} channel from the dropdown below.", status, next_channel_name))
                .color(0x5865F2);

              let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_link_cancel").label("Cancel").style(BS::Secondary)])];

              let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
              interaction.create_response(&ctx.http, response).await?;
              return Ok(());
            }

            // All channels found - search for existing dashboard messages
            let dashboard_channel = dashboard_channel.unwrap();
            let queue_channel = queue_channel.unwrap();
            let queue_vc_channel = queue_vc_channel.unwrap();
            let red_channel = red_channel.unwrap();
            let blue_channel = blue_channel.unwrap();

            // Check for existing categories using these channels
            let existing_categories = db.categories.get_categories_for_guild(guild_id).await?;
            let duplicate_category = existing_categories.iter().find(|g| {
              g.channels.dashboard == dashboard_channel
                || g.channels.queue_chat == queue_channel
                || g.channels.queue_vc == queue_vc_channel
                || g.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
            });

            // Search for bot messages in dashboard channel
            let bot_user_id = ctx.cache.current_user().id;
            let mut existing_dashboard_msgs = Vec::new();

            match dashboard_channel.messages(&ctx.http, GM::new().limit(50)).await {
              Ok(messages) => {
                for msg in messages {
                  if msg.author.id == bot_user_id && !msg.embeds.is_empty() {
                    // Check if it looks like a dashboard (has embed with "Queue" in title/description)
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

            // Build prompt based on what we found
            let mut description = String::new();
            let mut buttons = Vec::new();

            if let Some(dup_category) = duplicate_category {
              description.push_str(&format!(
                "⚠️ **Duplicate Category Detected**\n\n\
                                Category {} is already using one or more of these channels:\n\
                                • Dashboard: <#{}>\n\
                                • Queue Chat: <#{}>\n\
                                • Queue Voice: <#{}>\n\
                                • Red Team: <#{}>\n\
                                • Blue Team: <#{}>\n\n",
                dup_category.name(),
                dup_category.channels.dashboard.get(),
                dup_category.channels.queue_chat.get(),
                dup_category.channels.queue_vc.get(),
                dup_category.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                dup_category.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0)
              ));

              if !existing_dashboard_msgs.is_empty() {
                description.push_str(&format!(
                  "Found {} existing dashboard message(s) in <#{}>.\n\n\
                                    **Options:**\n\
                                    • Remove duplicate category and link to existing dashboard\n\
                                    • Create new dashboard (will create duplicate)\n\
                                    • Cancel",
                  existing_dashboard_msgs.len(),
                  dashboard_channel.get()
                ));

                // Encode state: channels + existing message ID
                let state = format!(
                  "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}",
                  dashboard_channel.get(),
                  queue_channel.get(),
                  queue_vc_channel.get(),
                  red_channel.get(),
                  blue_channel.get(),
                  existing_dashboard_msgs[0].0.get()
                );

                buttons.push(CB::new(format!("link_existing_remove_dup_{}", state)).label("Remove duplicate & link existing").style(BS::Success));
              } else {
                description.push_str(
                  "No existing dashboard messages found.\n\n\
                                    **Options:**\n\
                                    • Remove duplicate category and create new dashboard\n\
                                    • Provide message ID manually\n\
                                    • Cancel",
                );

                let state = format!("{:x}_{:x}_{:x}_{:x}_{:x}", dashboard_channel.get(), queue_channel.get(), queue_vc_channel.get(), red_channel.get(), blue_channel.get());

                buttons.push(CB::new(format!("link_remove_dup_new_{}", state)).label("Remove duplicate & create new").style(BS::Success));
                buttons.push(CB::new(format!("link_manual_msg_{}", state)).label("Provide message ID").style(BS::Primary));
              }
            } else if !existing_dashboard_msgs.is_empty() {
              description.push_str(&format!(
                "**Found {} existing dashboard message(s)**\n\n\
                                Found bot messages in <#{}> that appear to be dashboards.\n\
                                Most recent: <https://discord.com/channels/{}/{}/{}>\n\n\
                                **Options:**\n\
                                • Link to existing dashboard (recommended)\n\
                                • Create new dashboard\n\
                                • Provide different message ID\n\
                                • Cancel",
                existing_dashboard_msgs.len(),
                dashboard_channel.get(),
                guild_id.get(),
                dashboard_channel.get(),
                existing_dashboard_msgs[0].0.get()
              ));

              let state = format!(
                "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}",
                dashboard_channel.get(),
                queue_channel.get(),
                queue_vc_channel.get(),
                red_channel.get(),
                blue_channel.get(),
                existing_dashboard_msgs[0].0.get()
              );

              buttons.push(CB::new(format!("link_use_existing_{}", state)).label("Link to existing dashboard").style(BS::Success));
              buttons.push(CB::new(format!("link_create_new_{}", state)).label("Create new dashboard").style(BS::Primary));
              buttons.push(CB::new(format!("link_manual_msg_{}", state)).label("Provide message ID").style(BS::Secondary));
            } else {
              description.push_str(&format!(
                "ℹ️ **No existing dashboard messages found**\n\n\
                                Searched recent messages in <#{}> but didn't find any existing dashboards.\n\n\
                                **Options:**\n\
                                • Create new dashboard\n\
                                • Provide message ID manually (if you know it exists)\n\
                                • Cancel",
                dashboard_channel.get()
              ));

              let state = format!("{:x}_{:x}_{:x}_{:x}_{:x}", dashboard_channel.get(), queue_channel.get(), queue_vc_channel.get(), red_channel.get(), blue_channel.get());

              buttons.push(CB::new(format!("link_create_new_{}", state)).label("Create new dashboard").style(BS::Success));
              buttons.push(CB::new(format!("link_manual_msg_{}", state)).label("Provide message ID").style(BS::Secondary));
            }

            buttons.push(CB::new("server_settings_link_cancel").label("Cancel").style(BS::Danger));

            let embed = CE::new().title(format!("{} - Link Category Options", guild_name)).description(description).color(0x5865F2);

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(buttons)]));
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
          }
        }
      }
    }
    "server_settings_link_cancel" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    _ if button_id.starts_with("link_use_existing_") => {
      // Link to existing dashboard message
      let state_str = button_id.strip_prefix("link_use_existing_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() != 6 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel = parse_cid(parts[1])?;
      let queue_vc_channel = parse_cid(parts[2])?;
      let dashboard_msg_id = parse_mid(parts[5])?;

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      // Create category with existing message ID
      let category_config = crate::db::repo::category::CategoryConfig {
        channel_category_id: category_id.get(),
        dashboard_channel_id: dashboard_channel.get(),
        chat_channel_id: queue_channel.get(),
        queue_vc_id: queue_vc_channel.get(),
        ping_channel_id: 1,
        quota: crate::DEFAULT_QUOTA,
      };

      let guild_name = guild_name(ctx, guild_id);

      match db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
        Ok(db_category) => {
          info!("[{}] Category {} linked to existing dashboard {}", guild_name, db_category.id, dashboard_msg_id);

          // Add category to in-memory server
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_qguild(guild_id) {
            if let Err(e) = server.add_category(db_category.clone()) {
              error!("Failed to add category to server: {e}");
            }
          }
          drop(manager_lock);

          // Show success and return to category list
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

          let response = CIR::UpdateMessage(
            CIRM::new().content("Successfully linked category to existing dashboard!").embed(display.build_embed()).components(display.build_components()),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          warn!("[{}] Failed to save linked category: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_create_new_") => {
      // Create new dashboard message
      let state_str = button_id.strip_prefix("link_create_new_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() < 5 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel = parse_cid(parts[1])?;
      let queue_vc_channel = parse_cid(parts[2])?;
      let red_channel = parse_cid(parts[3])?;
      let blue_channel = parse_cid(parts[4])?;

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      use crate::models::{Category, Channels, TeamChannel};

      let mut temp_category = Category::new(
        guild_id,
        None,
        0,
        None,
        crate::DEFAULT_QUOTA,
        crate::DEFAULT_CONFIRM_TIME,
        MI::new(1),
        Channels {
          category: category_id,
          queue_chat: queue_channel,
          queue_vc: queue_vc_channel,
          ping_channel: CI::new(1),
          teams: vec![TeamChannel { red_vc: red_channel, blu_vc: blue_channel, set_index: 1, session_id: None }],
          dashboard: dashboard_channel,
        },
        vec![],
      );

      let guild_name = guild_name(ctx, guild_id);

      // Publish the dashboard to get the actual message ID
      match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
        Ok(_) => {
          let dashboard_msg_id = temp_category.dashboard_msg.get();
          info!("[{}] Dashboard message created with ID {} (linked category)", guild_name, dashboard_msg_id);

          // Create the category in the database
          let category_config = crate::db::repo::category::CategoryConfig {
            channel_category_id: category_id.get(),
            dashboard_channel_id: dashboard_channel.get(),
            chat_channel_id: queue_channel.get(),
            queue_vc_id: queue_vc_channel.get(),
            ping_channel_id: 1,
            quota: crate::DEFAULT_QUOTA,
          };
          match db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
            Ok(db_category) => {
              info!("[{}] Category {} linked and saved to database", guild_name, db_category.id);

              // Add category to in-memory server
              let mut manager_lock = manager.lock().await;
              if let Ok(server) = manager_lock.get_qguild(guild_id) {
                if let Err(e) = server.add_category(db_category.clone()) {
                  error!("Failed to add category to server: {e}");
                }
              }
              drop(manager_lock);

              // Show success message and return to category list
              let categories = db.categories.get_categories_for_guild(guild_id).await?;
              let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

              let response =
                CIR::UpdateMessage(CIRM::new().content("Successfully linked category from category!").embed(display.build_embed()).components(display.build_components()));
              interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
              // Database save failed - clean up dashboard message
              let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;

              warn!("[{}] Failed to save linked category to database: {}", guild_name, e);
              send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
            }
          }
        }
        Err(e) => {
          warn!("[{}] Failed to create dashboard for linked category: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to create dashboard: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_ch_") => {
      // Handle channel selection in manual linking flow
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(selected_channel_str) = values.first() {
          if let Ok(selected_channel_id) = selected_channel_str.parse::<u64>() {
            let selected_channel = CI::new(selected_channel_id);

            // Decode state from custom_id: link_ch_{hex_state}
            let state_str = button_id.strip_prefix("link_ch_").unwrap();
            let parts: Vec<&str> = state_str.split('_').collect();

            if parts.len() != 8 {
              send_component_error_response(interaction, ctx, "Invalid state. Please start over.").await;
              return Ok(());
            }

            let category_id = parse_cid(parts[0])?;
            let mut dashboard_channel = parse_opt_cid(parts[1])?;
            let mut queue_channel = parse_opt_cid(parts[2])?;
            let mut ping_channel = parse_opt_cid(parts[3])?;
            let mut queue_vc_channel = parse_opt_cid(parts[4])?;
            let mut red_channel = parse_opt_cid(parts[5])?;
            let mut blue_channel = parse_opt_cid(parts[6])?;
            let type_char = parts[7];
            let channel_type = match type_char {
              "d" => "dashboard",
              "q" => "queue",
              "p" => "ping",
              "v" => "queue_vc",
              "r" => "red",
              "b" => "blue",
              _ => "unknown",
            };

            // Update the appropriate channel based on type
            match channel_type {
              "dashboard" => dashboard_channel = Some(selected_channel),
              "queue" => queue_channel = Some(selected_channel),
              "ping" => ping_channel = Some(selected_channel),
              "queue_vc" => queue_vc_channel = Some(selected_channel),
              "red" => red_channel = Some(selected_channel),
              "blue" => blue_channel = Some(selected_channel),
              _ => {}
            }

            // Check if all channels are now selected
            if let (Some(dashboard_chan), Some(queue_chan), Some(ping_chan), Some(queue_vc_chan), Some(red_chan), Some(blue_chan)) = 
              (dashboard_channel, queue_channel, ping_channel, queue_vc_channel, red_channel, blue_channel) {
              // All channels selected - create the category
              let guild_name = guild_name(ctx, guild_id);

              use crate::models::{Category, Channels, TeamChannel};

              // Derive category from dashboard channel's parent
              let category_id = ctx.cache.channel(dashboard_chan).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

              let mut temp_category = Category::new(
                guild_id,
                None,
                0,
                None,
                crate::DEFAULT_QUOTA,
                crate::DEFAULT_CONFIRM_TIME,
                MI::new(1),
                Channels {
                  category: category_id,
                  queue_chat: queue_chan,
                  queue_vc: queue_vc_chan,
                  ping_channel: ping_chan,
                  teams: vec![TeamChannel { red_vc: red_chan, blu_vc: blue_chan, set_index: 1, session_id: None }],
                  dashboard: dashboard_chan,
                },
                vec![],
              );

              // Publish the dashboard
              match temp_category.dash_publish(ctx, dashboard_chan, db, guild_id).await {
                Ok(_) => {
                  let dashboard_msg_id = temp_category.dashboard_msg.get();
                  info!("[{}] Dashboard message created with ID {} (linked category)", guild_name, dashboard_msg_id);

                  // Create the category in the database
                  let category_config = crate::db::repo::category::CategoryConfig {
                    channel_category_id: category_id.get(),
                    dashboard_channel_id: dashboard_chan.get(),
                    chat_channel_id: queue_chan.get(),
                    queue_vc_id: queue_vc_chan.get(),
                    ping_channel_id: ping_chan.get(),
                    quota: crate::DEFAULT_QUOTA,
                  };

                  match db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
                    Ok(db_category) => {
                      info!("[{}] Category {} linked and saved to database", guild_name, db_category.id);

                      // Add category to in-memory server
                      let mut manager_lock = manager.lock().await;
                      if let Ok(server) = manager_lock.get_qguild(guild_id) {
                        if let Err(e) = server.add_category(db_category.clone()) {
                          error!("Failed to add category to server: {e}");
                        }
                      }
                      drop(manager_lock);

                      // Show success and return to category list
                      let categories = db.categories.get_categories_for_guild(guild_id).await?;
                      let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

                      let response =
                        CIR::UpdateMessage(CIRM::new().content("Successfully linked category from category!").embed(display.build_embed()).components(display.build_components()));
                      interaction.create_response(&ctx.http, response).await?;
                    }
                    Err(e) => {
                      let _ = dashboard_channel.unwrap().delete_message(&ctx.http, dashboard_msg_id).await;
                      warn!("[{}] Failed to save linked category to database: {}", guild_name, e);
                      send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
                    }
                  }
                }
                Err(e) => {
                  warn!("[{}] Failed to create dashboard for linked category: {}", guild_name, e);
                  send_component_error_response(interaction, ctx, &format!("Failed to create dashboard: {e}")).await;
                }
              }
            } else {
              // Continue to next channel - recursively trigger the same logic
              // by simulating a category selection with updated state
              let _fake_category_id_str = category_id.get().to_string();

              // Reuse the same logic by creating a fake interaction data
              // Actually, let's just redirect back to the category select handler
              // by crafting the state as if we just selected the category

              // Get guild name
              let guild_name = guild_name(ctx, guild_id);

              // Continue with manual channel selection flow (same code as above)

              let (text_channels, voice_channels) = {
                let guild = ctx.cache.guild(guild_id).ok_or_else(|| anyhow!("Guild not found"))?;
                let mut text_chans = Vec::new();
                let mut voice_chans = Vec::new();

                for (channel_id, channel) in &guild.channels {
                  if channel.parent_id == Some(category_id) {
                    match channel.kind {
                      ChannelType::Text => {
                        text_chans.push((*channel_id, channel.name.clone()));
                      }
                      ChannelType::Voice => {
                        voice_chans.push((*channel_id, channel.name.clone()));
                      }
                      _ => {}
                    }
                  }
                }
                (text_chans, voice_chans)
              };

              let (next_channel_type, next_channel_name, available_channels) = if dashboard_channel.is_none() {
                ("dashboard", "Dashboard (text)", text_channels)
              } else if queue_channel.is_none() {
                ("queue", "Queue chat (text)", text_channels)
              } else if queue_vc_channel.is_none() {
                ("queue_vc", "Queue voice channel", voice_channels)
              } else if red_channel.is_none() {
                ("red", "Red team voice channel", voice_channels)
              } else {
                ("blue", "Blue team voice channel", voice_channels)
              };

              let options: Vec<CSMO> = available_channels.iter().map(|(id, name)| CSMO::new(name.clone(), id.get().to_string())).collect();

              let type_char = match next_channel_type {
                "dashboard" => "d",
                "queue" => "q",
                "queue_vc" => "v",
                "red" => "r",
                "blue" => "b",
                _ => "x",
              };
              let state = format!(
                "{:x}_{:x}_{:x}_{:x}_{:x}_{:x}_{}",
                category_id.get(),
                dashboard_channel.map(|c| c.get()).unwrap_or(0),
                queue_channel.map(|c| c.get()).unwrap_or(0),
                queue_vc_channel.map(|c| c.get()).unwrap_or(0),
                red_channel.map(|c| c.get()).unwrap_or(0),
                blue_channel.map(|c| c.get()).unwrap_or(0),
                type_char
              );

              let select_menu = CSM::new(format!("link_ch_{}", state), CSMK::String { options }).placeholder(format!("Select {}", next_channel_name));

              let mut status = String::from("**Channel Linking Progress:**\n\n");
              status.push_str(&format!("Dashboard: {}\n", if let Some(id) = dashboard_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Chat: {}\n", if let Some(id) = queue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Queue Voice: {}\n", if let Some(id) = queue_vc_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Red Team: {}\n", if let Some(id) = red_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));
              status.push_str(&format!("Blue Team: {}\n", if let Some(id) = blue_channel { format!("<#{}>", id.get()) } else { "Not selected".to_string() }));

              let embed = CE::new()
                .title(format!("{} - Link Channels", guild_name))
                .description(format!("{}\n\n**Next:** Select the {} channel from the dropdown below.", status, next_channel_name))
                .color(0x5865F2);

              let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_link_cancel").label("Cancel").style(BS::Secondary)])];

              let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
              interaction.create_response(&ctx.http, response).await?;
            }
          }
        }
      }
    }
    _ if button_id.starts_with("link_existing_remove_dup_") => {
      // Remove duplicate category and link to existing dashboard
      let state_str = button_id.strip_prefix("link_existing_remove_dup_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() != 6 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel = parse_cid(parts[1])?;
      let queue_vc_channel = parse_cid(parts[2])?;
      let red_channel = parse_cid(parts[3])?;
      let blue_channel = parse_cid(parts[4])?;
      let dashboard_msg_id = u64::from_str_radix(parts[5], 16)?;

      let guild_name = guild_name(ctx, guild_id);

      // Find and remove duplicate category
      let existing_categories = db.categories.get_categories_for_guild(guild_id).await?;
      let duplicate_category = existing_categories.iter().find(|g| {
        g.channels.dashboard == dashboard_channel
          || g.channels.queue_chat == queue_channel
          || g.channels.queue_vc == queue_vc_channel
          || g.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
      });

      if let Some(dup_category) = duplicate_category {
        let dup_category_id = dup_category.id;

        // Delete duplicate from database
        if let Err(e) = db.categories.remove_category(guild_id, dup_category_id).await {
          warn!("[{}] Failed to delete duplicate category {}: {}", guild_name, dup_category_id, e);
          send_component_error_response(interaction, ctx, &format!("Failed to remove duplicate category: {e}")).await;
          return Ok(());
        }

        // Remove from in-memory server
        let mut manager_lock = manager.lock().await;
        if let Ok(server) = manager_lock.get_qguild(guild_id) {
          server.categories.retain(|g| g.id != dup_category_id);
        }
        drop(manager_lock);

        info!("[{}] Removed duplicate category {} before linking", guild_name, dup_category_id);
      }

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      // Create new category with existing message ID
      let category_config = crate::db::repo::category::CategoryConfig {
        channel_category_id: category_id.get(),
        dashboard_channel_id: dashboard_channel.get(),
        chat_channel_id: queue_channel.get(),
        queue_vc_id: queue_vc_channel.get(),
        ping_channel_id: 1,
        quota: crate::DEFAULT_QUOTA,
      };

      match db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
        Ok(db_category) => {
          info!("[{}] Category {} linked to existing dashboard {} (duplicate removed)", guild_name, db_category.id, dashboard_msg_id);

          // Add category to in-memory server
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_qguild(guild_id) {
            if let Err(e) = server.add_category(db_category.clone()) {
              error!("Failed to add category to server: {e}");
            }
          }
          drop(manager_lock);

          // Show success and return to category list
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

          let response = CIR::UpdateMessage(
            CIRM::new().content("Removed duplicate category and linked to existing dashboard!").embed(display.build_embed()).components(display.build_components()),
          );
          interaction.create_response(&ctx.http, response).await?;
        }
        Err(e) => {
          warn!("[{}] Failed to save linked category: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_remove_dup_new_") => {
      // Remove duplicate category and create new dashboard
      let state_str = button_id.strip_prefix("link_remove_dup_new_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() < 5 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let dashboard_channel = parse_cid(parts[0])?;
      let queue_channel = parse_cid(parts[1])?;
      let queue_vc_channel = parse_cid(parts[2])?;
      let red_channel = parse_cid(parts[3])?;
      let blue_channel = parse_cid(parts[4])?;

      let guild_name = guild_name(ctx, guild_id);

      // Find and remove duplicate category
      let existing_categories = db.categories.get_categories_for_guild(guild_id).await?;
      let duplicate_category = existing_categories.iter().find(|g| {
        g.channels.dashboard == dashboard_channel
          || g.channels.queue_chat == queue_channel
          || g.channels.queue_vc == queue_vc_channel
          || g.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
      });

      if let Some(dup_category) = duplicate_category {
        let dup_category_id = dup_category.id;

        // Delete duplicate from database
        if let Err(e) = db.categories.remove_category(guild_id, dup_category_id).await {
          warn!("[{}] Failed to delete duplicate category {}: {}", guild_name, dup_category_id, e);
          send_component_error_response(interaction, ctx, &format!("Failed to remove duplicate category: {e}")).await;
          return Ok(());
        }

        // Remove from in-memory server
        let mut manager_lock = manager.lock().await;
        if let Ok(server) = manager_lock.get_qguild(guild_id) {
          server.categories.retain(|g| g.id != dup_category_id);
        }
        drop(manager_lock);

        info!("[{}] Removed duplicate category {} before creating new dashboard", guild_name, dup_category_id);
      }

      // Create new dashboard
      use crate::models::{Category, Channels, TeamChannel};

      // Derive category from dashboard channel's parent
      let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

      let mut temp_category = Category::new(
        guild_id,
        None,
        0,
        None,
        crate::DEFAULT_QUOTA,
        crate::DEFAULT_CONFIRM_TIME,
        MI::new(1),
        Channels {
          category: category_id,
          queue_chat: queue_channel,
          queue_vc: queue_vc_channel,
          ping_channel: CI::new(1),
          teams: vec![TeamChannel { red_vc: red_channel, blu_vc: blue_channel, set_index: 1, session_id: None }],
          dashboard: dashboard_channel,
        },
        vec![],
      );

      match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
        Ok(_) => {
          let dashboard_msg_id = temp_category.dashboard_msg.get();

          let category_config = crate::db::repo::category::CategoryConfig {
            channel_category_id: category_id.get(),
            dashboard_channel_id: dashboard_channel.get(),
            chat_channel_id: queue_channel.get(),
            queue_vc_id: queue_vc_channel.get(),
            ping_channel_id: 1,
            quota: crate::DEFAULT_QUOTA,
          };

          match db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
            Ok(db_category) => {
              info!("[{}] Category {} created with new dashboard (duplicate removed)", guild_name, db_category.id);

              let mut manager_lock = manager.lock().await;
              if let Ok(server) = manager_lock.get_qguild(guild_id) {
                if let Err(e) = server.add_category(db_category.clone()) {
                  error!("Failed to add category to server: {e}");
                }
              }
              drop(manager_lock);

              let categories = db.categories.get_categories_for_guild(guild_id).await?;
              let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

              let response = CIR::UpdateMessage(
                CIRM::new().content("Removed duplicate category and created new dashboard!").embed(display.build_embed()).components(display.build_components()),
              );
              interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
              let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;
              warn!("[{}] Failed to save category: {}", guild_name, e);
              send_component_error_response(interaction, ctx, &format!("Failed to save category: {e}")).await;
            }
          }
        }
        Err(e) => {
          warn!("[{}] Failed to create dashboard: {}", guild_name, e);
          send_component_error_response(interaction, ctx, &format!("Failed to create dashboard: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("link_manual_msg_") => {
      // Prompt user to provide message ID manually
      let response = CIR::Message(
                CIRM::new().content("**Manual Message ID Input**\n\nPlease provide the dashboard message ID.\n\nYou can get this by:\n1. Right-clicking the dashboard message\n2. Selecting \"Copy message link\"\n3. The ID is the last number in the URL\n\nExample: `https://discord.com/channels/123/456/789` → Message ID is `789`\n\n*Note: This feature requires a modal input which will be implemented in a future update. For now, please use the automatic detection or create a new dashboard.*").ephemeral(true)
            );
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_remove_category" => {
      // Show category selection dropdown for removal

      let guild_name = guild_name(ctx, guild_id);
      let categories = db.categories.get_categories_for_guild(guild_id).await?;

      if categories.is_empty() {
        let response = CIR::Message(CIRM::new().content("No categories to remove.").ephemeral(true));
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }

      // Create dropdown with categories
      let options: Vec<CSMO> = categories
        .iter()
        .map(|category| {
          let name = category.name();
          CSMO::new(name.clone(), category.id.to_string()).description(format!("Category ID: {}", category.id))
        })
        .collect();

      let select_menu = CSM::new("server_settings_remove_category_select", CSMK::String { options }).placeholder("Select a category to remove");

      let embed = CE::new()
        .title(format!("{} - Remove Category", guild_name))
        .description(
          "**⚠️ Warning: This action cannot be undone!**\n\n\
                    Select a category to remove. This will:\n\
                    • Delete the category from the database\n\
                    • Remove it from the server manager\n\
                    • **NOT** delete the Discord channels\n\n\
                    You can manually delete the channels afterwards if needed.",
        )
        .color(0xFF0000);

      let components = vec![CAR::SelectMenu(select_menu), CAR::Buttons(vec![CB::new("server_settings_remove_cancel").label("Cancel").style(BS::Secondary)])];

      let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
      interaction.create_response(&ctx.http, response).await?;
    }
    "server_settings_remove_category_select" => {
      // Show confirmation prompt asking about channel deletion
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(category_id_str) = values.first() {
          if let Ok(category_id) = category_id_str.parse::<u8>() {
            let guild_name = guild_name(ctx, guild_id);

            // Get category info and channel list
            let (category_name, channel_list) = {
              let mut manager_lock = manager.lock().await;
              if let Ok(server) = manager_lock.get_qguild(guild_id) {
                if let Some(category) = server.categories.iter().find(|g| g.id == category_id) {
                  let name = category.name();
                  let mut channels = Vec::new();
                  if category.channels.category.get() > 1 {
                    channels.push(format!("• <#{}> (category)", category.channels.category.get()));
                  }
                  channels.push(format!("• <#{}> (dashboard)", category.channels.dashboard.get()));
                  channels.push(format!("• <#{}> (queue chat)", category.channels.queue_chat.get()));
                  channels.push(format!("• <#{}> (queue voice)", category.channels.queue_vc.get()));
                  for team in &category.channels.teams {
                    channels.push(format!("• <#{}> (red team)", team.red_vc.get()));
                    channels.push(format!("• <#{}> (blue team)", team.blu_vc.get()));
                  }
                  (Some(name), channels.join("\n"))
                } else {
                  (None, String::new())
                }
              } else {
                (None, String::new())
              }
            };

            let display_name = category_name.unwrap_or_else(|| format!("Category {}", category_id));

            let embed = CE::new()
              .title(format!("{} - Delete Channels?", guild_name))
              .description(format!(
                "**Removing category: {}**\n\n\
                                The following Discord channels are associated with this category:\n\n\
                                {}\n\n\
                                **Do you want to delete these Discord channels?**\n\n\
                                ⚠️ This action cannot be undone!",
                display_name, channel_list
              ))
              .color(0xFF0000);

            let components = vec![CAR::Buttons(vec![
              CB::new(format!("server_settings_remove_confirm_delete_{}", category_id)).label("Yes, delete channels").style(BS::Danger),
              CB::new(format!("server_settings_remove_confirm_keep_{}", category_id)).label("No, keep channels").style(BS::Success),
              CB::new("server_settings_remove_cancel").label("Cancel").style(BS::Secondary),
            ])];

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(components));
            interaction.create_response(&ctx.http, response).await?;
          }
        }
      }
    }
    _ if button_id.starts_with("server_settings_remove_confirm_delete_") => {
      // Confirm removal with channel deletion
      let category_id_str = button_id.strip_prefix("server_settings_remove_confirm_delete_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
        let guild_name = guild_name(ctx, guild_id);

        // Get category info and channels before deletion
        let (category_name, channels_to_delete) = {
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_qguild(guild_id) {
            if let Some(category) = server.categories.iter().find(|g| g.id == category_id) {
              let name = category.name();
              let category_channel = category.channels.category;
              let mut channels = vec![category.channels.dashboard, category.channels.queue_chat, category.channels.queue_vc];
              for team in &category.channels.teams {
                channels.push(team.red_vc);
                channels.push(team.blu_vc);
              }
              // Category last so children are deleted first
              if category_channel.get() > 1 {
                channels.push(category_channel);
              }
              (Some(name), channels)
            } else {
              (None, Vec::new())
            }
          } else {
            (None, Vec::new())
          }
        };

        // Delete from database first
        match db.categories.remove_category(guild_id, category_id).await {
          Ok(_) => {
            info!("[{}] Category {} deleted from database", guild_name, category_id);

            // Remove from in-memory server
            let mut manager_lock = manager.lock().await;
            if let Ok(server) = manager_lock.get_qguild(guild_id) {
              server.categories.retain(|g| g.id != category_id);
            }
            drop(manager_lock);

            // Delete Discord channels
            let mut deleted_count = 0;
            for channel_id in channels_to_delete {
              let channel_id: CI = channel_id;
              match channel_id.delete(&ctx.http).await {
                Ok(_) => {
                  deleted_count += 1;
                  info!("[{}] Deleted channel {}", guild_name, channel_id.get());
                }
                Err(e) => {
                  warn!("[{}] Failed to delete channel {}: {}", guild_name, channel_id.get(), e);
                }
              }
            }

            // Show success and return to category list
            let categories = db.categories.get_categories_for_guild(guild_id).await?;
            let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

            let success_msg = if let Some(name) = category_name {
              format!("Successfully removed category: {}\n🗑️ Deleted {} Discord channels", name, deleted_count)
            } else {
              format!("Successfully removed category {}\n🗑️ Deleted {} Discord channels", category_id, deleted_count)
            };

            let response = CIR::UpdateMessage(CIRM::new().content(success_msg).embed(display.build_embed()).components(display.build_components()));
            interaction.create_response(&ctx.http, response).await?;
          }
          Err(e) => {
            warn!("[{}] Failed to delete category {}: {}", guild_name, category_id, e);
            send_component_error_response(interaction, ctx, &format!("Failed to remove category: {e}")).await;
          }
        }
      }
    }
    _ if button_id.starts_with("server_settings_remove_confirm_keep_") => {
      // Confirm removal without channel deletion
      let category_id_str = button_id.strip_prefix("server_settings_remove_confirm_keep_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
        let guild_name = guild_name(ctx, guild_id);

        // Get category info before deletion
        let category_name = {
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_qguild(guild_id) {
            server.categories.iter().find(|g| g.id == category_id).map(|g| g.name())
          } else {
            None
          }
        };

        // Delete from database
        match db.categories.remove_category(guild_id, category_id).await {
          Ok(_) => {
            info!("[{}] Category {} deleted from database (channels kept)", guild_name, category_id);

            // Remove from in-memory server
            let mut manager_lock = manager.lock().await;
            if let Ok(server) = manager_lock.get_qguild(guild_id) {
              server.categories.retain(|g| g.id != category_id);
            }
            drop(manager_lock);

            // Show success and return to category list
            let categories = db.categories.get_categories_for_guild(guild_id).await?;
            let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

            let success_msg = if let Some(name) = category_name {
              format!("Successfully removed category: {}\nDiscord channels were kept", name)
            } else {
              format!("Successfully removed category {}\nDiscord channels were kept", category_id)
            };

            let response = CIR::UpdateMessage(CIRM::new().content(success_msg).embed(display.build_embed()).components(display.build_components()));
            interaction.create_response(&ctx.http, response).await?;
          }
          Err(e) => {
            warn!("[{}] Failed to delete category {}: {}", guild_name, category_id, e);
            send_component_error_response(interaction, ctx, &format!("Failed to remove category: {e}")).await;
          }
        }
      }
    }
    "server_settings_remove_cancel" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    "server_settings_category_select" => {
      // Handle category selection from dropdown - show settings screen with buttons
      if let CIDK::StringSelect { values } = &interaction.data.kind {
        if let Some(value_str) = values.first() {
          // Parse format "categoryid_queueid" to handle duplicate category_id values
          let parts: Vec<&str> = value_str.split('_').collect();
          if parts.len() >= 2 {
            if let Ok(category_id) = parts[0].parse::<u8>() {
              // Find the category
              let categories = db.categories.get_categories_for_guild(guild_id).await?;
              if let Some(category) = categories.iter().find(|g| g.id == category_id) {
                // Show category settings screen with buttons including Formats
                let settings = CategorySettings::from_category(category);

                let embed = build_category_settings_embed(&settings);
                let buttons = build_category_settings_buttons(settings.category_id);

                let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
                interaction.create_response(&ctx.http, response).await?;
              }
            }
          }
        }
      }
    }
    "server_settings_category_back" => {
      send_nav!(interaction, ctx, db, nav_category_list, guild_id)?;
    }
    _ if button_id.starts_with("category_settings_link_message_") => {
      // Handle link message button - search for existing dashboard messages for this specific category
      let category_id_str = button_id.strip_prefix("category_settings_link_message_").unwrap();
      if let Ok(category_id) = category_id_str.parse::<u8>() {
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
                            Found bot messages in <#{}> that appear to be dashboards.\n\
                            Most recent: <https://discord.com/channels/{}/{}/{}>\n\n\
                            **Select an option:**\n\
                            • Link to existing dashboard (will update category's dashboard_msg)\n\
                            • Cancel",
              existing_dashboard_msgs.len(),
              dashboard_channel.get(),
              guild_id.get(),
              dashboard_channel.get(),
              existing_dashboard_msgs[0].0.get()
            ));

            // Encode state: category_id + message_id
            let state = format!("{}_{:x}", category_id, existing_dashboard_msgs[0].0.get());

            buttons.push(CB::new(format!("category_link_msg_confirm_{}", state)).label("Link to this message").style(BS::Success));
          } else {
            description.push_str(&format!(
              "ℹ️ **No existing dashboard messages found**\n\n\
                            Searched recent messages in <#{}> but didn't find any existing dashboards.\n\n\
                            The bot will continue using the current dashboard message.",
              dashboard_channel.get()
            ));
          }

          buttons.push(CB::new(format!("category_settings_back_{}", category_id)).label("Back").style(BS::Secondary));

          let embed = CE::new().title(format!("{} - Link Dashboard Message", category.name())).description(description).color(0x5865F2);

          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::Buttons(buttons)]));
          interaction.create_response(&ctx.http, response).await?;
        } else {
          warn!("Category {category_id} not found for guild {guild_id}");
        }
      }
    }
    _ if button_id.starts_with("category_link_msg_confirm_") => {
      // Confirm linking message to category
      let state_str = button_id.strip_prefix("category_link_msg_confirm_").unwrap();
      let parts: Vec<&str> = state_str.split('_').collect();

      if parts.len() != 2 {
        send_component_error_response(interaction, ctx, "Invalid state data").await;
        return Ok(());
      }

      let category_id = parts[0].parse::<u8>()?;
      let dashboard_msg_id = parse_mid(parts[1])?;

      let guild_name = guild_name(ctx, guild_id);

      // Update the category's dashboard_msg in database
      match db.categories.update_dashboard_msg_by_category_id(guild_id, category_id, dashboard_msg_id).await {
        Ok(_) => {
          info!("[{}] Updated category {} dashboard message to {}", guild_name, category_id, dashboard_msg_id);

          // Update in-memory category
          let mut manager_lock = manager.lock().await;
          if let Ok(server) = manager_lock.get_qguild(guild_id) {
            if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
              category.dashboard_msg = MI::new(dashboard_msg_id);
            }
          }
          drop(manager_lock);

          // Return to category settings
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          if let Some(category) = categories.iter().find(|g| g.id == category_id) {
            let settings = CategorySettings::from_category(category);

            let embed = build_category_settings_embed(&settings);
            let buttons = build_category_settings_buttons(settings.category_id);

            let response = CIR::UpdateMessage(CIRM::new().content("Successfully linked dashboard message!").embed(embed).components(buttons));
            interaction.create_response(&ctx.http, response).await?;
          }
        }
        Err(e) => {
          warn!("[{}] Failed to update dashboard message for category {}: {}", guild_name, category_id, e);
          send_component_error_response(interaction, ctx, &format!("Failed to link message: {e}")).await;
        }
      }
    }
    _ if button_id.starts_with("category_settings_back_") => {
      // Return to category settings from link message screen
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
    }
    _ if button_id.starts_with("server_settings_category_select_") => {
      // Handle category selection from button - show settings screen with Link Message button
      let value_str = button_id.strip_prefix("server_settings_category_select_").unwrap();

      // Parse format "categoryid_queueid" to handle duplicate category_id values
      let parts: Vec<&str> = value_str.split('_').collect();
      if parts.len() == 2 {
        if let (Ok(category_id), Ok(queue_id)) = (parts[0].parse::<u8>(), parts[1].parse::<u64>()) {
          // Find the category by both category_id and queue channel ID
          let categories = db.categories.get_categories_for_guild(guild_id).await?;
          if let Some(category) = categories.iter().find(|g| g.id == category_id && g.channels.queue_vc.get() == queue_id) {
            // Show category settings screen with buttons including Link Message
            let settings = CategorySettings::from_category(category);

            let embed = build_category_settings_embed(&settings);
            let buttons = build_category_settings_buttons(settings.category_id);

            let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
            interaction.create_response(&ctx.http, response).await?;
          } else {
            warn!("Category {} not found for guild {}", category_id, guild_id);
          }
        } else {
          warn!("Invalid category ID format in button: {}", value_str);
        }
      } else {
        warn!("Invalid category ID format in button: {}", value_str);
      }
    }
    _ => {
      warn!("Unknown server settings button: {}", button_id);
    }
  }

  Ok(())
}

/// Handle server settings modal submissions
pub async fn handle_server_settings_modal(
  ctx: &Context,
  interaction: &ModalInteraction,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");
  let modal_id = &interaction.data.custom_id;

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("{} submitted modal {}", user_tag, modal_id);

  if modal_id == "server_settings_rank_modal_add" {
    // Handle add new rank modal
    let mut name_value = String::new();
    let mut elo_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          match input.custom_id.as_str() {
            "name" => name_value = input.value.clone().unwrap_or_default(),
            "elo" => elo_value = input.value.clone().unwrap_or_default(),
            _ => {}
          }
        }
      }
    }

    let name = name_value.trim();
    if name.is_empty() {
      send_modal_error_response(interaction, ctx, "Rank name cannot be empty.").await;
      return Ok(());
    }

    let elo: u16 = match elo_value.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Check if rank name already exists
    if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, name).await {
      send_modal_error_response(interaction, ctx, "A rank with this name already exists. Please choose a different name.").await;
      return Ok(());
    }

    // Create a new Discord role for this rank
    let guild_name = guild_name(ctx, guild_id);
    let role_name = name.to_string();

    let role_id = match guild_id
      .create_role(&ctx.http, ER::new().name(&role_name).colour(Color::from_rgb(128, 128, 128)).hoist(false).mentionable(true).permissions(Permissions::empty()))
      .await
    {
      Ok(role) => {
        info!("[{}] Created new role {} for rank {}", guild_name, role.name, name);
        role.id
      }
      Err(e) => {
        warn!("[{}] Failed to create role for rank {}: {}", guild_name, name, e);
        send_modal_error_response(interaction, ctx, "Failed to create Discord role. Please check bot permissions.").await;
        return Ok(());
      }
    };

    // Add rank to DB with the created role ID
    db.ranks.add_rank(guild_id, name, elo, role_id).await?;
    let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
    info!("{} added rank '{}' with ELO {} and role {}", user_tag, name, elo, role_id.get());

    send_nav_modal!(interaction, ctx, db, nav_rank_config, guild_id)?;
  } else if modal_id.starts_with("server_settings_rank_modal_link_") {
    // Handle link existing rank modal
    let role_id_str = modal_id.strip_prefix("server_settings_rank_modal_link_").unwrap();
    let role_id = match role_id_str.parse::<u64>() {
      Ok(id) => RoleId::new(id),
      Err(_) => {
        send_modal_error_response(interaction, ctx, "Invalid role ID.").await;
        return Ok(());
      }
    };

    let mut name_value = String::new();
    let mut elo_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          match input.custom_id.as_str() {
            "name" => name_value = input.value.clone().unwrap_or_default(),
            "elo" => elo_value = input.value.clone().unwrap_or_default(),
            _ => {}
          }
        }
      }
    }

    let name = name_value.trim();
    if name.is_empty() {
      send_modal_error_response(interaction, ctx, "Rank name cannot be empty.").await;
      return Ok(());
    }

    let elo: u16 = match elo_value.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Check if rank name already exists
    if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, name).await {
      send_modal_error_response(interaction, ctx, "A rank with this name already exists. Please choose a different name.").await;
      return Ok(());
    }

    // Add rank to DB with the selected role ID
    db.ranks.add_rank(guild_id, name, elo, role_id).await?;
    let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
    info!("{} linked rank '{}' with ELO {} to role {}", user_tag, name, elo, role_id.get());

    send_nav_modal!(interaction, ctx, db, nav_rank_config, guild_id)?;
  } else if modal_id.starts_with("server_settings_rank_modal_") {
    // Handle rank name/ELO edit modal
    let old_rank_name = modal_id.strip_prefix("server_settings_rank_modal_").ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

    let mut name_value = String::new();
    let mut elo_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          match input.custom_id.as_str() {
            "name" => name_value = input.value.clone().unwrap_or_default(),
            "elo" => elo_value = input.value.clone().unwrap_or_default(),
            _ => {}
          }
        }
      }
    }

    let new_name = name_value.trim();
    if new_name.is_empty() {
      send_modal_error_response(interaction, ctx, "Rank name cannot be empty.").await;
      return Ok(());
    }

    let elo: u16 = match elo_value.trim().parse() {
      Ok(e) => e,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid ELO. Must be a valid number.").await;
        return Ok(());
      }
    };

    // Check if new rank name already exists (and it's not the same rank being renamed)
    if new_name != old_rank_name {
      if let Ok(Some(_)) = db.ranks.get_rank_by_name(guild_id, new_name).await {
        send_modal_error_response(interaction, ctx, "A rank with this name already exists. Please choose a different name.").await;
        return Ok(());
      }
    }

    // Update rank in DB using name instead of position
    db.ranks.update_rank_name(guild_id, old_rank_name, new_name).await?;
    db.ranks.update_rank_elo(guild_id, new_name, elo).await?;

    send_nav_modal!(interaction, ctx, db, nav_rank_config, guild_id)?;
  } else if modal_id.starts_with("server_settings_category_modal_") {
    // Handle category settings modal submission
    let category_id: u8 =
      modal_id.strip_prefix("server_settings_category_modal_").and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("Invalid modal ID format: {}", modal_id))?;

    // Extract all values from the modal
    let mut name_value = String::new();
    let mut quota_value = String::new();
    let mut confirm_time_value = String::new();
    let mut connect_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          match input.custom_id.as_str() {
            "name" => name_value = input.value.clone().unwrap_or_default(),
            "quota" => quota_value = input.value.clone().unwrap_or_default(),
            "confirm_time" => confirm_time_value = input.value.clone().unwrap_or_default(),
            "connect" => connect_value = input.value.clone().unwrap_or_default(),
            _ => {}
          }
        }
      }
    }

    // Parse and validate quota
    let quota: u8 = match quota_value.trim().parse() {
      Ok(q) if (2..=100).contains(&q) => q,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid quota. Must be between 2 and 100.").await;
        return Ok(());
      }
    };

    // Parse and validate timeout
    let confirm_time: u16 = match confirm_time_value.trim().parse() {
      Ok(t) if t > 0 => t,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid time. Must be a positive number.").await;
        return Ok(());
      }
    };

    let name = if name_value.trim().is_empty() { None } else { Some(name_value.trim().to_string()) };
    let connect_info = if connect_value.trim().is_empty() { None } else { Some(connect_value.trim().to_string()) };

    // Update in database
    db.categories.update_name(guild_id, category_id, name.as_deref()).await?;
    db.categories.update_quota(guild_id, category_id, quota).await?;
    db.categories.update_confirm_time(guild_id, category_id, confirm_time).await?;
    if connect_info.is_some() || connect_value.trim().is_empty() {
      db.categories.update_connect_info(guild_id, category_id, connect_info.as_deref()).await?;
    }

    // Update in-memory category and show full settings screen
    {
      let mut manager_lock = manager.lock().await;
      if let Ok(server) = manager_lock.get_qguild(guild_id) {
        if let Some(category) = server.categories.iter_mut().find(|g| g.id == category_id) {
          category.name = name.clone();
          category.confirm_time = confirm_time;
          category.set_quota(quota);
          category.set_connect_info(connect_info.clone());

          // Update dashboard to reflect quota change
          category.queue_dash_update(ctx, guild_id).await;

          // Show full category settings screen with all buttons
          let settings = CategorySettings::from_category(category);
          let embed = build_category_settings_embed(&settings);
          let buttons = build_category_settings_buttons(settings.category_id);

          let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
          interaction.create_response(&ctx.http, response).await?;
          return Ok(());
        }
      }
    }

    // Fallback if category not found
    send_modal_error_response(interaction, ctx, "Category not found").await;
  } else if modal_id == "server_settings_modal_create_category" {
    // Extract modal fields
    let mut category_name = String::new();
    let mut channel_prefix = String::new();
    let mut guild_category_name = String::new();
    let mut quota_str = String::new();
    let mut bot_only_dashboard_str = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          match input.custom_id.as_str() {
            "category_name" => category_name = input.value.clone().unwrap_or_default(),
            "channel_prefix" => channel_prefix = input.value.clone().unwrap_or_default(),
            "discord_category" => guild_category_name = input.value.clone().unwrap_or_default(),
            "quota" => quota_str = input.value.clone().unwrap_or_default(),
            "bot_only_dashboard" => bot_only_dashboard_str = input.value.clone().unwrap_or_default(),
            _ => {}
          }
        }
      }
    }

    let category_name = category_name.trim().to_string();
    let channel_prefix = channel_prefix.trim().to_lowercase().replace(' ', "-");
    let guild_category_name = guild_category_name.trim().to_string();
    let bot_only_dashboard = bot_only_dashboard_str.trim().to_lowercase();

    if category_name.is_empty() || channel_prefix.is_empty() || guild_category_name.is_empty() {
      send_modal_error_response(interaction, ctx, "Category name, channel prefix, and category name cannot be empty.").await;
      return Ok(());
    }

    if !["yes", "no"].contains(&bot_only_dashboard.as_str()) {
      send_modal_error_response(interaction, ctx, "Bot-only dashboard must be 'yes' or 'no'.").await;
      return Ok(());
    }

    let quota: u8 = match quota_str.trim().parse() {
      Ok(q) if (2..=100).contains(&q) => q,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid quota. Must be between 2 and 100.").await;
        return Ok(());
      }
    };

    // Defer the response so we have time to create channels
    interaction.create_response(&ctx.http, CIR::Defer(CIRM::new().ephemeral(true))).await?;

    let guild_name = guild_name(ctx, guild_id);

    // Create channels (runner_role will be None for manual category creation)
    match crate::handlers::admin::create_category_channels(ctx, guild_id, &guild_category_name, &channel_prefix, bot_only_dashboard.as_str() == "yes", None).await {
      Ok((category_id, dashboard_channel, queue_channel, queue_vc_channel, ping_channel)) => {
        use crate::models::{Category, Channels};

        let mut temp_category = Category::new(
          guild_id,
          Some(category_name.clone()),
          0,
          Some(category_name.clone()),
          quota,
          crate::DEFAULT_CONFIRM_TIME,
          MI::new(1),
          Channels { category: category_id, queue_chat: queue_channel, queue_vc: queue_vc_channel, ping_channel, teams: vec![], dashboard: dashboard_channel },
          vec![],
        );

        // Publish the dashboard
        match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
          Ok(_) => {
            let dashboard_msg_id = temp_category.dashboard_msg.get();
            info!("[{}] Dashboard message created with ID {}", guild_name, dashboard_msg_id);

            let category_config = crate::db::repo::category::CategoryConfig {
              channel_category_id: category_id.get(),
              dashboard_channel_id: dashboard_channel.get(),
              chat_channel_id: queue_channel.get(),
              queue_vc_id: queue_vc_channel.get(),
              ping_channel_id: ping_channel.get(),
              quota,
            };
            match db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await {
              Ok(db_category) => {
                info!("[{}] Category {} saved to database", guild_name, db_category.id);

                // Update category name in DB
                let _ = db.categories.update_name(guild_id, db_category.id, Some(&category_name)).await;

                // Add category to in-memory server
                let mut manager_lock = manager.lock().await;
                if let Ok(server) = manager_lock.get_qguild(guild_id) {
                  let mut category = db_category.clone();
                  category.name = Some(category_name.clone());
                  if let Err(e) = server.add_category(category) {
                    error!("Failed to add category to server: {e}");
                  }
                }
                drop(manager_lock);

                // Follow up with success
                let categories = db.categories.get_categories_for_guild(guild_id).await?;
                let display = CategoryListDisplay { guild_name: guild_name.clone(), categories };

                let followup = CIRF::new().embed(display.build_embed()).components(display.build_components()).ephemeral(true);
                interaction.create_followup(&ctx.http, followup).await?;
              }
              Err(e) => {
                let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;
                let _ = dashboard_channel.delete(&ctx.http).await;
                let _ = queue_channel.delete(&ctx.http).await;
                let _ = queue_vc_channel.delete(&ctx.http).await;
                let _ = category_id.delete(&ctx.http).await;

                warn!("[{}] Failed to save category to database: {}", guild_name, e);
                let followup = CIRF::new().content(format!("Failed to save category: {e}")).ephemeral(true);
                interaction.create_followup(&ctx.http, followup).await?;
              }
            }
          }
          Err(e) => {
            let _ = dashboard_channel.delete(&ctx.http).await;
            let _ = queue_channel.delete(&ctx.http).await;
            let _ = queue_vc_channel.delete(&ctx.http).await;
            let _ = category_id.delete(&ctx.http).await;

            warn!("[{}] Failed to create dashboard: {}", guild_name, e);
            let followup = CIRF::new().content(format!("Failed to create dashboard: {e}")).ephemeral(true);
            interaction.create_followup(&ctx.http, followup).await?;
          }
        }
      }
      Err(e) => {
        warn!("[{}] Failed to create channels: {}", guild_name, e);
        let followup = CIRF::new().content(format!("Failed to create channels: {e}")).ephemeral(true);
        interaction.create_followup(&ctx.http, followup).await?;
      }
    }
  } else if modal_id == "server_settings_post_game_confirm_time_modal" {
    // Handle post-game timeout modal
    let mut post_game_confirm_time_value = String::new();

    for row in &interaction.data.components {
      for component in &row.components {
        if let ARC::InputText(input) = component {
          if input.custom_id == "post_game_confirm_time_input" {
            post_game_confirm_time_value = input.value.clone().unwrap_or_default();
          }
        }
      }
    }

    // Parse and validate timeout
    let post_game_confirm_time: u16 = match post_game_confirm_time_value.trim().parse() {
      Ok(t) if (30..=300).contains(&t) => t,
      _ => {
        send_modal_error_response(interaction, ctx, "Invalid time. Must be between 30 and 300 seconds.").await;
        return Ok(());
      }
    };

    // Update database
    db.config.set_post_game_confirm_time(guild_id, post_game_confirm_time).await?;
    let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
    info!("{} set post-game confirm time to {} seconds", user_tag, post_game_confirm_time);

    send_nav_modal!(interaction, ctx, db, nav_role_config, guild_id)?;
  } else {
    warn!("Unknown server settings modal: {}", modal_id);
  }

  Ok(())
}

/// Handle server-level team balance method selection
pub async fn handle_server_settings_balance_select(
  ctx: &Context,
  interaction: &CoI,
  db: &Arc<Database>,
  manager: &Arc<tokio::sync::Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = interaction.guild_id.expect("Guild ID not found");

  let user_tag = crate::log::get_user_tag(ctx, interaction.user.id, db).await;
  info!("[Server Settings] {} selected team balance method", user_tag);

  // Extract selected value
  let method_str = match &interaction.data.kind {
    CIDK::StringSelect { values } => values.first().ok_or_else(|| anyhow::anyhow!("No value selected"))?.clone(),
    _ => return Err(anyhow::anyhow!("Expected string select interaction")),
  };

  let method = crate::models::TeamBalanceMethod::parse(&method_str);

  // Update all categories in-memory and persist to database
  let mut manager_lock = manager.lock().await;
  {
    let server = manager_lock.get_qguild(guild_id)?;
    for category in server.categories.iter_mut() {
      category.team_balance_method = method;
      if let Err(e) = db.categories.update_team_balance_method(guild_id, category.id, method).await {
        warn!("Failed to persist team_balance_method for category {}: {e}", category.id);
      }
    }
  }
  drop(manager_lock);

  // Return to server settings
  let settings = get_server_settings(db, guild_id).await?;
  let guild_name = guild_name(ctx, guild_id);
  let embed = build_server_settings_embed(&settings, &guild_name);
  let buttons = build_server_settings_buttons(&settings, &guild_name);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(buttons));
  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Get server settings from database
pub async fn get_server_settings(db: &Arc<Database>, guild_id: GI) -> Result<ServerSettings> {
  use SERVER_CONFIG_TOGGLES;

  let config_map = db.config.get_config_map(guild_id).await?;
  let runner_role = config_map.get("runner_id").cloned();
  let admin_role = config_map.get("admin_id").cloned();
  let balance_method = config_map.get("balance_method").cloned().unwrap_or_else(|| "bch".to_string());

  let mut toggle_states = Vec::with_capacity(SERVER_CONFIG_TOGGLES.len());
  for toggle in SERVER_CONFIG_TOGGLES {
    toggle_states.push(db.config.get_bool(guild_id, toggle.column, toggle.default).await?);
  }

  let post_game_confirm_time = db.config.get_post_game_confirm_time(guild_id).await.unwrap_or(120);

  Ok(ServerSettings { runner_role, admin_role, toggle_states, balance_method, post_game_confirm_time })
}