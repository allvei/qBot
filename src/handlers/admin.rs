use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::{anyhow, Result};
use serenity::all::{
  ChannelId as CI, ChannelType, CommandDataOption, ComponentInteraction as CX, ComponentInteractionDataKind as CXD, Context, CreateActionRow as CAR, CreateEmbed as CE,
  CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, CreateMessage, CreateSelectMenu as CSM, CreateSelectMenuKind as CSMK, CreateSelectMenuOption as CSMO,
  GuildId as GI, PartialGuild as PG, RoleId as RI, User, UserId as UI,
};
use tracing::{error, info, warn};

use crate::db::repo::Repository;
use crate::handlers::player::validate_system_roles;
use crate::models::embeds::Ephemeral;
use crate::models::{CommandContext as CC, QGuild, SETUP_STATE};
use crate::player::{is_admin, is_runner};
use crate::{guild_name, Database, Manager, CYAN, DEFAULT_QUOTA, GREEN, ORANGE, RED};

/// `/config`
///
/// * `key`   - The key to modify.
/// * `value` - The value to set for the key.
pub async fn cmd_config(cc: &CC<'_>, key: String, value: Option<String>) -> Result<()> {
  if !is_admin(cc).await? {
    return Ok(());
  }

  if let Some(val) = value {
    cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found")).await?;
    let embed = CE::new().title("Config updated").description(format!("Set `{key}` = `{val}`"));
    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
  } else {
    let config = match cc.db.get_config(cc.intax.guild_id.expect("Guild ID not found")).await {
      Ok(cfg) => cfg,
      Err(e) => {
        let err_embed = CE::new().title("Failed to load config").description(format!("Error: {e}\nPlease create a config using `/config`."));
        cc.intax.create_response(&cc.ctx.http, Ephemeral::send(err_embed)).await?;
        return Ok(());
      }
    };

    let config_text = format!(
      "**Current Configuration:**\n\
                                     guild: `{}`\n\
                                     roles: `{}`\n\
                                     categories: `{}`",
      config.id, config.roles.runner, config.roles.admin
    );
    let embed = CE::new().title("Bot configuration").description(config_text);
    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
  }

  Ok(())
}

/// `/roles`
///
/// Manage runner and admin roles for the guild
/// * `role_type` - The role type to manage ("runner" or "admin")
/// * `role` - The Discord role mention/ID to assign
pub async fn cmd_roles(cc: &CC<'_>, role_type: String, role: Option<String>) -> Result<()> {
  // Check admin permissions
  if !is_admin(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");

  // If no parameters, show current role configuration
  if role_type.is_empty() && role.is_none() {
    let runner_role = cc.db.config.get_runner_role_id(guild_id).await?;
    let admin_role = cc.db.config.get_admin_role_id(guild_id).await?;

    let role_text = format!(
      "**Current Role Configuration:**\n\
             Runner Role: {}\n\
             Admin Role: {}",
      runner_role.map(|r| format!("<@&{}>", r.get())).unwrap_or_else(|| "Not set".to_string()),
      admin_role.map(|r| format!("<@&{}>", r.get())).unwrap_or_else(|| "Not set".to_string())
    );

    let embed = CE::new().title("Role configuration").description(role_text);

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
    return Ok(());
  }

  // Validate role type
  let is_runner = match role_type.to_lowercase().as_str() {
    "runner" => true,
    "admin" => false,
    "" => {
      cc.reply_ephemeral("Please specify role type: `runner` or `admin`").await?;
      return Ok(());
    }
    _ => {
      cc.reply_ephemeral("Invalid role type. Use `runner` or `admin`").await?;
      return Ok(());
    }
  };

  // If role is provided, set it
  if let Some(role_value) = role {
    // Parse role ID from mention format <@&123456> or raw ID
    let role_id_str = if role_value.starts_with("<@&") && role_value.ends_with('>') { role_value[3..role_value.len() - 1].to_string() } else { role_value };

    let role_id = serenity::all::RoleId::new(role_id_str.parse::<u64>()?);

    // Save to database
    if is_runner {
      cc.db.config.set_runner_role_id(guild_id, role_id).await?;
    } else {
      cc.db.config.set_admin_role_id(guild_id, role_id).await?;
    }

    let embed = CE::new().title("Role updated").description(format!("Set {} role to <@&{}>", role_type.to_lowercase(), role_id.get())).color(GREEN);

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
  } else {
    // Show current value for this role type
    let current_role = if is_runner { cc.db.config.get_runner_role_id(guild_id).await? } else { cc.db.config.get_admin_role_id(guild_id).await? };

    let embed = CE::new().title(format!("{role_type} Role")).description(format!(
      "Current {} role: {}",
      role_type.to_lowercase(),
      current_role.map(|r| format!("<@&{}>", r.get())).unwrap_or_else(|| "Not set".to_string())
    ));

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
  }

  Ok(())
}

/// Creates a category and all necessary category channels
/// Flow: Create category -> Create dashboard -> Test message send -> Create other channels
/// If dashboard message send fails, cleanup and abort
/// Returns: (category_id, dashboard_id, queue_chat_id, queue_vc_id, ping_channel_id)
pub async fn create_category_channels(ctx: &Context, guild_id: GI, category_name: &str, channel_prefix: &str, bot_only_dashboard: bool, runner_role: Option<RI>) -> Result<(CI, CI, CI, CI, CI)> {
  use serenity::all::{CreateChannel, CreateEmbed, CreateMessage, PermissionOverwrite, PermissionOverwriteType, Permissions};

  let guild = guild_id.to_partial_guild(&ctx.http).await?;
  let guild_name = guild_name(ctx, guild_id);

  // Get bot's user ID and find bot's integration role
  let bot_user_id = ctx.cache.current_user().id;
  let bot_role = guild.roles.values().find(|r| r.managed && r.tags.bot_id == Some(bot_user_id)).map(|r| r.id);

  // Pre-flight: check bot permissions
  let bot_member = guild_id.member(&ctx.http, bot_user_id).await.map_err(|e| anyhow!("Failed to fetch bot member: {e}"))?;
  let bot_perms = guild.member_permissions(&bot_member);
  info!("[{}] Bot permissions: {:?}", guild_name, bot_perms);
  let required = [
    (Permissions::MANAGE_CHANNELS, "Manage Channels"),
    (Permissions::MANAGE_ROLES, "Manage Roles"),
    (Permissions::SEND_MESSAGES, "Send Messages"),
    (Permissions::EMBED_LINKS, "Embed Links"),
    (Permissions::VIEW_CHANNEL, "View Channels"),
    (Permissions::CONNECT, "Connect"),
    (Permissions::MOVE_MEMBERS, "Move Members"),
  ];
  let missing: Vec<&str> = required.iter().filter(|(perm, _)| !bot_perms.contains(*perm)).map(|(_, name)| *name).collect();
  if !missing.is_empty() {
    let list = missing.join(", ");
    error!("[{}] Bot missing permissions: {}", guild_name, list);
    return Err(anyhow!("Bot is missing permissions: {list}"));
  }

  // Bot permissions to set on the category and channels so the bot retains
  // access even after ELO gate or other permission overwrites are applied.
  let bot_channel_perms = Permissions::VIEW_CHANNEL
    | Permissions::SEND_MESSAGES
    | Permissions::EMBED_LINKS
    | Permissions::CONNECT
    | Permissions::MOVE_MEMBERS
    | Permissions::MANAGE_CHANNELS
    | Permissions::MANAGE_ROLES;

  // Step 1: Create category with bot permission overwrites
  let mut cat_permissions = vec![PermissionOverwrite { allow: bot_channel_perms, deny: Permissions::empty(), kind: PermissionOverwriteType::Member(bot_user_id) }];
  if let Some(role_id) = bot_role {
    cat_permissions.push(PermissionOverwrite { allow: bot_channel_perms, deny: Permissions::empty(), kind: PermissionOverwriteType::Role(role_id) });
  }

  let category = match guild_id.create_channel(&ctx.http, CreateChannel::new(category_name).kind(ChannelType::Category).permissions(cat_permissions)).await {
    Ok(cat) => cat,
    Err(e) => {
      error!("[{}] Failed to create category: {}", guild_name, e);
      return Err(anyhow!("Failed to create category: {e}"));
    }
  };

  let category_id = category.id;

  // Step 2: Create dashboard text channel with proper permissions
  let mut permissions = vec![
    // Allow bot user explicitly
    PermissionOverwrite { allow: bot_channel_perms, deny: Permissions::empty(), kind: PermissionOverwriteType::Member(bot_user_id) },
  ];

  // Add bot's integration role if found
  if let Some(role_id) = bot_role {
    permissions.push(PermissionOverwrite { allow: bot_channel_perms, deny: Permissions::empty(), kind: PermissionOverwriteType::Role(role_id) });
  }

  // If bot-only dashboard is enabled, deny @everyone from sending messages
  if bot_only_dashboard {
    // Only deny permissions the bot itself has (Discord requires this)
    let mut everyone_deny = Permissions::SEND_MESSAGES | Permissions::ADD_REACTIONS;
    if let Ok(member) = guild_id.member(&ctx.http, bot_user_id).await {
      let bot_perms = guild.member_permissions(&member);
      if bot_perms.contains(Permissions::CREATE_PUBLIC_THREADS) {
        everyone_deny |= Permissions::CREATE_PUBLIC_THREADS;
      }
      if bot_perms.contains(Permissions::CREATE_PRIVATE_THREADS) {
        everyone_deny |= Permissions::CREATE_PRIVATE_THREADS;
      }
    }

    permissions.push(PermissionOverwrite { allow: Permissions::empty(), deny: everyone_deny, kind: PermissionOverwriteType::Role(guild_id.everyone_role()) });
  }

  let dashboard_channel = match guild_id
    .create_channel(
      &ctx.http,
      CreateChannel::new(format!("{channel_prefix}-dashboard"))
        .kind(ChannelType::Text)
        .category(category_id)
        .topic("PUG queue dashboard - use buttons to join/leave")
        .permissions(permissions),
    )
    .await
  {
    Ok(ch) => ch,
    Err(e) => {
      // Diagnostic: log bot permissions and attempted overwrites
      if let Ok(member) = guild_id.member(&ctx.http, bot_user_id).await {
        let bot_perms = guild.member_permissions(&member);
        let deny_info = if bot_only_dashboard { "SEND_MESSAGES (and threads if bot has those perms)" } else { "none" };
        error!(
          "[{}] Failed to create dashboard channel: {} | Bot perms: {:?} | Bot-only dashboard: {} | Deny overwrites: {}",
          guild_name, e, bot_perms, bot_only_dashboard, deny_info
        );
      } else {
        error!("[{}] Failed to create dashboard channel: {}", guild_name, e);
      }
      // Clean up category
      let _ = category_id.delete(&ctx.http).await;
      return Err(anyhow!("Failed to create dashboard channel: {e}"));
    }
  };

  // Step 3: Test dashboard message send - CRITICAL STEP
  let test_embed = CreateEmbed::new().title("PUG Dashboard").description("Setting up queue system...").color(ORANGE);

  let test_msg = dashboard_channel.id.send_message(&ctx.http, CreateMessage::new().embed(test_embed)).await;

  if let Err(e) = test_msg {
    error!("[{}] Failed to send dashboard message: {}", guild_name, e);
    // Clean up dashboard channel and category
    info!("[{}] Cleaning up dashboard channel and category", guild_name);
    let _ = dashboard_channel.id.delete(&ctx.http).await;
    let _ = category_id.delete(&ctx.http).await;
    return Err(anyhow!("Failed to send dashboard message (bot may lack permissions): {e}"));
  }

  // Delete the test message - we'll create the real one later
  if let Ok(msg) = test_msg {
    let _ = dashboard_channel.id.delete_message(&ctx.http, msg.id).await;
  }

  info!("[{}] Dashboard channel verified, creating remaining channels", guild_name);

  // Step 4: Create remaining channels (only if dashboard works)
  let queue_channel = match guild_id
    .create_channel(&ctx.http, CreateChannel::new(format!("{channel_prefix}-chat")).kind(ChannelType::Text).category(category_id).topic("Queue discussion and commands"))
    .await
  {
    Ok(ch) => ch,
    Err(e) => {
      error!("[{}] Failed to create queue text channel: {}", guild_name, e);
      let _ = dashboard_channel.id.delete(&ctx.http).await;
      let _ = category_id.delete(&ctx.http).await;
      return Err(anyhow!("Failed to create queue text channel: {e}"));
    }
  };

  let queue_vc_channel = match guild_id.create_channel(&ctx.http, CreateChannel::new("Queue").kind(ChannelType::Voice).category(category_id)).await {
    Ok(ch) => ch,
    Err(e) => {
      error!("[{}] Failed to create queue voice channel: {}", guild_name, e);
      let _ = queue_channel.id.delete(&ctx.http).await;
      let _ = dashboard_channel.id.delete(&ctx.http).await;
      let _ = category_id.delete(&ctx.http).await;
      return Err(anyhow!("Failed to create queue voice channel: {e}"));
    }
  };

  // Step 5: Create ping channel with Runner-only send permissions
  let mut ping_permissions = vec![
    PermissionOverwrite { allow: bot_channel_perms, deny: Permissions::empty(), kind: PermissionOverwriteType::Member(bot_user_id) },
  ];
  
  if let Some(role_id) = bot_role {
    ping_permissions.push(PermissionOverwrite { allow: bot_channel_perms, deny: Permissions::empty(), kind: PermissionOverwriteType::Role(role_id) });
  }
  
  // Deny @everyone from sending messages (only Runner role can send)
  ping_permissions.push(PermissionOverwrite {
    allow: Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY,
    deny: Permissions::SEND_MESSAGES | Permissions::ADD_REACTIONS | Permissions::CREATE_PUBLIC_THREADS | Permissions::CREATE_PRIVATE_THREADS,
    kind: PermissionOverwriteType::Role(guild_id.everyone_role()),
  });
  
  // Allow Runner role to send messages
  if let Some(runner_role_id) = runner_role {
    ping_permissions.push(PermissionOverwrite {
      allow: Permissions::SEND_MESSAGES | Permissions::MENTION_EVERYONE,
      deny: Permissions::empty(),
      kind: PermissionOverwriteType::Role(runner_role_id),
    });
  }

  let ping_channel = match guild_id
    .create_channel(
      &ctx.http,
      CreateChannel::new(format!("{channel_prefix}-ping"))
        .kind(ChannelType::Text)
        .category(category_id)
        .topic("Ping channel - Only Runners can send @here messages to encourage queue participation")
        .permissions(ping_permissions),
    )
    .await
  {
    Ok(ch) => ch,
    Err(e) => {
      error!("[{}] Failed to create ping channel: {}", guild_name, e);
      let _ = queue_vc_channel.id.delete(&ctx.http).await;
      let _ = queue_channel.id.delete(&ctx.http).await;
      let _ = dashboard_channel.id.delete(&ctx.http).await;
      let _ = category_id.delete(&ctx.http).await;
      return Err(anyhow!("Failed to create ping channel: {e}"));
    }
  };

  info!("[{}] Successfully created all category channels (team VCs are dynamic)", guild_name);

  Ok((category_id, dashboard_channel.id, queue_channel.id, queue_vc_channel.id, ping_channel.id))
}

/// `/dashboard`
///
/// Creates or updates the dashboard in the current channel
pub async fn cmd_dashboard(cc: &CC<'_>, guild: &mut QGuild) -> Result<()> {
  if !is_runner(cc).await? && !is_admin(cc).await? {
    return Ok(());
  }

  let channel = cc.intax.channel_id;
  let guild_id = cc.intax.guild_id.ok_or_else(|| anyhow!("This command must be used in a server"))?;
  let category = guild.get_category(channel)?;

  // Create and send dashboard
  category.dash_publish(cc.ctx, channel, &cc.db, guild_id).await?;

  cc.reply("Dashboard created/updated successfully!").await?;

  Ok(())
}

/// `/setup`
///
/// Sets up the bot for a guild using an interactive ephemeral message flow
pub async fn cmd_setup(cc: &CC<'_>) -> Result<()> {
  if !is_admin(cc).await? {
    return Ok(());
  }

  let guild_id: GI = cc.intax.guild_id.expect("Guild ID not found");
  let user_id: UI = cc.intax.user.id;

  // Start the setup flow with ephemeral message
  start_setup_flow(cc, guild_id, user_id).await?;

  Ok(())
}

/// Starts the interactive setup flow with ephemeral messages
async fn start_setup_flow(cc: &CC<'_>, guild_id: GI, user_id: UI) -> Result<()> {
  // Initialize setup state
  SETUP_STATE.start_setup(user_id, guild_id);

  // Get guild information
  let guild = guild_id.to_partial_guild(&cc.ctx.http).await?;

  // Send welcome message as ephemeral reply
  let welcome_embed = CE::new()
    .title("Guild setup wizard")
    .description(format!(
      "Welcome to the setup wizard for **{}**!\n\n\
            I'll guide you through configuring the bot step by step.\n\
            You'll select channels and roles using dropdown menus.\n\n\
            **Before continuing**, make sure you have created:\n\
            • A text channel for the dashboard\n\
            • A text channel for queue commands\n\
            • A voice channel for the queue\n\
            • Voice channels for Red and Blue teams\n\n\
            **Step 1/7: Dashboard Channel**\n\
            Select the channel where the queue dashboard will be displayed:",
      guild.name
    ))
    .color(GREEN);

  // Get text channels for dropdown
  let channels = get_text_channels(&guild, cc.ctx).await?;
  let channel_options = create_channel_options(&channels, "dashboard");

  let select_menu = CSM::new("setup_dashboard", CSMK::String { options: channel_options }).placeholder("Select dashboard channel...").max_values(1);

  let action_row = CAR::SelectMenu(select_menu);

  let response = CIR::Message(CIRM::new().embed(welcome_embed).components(vec![action_row]).ephemeral(true));

  cc.intax.create_response(&cc.ctx.http, response).await?;

  Ok(())
}

/// Gets text channels from a guild
async fn get_text_channels(guild: &PG, ctx: &Context) -> Result<Vec<(CI, String)>> {
  let mut channels = Vec::new();

  let guild_channels = guild.channels(&ctx.http).await?;
  for (channel_id, channel) in guild_channels {
    if channel.kind == ChannelType::Text {
      channels.push((channel_id, channel.name.clone()));
    }
  }

  // Sort by name for better UX
  channels.sort_by(|a, b| a.1.cmp(&b.1));
  Ok(channels)
}

/// Gets voice channels from a guild
async fn get_voice_channels(guild: &PG, ctx: &Context) -> Result<Vec<(CI, String)>> {
  let mut channels = Vec::new();

  let guild_channels = guild.channels(&ctx.http).await?;
  for (channel_id, channel) in guild_channels {
    if channel.kind == ChannelType::Voice {
      channels.push((channel_id, channel.name.clone()));
    }
  }

  // Sort by name for better UX
  channels.sort_by(|a, b| a.1.cmp(&b.1));
  Ok(channels)
}

/// Gets roles from a guild (excluding @everyone)
async fn get_guild_roles(guild: &PG) -> Result<Vec<(RI, String)>> {
  let mut roles = Vec::new();

  for (role_id, role) in &guild.roles {
    // Skip @everyone role
    if role.name != "@everyone" {
      roles.push((*role_id, role.name.clone()));
    }
  }

  // Sort by name for better UX
  roles.sort_by(|a, b| a.1.cmp(&b.1));
  Ok(roles)
}

/// Creates channel select options for dropdown
fn create_channel_options(channels: &[(CI, String)], prefix: &str) -> Vec<CSMO> {
  channels
    .iter()
    .take(25) // Discord limit
    .map(|(id, name)| CSMO::new(name.clone(), format!("{prefix}_{}", id.get())).description(format!("Channel ID: {}", id.get())))
    .collect()
}

/// Creates role select options for dropdown
fn create_role_options(roles: &[(RI, String)], prefix: &str) -> Vec<CSMO> {
  roles
    .iter()
    .take(25) // Discord limit
    .map(|(id, name)| CSMO::new(name.clone(), format!("{prefix}_{}", id.get())).description(format!("Role ID: {}", id.get())))
    .collect()
}

/// Handles setup interaction responses
pub async fn handle_setup_interaction(ctx: &Context, interaction: &CX, db: &Arc<Database>, manager: &Arc<Mutex<Manager>>) -> Result<()> {
  use crate::models::ButtonType;

  let button_type = ButtonType::parse(&interaction.data.custom_id);

  // Extract selected value from dropdown
  let selected_values = match &interaction.data.kind {
    CXD::StringSelect { values } => values,
    _ => return Ok(()),
  };

  if selected_values.is_empty() {
    return Ok(());
  }

  let selected_value = &selected_values[0];
  let value_parts: Vec<&str> = selected_value.split('_').collect();
  if value_parts.len() < 2 {
    return Ok(());
  }

  // The ID is always the last part after splitting (e.g., "init_queue_123" -> "123")
  let channel_or_role_id: u64 = value_parts.last().ok_or_else(|| anyhow!("No ID found in selected value"))?.parse()?;

  // Try data-driven wizard step first (handles all non-final steps)
  if let Some((step, label)) = get_wizard_step(&interaction.data.custom_id) {
    handle_wizard_step(ctx, interaction, channel_or_role_id, &step, label).await?;
    return Ok(());
  }

  // Final steps require db/manager access
  match button_type {
    ButtonType::SetupAdmin => handle_admin_selection(ctx, interaction, channel_or_role_id, db, manager).await?,
    ButtonType::InitAdmin => handle_init_admin_selection(ctx, interaction, channel_or_role_id, db, manager).await?,
    ButtonType::CategoryLinkBlue => handle_categorylink_blue_selection(ctx, interaction, channel_or_role_id, db, manager).await?,
    _ => {}
  }

  Ok(())
}

// ==================== DATA-DRIVEN WIZARD STEPS ====================

/// Which field on SetupConfig to set
enum ConfigField {
  Dashboard,
  Queue,
  QueueVc,
  Red,
  Blue,
  Runner,
  Admin,
}

impl ConfigField {
  fn set(&self, config: &mut crate::models::SetupConfig, value: u64) {
    match self {
      Self::Dashboard => config.dashboard_channel = Some(value),
      Self::Queue => config.queue_channel = Some(value),
      Self::QueueVc => config.queue_vc_channel = Some(value),
      Self::Red => config.red_channel = Some(value),
      Self::Blue => config.blue_channel = Some(value),
      Self::Runner => config.runner_role = Some(value),
      Self::Admin => config.admin_role = Some(value),
    }
  }

  fn format_mention(&self, id: u64) -> String {
    match self {
      Self::Runner | Self::Admin => format!("<@&{id}>"),
      _ => format!("<#{id}>"),
    }
  }
}

/// What kind of options to show in the next step's dropdown
enum OptionSource {
  TextChannels,
  VoiceChannels,
  Roles,
}

/// A single non-final wizard step
struct WizardStep {
  field: ConfigField,
  title: &'static str,
  next_label: &'static str,
  next_id: &'static str,
  next_source: OptionSource,
  placeholder: &'static str,
}

impl WizardStep {
  fn build_description(&self, selected_id: u64, step_label: &str) -> String {
    format!("{}: {}\n\n**{}**\n{}", self.title, self.field.format_mention(selected_id), step_label, self.next_label,)
  }
}

/// Generic handler for any non-final wizard step
async fn handle_wizard_step(ctx: &Context, interaction: &CX, selected_id: u64, step: &WizardStep, step_label: &str) -> Result<()> {
  let user_id = interaction.user.id;
  let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
  let guild = guild_id.to_partial_guild(&ctx.http).await?;

  SETUP_STATE.update_setup(user_id, guild_id, |config| {
    step.field.set(config, selected_id);
  });

  let embed = CE::new().title(step.title).description(step.build_description(selected_id, step_label)).color(GREEN);

  let options = match step.next_source {
    OptionSource::TextChannels => {
      let channels = get_text_channels(&guild, ctx).await?;
      create_channel_options(&channels, step.next_id)
    }
    OptionSource::VoiceChannels => {
      let channels = get_voice_channels(&guild, ctx).await?;
      create_channel_options(&channels, step.next_id)
    }
    OptionSource::Roles => {
      let roles = get_guild_roles(&guild).await?;
      create_role_options(&roles, step.next_id)
    }
  };

  let select_menu = CSM::new(step.next_id, CSMK::String { options }).placeholder(step.placeholder).max_values(1);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![CAR::SelectMenu(select_menu)]));
  interaction.create_response(&ctx.http, response).await?;
  Ok(())
}

fn get_wizard_step(button_id: &str) -> Option<(WizardStep, &'static str)> {
  // Returns (step definition, step progress label)
  match button_id {
    "setup_dashboard" => Some((
      WizardStep {
        field: ConfigField::Dashboard,
        title: "Dashboard channel selected",
        next_label: "Select the text channel where players will use queue commands:",
        next_id: "setup_queue",
        next_source: OptionSource::TextChannels,
        placeholder: "Select queue channel...",
      },
      "Step 2/7: Queue Text Channel",
    )),
    "setup_queue" => Some((
      WizardStep {
        field: ConfigField::Queue,
        title: "Queue text channel selected",
        next_label: "Select the voice channel where players will wait in queue:",
        next_id: "setup_queuevc",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select queue voice channel...",
      },
      "Step 3/7: Queue Voice Channel",
    )),
    "setup_queuevc" => Some((
      WizardStep {
        field: ConfigField::QueueVc,
        title: "Queue voice channel selected",
        next_label: "Select the voice channel for the Red team:",
        next_id: "setup_red",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select red team voice channel...",
      },
      "Step 4/7: Red Team Voice Channel",
    )),
    "setup_red" => Some((
      WizardStep {
        field: ConfigField::Red,
        title: "Red team channel selected",
        next_label: "Select the voice channel for the Blue team:",
        next_id: "setup_blue",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select blue team voice channel...",
      },
      "Step 5/7: Blue Team Voice Channel",
    )),
    "setup_blue" => Some((
      WizardStep {
        field: ConfigField::Blue,
        title: "Blue team channel selected",
        next_label: "Select the role that can manage PUG games:",
        next_id: "setup_runner",
        next_source: OptionSource::Roles,
        placeholder: "Select runner role...",
      },
      "Step 6/7: Runner Role",
    )),
    "setup_runner" => Some((
      WizardStep {
        field: ConfigField::Runner,
        title: "Runner role selected",
        next_label: "Select the role that can configure the bot:",
        next_id: "setup_admin",
        next_source: OptionSource::Roles,
        placeholder: "Select admin role...",
      },
      "Step 7/7: Admin Role",
    )),
    // Init flow steps
    "init_queue" => Some((
      WizardStep {
        field: ConfigField::Queue,
        title: "Queue text channel selected",
        next_label: "Select the voice channel players will join for the queue:",
        next_id: "init_queuevc",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select queue voice channel...",
      },
      "Step 3/5: Queue Voice Channel",
    )),
    "init_queuevc" => Some((
      WizardStep {
        field: ConfigField::QueueVc,
        title: "Queue voice channel selected",
        next_label: "Select the voice channel for the Red team:",
        next_id: "init_red",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select red team voice channel...",
      },
      "Step 4/5: Red Team Voice Channel",
    )),
    "init_red" => Some((
      WizardStep {
        field: ConfigField::Red,
        title: "Red team channel selected",
        next_label: "Select the voice channel for the Blue team:",
        next_id: "init_blue",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select blue team voice channel...",
      },
      "Step 5/5: Blue Team Voice Channel",
    )),
    "init_blue" => Some((
      WizardStep {
        field: ConfigField::Blue,
        title: "Blue team channel selected",
        next_label: "Select the role for bot administrators:",
        next_id: "init_admin",
        next_source: OptionSource::Roles,
        placeholder: "Select admin role...",
      },
      "Step 2/2: Admin Role",
    )),
    "init_runner" => Some((
      WizardStep {
        field: ConfigField::Runner,
        title: "Runner role selected",
        next_label: "Select the role for bot administrators:",
        next_id: "init_admin",
        next_source: OptionSource::Roles,
        placeholder: "Select admin role...",
      },
      "Step 2/2: Admin Role",
    )),
    // CategoryLink flow steps
    "categorylink_dashboard" => Some((
      WizardStep {
        field: ConfigField::Dashboard,
        title: "Dashboard channel selected",
        next_label: "Select the text channel for queue commands:",
        next_id: "categorylink_queue",
        next_source: OptionSource::TextChannels,
        placeholder: "Select queue text channel...",
      },
      "Step 2/5: Queue Text Channel",
    )),
    "categorylink_queue" => Some((
      WizardStep {
        field: ConfigField::Queue,
        title: "Queue text channel selected",
        next_label: "Select the voice channel where players wait:",
        next_id: "categorylink_queuevc",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select queue voice channel...",
      },
      "Step 3/5: Queue Voice Channel",
    )),
    "categorylink_queuevc" => Some((
      WizardStep {
        field: ConfigField::QueueVc,
        title: "Queue voice channel selected",
        next_label: "Select the Red team voice channel:",
        next_id: "categorylink_red",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select red team channel...",
      },
      "Step 4/5: Red Team Voice Channel",
    )),
    "categorylink_red" => Some((
      WizardStep {
        field: ConfigField::Red,
        title: "Red team channel selected",
        next_label: "Select the Blue team voice channel:",
        next_id: "categorylink_blue",
        next_source: OptionSource::VoiceChannels,
        placeholder: "Select blue team channel...",
      },
      "Step 5/5: Blue Team Voice Channel",
    )),
    _ => None,
  }
}

/// Handles admin role selection and completes setup
async fn handle_admin_selection(ctx: &Context, interaction: &CX, role_id: u64, db: &Arc<Database>, manager: &Arc<Mutex<Manager>>) -> Result<()> {
  let user_id = interaction.user.id;
  let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;

  // Store the final selection and retrieve complete config
  let config = SETUP_STATE.update_setup(user_id, guild_id, |config| {
    config.admin_role = Some(role_id);
  });

  let config = match config {
    Some(cfg) if cfg.is_complete() => cfg,
    _ => {
      let error_embed = CE::new().title("Setup error").description("Configuration is incomplete. Please restart the setup process.").color(RED);

      let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![]));

      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }
  };

  let dashboard_channel = config.dashboard_channel.unwrap();
  let queue_channel = config.queue_channel.unwrap();
  let queue_vc_channel = config.queue_vc_channel.unwrap();
  let red_channel = config.red_channel.unwrap();
  let blue_channel = config.blue_channel.unwrap();
  let runner_role = config.runner_role.unwrap();
  let admin_role = role_id;

  // Create the initial dashboard message
  let dashboard_channel_id = CI::new(dashboard_channel);
  let initial_embed = CE::new().title("PUG Queue Dashboard").description("Queue is empty. Be the first to join!").color(CYAN);

  let dashboard_message = match dashboard_channel_id.send_message(&ctx.http, CreateMessage::new().embed(initial_embed)).await {
    Ok(msg) => msg,
    Err(e) => {
      let error_embed = CE::new().title("Setup failed").description(format!("Failed to create dashboard message: {e}")).color(RED);

      interaction.create_response(&ctx.http, Ephemeral::update(error_embed)).await?;
      return Ok(());
    }
  };

  let dashboard_msg_id = dashboard_message.id.get();

  match save_and_create_category(ctx, db, manager, guild_id, dashboard_channel, queue_channel, queue_vc_channel, runner_role, admin_role, dashboard_msg_id).await {
    Ok(_) => {
      SETUP_STATE.complete_setup(user_id, guild_id);

      let success_embed = CE::new()
        .title("Setup complete!")
        .description(format!(
          "Your PUG bot is now fully configured and ready to use!\n\n\
                    **Configuration Summary:**\n\
                    - Dashboard: <#{dashboard_channel}>\n\
                    - Queue Text: <#{queue_channel}>\n\
                    - Queue Voice: <#{queue_vc_channel}>\n\
                    - Red Team: <#{red_channel}>\n\
                    - Blue Team: <#{blue_channel}>\n\
                    - Runner Role: <@&{runner_role}>\n\
                    - Admin Role: <@&{admin_role}>\n\n\
                    **The dashboard is ready!** Players can now:\n\
                    - Click \"Join\" to queue up or \"Leave\" to exit the queue\n\
                    - Join the queue voice channel to auto-queue\n\n\
                    Runners can use the dashboard buttons to manage matches.",
        ))
        .color(GREEN);

      interaction.create_response(&ctx.http, Ephemeral::update(success_embed)).await?;
    }
    Err(e) => {
      let error_embed = CE::new().title("Setup failed").description(format!("Failed to save configuration: {e}")).color(RED);

      interaction.create_response(&ctx.http, Ephemeral::update(error_embed)).await?;
    }
  }

  Ok(())
}

/// Handles admin role selection and completes init_category setup
async fn handle_init_admin_selection(
  ctx: &Context,
  interaction: &CX,
  role_id: u64,
  db: &std::sync::Arc<crate::Database>,
  manager: &Arc<Mutex<crate::models::Manager>>,
) -> Result<()> {
  let guild_id = match interaction.guild_id {
    Some(id) => id,
    None => return Err(anyhow!("Guild ID not found - setup must be run in a server")),
  };
  let user_id = interaction.user.id;

  // Store the admin role selection
  let config = SETUP_STATE.update_setup(user_id, guild_id, |config| {
    config.admin_role = Some(role_id);
  });

  // Validate all required fields are present
  let config = match config {
    Some(cfg)
      if cfg.dashboard_channel.is_some()
        && cfg.dashboard_msg_id.is_some()
        && cfg.queue_channel.is_some()
        && cfg.queue_vc_channel.is_some()
        && cfg.red_channel.is_some()
        && cfg.blue_channel.is_some()
        && cfg.runner_role.is_some()
        && cfg.admin_role.is_some() =>
    {
      cfg
    }
    _ => {
      let error_embed = CE::new().title("Setup error").description("Configuration is incomplete. Please restart the setup process.").color(RED);

      let response = CIR::UpdateMessage(CIRM::new().embed(error_embed).components(vec![]));

      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }
  };

  let dashboard_channel = config.dashboard_channel.unwrap();
  let dashboard_msg_id = config.dashboard_msg_id.unwrap();
  let queue_channel = config.queue_channel.unwrap();
  let queue_vc_channel = config.queue_vc_channel.unwrap();
  let red_channel = config.red_channel.unwrap();
  let blue_channel = config.blue_channel.unwrap();
  let runner_role = config.runner_role.unwrap();
  let admin_role = role_id;

  match save_and_create_category(ctx, db, manager, guild_id, dashboard_channel, queue_channel, queue_vc_channel, runner_role, admin_role, dashboard_msg_id).await {
    Ok(_) => {
      SETUP_STATE.complete_setup(user_id, guild_id);

      let success_embed = CE::new()
        .title("Category setup complete!")
        .description(format!(
          "Category configuration has been saved successfully!\n\n\
                    **Configuration Summary:**\n\
                    - Dashboard: <#{dashboard_channel}>\n\
                    - Queue Text: <#{queue_channel}>\n\
                    - Queue Voice: <#{queue_vc_channel}>\n\
                    - Red Team: <#{red_channel}>\n\
                    - Blue Team: <#{blue_channel}>\n\n\
                    The dashboard has been initialized in <#{dashboard_channel}> with the interactive queue interface!",
        ))
        .color(GREEN);

      interaction.create_response(&ctx.http, Ephemeral::update(success_embed)).await?;
    }
    Err(e) => {
      let error_embed = CE::new().title("Setup failed").description(format!("Failed to create category configuration: {e}")).color(RED);

      interaction.create_response(&ctx.http, Ephemeral::update(error_embed)).await?;
    }
  }

  Ok(())
}

/// `/check_ranks` - Check and offer to create missing rank roles
pub async fn cmd_check_ranks(cc: &CC<'_>) -> Result<()> {
  if !is_admin(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");

  // Check for missing system roles (Runner and Admin)
  let missing_system_roles = match validate_system_roles(cc.ctx, &cc.db, guild_id).await {
    Ok(roles) => roles,
    Err(e) => {
      let error_embed = CE::new().title("Error").description(format!("Failed to check system roles: {e}")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  // Initialize default ranks if needed
  if let Err(e) = cc.db.ranks.init_default_ranks(guild_id).await {
    warn!("Failed to initialize default ranks: {e}");
  }

  // Build response based on what's missing
  if missing_system_roles.is_empty() {
    // All roles exist
    let success_embed = CE::new().title("All roles configured").description("All system roles (Runner, Admin) and rank roles are properly configured in this server!").color(GREEN);

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
  } else {
    // Build description for missing roles
    let system_list = missing_system_roles.join(", ");
    let description = format!(
      "**Missing System Roles:**\n{system_list}\n\n\
             System roles should be created manually and assigned appropriate permissions.",
    );

    let embed = CE::new().title("Missing roles").description(description).color(ORANGE);

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
  }

  Ok(())
}

/// Handle rank role creation confirmation button (deprecated - ranks are now ELO-based only)
pub async fn handle_create_rank_roles(ctx: &Context, db: &crate::Database, interaction: &CX, _create: bool) -> Result<()> {
  let guild_id = match interaction.guild_id {
    Some(id) => id,
    None => return Err(anyhow!("Guild ID not found - this command must be run in a server")),
  };

  // Initialize default ranks in database
  if let Err(e) = db.ranks.init_default_ranks(guild_id).await {
    warn!("Failed to initialize default ranks: {e}");
  }

  let embed =
    CE::new().title("Ranks initialized").description("Default ranks have been initialized in the database. Ranks are now ELO-based and do not require Discord roles.").color(GREEN);

  let response = CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]));

  interaction.create_response(&ctx.http, response).await?;

  Ok(())
}

/// Saves role configs, initializes ranks, creates the category in DB, and finalizes it in the manager.
/// Returns Ok(guild_name) on success, or an error string on failure.
async fn save_and_create_category(
  ctx: &Context,
  db: &Arc<Database>,
  manager: &Arc<Mutex<Manager>>,
  guild_id: GI,
  dashboard_channel: u64,
  queue_channel: u64,
  queue_vc_channel: u64,
  runner_role: u64,
  admin_role: u64,
  dashboard_msg_id: u64,
) -> Result<String> {
  // Save role configurations
  if let Err(e) = db.config.set_runner_role_id(guild_id, serenity::all::RoleId::new(runner_role)).await {
    warn!("Failed to save runner_role config: {e}");
  }
  if let Err(e) = db.config.set_admin_role_id(guild_id, serenity::all::RoleId::new(admin_role)).await {
    warn!("Failed to save admin_role config: {e}");
  }

  // Initialize default ranks
  let guild_name = guild_name(ctx, guild_id);
  info!("[{}] Initializing default ranks", guild_name);
  if let Err(e) = db.ranks.init_default_ranks(guild_id).await {
    warn!("[{}] Failed to initialize default ranks: {}", guild_name, e);
  }

  // Derive category from dashboard channel's parent
  let category = ctx.cache.channel(CI::new(dashboard_channel)).and_then(|ch| ch.parent_id).map(|id| id.get()).unwrap_or(0);

  // Create category in database
  let category_config = crate::db::repo::category::CategoryConfig {
    channel_category_id: category,
    dashboard_channel_id: dashboard_channel,
    chat_channel_id: queue_channel,
    queue_vc_id: queue_vc_channel,
    ping_channel_id: 1,
    quota: DEFAULT_QUOTA,
  };
  db.categories.add_category(guild_id, &guild_name, dashboard_msg_id, category_config).await?;
  info!("[{}] Category configuration saved to database", guild_name);

  // Load into manager and update dashboard
  finalize_category_setup(ctx, db, manager, guild_id, dashboard_msg_id).await?;

  Ok(guild_name)
}

/// Helper function to finalize category setup by loading it into manager and immediately updating dashboard
async fn finalize_category_setup(ctx: &Context, db: &Arc<Database>, manager: &Arc<Mutex<Manager>>, guild_id: GI, dashboard_msg_id: u64) -> Result<()> {
  let guild_name = guild_name(ctx, guild_id);

  // Load the category from database
  match db.categories.get_categories_for_guild(guild_id).await {
    Ok(categories) if !categories.is_empty() => {
      let new_category = categories.into_iter().find(|g| g.dashboard_msg.get() == dashboard_msg_id).ok_or_else(|| anyhow!("Could not find newly created category"))?;

      use crate::models::QGuild;
      let mut mgr = manager.lock().await;

      // Ensure server exists in manager
      if mgr.get_qguild(guild_id).is_err() {
        let server = QGuild::empty(guild_id, guild_name.clone());
        mgr.qguilds.push(server);
      }

      let server = mgr.get_qguild(guild_id)?;
      server.add_category(new_category)?;

      // Get the newly added category and immediately update its dashboard
      let category = server.categories.last_mut().ok_or_else(|| anyhow!("Failed to get newly added category"))?;

      // Use dash_update for immediate synchronous update instead of queued async update
      if let Err(e) = category.dash_update(ctx).await {
        warn!("[{}] Failed to update dashboard: {}", guild_name, e);
      } else {
        info!("[{}] Category added to manager and dashboard updated successfully", guild_name);
      }

      Ok(())
    }
    Ok(_) => Err(anyhow!("[{}] No categories found after creation", guild_name)),
    Err(e) => Err(anyhow!("[{}] Failed to load categories from database: {}", guild_name, e)),
  }
}

/// Handles categorylink blue team channel selection - Step 5 (final step, creates the category)
async fn handle_categorylink_blue_selection(
  ctx: &Context,
  interaction: &CX,
  channel_id: u64,
  db: &std::sync::Arc<crate::Database>,
  manager: &Arc<Mutex<crate::models::Manager>>,
) -> Result<()> {
  let user_id = interaction.user.id;
  let guild_id = interaction.guild_id.ok_or_else(|| anyhow!("Guild ID not found"))?;
  let guild_name = guild_name(ctx, guild_id);

  SETUP_STATE.update_setup(user_id, guild_id, |config| {
    config.blue_channel = Some(channel_id);
  });

  // Get complete configuration
  let config = SETUP_STATE.get_setup(user_id, guild_id).ok_or_else(|| anyhow!("Setup configuration not found"))?;

  let dashboard_channel = CI::new(config.dashboard_channel.ok_or_else(|| anyhow!("Dashboard channel not set"))?);
  let queue_channel = CI::new(config.queue_channel.ok_or_else(|| anyhow!("Queue channel not set"))?);
  let queue_vc_channel = CI::new(config.queue_vc_channel.ok_or_else(|| anyhow!("Queue VC channel not set"))?);
  let red_channel = CI::new(config.red_channel.ok_or_else(|| anyhow!("Red channel not set"))?);
  let blue_channel = CI::new(config.blue_channel.ok_or_else(|| anyhow!("Blue channel not set"))?);

  // Send "creating category" message
  let loading_embed = CE::new().title("Creating category").description("Linking channels and creating PUG category...\n\nCleaning up any old configurations...").color(ORANGE);

  let response = CIR::UpdateMessage(CIRM::new().embed(loading_embed).components(vec![]));
  interaction.create_response(&ctx.http, response).await?;

  // Check and clean up old categories that use these channels
  let mut mgr = manager.lock().await;
  let server_opt = mgr.qguilds.iter_mut().find(|s| s.id == guild_id);

  if let Some(server) = server_opt {
    let mut categories_to_remove = Vec::new();

    for (idx, category) in server.categories.iter().enumerate() {
      if category.channels.dashboard == dashboard_channel
        || category.channels.queue_chat == queue_channel
        || category.channels.queue_vc == queue_vc_channel
        || category.channels.teams.iter().any(|t| t.red_vc == red_channel || t.blu_vc == blue_channel)
      {
        categories_to_remove.push((idx, category.id));
      }
    }

    // Remove old configurations from database and memory
    for (idx, category_id) in categories_to_remove.iter().rev() {
      info!("[{}] Removing old category {} configuration", guild_name, category_id);
      if let Err(e) = db.categories.delete(*category_id).await {
        warn!("[{}] Failed to delete old category {}: {}", guild_name, category_id, e);
      }
      server.categories.remove(*idx);
    }
  }
  drop(mgr);

  // Create temporary category and publish dashboard
  use crate::models::{Category, Channels, TeamChannel};
  use serenity::all::MessageId;

  // Derive category from dashboard channel's parent
  let category_id = ctx.cache.channel(dashboard_channel).and_then(|ch| ch.parent_id).unwrap_or(CI::new(1));

  let mut temp_category = Category::new(
    guild_id,
    None,
    0,
    None,
    crate::DEFAULT_QUOTA,
    crate::DEFAULT_CONFIRM_TIME,
    MessageId::new(1),
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

  // Publish dashboard to get message ID
  match temp_category.dash_publish(ctx, dashboard_channel, db, guild_id).await {
    Ok(_) => {
      let dashboard_msg_id = temp_category.dashboard_msg.get();

      // Save to database
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
          info!("[{}] Category {} created via categorylink", guild_name, db_category.id);

          // Add to manager
          let mut mgr = manager.lock().await;
          if let Ok(server) = mgr.get_qguild(guild_id) {
            if let Err(e) = server.add_category(db_category.clone()) {
              error!("[{}] Failed to add category: {}", guild_name, e);
            }
          }
          drop(mgr);

          // Clean up setup state
          SETUP_STATE.complete_setup(user_id, guild_id);

          let success_embed = CE::new()
            .title("Category created!")
            .description(format!(
              "Successfully linked existing channels!\n\n\
                            **Configuration:**\n\
                            • Dashboard: <#{}>\n\
                            • Queue Text: <#{}>\n\
                            • Queue Voice: <#{}>\n\
                            • Red Team: <#{}>\n\
                            • Blue Team: <#{}>\n\n\
                            The PUG queue is now ready to use!",
              dashboard_channel.get(),
              queue_channel.get(),
              queue_vc_channel.get(),
              red_channel.get(),
              blue_channel.get()
            ))
            .color(GREEN);

          interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(success_embed)).await?;
        }
        Err(e) => {
          // Delete dashboard message on failure
          let _ = dashboard_channel.delete_message(&ctx.http, dashboard_msg_id).await;

          let error_embed = CE::new().title("Failed to save category").description(format!("Error saving to database: {e}")).color(RED);

          interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(error_embed)).await?;
        }
      }
    }
    Err(e) => {
      let error_embed = CE::new().title("Dashboard creation failed").description(format!("Failed to create dashboard: {e}")).color(RED);

      interaction.edit_response(&ctx.http, serenity::all::EditInteractionResponse::new().embed(error_embed)).await?;
    }
  }

  Ok(())
}

/// `/elo` - View ELO and rank information for a player
///
/// * `user` - The Discord user (mention or ID, optional - defaults to command user)
pub async fn cmd_get_player_elo(cc: &CC<'_>, user: Option<serenity::all::User>) -> Result<()> {
  let guild_id = cc.intax.guild_id.expect("Guild ID not found");
  let user_id = user.as_ref().map(|u| u.id).unwrap_or(cc.intax.user.id);
  let is_self = user_id == cc.intax.user.id;

  if !is_self && !is_admin(cc).await? {
    return Ok(());
  }

  // Get guild-specific ELO data
  let guild_elo = cc.db.elo.get(user_id, guild_id, &cc.db).await?;

  // Get base player info for steam_id
  let player = match cc.db.players.get(user_id).await {
    Ok(p) => p,
    Err(_) => {
      let error_embed = CE::new().title("Player not found").description(format!("<@{}> is not in the database.", user_id)).color(RED);
      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
  info!("DEBUG: User {} - Guild ELO: {}, Rank: {}, Games: {}, Wins: {}", user_tag, guild_elo.elo, guild_elo.rank.name, guild_elo.games, guild_elo.wins);

  // Get user info - if no user provided, we can't continue
  let user_info = user.ok_or_else(|| {
    let _error_embed = CE::new().title("User required").description("You must specify a user to view their ELO information, or use the command on yourself.").color(RED);
    anyhow::anyhow!("User not provided")
  })?;

  // Create embed with player info
  let mut embed = CE::new().title(format!("{}'s ELO Information", user_info.tag())).color(CYAN);

  // ELO information
  embed = embed.field("ELO Rating", format!("**{}**", guild_elo.elo), true);

  // Rank information
  embed = embed.field("Rank", format!("**{}**", guild_elo.rank.name), true);

  // Stats
  let win_rate = if guild_elo.games > 0 { format!("{:.1}%", (guild_elo.wins as f64 / guild_elo.games as f64) * 100.0) } else { "N/A".to_string() };
  embed = embed.field("Games", format!("**{}** ({} wins, {} win rate)", guild_elo.games, guild_elo.wins, win_rate), false);

  // Additional info
  embed =
    embed.field("Discord ID", format!("`{}`", user_id), false).field("Steam ID", player.steam_id.map(|id| format!("`{id}`")).unwrap_or_else(|| "*Not linked*".to_string()), false);

  cc.intax.create_response(&cc.ctx.http, Ephemeral::send(embed)).await?;
  Ok(())
}

/// `/enableactiveelo` - Enable automatic ELO adjustments from match results
pub async fn cmd_enable_active_elo(cc: &CC<'_>) -> Result<()> {
  // Check admin permissions
  if !is_admin(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");

  // Enable active ELO in config
  cc.db.config.set_active_elo(guild_id, true).await?;

  let success_embed = CE::new()
    .title("Active ELO enabled")
    .description("Automatic ELO adjustments from match results are now **enabled**.\n\n*Note: This requires webhooks and game server API to be configured to actually work.*")
    .color(GREEN);

  cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
  Ok(())
}

/// `/disableactiveelo` - Disable automatic ELO adjustments from match results
pub async fn cmd_disable_active_elo(cc: &CC<'_>) -> Result<()> {
  // Check admin permissions
  if !is_admin(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");

  // Disable active ELO in config
  cc.db.config.set_active_elo(guild_id, false).await?;

  let success_embed = CE::new().title("Active ELO disabled").description("Automatic ELO adjustments from match results are now **disabled**.").color(ORANGE);

  cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
  Ok(())
}

/// `/activeelostatus` - Check if automatic ELO adjustments are enabled
pub async fn cmd_active_elo_status(cc: &CC<'_>) -> Result<()> {
  // Check admin permissions
  if !is_admin(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");

  // Check current status
  let is_enabled = match cc.db.config.get_active_elo(guild_id).await {
    Ok(enabled) => enabled,
    Err(_) => crate::DEFAULT_ACTIVE_ELO,
  };

  let status_embed = CE::new()
    .title("Active ELO status")
    .description(format!(
      "Automatic ELO adjustments are currently **{}**\n\n\
            When enabled, this feature will:\n\
            • Receive match results from game server API\n\
            • Automatically adjust player ELO based on wins/losses\n\
            • Update player ranks based on new ELO values\n\n\
            *Note: Webhooks and game server integration required for full functionality.*",
      if is_enabled { "ENABLED" } else { "DISABLED" }
    ))
    .color(if is_enabled { GREEN } else { RED });

  cc.intax.create_response(&cc.ctx.http, Ephemeral::send(status_embed)).await?;
  Ok(())
}

// RUNNER COMMANDS

/// `/buffer`
///
/// * `user_id` - The user ID to buffer.
/// * `server` - The server (already has manager lock held by caller)
pub async fn cmd_buffer(cc: &CC<'_>, server: &mut QGuild, user_id: UI) -> Result<()> {
  if !is_runner(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");
  let guild_name = guild_name(cc.ctx, guild_id);

  info!("[{}] Getting category from channel {}", guild_name, cc.intax.channel_id);
  // Get the category from the current channel
  let category = match server.get_category(cc.intax.channel_id) {
    Ok(g) => g,
    Err(e) => {
      warn!("[{}] No category found in channel {}: {}", guild_name, cc.intax.channel_id, e);
      let error_embed = CE::new().title("Category not found").description(format!("No queue category found in this channel: {e}")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
  info!("[{}] Finding session for user {}", guild_name, user_tag);
  // Find the session containing the player
  let session = match category.get_user_sesh(user_id).await {
    Ok(s) => s,
    Err(e) => {
      let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
      warn!("[{}] User {} not found in any session: {}", guild_name, user_tag, e);
      let error_embed = CE::new().title("Player not found").description(format!("<@{user_id}> is not in any queue.")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  // Find the player's index in the pool
  let player_idx = match session.pool.iter().position(|p| p.player.user_id == user_id) {
    Some(idx) => idx,
    None => {
      let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
      error!("[{}] Player {} not found in pool despite being in session", guild_name, user_tag);
      let error_embed = CE::new().title("Player not found").description(format!("<@{user_id}> is not in the queue pool.")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  // Remove the player from their current position
  let player = session.pool.remove(player_idx);

  // Insert the player at the front of the queue (index 0)
  session.pool.insert(0, player);

  let is_hot = session.is_hot();

  // Validate VC status to sync in_queue_vc flags with actual Discord state
  category.validate_vc_status(cc.ctx, guild_id).await;

  // If session is hot, regenerate teams with new order
  // Note: generate_teams() already handles dashboard update
  if is_hot {
    category.generate_teams(cc.ctx, guild_id, Some(&cc.db)).await;
  } else {
    // Only update dashboard if not hot (since generate_teams() handles it for hot sessions)
    category.queue_dash_update(cc.ctx, guild_id).await;
  }

  let success_embed = CE::new().title("Player buffered").description(format!("<@{user_id}> moved to front of queue.")).color(GREEN);

  cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
  Ok(())
}

/// `/fatkid`
///
/// * `user_id` - The user ID to fatkid (move to end of queue).
/// * `server` - The server (already has manager lock held by caller)
pub async fn cmd_fatkid(cc: &CC<'_>, server: &mut QGuild, user_id: UI) -> Result<()> {
  if !is_runner(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");
  let guild_name = guild_name(cc.ctx, guild_id);

  info!("[{}] Getting category from channel {}", guild_name, cc.intax.channel_id);
  // Get the category from the current channel
  let category = match server.get_category(cc.intax.channel_id) {
    Ok(g) => g,
    Err(e) => {
      warn!("[{}] No category found in channel {}: {}", guild_name, cc.intax.channel_id, e);
      let error_embed = CE::new().title("Category not found").description(format!("No queue category found in this channel: {e}")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
  info!("[{}] Finding session for user {}", guild_name, user_tag);
  // Find the session containing the player
  let session = match category.get_user_sesh(user_id).await {
    Ok(s) => s,
    Err(e) => {
      let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
      warn!("[{}] User {} not found in any session: {}", guild_name, user_tag, e);
      let error_embed = CE::new().title("Player not found").description(format!("<@{user_id}> is not in any queue.")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  // Find the player's index in the pool
  let player_idx = match session.pool.iter().position(|p| p.player.user_id == user_id) {
    Some(idx) => idx,
    None => {
      let user_tag = crate::log::get_user_tag(cc.ctx, user_id, &cc.db).await;
      error!("[{}] Player {} not found in pool despite being in session", guild_name, user_tag);
      let error_embed = CE::new().title("Player not found").description(format!("<@{user_id}> is not in the queue pool.")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  // Remove the player from their current position
  let player = session.pool.remove(player_idx);

  // Insert the player at the end of the queue
  session.pool.push(player);

  let is_hot = session.is_hot();

  // Validate VC status to sync in_queue_vc flags with actual Discord state
  category.validate_vc_status(cc.ctx, guild_id).await;

  // If session is hot, regenerate teams with new order
  if is_hot {
    category.generate_teams(cc.ctx, guild_id, Some(&cc.db)).await;
  }

  category.queue_dash_update(cc.ctx, guild_id).await;

  let success_embed = CE::new().title("Player fatkidded").description(format!("<@{user_id}> moved to end of queue.")).color(GREEN);

  cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
  Ok(())
}

/// `/remove` - Remove all players from the queue, or a specific player
/// Works in any category channel (queue chat, dashboard, team VCs, etc.)
/// Handles all formats, not just the first one
pub async fn cmd_remove_queue(cc: &CC<'_>, server: &mut QGuild, user_option: Option<&CommandDataOption>) -> Result<()> {
  if !is_runner(cc).await? {
    return Ok(());
  }

  let guild_id = cc.intax.guild_id.expect("Guild ID not found");

  // Get the category from the current channel (works in any category channel)
  let category = match server.get_category(cc.intax.channel_id) {
    Ok(g) => g,
    Err(e) => {
      let error_embed = CE::new().title("Category not found").description(format!("No queue category found in this channel: {e}")).color(RED);

      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
      return Ok(());
    }
  };

  // Determine which format(s) to target
  let target_formats = if let Some(user_opt) = user_option {
    // For specific user removal, search all formats for that user
    if let Some(user_id) = user_opt.value.as_user_id() {
      let mut target_fmt_ids = Vec::new();
      for (fmt_idx, sg) in category.formats.iter().enumerate() {
        // Check if user is in any session of this format
        for session in &sg.sessions {
          if session.pool.iter().any(|p| p.player.user_id == user_id.get()) {
            target_fmt_ids.push(fmt_idx);
            break;
          }
        }
      }

      if target_fmt_ids.is_empty() {
        // User not found in any format, default to first format
        vec![0]
      } else {
        target_fmt_ids
      }
    } else {
      vec![0] // Default to first format if user parsing fails
    }
  } else {
    // For clearing queue, target all formats
    (0..category.formats.len()).collect()
  };

  // Handle specific user removal vs clear all
  if let Some(user_opt) = user_option {
    // Remove specific user from relevant formats
    if let Some(user_id) = user_opt.value.as_user_id() {
      let user: Result<User, serenity::Error> = cc.ctx.http.get_user(user_id).await;

      match user {
        Ok(target_user) => {
          let mut total_removed = 0;
          let mut removed_from_formats = Vec::new();

          // Remove from each targeted format
          for &fmt_idx in &target_formats {
            if let Some(sg) = category.formats.get_mut(fmt_idx) {
              let quota = sg.quota as usize;
              
              for session in &mut sg.sessions {
                if !session.is_active() {
                  let initial_len = session.pool.len();
                  session.pool.retain(|p| p.player.user_id != user_id.get());
                  let removed_count = initial_len - session.pool.len();
                  
                  if removed_count > 0 {
                    total_removed += removed_count;
                    removed_from_formats.push((fmt_idx, sg.name.clone()));
                    
                    // If this was a Hot session and now below quota, transition back to Idle
                    if session.is_hot() && session.pool.len() < quota {
                      session.idle();
                      info!("Hot session dropped below quota after removing player, transitioning back to Idle");
                    }
                  }
                }
              }
              
              // After removal, check if we can pull waiting players to meet quota
              if category.is_quota_fmt(fmt_idx as u8) {
                if let Err(e) = category.hot_fmt(fmt_idx as u8, cc.ctx, Some(guild_id), Some(&*cc.db), None, false).await {
                  warn!("Failed to transition to hot after player removal: {}", e);
                }
              }
            }
          }

          // Update the dashboard
          category.queue_dash_update(cc.ctx, guild_id).await;

          if total_removed > 0 {
            let fmt_names: Vec<String> = removed_from_formats.iter().map(|(_, name)| format!("**{}**", name)).collect();

            let success_embed = CE::new()
              .title("Player removed")
              .description(format!(
                "Removed **{}** from the queue{}.",
                target_user.name,
                if removed_from_formats.len() > 1 { format!(" from formats: {}", fmt_names.join(", ")) } else { String::new() }
              ))
              .color(GREEN);
            cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
          } else {
            let error_embed = CE::new().title("Player not in queue").description(format!("**{}** is not currently in the queue.", target_user.name)).color(ORANGE);
            cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
          }
        }
        Err(_) => {
          let error_embed = CE::new().title("Failed to get user").description("Could not find the specified Discord user.").color(RED);
          cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
        }
      }
    } else {
      let error_embed = CE::new().title("Invalid user").description("Could not parse the user from the command.").color(RED);
      cc.intax.create_response(&cc.ctx.http, Ephemeral::send(error_embed)).await?;
    }
  } else {
    // Clear all players from all formats
    let mut total_players = 0;
    let mut cleared_formats = Vec::new();

    for (fmt_idx, sg) in category.formats.iter_mut().enumerate() {
      for session in &mut sg.sessions {
        if !session.is_active() {
          let count = session.pool.len();
          if count > 0 {
            total_players += count;
            // Use idle() to properly reset session state (clears pool, team assignments, timestamps)
            session.idle();
            cleared_formats.push((fmt_idx, sg.name.clone()));
          }
        }
      }
    }

    // Update the dashboard
    category.queue_dash_update(cc.ctx, guild_id).await;

    let success_embed = CE::new()
      .title("Queue cleared")
      .description(format!(
        "Removed {total_players} player(s) from the queue{}.",
        if cleared_formats.len() > 1 {
          let fmt_names: Vec<String> = cleared_formats.iter().map(|(_, name)| format!("**{}**", name)).collect();
          format!(" from formats: {}", fmt_names.join(", "))
        } else {
          String::new()
        }
      ))
      .color(GREEN);

    cc.intax.create_response(&cc.ctx.http, Ephemeral::send(success_embed)).await?;
  }

  Ok(())
}
