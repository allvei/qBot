//! Application initialization and management
//!
//! This module handles the setup and lifecycle of the Discord bot application,
//! including database initialization, client configuration, and graceful shutdown.
use std::sync::Arc;

use anyhow::Result;
use serenity::all::{
  Client, Command, CommandDataOption, CommandInteraction, CommandOptionType as COT, Context, EventHandler, GatewayIntents, Guild, GuildId, Interaction, Ready, UserId, VoiceState,
};
use serenity::async_trait;
use serenity::builder::{CreateCommand as CC, CreateCommandOption as CCO, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};
use serenity::prelude::TypeMapKey;
use tokio::sync::{Mutex, oneshot, mpsc};
use tracing::{debug, error, info, warn};

use crate::commands;
use crate::db::migrations::DatabaseMigrations;
use crate::db::repo::CategoryRepository;
use crate::handlers::{admin, InteractionHelpers};
use crate::gui::command_handler;
use crate::gui::commands::GuiCommand;
use crate::models::server::QueueContext;
use crate::repo::GuildRepository;
use crate::{
  guild_name, log_prefix_category, log_prefix_format, log_queue_toggle, ButtonType, Category, CommandContext, ComponentContext, DashboardQueueKey,
  DashboardUpdateQueue, Database, DmMessageTracker, DmTrackerKey, Manager, QGuild, Roles, SessionStatus, VoiceStateUpdate, RED,
};

// Helper macros and functions that need to be available
macro_rules! cmd {
    ($name:expr, $desc:expr) => {
        CC::new($name).description($desc)
    };
    ($name:expr, $desc:expr, $($rest:tt)*) => {
        CC::new($name).description($desc)$( $rest )*
    };
}

pub trait CmdOp: Sized {
  fn op_int(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self;
  fn op_string(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self;
  fn op_user(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self;
  fn op_role(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self;
}

impl CmdOp for CC {
  /// Adds an integer option to the command
  fn op_int(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self {
    self.add_option(CCO::new(COT::Integer, name, desc).required(req))
  }

  /// Adds a string option to the command
  fn op_string(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self {
    self.add_option(CCO::new(COT::String, name, desc).required(req))
  }

  /// Adds a user option to the command
  fn op_user(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self {
    self.add_option(CCO::new(COT::User, name, desc).required(req))
  }

  /// Adds a role option to the command
  fn op_role(self, name: impl Into<String>, desc: impl Into<String>, req: bool) -> Self {
    self.add_option(CCO::new(COT::Role, name, desc).required(req))
  }
}

// Helper functions that were in main.rs
async fn get_server_with_error<'a>(manager: &'a mut Manager, guild_id: GuildId, _itx: &CommandInteraction, _ctx: &Context) -> Result<&'a mut QGuild, String> {
  manager.get_qguild(guild_id).map_err(|_| "Server not found. Please run `/setup` first.".to_string())
}

async fn extract_user_option(options: &[CommandDataOption], name: &str) -> Option<UserId> {
  options.iter().find(|opt| opt.name == name).and_then(|opt| opt.value.as_user_id())
}

async fn send_error_response(itx: &CommandInteraction, ctx: &Context, message: &str) -> Result<(), serenity::Error> {
  let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
  itx.create_response(&ctx.http, response).await
}

fn is_interaction_valid(_pl: &Interaction) -> bool {
  // Add validation logic here
  true // Placeholder
}

// Define TypeMapKey for Manager (temporary until moved to models)
pub struct GuildKey;
impl TypeMapKey for GuildKey {
  type Value = Arc<Mutex<Manager>>;
}

/// Application state and lifecycle management
pub struct Application {
  pub db: Arc<Database>,
  pub manager: Arc<Mutex<Manager>>,
  pub dashboard_queue: Arc<Mutex<Option<DashboardUpdateQueue>>>,
  pub cmd_rx: Option<mpsc::Receiver<GuiCommand>>,
  pub latest_manager: Option<Arc<tokio::sync::RwLock<Option<Manager>>>>,
  pub gui_shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl Application {
  /// Initialize the application with all required components
  pub async fn new() -> Result<Self> {
    // Load configuration
    let config = crate::util::Config::load()?;

    // Initialize database
    let db = Self::setup_database(&config.database_url).await?;

    // Initialize manager
    let manager = Arc::new(Mutex::new(Manager::default()));

    // Initialize dashboard queue
    let dashboard_queue = Arc::new(Mutex::new(None));

    Ok(Self { db, manager, dashboard_queue, cmd_rx: None, latest_manager: None, gui_shutdown_rx: None })
  }

  /// Initialize the application with pre-created manager and db (for GUI integration)
  pub async fn new_with_shared(manager: Arc<Mutex<Manager>>, db: Arc<Database>) -> Result<Self> {
    // Initialize dashboard queue
    let dashboard_queue = Arc::new(Mutex::new(None));

    Ok(Self { db, manager, dashboard_queue, cmd_rx: None, latest_manager: None, gui_shutdown_rx: None })
  }

  /// Set the command receiver for GUI commands
  pub fn with_cmd_rx(mut self, cmd_rx: mpsc::Receiver<GuiCommand>) -> Self {
    self.cmd_rx = Some(cmd_rx);
    self
  }

  /// Set the latest_manager snapshot target for GUI
  pub fn with_latest_manager(mut self, latest_manager: Arc<tokio::sync::RwLock<Option<Manager>>>) -> Self {
    self.latest_manager = Some(latest_manager);
    self
  }

  /// Set the GUI shutdown receiver
  pub fn with_gui_shutdown(mut self, rx: oneshot::Receiver<()>) -> Self {
    self.gui_shutdown_rx = Some(rx);
    self
  }

  /// Setup database connection and run migrations
  async fn setup_database(database_url: &str) -> Result<Arc<Database>> {
    let db = Arc::new(Database::new(database_url).await?);

    let migrations = DatabaseMigrations::new(db.pool());
    migrations.create_tables().await?;
    migrations.verify_schemas().await?;

    Ok(db)
  }

  /// Create and configure Discord client
  async fn create_client(&self) -> Result<Client> {
    let config = crate::util::Config::load()?;
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILD_VOICE_STATES | GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;

    let client =
      Client::builder(&config.token, intents).event_handler(Handler { db: self.db.clone(), manager: self.manager.clone(), dashboard_queue: self.dashboard_queue.clone() }).await?;

    // Set the manager in the client data for global access
    client.data.write().await.insert::<GuildKey>(self.manager.clone());
    Ok(client)
  }

  /// Run the application with graceful shutdown
  pub async fn run(mut self) -> Result<()> {
    let mut client = self.create_client().await?;

    // Set up signal handling
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let shutdown_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Spawn shutdown handler
    let shutdown_handler = crate::shutdown::ShutdownHandler::new(self.manager.clone(), self.dashboard_queue.clone(), client.cache.clone(), client.http.clone(), self.db.clone());
    let shutdown_flag_signal = shutdown_flag.clone();

    tokio::spawn(async move {
      shutdown_handler.handle_signals(shutdown_tx).await;
      shutdown_flag_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    // Spawn command handler if cmd_rx is set
    if let Some(mut cmd_rx) = self.cmd_rx.take() {
      let manager = self.manager.clone();
      let db = self.db.clone();
      let shutdown_flag_cmd = shutdown_flag.clone();
      // Clone refs so the command task can update the snapshot and refresh the Discord dashboard
      let latest_manager_cmd  = self.latest_manager.clone();
      let dashboard_queue_cmd = self.dashboard_queue.clone();

      tokio::spawn(async move {
        loop {
          tokio::select! {
            biased;
            _ = tokio::task::yield_now() => {
              if shutdown_flag_cmd.load(std::sync::atomic::Ordering::Relaxed) {
                break;
              }
            }
            cmd = cmd_rx.recv() => {
              match cmd {
                Some(command) => {
                  let snapshot = {
                    let mut manager_lock = manager.lock().await;
                    if let Err(e) = command_handler::handle_command(command, &mut manager_lock, &db).await {
                      error!("Error handling GUI command: {}", e);
                    }
                    manager_lock.clone() // snapshot taken while lock is still held
                  };

                  // Immediately push updated state to the GUI
                  if let Some(ref lm) = latest_manager_cmd {
                    *lm.write().await = Some(snapshot);
                  }

                  // Trigger Discord dashboard refresh for all categories
                  if let Some(ref queue) = *dashboard_queue_cmd.lock().await {
                    queue.request_update_all_deferred();
                  }
                }
                None => break, // Channel closed
              }
            }
          }
        }
      });
    }

    // Spawn periodic snapshot task for GUI
    if let Some(latest_manager) = self.latest_manager.take() {
      let manager = self.manager.clone();
      let shutdown_flag_snap = shutdown_flag.clone();

      tokio::spawn(async move {
        use tokio::time::{interval, Duration};
        let mut snapshot_interval = interval(Duration::from_millis(100));

        loop {
          tokio::select! {
            biased;
            _ = tokio::task::yield_now() => {
              if shutdown_flag_snap.load(std::sync::atomic::Ordering::Relaxed) {
                break;
              }
            }
            _ = snapshot_interval.tick() => {
              // Clone manager state under lock (brief lock)
              let manager_clone = if let Ok(manager_lock) = manager.try_lock() {
                manager_lock.clone()
              } else {
                continue; // Skip this snapshot if lock is held
              };

              // Update snapshot
              let mut latest = latest_manager.write().await;
              *latest = Some(manager_clone);
            }
          }
        }
      });
    }

    // Start terminal command reader for testing
    crate::terminal::start_terminal_reader(self.manager.clone(), self.db.clone()).await;

    // Start client
    // Listen for signal-based shutdown AND GUI-based shutdown
    let gui_rx = self.gui_shutdown_rx.take();
    tokio::select! {
      result = client.start() => {
        if let Err(why) = result {
          error!("Client error: {:?}", why);
        }
      }
      _ = shutdown_rx => {}
      _ = async { if let Some(rx) = gui_rx { let _ = rx.await; } else { std::future::pending::<()>().await; } } => {}
    }

    Ok(())
  }
}

struct Handler {
  db: Arc<Database>,
  manager: Arc<Mutex<Manager>>,
  dashboard_queue: Arc<tokio::sync::Mutex<Option<DashboardUpdateQueue>>>,
}

/// Handler for Discord events
#[async_trait]
impl EventHandler for Handler {
  /// When the bot is ready
  async fn ready(&self, ctx: Context, _ready: Ready) {
    let _guild_count = ctx.cache.guilds().len();

    // Initialize dashboard update queue (done once on ready)
    {
      let mut queue_lock = self.dashboard_queue.lock().await;
      if queue_lock.is_none() {
        let queue = DashboardUpdateQueue::new(ctx.clone(), self.manager.clone(), self.db.clone());
        let queue_arc = Arc::new(tokio::sync::Mutex::new(queue.clone()));
        *queue_lock = Some(queue);

        // Store in Context data for global access
        ctx.data.write().await.insert::<DashboardQueueKey>(queue_arc);
      }
    }

    // Initialize DM message tracker and start cleanup task
    {
      let dm_tracker = Arc::new(DmMessageTracker::new());
      ctx.data.write().await.insert::<DmTrackerKey>(dm_tracker.clone());

      // Start the cleanup background task
      dm_tracker.start_cleanup_task(ctx.http.clone());
    }

    // Initialize player queue expiration scheduler
    {
      let scheduler = crate::models::QueueExpirationScheduler::new(self.manager.clone(), self.db.clone(), ctx.clone());
      let scheduler_arc = Arc::new(tokio::sync::Mutex::new(scheduler));
      ctx.data.write().await.insert::<crate::models::QueueExpirationSchedulerKey>(scheduler_arc);
    }

    // Start background task for team switch validation only
    {
      let manager = self.manager.clone();
      let ctx_clone = ctx.clone();

      tokio::spawn(async move {
        use tokio::time::{interval, Duration};
        let mut check_interval = interval(Duration::from_secs(60)); // Check every minute

        loop {
          check_interval.tick().await;

          let mut manager_lock = manager.lock().await;

          // Validate pending team switches (commit if stable for 2+ minutes)
          for server in &mut manager_lock.qguilds {
            for category in &mut server.categories {
              for fmt in &mut category.formats {
                for session in &mut fmt.sessions {
                  if session.pending_team_switch.is_some() {
                    session.validate_and_commit_team_switch(&ctx_clone, server.id);
                  }
                }
              }
            }
          }
        }
      });
    }

    // Spawn console command handler in a separate task
    //let console_handler = command::ConsoleHandler::new(
    //    self.manager.clone(),
    //    self.database.clone(),
    //    Arc::new(ctx.clone()),
    //);

    //tokio::spawn(async move {
    //    console_handler.start_console_loop().await;
    //});

    // Register slash commands globally or for specific guild
    let cmds = vec![
      // Player commands
      cmd!("buffer", "Move a player to the start of the queue").op_user("user", "User to buffer", true),
      cmd!("fatkid", "Move a player to the end of the queue").op_user("user", "User to fatkid", true),
      cmd!("remove", "Remove all players from the queue, or a specific player").op_user("user", "User to remove (optional)", false),
      cmd!("elo", "View ELO and rank information for a player").op_user("user", "The Discord user (mention or ID, optional)", false),
      cmd!("prefs", "Open your preferences"),
      cmd!("config", "Open server settings"),
      cmd!("edit", "Open player menu").op_user("user", "The Discord user to edit", true),
      cmd!("migrate", "Bulk-assign ELO to all members with a role").op_role("role", "The role to migrate", true).op_int("elo", "The ELO value to assign", true),
    ];

    if let Err(why) = Command::set_global_commands(&ctx.http, cmds).await {
      error!("Failed to register commands: {}", why);
    }
  }

  /// When the bot is connected to a new guild
  async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
    let guild_id = guild.id;
    let repo = CategoryRepository::new(self.db.pool().clone());

    // 1. Check existence without unwrapping
    match self.db.guilds.exists(&guild_id).await {
      Ok(false) => {
        let qguild = QGuild::new(guild.id, guild.name.clone(), Roles::empty());
        if let Err(e) = GuildRepository::add(&qguild).await {
          error!("Failed to save new guild {} to database: {}", guild_id, e);
        }
      }
      Err(e) => {
        error!("DB error checking guild existence: {e}");
        return;
      }
      _ => {}
    }

    // 2. Load data BEFORE locking the manager
    let categories = repo.get_categories_for_guild(guild_id).await.unwrap_or_default();

    // 3. Perform external cleanup BEFORE locking (if possible)
    // Note: If cleanup requires the manager, this gets tricky.

    let mut manager = self.manager.lock().await;

    if manager.get_qguild(guild_id).is_err() {
      let mut qguild = QGuild::new(guild_id, guild.name.clone(), Roles::empty());

      for mut category in categories {
        // Clean up orphanned VCs (Consider if this can be moved outside the lock)
        category.clean_orphaned_vcs(&ctx, &self.db).await;

        if let Err(e) = qguild.add_category(category) {
          error!("Failed to add category: {e}");
        }
      }

      let has_categories = qguild.has_categories();
      manager.qguilds.push(qguild);

      if has_categories {
        self.check_existing_voice_users(&ctx, &guild, &mut manager).await;
        self.create_dashboard_from_manager(&ctx, &guild, &mut manager).await;
      }
    }
  }

  /// When an interaction is created
  async fn interaction_create(&self, ctx: Context, pl: Interaction) {
    match pl {
      Interaction::Command(itx) => {
        let cmd_ctx = CommandContext { ctx: &ctx, intax: &itx, db: self.db.clone(), manager: &self.manager.clone() };
        let cd = &itx.data;
        let cdo = &cd.options;

        // Handle commands that don't need a server/category first
        let result = match cd.name.as_str() {
          "prefs" => {
            crate::log::log_command_usage_simple(&ctx, &itx, "prefs", None);
            commands::cmd_prefs(&cmd_ctx).await
          }
          "config" => {
            crate::log::log_command_usage_simple(&ctx, &itx, "config", None);
            commands::cmd_config(&cmd_ctx).await
          }
          "edit" => {
            crate::log::log_command_usage_simple(&ctx, &itx, "edit", None);
            commands::cmd_edit_player(&cmd_ctx).await
          }
          "migrate" => {
            crate::log::log_command_usage_simple(&ctx, &itx, "migrate", None);
            commands::cmd_migrate(&cmd_ctx).await
          }
          _ => {
            // All other commands need a server
            let mut manager = self.manager.lock().await;
            let server = match get_server_with_error(&mut manager, itx.guild_id.unwrap(), &itx, &ctx).await {
              Ok(s) => s,
              Err(_) => return, // Error already handled by helper
            };

            match cd.name.as_str() {
              "buffer" => {
                if let Some(user_id) = extract_user_option(cdo, "user").await {
                  crate::log::log_command_usage(&ctx, &itx, &self.db, "buffer", Some(user_id), None).await;
                  admin::cmd_buffer(&cmd_ctx, server, user_id).await
                } else {
                  crate::log::log_command_usage_simple(&ctx, &itx, "buffer", None);
                  Ok(())
                }
              }
              "fatkid" => {
                if let Some(user_id) = extract_user_option(cdo, "user").await {
                  crate::log::log_command_usage(&ctx, &itx, &self.db, "fatkid", Some(user_id), None).await;
                  admin::cmd_fatkid(&cmd_ctx, server, user_id).await
                } else {
                  crate::log::log_command_usage_simple(&ctx, &itx, "fatkid", None);
                  Ok(())
                }
              }
              "remove" => {
                crate::log::log_command_usage_simple(&ctx, &itx, "remove", None);
                admin::cmd_remove_queue(&cmd_ctx, server, cdo.first()).await
              }
              "elo" => {
                if let Some(user_option) = cdo.first() {
                  if let Some(user_id) = user_option.value.as_user_id() {
                    crate::log::log_command_usage(&ctx, &itx, &self.db, "elo", Some(user_id), None).await;
                    match ctx.http.get_user(user_id).await {
                      Ok(user) => admin::cmd_get_player_elo(&cmd_ctx, Some(user)).await,
                      Err(_) => {
                        let _ = send_error_response(&itx, &ctx, "Failed to get user").await;
                        Ok(())
                      }
                    }
                  } else {
                    crate::log::log_command_usage_simple(&ctx, &itx, "elo", Some("invalid user"));
                    let _ = send_error_response(&itx, &ctx, "Invalid user specified").await;
                    Ok(())
                  }
                } else {
                  let _ = send_error_response(&itx, &ctx, "No user specified").await;
                  Ok(())
                }
              }
              _ => send_error_response(&itx, &ctx, "Unknown command").await.map_err(|e| anyhow::anyhow!(e)),
            }
          }
        };

        if let Err(e) = result {
          error!("Error handling command '{}': {}", itx.data.name, e);
          let _ = send_error_response(&itx, &ctx, "An error occurred while processing your command").await;
        }
      }
      Interaction::Component(ref itx) => {
        debug!("Component interaction received: '{}' from user {}", itx.data.custom_id, itx.user.id);
        let button_type = ButtonType::parse(&itx.data.custom_id);

        if matches!(button_type, ButtonType::ConfirmPermissions) {
          let guild_id = itx.guild_id.unwrap();
          let user_id = itx.user.id;

          // Check if user is an admin
          // Try cache first (fast path)
          let member_opt = if let Some(guild) = ctx.cache.guild(guild_id) { guild.members.get(&user_id).cloned() } else { None };

          // Fallback to HTTP if not in cache
          let member = match member_opt {
            Some(m) => Some(m),
            None => guild_id.member(&ctx.http, user_id).await.ok(),
          };

          let is_admin = match member {
            Some(member) => {
              match self.db.config.get_admin_role_id(guild_id).await {
                Ok(Some(admin_role)) => member.roles.contains(&admin_role),
                _ => {
                  // If no admin role configured, check for server Administrator permission
                  if let Some(guild_ref) = guild_id.to_guild_cached(&ctx.cache) {
                    let perms = guild_ref.member_permissions(&member);
                    perms.contains(serenity::all::Permissions::ADMINISTRATOR)
                  } else {
                    false
                  }
                }
              }
            }
            None => false,
          };

          if !is_admin {
            InteractionHelpers::send_component_error_embed(itx, &ctx, "Only administrators can confirm bot permissions.").await;
            return;
          }

          // Clone the guild data to avoid Send issues
          let guild = match guild_id.to_guild_cached(&ctx.cache) {
            Some(g) => g.clone(),
            None => {
              error!("Failed to get guild from cache");
              return;
            }
          };

          // Re-check permissions
          let (has_perms, missing_perms) = self.check_perms(&ctx, &guild).await;

          if !has_perms {
            // Still missing permissions
            InteractionHelpers::send_component_error_embed(itx, &ctx, &format!("Still missing permissions: {missing_perms}")).await;
          } else {
            // Permissions granted! Delete the warning message and create dashboard
            if let Err(e) = itx.message.delete(&ctx.http).await {
              error!("Failed to delete permission warning: {e}");
            }

            let success_response = serenity::all::CreateInteractionResponse::Message(
              serenity::all::CreateInteractionResponseMessage::new().content("Permissions confirmed! Setting up dashboard...").ephemeral(true),
            );
            if let Err(e) = itx.create_response(&ctx.http, success_response).await {
              error!("Failed to send success response: {e}");
            }

            // Now create the dashboard
            let mut manager = self.manager.lock().await;
            self.create_dashboard_from_manager(&ctx, &guild, &mut manager).await;
          }
          return;
        }

        if matches!(button_type, ButtonType::CreateRankRolesYes | ButtonType::CreateRankRolesNo) {
          let create = matches!(button_type, ButtonType::CreateRankRolesYes);
          let result = admin::handle_create_rank_roles(&ctx, &self.db, itx, create).await;
          if let Err(e) = result {
            error!("Error handling rank role creation: {e}");
          }
          return;
        }

        if button_type.is_setup_button() {
          let result = admin::handle_setup_interaction(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling setup interaction: {e}");
          }
          return;
        }

        // Handle settings buttons (user settings)
        if itx.data.custom_id.starts_with("settings_") {
          let result = crate::handlers::handle_settings_button(&ctx, itx, &self.db).await;
          if let Err(e) = result {
            error!("Error handling settings interaction: {e}");
          }
          return;
        }

        // Handle category settings select menu
        if itx.data.custom_id == "category_settings_select" {
          let result = crate::handlers::handle_category_settings_select(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling category settings select: {e}");
          }
          return;
        }

        // Handle server-level team balance method select (must be before server_settings_ prefix)
        if itx.data.custom_id == "server_settings_balance" {
          let result = crate::handlers::handle_server_settings_balance_select(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling server settings balance select: {e}");
          }
          return;
        }

        // Handle server settings buttons (including link channel flow)
        if itx.data.custom_id.starts_with("server_settings_") || itx.data.custom_id.starts_with("server_cfg_") || itx.data.custom_id.starts_with("link_ch_") {
          let result = crate::handlers::handle_server_settings_button(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling server settings interaction: {e}");
          }
          return;
        }

        // Handle player settings rank selection
        if itx.data.custom_id.starts_with("player_settings_rank_select_") {
          let result = crate::handlers::handle_player_settings_rank_select(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling player settings rank select: {e}");
          }
          return;
        }

        // Handle category settings buttons (including link message, format, and elo gate buttons)
        if itx.data.custom_id.starts_with("category_settings_")
          || itx.data.custom_id.starts_with("category_link_msg_")
          || itx.data.custom_id.starts_with("category_fmt_")
          || itx.data.custom_id.starts_with("elo_gate_")
        {
          let result = crate::handlers::handle_category_settings_button(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling category settings interaction: {e}");
          }
          return;
        }

        // Handle ELO change confirmation buttons
        if itx.data.custom_id.starts_with("confirm_elo_change_") || itx.data.custom_id.starts_with("cancel_elo_change_") {
          let result = crate::handlers::handle_elo_change_confirmation(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling ELO change confirmation: {e}");
          }
          return;
        }

        // Handle disable DM notifications button
        if itx.data.custom_id == "disable_dm_notifications" {
          let user_id = itx.user.id;
          match self.db.players.set_pm_hot_alert(user_id, false).await {
            Ok(_) => {
              let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                  .embed(
                    serenity::all::CreateEmbed::new()
                      .title("DM Notifications Disabled")
                      .description("You will no longer receive direct messages when a game is ready.\n\nYou can re-enable this in your settings using `/prefs`.")
                      .color(0x00FF00),
                  )
                  .components(vec![]),
              );
              if let Err(e) = itx.create_response(&ctx.http, response).await {
                error!("Failed to send disable DM response: {e}");
              }
              info!("User {} disabled DM notifications", user_id);
            }
            Err(e) => {
              error!("Failed to disable DM notifications for user {}: {}", user_id, e);
              let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new().content("Failed to disable DM notifications. Please try again later.").ephemeral(true),
              );
              let _ = itx.create_response(&ctx.http, response).await;
            }
          }
          return;
        }

        // Handle player settings buttons
        if itx.data.custom_id.starts_with("player_settings_") {
          let result = crate::handlers::handle_player_settings_button(&ctx, itx, &self.db).await;
          if let Err(e) = result {
            error!("Error handling player settings interaction: {e}");
          }
          return;
        }

        // Handle remove all action (must be before general runner_action_ check)
        if itx.data.custom_id == "runner_action_remove_all" {
          info!("Runner action 'remove_all' triggered by user {}", itx.user.id);
          let result = crate::handlers::runner_menu::handle_remove_all(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling remove all action: {e}");
          }
          return;
        }

        // Handle runner menu actions
        if itx.data.custom_id.starts_with("runner_action_") {
          let action = itx.data.custom_id.strip_prefix("runner_action_").unwrap_or("");
          info!("Runner action '{}' triggered by {}", action, itx.user.tag());
          let result = crate::handlers::runner_menu::handle_runner_action(&ctx, itx, &self.db, &self.manager, action).await;
          if let Err(e) = result {
            error!("Error handling runner action '{}': {e}", action);
          }
          return;
        }

        // Handle runner player selection (buttons or select menu)
        if itx.data.custom_id.starts_with("runner_player_") {
          let parts: Vec<&str> = itx.data.custom_id.split('_').collect();
          let action = parts.get(2).unwrap_or(&"");

          // Extract user_id from button click or select menu
          let user_id_str = if let serenity::all::ComponentInteractionDataKind::Button = itx.data.kind {
            // Button variant: custom_id is "runner_player_ACTION_USERID"
            parts.get(3).unwrap_or(&"")
          } else if let serenity::all::ComponentInteractionDataKind::StringSelect { values } = &itx.data.kind {
            // Select menu variant: value is the user_id
            values.first().map(|s| s.as_str()).unwrap_or("")
          } else {
            ""
          };

          if !user_id_str.is_empty() {
            let result = crate::handlers::runner_menu::handle_player_selection(&ctx, itx, &self.db, &self.manager, action, user_id_str).await;
            if let Err(e) = result {
              error!("Error handling runner player selection for action '{}': {e}", action);
            }
          }
          return;
        }

        // Handle runner menu back button
        if itx.data.custom_id == "runner_menu_back" {
          let cc = crate::models::ComponentContext { ctx: &ctx, component: itx, db: self.db.clone(), manager: &self.manager };
          let result = crate::handlers::runner_menu::update_runner_menu(&cc).await;
          if let Err(e) = result {
            error!("Error updating runner menu: {e}");
          }
          return;
        }

        // Handle dashboard back button (from help screen)
        if itx.data.custom_id == "dashboard_back" {
          // Dismiss the ephemeral message by updating it to be empty
          let response = serenity::all::CreateInteractionResponse::UpdateMessage(serenity::all::CreateInteractionResponseMessage::new().components(vec![]));
          if let Err(e) = itx.create_response(&ctx.http, response).await {
            error!("Error dismissing help message: {e}");
          }
          return;
        }

        // Handle score result buttons (RED WON/DRAW/BLU WON)
        if itx.data.custom_id.starts_with("score_red_") || itx.data.custom_id.starts_with("score_draw_") || itx.data.custom_id.starts_with("score_blu_") {
          let result = self.handle_score_button(&ctx, itx).await;
          if let Err(e) = result {
            error!("Error handling score button: {e}");
          }
          return;
        }

        // Handle runner menu end match buttons (runner_end_red/draw/blu_{category_id}_{format_id})
        if itx.data.custom_id.starts_with("runner_end_") {
          let result = crate::handlers::runner_menu::handle_end_match_result(&ctx, itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling runner end match: {e}");
          }
          return;
        }

        // Handle ping format selection buttons (ping_format_{category_id}_{format_id})
        if itx.data.custom_id.starts_with("ping_format_") {
          let parts: Vec<&str> = itx.data.custom_id.split('_').collect();
          if let (Some(cat_id), Some(fmt_id)) = (parts.get(2).and_then(|s| s.parse::<u8>().ok()), parts.get(3).and_then(|s| s.parse::<u8>().ok())) {
            let guild_id = itx.guild_id.unwrap();
            let mut manager = self.manager.lock().await;
            if let Ok(server) = manager.get_qguild(guild_id) {
              if let Some(category) = server.categories.iter_mut().find(|c| c.id == cat_id) {
                let cc = crate::models::ComponentContext { ctx: &ctx, component: itx, db: self.db.clone(), manager: &self.manager };
                if let Err(e) = category.handle_ping_format(&cc, fmt_id).await {
                  error!("Error handling ping format: {e}");
                }
              }
            }
          }
          return;
        }

        let mut manager = self.manager.lock().await;
        let guild_id = itx.guild_id.unwrap();
        let channel_id = itx.channel_id;

        let category = match manager.get_category_by_channel(guild_id, channel_id) {
          Ok(category) => category,
          Err(_) => {
            // Category not in manager - try to recover from database
            let guild_name = guild_name(&ctx, guild_id);
            let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{channel_id}"));
            info!("[{}] Category not found in manager for #{}, attempting recovery from database", guild_name, channel_name);

            // Get the message ID from the interaction
            let message_id = itx.message.id;
            let guild_id_u64 = guild_id;
            let channel_id_u64 = channel_id.get();
            let message_id_u64 = message_id.get();

            // Load categories from database for this guild
            let category_repo = CategoryRepository::new(self.db.pool().clone());
            match category_repo.get_categories_for_guild(guild_id_u64).await {
              Ok(categories) => {
                // Find the category that matches this dashboard channel
                if let Some(mut recovered_category) = categories.into_iter().find(|g| g.channels.dashboard.get() == channel_id_u64) {
                  info!("[{}] Found category in database for #{}", guild_name, channel_name);

                  // Update the dashboard message ID in the database
                  if let Err(e) = category_repo.update_dashboard_msg(guild_id_u64, channel_id_u64, message_id_u64).await {
                    error!("[{}] Failed to update dashboard message ID: {}", guild_name, e);
                  } else {
                    info!("[{}] Updated dashboard message ID in database", guild_name);
                    // Update the in-memory category too
                    recovered_category.dashboard_msg = message_id;
                  }

                  // Add the recovered category to the manager
                  let server = manager.get_qguild(guild_id);
                  if let Ok(server) = server {
                    if let Err(e) = server.add_category(recovered_category) {
                      error!("[{}] Failed to add recovered category: {}", guild_name, e);
                    } else {
                      info!("[{}] Recovered category added to manager", guild_name);
                    }

                    // Now get the category from the manager
                    manager.get_category_by_channel(guild_id, channel_id).unwrap()
                  } else {
                    error!("[{}] Could not get server from manager", guild_name);
                    InteractionHelpers::send_component_error_embed(itx, &ctx, "Dashboard state was lost.").await;
                    return;
                  }
                } else {
                  error!("[{}] No category found in database for #{}", guild_name, channel_name);
                  InteractionHelpers::send_component_error_embed(itx, &ctx, "Dashboard configuration not found.").await;
                  return;
                }
              }
              Err(e) => {
                error!("Failed to load categories from database: {e}");
                InteractionHelpers::send_component_error_embed(itx, &ctx, "Failed to access database. Please contact an administrator.").await;
                return;
              }
            }
          }
        };

        let comp_ctx = ComponentContext { ctx: &ctx, component: itx, db: self.db.clone(), manager: &self.manager };

        let _button_id = &itx.data.custom_id;
        let _user_id = itx.user.id;

        debug!("Handling button interaction: '{}' | User: {} | Message: {} | Token: {:?}", itx.data.custom_id, itx.user.id, itx.message.id, itx.token);

        let result = category.dash_handle_button_interaction(&comp_ctx).await;

        if let Err(e) = result {
          error!(
            "Error handling button '{}': {} | User: {} | Guild: {} | Message: {} | Token: {:?}",
            itx.data.custom_id,
            e,
            itx.user.id,
            itx.guild_id.unwrap_or_default(),
            itx.message.id,
            itx.token
          );
          if is_interaction_valid(&pl) {
            InteractionHelpers::send_component_error_embed(itx, &ctx, "An error occurred while processing your button click").await;
          } else {
            error!("Interaction no longer valid for button '{}'", itx.data.custom_id);
          }
        } else {
          debug!("Successfully handled button '{}': User: {} | Message: {}", itx.data.custom_id, itx.user.id, itx.message.id);
        }
      }
      Interaction::Modal(itx) => {
        // Handle modal submissions for user settings
        if itx.data.custom_id.starts_with("settings_modal_") {
          let result = crate::handlers::handle_settings_modal(&ctx, &itx, &self.db).await;
          if let Err(e) = result {
            error!("Error handling settings modal '{}': {}", itx.data.custom_id, e);
          }
        }
        // Handle modal submissions for server settings
        if itx.data.custom_id.starts_with("server_settings_modal_")
          || itx.data.custom_id.starts_with("server_settings_rank_modal_")
          || itx.data.custom_id.starts_with("server_settings_category_modal_")
        {
          let result = crate::handlers::handle_server_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling server settings modal '{}': {}", itx.data.custom_id, e);
          }
        }
        // Handle modal submissions for category settings (including format modals)
        if itx.data.custom_id.starts_with("category_settings_modal_") || itx.data.custom_id.starts_with("category_fmt_modal_") {
          let result = crate::handlers::handle_category_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling category settings modal '{}': {}", itx.data.custom_id, e);
          }
        }
        // Handle modal submissions for linking dashboard message
        if itx.data.custom_id.starts_with("category_link_msg_modal_") {
          let result = crate::handlers::handle_category_link_msg_modal(&ctx, &itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling category link message modal '{}': {}", itx.data.custom_id, e);
          }
        }
        // Handle modal submissions for player settings
        if itx.data.custom_id.starts_with("player_settings_modal_") {
          let result = crate::handlers::handle_player_settings_modal(&ctx, &itx, &self.db, &self.manager).await;
          if let Err(e) = result {
            error!("Error handling player settings modal '{}': {}", itx.data.custom_id, e);
          }
        }
        // Handle modal submissions for score reporting
        if itx.data.custom_id.starts_with("report_score_modal") {
          let result = self.handle_report_score_modal(&ctx, &itx).await;
          if let Err(e) = result {
            error!("Error handling report score modal: {}", e);
          }
        }
      }
      _ => {
        // Other interaction types not handled yet
      }
    }
  }

  /// When a user joins or leaves a voice channel
  async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
    let state = VoiceStateUpdate::get(&old, &new);
    let user_id = new.user_id;
    let _user = match ctx.http.get_user(user_id).await {
      Ok(u) => u,
      Err(e) => {
        error!("Failed to get user {}: {}", user_id, e);
        return;
      }
    };

    let guild_id = match new.guild_id {
      Some(s) => s,
      None => {
        return;
      }
    };

    // Get player tag from database (primary source)
    let tag = match self.db.get_player(user_id, &ctx).await {
      Ok(player) => {
        if !player.tag.is_empty() {
          player.tag
        } else {
          // Fallback to Discord API tag (not display name)
          ctx.http.get_user(user_id).await.map(|user| user.tag()).unwrap_or_else(|e| {
            error!("Failed to get user {} from Discord API: {}, using user ID as fallback", user_id, e);
            user_id.to_string()
          })
        }
      }
      Err(_) => {
        // Fallback to Discord API tag (not display name)
        ctx.http.get_user(user_id).await.map(|user| user.tag()).unwrap_or_else(|e| {
          error!("Failed to get user {} from Discord API: {}, using user ID as fallback", user_id, e);
          user_id.to_string()
        })
      }
    };

    // First manager lock scope
    let left_team_vc = {
      let mut manager = self.manager.lock().await;

      // Determine which channel to use for category lookup based on state
      // For disconnects/moves, use old channel; for connects, use new channel
      let lookup_channel = match state {
        VoiceStateUpdate::Disconnected | VoiceStateUpdate::Moved => match &old {
          Some(s) => match s.channel_id {
            Some(ch) => ch,
            None => return,
          },
          None => return,
        },
        VoiceStateUpdate::Connected => match new.channel_id {
          Some(ch) => ch,
          None => return,
        },
        VoiceStateUpdate::Reconnected => return,
      };

      let category = match manager.get_category_by_channel(guild_id, lookup_channel) {
        Ok(g) => g,
        Err(_) => return,
      };

      match state {
        VoiceStateUpdate::Disconnected => {
          let was_team_vc = category.is_team_vc(lookup_channel);
          self.handle_player_leave_vc(&ctx, category, guild_id, user_id, &tag).await;
          category.check_team_vc_cleanup_on_leave(&ctx).await;
          category.queue_dash_update(&ctx, guild_id).await;
          was_team_vc
        }
        VoiceStateUpdate::Connected => {
          if category.get_inactives().is_empty() {
            if let Err(e) = category.create_session() {
              warn!("Failed to create session on VC connect: {e}");
            }
          }
          false
        }
        VoiceStateUpdate::Moved => {
          let was_team_vc = category.is_team_vc(lookup_channel);

          // Check if player moved between team VCs during a live game
          if let Some(new_channel) = new.channel_id {
            if category.is_team_vc(new_channel) && was_team_vc {
              // Player moved from one team VC to another - check for team switch
              for fmt in &mut category.formats {
                for session in &mut fmt.sessions {
                  if session.status == SessionStatus::Live {
                    session.detect_team_switch(&ctx, guild_id);
                  }
                }
              }
            }

            // If moving from queue VC to team VC, don't remove from queue
            // (they were just moved by the bot for a match)
            if category.channels.queue_vc == lookup_channel && category.is_team_vc(new_channel) {
              // Player moved from queue VC to team VC - this is expected, don't remove from queue
              return;
            }
          }

          if category.channels.queue_vc == lookup_channel {
            self.handle_player_leave_vc(&ctx, category, guild_id, user_id, &tag).await;
            category.queue_dash_update(&ctx, guild_id).await;
          }
          was_team_vc
        }
        VoiceStateUpdate::Reconnected => {
          return;
        }
      }
    }; // Release first manager lock here

    // If a player left a team VC, check if all team VCs are now empty
    // and auto-end the game if so. Requires a separate lock scope since
    // pull needs the manager Arc.
    if left_team_vc {
      let mut manager = self.manager.lock().await;
      if let Ok(category) = manager.get_category_by_channel(guild_id, {
        match &old {
          Some(s) => s.channel_id.unwrap(),
          None => return,
        }
      }) {
        category.check_team_vc_empty_auto_end(&ctx, guild_id, &self.db, Some(self.manager.clone())).await;
      }
    }

    // Only process joining logic if player is joining a channel (not disconnecting)
    if new.channel_id.is_none() {
      return;
    }

    // Note: Join logging is done inside the queue VC check below to avoid logging unrelated channel joins

    // Get player data
    let _player = match self.db.get_player(user_id, &ctx).await {
      Ok(user) => user,
      Err(_) => match self.db.new_player(user_id, &ctx).await {
        Ok(new_user) => new_user,
        Err(e) => {
          error!("Failed to create new user: {e}");
          return;
        }
      },
    };

    // Mutex scope
    {
      let mut manager = self.manager.lock().await;

      // Find the guild by ID and check if the new channel is a queue voice channel in any category
      match manager.get_category_by_channel(guild_id, new.channel_id.unwrap()) {
        Ok(category) => {
          if category.channels.queue_vc == new.channel_id.unwrap() {
            // Check if player is already in any session and mark them as in VC
            if let Ok(session) = category.get_user_sesh(user_id).await {
              let was_hot = session.is_hot();
              if let Some(player) = session.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                let was_missing = !player.in_vc;
                player.vc_on();

                // Clear any VC leave grace period since they rejoined
                if player.vc_leave_grace_until.is_some() {
                  info!("Player {} rejoined queue VC, clearing grace period", tag);
                  player.vc_leave_grace_until = None;
                }

                // Cancel expiration since player is now in VC
                for format in &category.formats {
                  category.cancel_player_rejoin_expiration(&ctx, guild_id, format.id, user_id).await;
                }

                // Update dashboard if player was missing in a hot session
                // This removes them from the "Missing players" list
                if was_hot && was_missing {
                  info!("{} joined VC during hot session, updating dashboard", tag);
                  category.on_player_joined_vc(&ctx, user_id).await;
                  category.queue_dash_update(&ctx, guild_id).await;
                }
              }
            } else {
              // Player not in session yet - check if they want auto-queue
              let user_prefs = self.db.players.get_prefs(user_id).await.unwrap_or_default();
              if !user_prefs.vc_auto_join {
                // User has disabled VC auto-queue, log that they joined VC but didn't join queue
                let guild_name = guild_name(&ctx, guild_id);
                let category_name = category.name.as_deref().unwrap_or("Unknown");
                info!("{} {} joined VC (spec)", log_prefix_category(&guild_name, category_name), tag);
                return;
              }

              // Ensure a session exists before trying to add player
              if category.get_inactives().is_empty() {
                warn!("No idle sessions present when player {} joined VC, creating one", tag);
                if let Err(e) = category.create_session() {
                  error!("Failed to create session for player {}: {}", tag, e);
                  let guild_name = guild_name(&ctx, guild_id);
                  let category_name = category.name.as_deref().unwrap_or("Unknown");
                  error!("{} {} joined VC but could not be added to queue (failed to create session)", log_prefix_category(&guild_name, category_name), tag);
                  return;
                }
              }

              // Now we're guaranteed to have a session
              {
                use crate::handlers::player::resolve_player_for_queue;

                let (player, discord_rank, rank_mismatch) = match resolve_player_for_queue(&ctx, &self.db, guild_id, user_id).await {
                  Ok(result) => result,
                  Err(e) => {
                    error!("Failed to resolve player for queue: {e}");
                    return;
                  }
                };

                // Use queue_player_with_vc_status to set in_queue_vc BEFORE quota check/notification
                let queue_ctx = QueueContext { ctx: &ctx, guild_id: Some(guild_id), db: Some(&self.db), manager: Some(self.manager.clone()) };
                if let Err(e) = category.queue_player_fmt(player.clone(), discord_rank, queue_ctx, true).await {
                  error!("Failed to add player to queue: {e}");
                } else {
                  let guild_name = guild_name(&ctx, guild_id);
                  let category_name = category.name.as_deref().unwrap_or("Unknown");
                  let pool_len: usize = category.formats[0].sessions.iter().map(|s| s.pool.len()).sum();
                  let _fmt_name = category.formats.first().map(|fmt| fmt.name.as_str());

                  // Check if queue was already full when this player joined
                  if pool_len > category.quota() as usize {
                    warn!(
                      "{} {} joined VC and was added to queue, but queue exceeded quota ({} > {})",
                      log_prefix_category(&guild_name, category_name),
                      tag,
                      pool_len,
                      category.quota()
                    );
                  }

                  let _format = &category.formats[0];
                  if let Err(e) = log_queue_toggle(&ctx, &self.db, guild_id, category.id, &category.formats[0], &player, "joined", rank_mismatch).await {
                    warn!("Failed to log queue toggle: {e}");
                  }
                }

                category.queue_dash_update(&ctx, guild_id).await;
              }
            }

            // Get post-game confirm time window from database
            let post_game_confirm_time = self.db.config.get_post_game_confirm_time(guild_id).await.ok();

            if category.check_hot_confirm_time(&ctx, guild_id, post_game_confirm_time).await {
              info!("Hot session confirm time detected, updating dashboard");
              category.queue_dash_update(&ctx, guild_id).await;
            }
          }
        }
        Err(_) => {
          // Silently ignore - not a queue channel (expected for non-queue VCs)
        }
      }
    }
  }
}

impl Handler {
  /// Handle a player leaving the queue VC (disconnect or move away).
  /// Checks auto-leave preference, removes or resets queue expiration time, and regenerates teams if needed.
  async fn handle_player_leave_vc(&self, ctx: &Context, category: &mut Category, guild_id: GuildId, user_id: UserId, _tag: &str) {
    let queue_ctx = QueueContext { ctx, guild_id: Some(guild_id), db: Some(&self.db), manager: Some(self.manager.clone()) };
    let guild_name = guild_name(ctx, guild_id);
    let ctg_nm = category.name.as_deref().unwrap_or("Unknown").to_string();
    let fmt_nm = category.get_user_fmt_name(user_id);
    let quota = category.quota() as usize;
    let player = category.get_player(user_id).unwrap();

    let category_id = category.id; // Capture category_id before mutable borrow

    // Extract team channel IDs before any mutable borrows
    let team_channel_ids: Vec<_> = category.channels.teams.iter().flat_map(|t| vec![t.red_vc, t.blu_vc]).collect();

    // Check if player is currently in a team channel before getting session
    let is_in_team_vc = if let Some(guild) = ctx.cache.guild(guild_id) {
      if let Some(voice_state) = guild.voice_states.get(&user_id) {
        if let Some(channel_id) = voice_state.channel_id {
          team_channel_ids.contains(&channel_id)
        } else {
          false
        }
      } else {
        false
      }
    } else {
      false
    };

    let (should_regenerate, should_remove_player, should_schedule_queue_expiration) = if let Ok(sesh) = category.get_user_sesh(user_id).await {
      if sesh.is_active() {
        // Player is in an active session (Push/Live) - bot is moving them, not a voluntary leave
        return;
      }

      let was_hot = sesh.is_hot();

      let should_remove_player = {
        if is_in_team_vc {
          // Player is in a team channel, not the queue VC - don't apply post-game grace
          false
        } else {
          // Check if this is a post-game scenario (match_ended_at is set after pull)
          let is_post_game = sesh.match_ended_at.is_some();

          if is_post_game {
            // Post-game behavior: give 10-second grace period for position < quota
            if let Some(position) = sesh.pool.iter().position(|p| p.player.user_id == user_id) {
              if position < quota {
                // Player has position < quota, give them 10 second grace period
                if let Some(session_player) = sesh.pool.iter_mut().find(|p| p.player.user_id == user_id) {
                  let grace_until = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
                  session_player.vc_leave_grace_until = Some(grace_until);
                  info!("{} #{} {} left VC post-game, 10s grace period started", log_prefix_format(&guild_name, &ctg_nm, &fmt_nm), position + 1, player.tag);
                }
                false // Don't remove immediately
              } else {
                // Position >= quota, remove immediately using post_game_auto_leave setting
                self.db.config.get_bool(guild_id, "post_game_auto_leave", true).await.unwrap_or(true)
              }
            } else {
              // Player not found in pool, use post_game_auto_leave setting
              self.db.config.get_bool(guild_id, "post_game_auto_leave", true).await.unwrap_or(true)
            }
          } else if sesh.is_hot() {
            // Hot game behavior: check user's vc_auto_leave preference
            if let Ok(settings) = self.db.players.get_prefs(user_id).await {
              settings.vc_auto_leave
            } else {
              false
            }
          } else {
            // Regular idle session: don't auto-remove
            false
          }
        }
      };

      // Track if we need to schedule rejoin expiration (player left VC but in queue)
      let should_schedule_rejoin_expiration = if !should_remove_player {
        if let Some(player) = sesh.pool.iter_mut().find(|p| p.player.user_id == user_id) {
          player.joined_at = std::time::SystemTime::now();
          player.in_vc = false;
          true // player left VC but in queue - schedule expiration
        } else {
          false
        }
      } else {
        false
      };

      // Capture position before removal for logging
      let _position_before_removal = sesh.pool.iter().position(|p| p.player.user_id == user_id).map(|p| p + 1);

      if should_remove_player {
        sesh.remove_player(user_id);
      }

      // Capture pool length and determine regeneration before dropping sesh borrow
      let pool_len = sesh.pool.len();
      let should_idle = was_hot && pool_len < quota;
      if should_idle {
        sesh.idle();
      }

      // Clone format after removal so log gets updated pool count
      let format = category.formats[0].clone();

      // Log after removal so pool count is accurate, but use position before removal
      // Resolve player for logging
      if let Ok(player) = self.db.get_player(user_id, ctx).await {
        if let Err(e) = log_queue_toggle(ctx, &self.db, guild_id, category_id, &format, &player, "left", None).await {
          warn!("Failed to log queue toggle: {e}");
        }
      }

      (was_hot && pool_len >= quota, should_remove_player, should_schedule_rejoin_expiration)
    } else {
      // Player not found in any session
      let format = category.formats[0].clone();

      // Resolve player for logging
      if let Ok(player) = self.db.get_player(user_id, ctx).await {
        if let Err(e) = log_queue_toggle(ctx, &self.db, guild_id, category_id, &format, &player, "left", None).await {
          warn!("Failed to log queue toggle: {e}");
        }
      }
      (false, false, false)
    };

    // Cancel expiration if player was removed, or schedule new one if they left VC but stayed in queue
    if should_remove_player {
      for format in &category.formats {
        category.cancel_player_rejoin_expiration(ctx, guild_id, format.id, user_id).await;
      }
    } else if should_schedule_queue_expiration {
      // Player left VC but is still in queue - schedule expiration
      let duration = queue_ctx.db.unwrap().players.get_prefs(user_id).await.unwrap().queue_expiration;
      category.set_player_rejoin_expiration(ctx, guild_id, player, duration).await;
    }

    if should_regenerate {
      category.generate_teams(ctx, guild_id, Some(&self.db)).await;
    }
  }

  /// Handle score result button click (RED WON/DRAW/BLU WON)
  async fn handle_score_button(&self, ctx: &Context, interaction: &serenity::all::ComponentInteraction) -> Result<(), anyhow::Error> {
    use serenity::all::{CreateInteractionResponse, CreateInteractionResponseMessage, EditMessage};

    // Parse result and IDs from custom_id (format: score_{result}_{category_id}_{format_id})
    let custom_id = &interaction.data.custom_id;
    let parts: Vec<&str> = custom_id.split('_').collect();
    let result = parts.get(1).unwrap_or(&"");
    let category_id = parts.get(2).and_then(|s| s.parse::<i64>().ok());
    let _format_id = parts.get(3).and_then(|s| s.parse::<i64>().ok());
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

    // Validate result
    if !matches!(*result, "red" | "draw" | "blu") {
      return Ok(());
    }

    // Check if result was already reported and update database atomically
    let mut already_reported = false;
    if let Some(cat_id) = category_id {
      let mut mgr = self.manager.lock().await;
      if let Ok(server) = mgr.get_qguild(guild_id) {
        if let Some(category) = server.categories.iter_mut().find(|c| c.id == cat_id as u8) {
          if let Some(session) = category.formats.iter_mut().flat_map(|f| &mut f.sessions).find(|s| matches!(s.status, crate::models::SessionStatus::Pull)) {
            if session.score_reported {
              already_reported = true;
            } else {
              session.score_reported = true;
              drop(mgr);
              if let Some(match_id) = self.db.matches.get_latest_match_id(guild_id, cat_id).await? {
                if let Err(e) = self.db.matches.update_match_result(match_id, result).await {
                  error!("Failed to update match result in database: {e}");
                }
              }
            }
          }
        }
      }
    }

    if already_reported {
      let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Result has already been reported for this match.").ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }

    // Update the message embed with result indicator
    let message = &interaction.message;
    if let Some(mut embed) = message.embeds.first().cloned() {
      // Add result to team headers
      if embed.fields.len() >= 2 {
        for field in &mut embed.fields {
          if field.name.contains("🔵 BLU") {
            let indicator = match *result {
              "blu" => " ✓",
              "draw" => " ―",
              _ => "",
            };
            field.name = format!("{}{}", field.name.trim(), indicator);
          } else if field.name.contains("🔴 RED") {
            let indicator = match *result {
              "red" => " ✓",
              "draw" => " ―",
              _ => "",
            };
            field.name = format!("{}{}", field.name.trim(), indicator);
          }
        }
      }

      // Update message and remove buttons
      message.channel_id.edit_message(&ctx.http, message.id, EditMessage::new().embed(embed.into()).components(Vec::new())).await?;

      let result_text = match *result {
        "red" => "RED team victory",
        "draw" => "Draw",
        "blu" => "BLU team victory",
        _ => "Result",
      };

      let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("{} recorded!", result_text)).ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
    }

    Ok(())
  }

  /// Handle report score modal submission
  async fn handle_report_score_modal(&self, ctx: &Context, interaction: &serenity::all::ModalInteraction) -> Result<(), anyhow::Error> {
    use serenity::all::{CreateInteractionResponse, CreateInteractionResponseMessage, EditMessage};

    // Parse category_id and format_id from modal custom_id (format: report_score_modal_CATID_FMTID)
    let custom_id = &interaction.data.custom_id;
    let parts: Vec<&str> = custom_id.split('_').collect();
    let category_id = parts.get(3).and_then(|s| s.parse::<i64>().ok());
    let _format_id = parts.get(4).and_then(|s| s.parse::<i64>().ok());
    let guild_id = interaction.guild_id.ok_or_else(|| anyhow::anyhow!("Guild ID not found"))?;

    // Extract scores from modal
    let mut blu_score = String::new();
    let mut red_score = String::new();

    for row in &interaction.data.components {
      if let Some(serenity::all::ActionRowComponent::InputText(input)) = row.components.first() {
        match input.custom_id.as_str() {
          "blu_score" => blu_score = input.value.clone().unwrap_or_default(),
          "red_score" => red_score = input.value.clone().unwrap_or_default(),
          _ => {}
        }
      }
    }

    // Validate scores are numbers and within range
    let blu_score_num: u8 = match blu_score.parse() {
      Ok(n) if n <= crate::models::constants::MAX_MATCH_SCORE => n,
      Ok(_) => {
        let response = CreateInteractionResponse::Message(
          CreateInteractionResponseMessage::new()
            .content(format!("Invalid blue team score. Please enter a number between 0-{}.", crate::models::constants::MAX_MATCH_SCORE))
            .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
      Err(_) => {
        let response = CreateInteractionResponse::Message(
          CreateInteractionResponseMessage::new()
            .content(format!("Invalid blue team score. Please enter a number between 0-{}.", crate::models::constants::MAX_MATCH_SCORE))
            .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
    };

    let red_score_num: u8 = match red_score.parse() {
      Ok(n) if n <= crate::models::constants::MAX_MATCH_SCORE => n,
      Ok(_) => {
        let response = CreateInteractionResponse::Message(
          CreateInteractionResponseMessage::new()
            .content(format!("Invalid red team score. Please enter a number between 0-{}.", crate::models::constants::MAX_MATCH_SCORE))
            .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
      Err(_) => {
        let response = CreateInteractionResponse::Message(
          CreateInteractionResponseMessage::new()
            .content(format!("Invalid red team score. Please enter a number between 0-{}.", crate::models::constants::MAX_MATCH_SCORE))
            .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        return Ok(());
      }
    };

    // Derive result from scores
    let result = if red_score_num > blu_score_num {
      "red"
    } else if blu_score_num > red_score_num {
      "blu"
    } else {
      "draw"
    };

    // Check if score was already reported and update database atomically within the lock
    let mut score_already_reported = false;
    if let Some(cat_id) = category_id {
      let mut mgr = self.manager.lock().await;
      if let Ok(server) = mgr.get_qguild(guild_id) {
        if let Some(category) = server.categories.iter_mut().find(|c| c.id == cat_id as u8) {
          // Find the session that just ended (Pull status)
          if let Some(session) = category.formats.iter_mut().flat_map(|f| &mut f.sessions).find(|s| matches!(s.status, crate::models::SessionStatus::Pull)) {
            if session.score_reported {
              score_already_reported = true;
            } else {
              session.score_reported = true;

              // Update database immediately while holding the lock to prevent race condition
              drop(mgr);
              if let Some(match_id) = self.db.matches.get_latest_match_id(guild_id, cat_id).await? {
                if let Err(e) = self.db.matches.update_match_result(match_id, result).await {
                  error!("Failed to update match result in database: {e}");
                }
              }
            }
          }
        }
      }
    }

    if score_already_reported {
      let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Score has already been reported for this match.").ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
      return Ok(());
    }

    // Update the message embed with scores in team headers
    if let Some(message) = &interaction.message {
      if let Some(mut embed) = message.embeds.first().cloned() {
        // Update team field headers to include scores
        // Find and update the BLU and RED team fields
        if embed.fields.len() >= 2 {
          for field in &mut embed.fields {
            if field.name.contains("🔵 BLU") {
              // Extract ELO from existing header (format: "‹**elo**› 🔵 BLU")
              let elo_part = field.name.split("›").next().unwrap_or("‹**0**");
              field.name = format!("{} - **{}**", field.name.trim(), blu_score_num);
            } else if field.name.contains("🔴 RED") {
              // Extract ELO from existing header
              let elo_part = field.name.split("›").next().unwrap_or("‹**0**");
              field.name = format!("{} - **{}**", field.name.trim(), red_score_num);
            }
          }
        }

        // Update the message and remove the Report Score button
        message.channel_id.edit_message(&ctx.http, message.id, EditMessage::new().embed(embed.into()).components(Vec::new())).await?;

        // Send confirmation
        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Score reported successfully!").ephemeral(true));
        interaction.create_response(&ctx.http, response).await?;
      } else {
        let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to update score: no embed found.").ephemeral(true));
        interaction.create_response(&ctx.http, response).await?;
      }
    } else {
      let response = CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to update score: message not found.").ephemeral(true));
      interaction.create_response(&ctx.http, response).await?;
    }

    Ok(())
  }

  /// Check if bot has necessary permissions in the guild
  async fn check_perms(&self, ctx: &Context, guild: &Guild) -> (bool, String) {
    use serenity::all::Permissions;

    let mut missing_perms = Vec::new();

    // Get bot's member object in the guild
    let bot_user_id = ctx.cache.current_user().id;
    let bot_member = match guild.id.member(&ctx.http, bot_user_id).await {
      Ok(member) => member,
      Err(e) => {
        error!("Failed to get bot member: {e}");
        return (false, "Unable to check bot permissions".to_string());
      }
    };

    // Get bot's guild-level permissions
    let guild_permissions = guild.member_permissions(&bot_member);

    // Check required permissions
    let required_perms = [
      (Permissions::MOVE_MEMBERS, "Move members"),
      (Permissions::SEND_MESSAGES, "Send messages"),
      (Permissions::EMBED_LINKS, "Embed links"),
      (Permissions::VIEW_CHANNEL, "View channels"),
      (Permissions::MANAGE_CHANNELS, "Manage channels"),
    ];

    for (perm, name) in required_perms {
      if !guild_permissions.contains(perm) {
        missing_perms.push(name);
      }
    }

    if missing_perms.is_empty() {
      (true, String::new())
    } else {
      (false, missing_perms.join(", "))
    }
  }

  /// Check for users already in queue voice channels and add them to the queue
  async fn check_existing_voice_users(&self, ctx: &Context, guild: &Guild, manager: &mut Manager) {
    // Get the server from the manager
    let server = manager.get_qguild(guild.id).unwrap();

    // Iterate through all categories and check their queue voice channels
    for category in &mut server.categories {
      let queue_vc_id = category.channels.queue_vc;
      let _dashboard_channel = category.channels.dashboard;

      // Check if there's an idle session available
      let has_idle_session = !category.get_seshs_by_status(&SessionStatus::Idle).is_empty();

      if !has_idle_session {
        info!("No idle session available for existing users in {}", queue_vc_id);
        continue;
      }

      // Collect all players to add first (to avoid quota check per player)
      let mut players_to_add: Vec<(serenity::all::UserId, String)> = Vec::new();
      for (user_id, voice_state) in &guild.voice_states {
        // Check if user is in this queue voice channel
        if voice_state.channel_id == Some(queue_vc_id) {
          if category.get_user_sesh(*user_id).await.is_ok() {
            info!("User {} already in session, skipping", user_id);
            continue;
          }

          let tag = if let Ok(player) = self.db.get_player(*user_id, ctx).await { player.tag } else { "Unknown".to_string() };

          players_to_add.push((*user_id, tag));
        }
      }

      // Add all players to the session WITHOUT quota check
      let _fmt_name_owned = category.formats.first().map(|fmt| fmt.name.clone());
      let _category_name = category.name.as_deref().unwrap_or("Unknown").to_string();
      let format = category.formats[0].clone(); // Clone format before mutable borrow
      let category_id = category.id; // Capture category_id before mutable borrow
      if let Ok(session) = category.get_queue().await {
        // Get server and category names for logging
        let _guild_name = guild.name.clone();

        use crate::handlers::player::resolve_player_for_queue;

        for (user_id, _tag) in &players_to_add {
          let user_id = *user_id;

          let (player, rank_mismatch) = match resolve_player_for_queue(ctx, &self.db, guild.id, user_id).await {
            Ok((p, _rank, rank_mismatch)) => (p, rank_mismatch),
            Err(e) => {
              error!("Failed to resolve player {} for queue: {e}", user_id);
              continue;
            }
          };

          // Add player to pool and log only on success
          match session.add_ply(player.clone(), false) {
            Ok(_position) => {
              // Player successfully added, now log
              if let Err(e) = log_queue_toggle(ctx, &self.db, guild.id, category_id, &format, &player, "joined", rank_mismatch).await {
                warn!("Failed to log queue toggle: {e}");
              }
            }
            Err(e) => {
              error!("Failed to add player {} to queue: {e}", user_id);
            }
          }
        }
      }

      let users_added = players_to_add.len();
      if users_added > 0 {
        // NOW check quota once after all players added
        if category.is_quota() {
          if let Err(e) = category.hot(ctx, Some(guild.id), Some(&self.db), Some(self.manager.clone())).await {
            error!("Failed to transition to hot: {e}");
          }
        }

        // Update the dashboard to reflect the new users
        category.queue_dash_update(ctx, guild.id).await;
      }
    }
  }

  /// Creates dashboard for a guild using in-memory categories from manager
  async fn create_dashboard_from_manager(&self, ctx: &Context, guild: &Guild, manager: &mut Manager) {
    // FIRST: Check bot permissions
    let (has_perms, missing_perms) = self.check_perms(ctx, guild).await;

    if !has_perms {
      warn!("Bot is missing permissions in guild {}: {}", guild.name, missing_perms);

      // Create a warning dashboard in the first available text channel
      if let Some(channel) = guild.channels.values().find(|c| c.kind == serenity::all::ChannelType::Text) {
        let warning_embed = serenity::all::CreateEmbed::new()
          .title("Missing bot permissions")
          .description(format!(
            "The bot is missing required permissions to function properly.\n\n\
                        **Missing Permissions:**\n{missing_perms}\n\n\
                        Please grant these permissions to the bot and click the button below to confirm.",
          ))
          .color(RED);

        let button = serenity::all::CreateButton::new("confirm_permissions").label("Confirm permissions").style(serenity::all::ButtonStyle::Success);

        let action_row = serenity::all::CreateActionRow::Buttons(vec![button]);

        let msg = serenity::all::CreateMessage::new().embed(warning_embed).components(vec![action_row]);

        if let Err(e) = channel.id.send_message(&ctx.http, msg).await {
          error!("Failed to send permission warning: {e}");
        }
      }
      return;
    }

    // Get server from manager (already has categories with existing users loaded)
    let server = manager.get_qguild(guild.id).unwrap();

    for category in &mut server.categories {
      // Validate that the dashboard channel still exists
      let channel_id = category.channels.dashboard;
      let channel_exists = match ctx.http.get_channel(channel_id).await {
        Ok(_) => true,
        Err(e) => {
          if e.to_string().contains("10003") || e.to_string().contains("Unknown channel") {
            warn!("[{}] Dashboard channel {} no longer exists, skipping category", guild.name, channel_id);
            false
          } else {
            warn!("[{}] Error checking dashboard channel {}: {}", guild.name, channel_id, e);
            false
          }
        }
      };

      if !channel_exists {
        continue;
      }

      // Check if dashboard already exists
      if category.has_dash(ctx).await {
        category.queue_dash_update(ctx, guild.id).await;
        continue;
      }

      // Create dashboard for each category's dashboard channel
      let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| "Unknown".to_string());
      let guild_name = guild_name(ctx, guild.id);
      let category_name = category.name.clone().unwrap_or_else(|| "Unknown".to_string());

      // Create dashboard in the dashboard channel
      match category.dash_publish(ctx, channel_id, &self.db, guild.id).await {
        Ok(_) => {
          info!("{} Dashboard created successfully for channel {}", log_prefix_category(&guild_name, &category_name), channel_name);

          // Persist the dashboard message ID to database
          let dashboard_msg_id = category.dashboard_msg.get();
          if let Err(e) = self.db.categories.update_dashboard_msg(guild.id, channel_id.get(), dashboard_msg_id).await {
            warn!("{} Failed to persist dashboard message ID to database: {e}", log_prefix_category(&guild_name, &category_name));
          } else {
            info!("{} Persisted dashboard message ID {} to database", log_prefix_category(&guild_name, &category_name), dashboard_msg_id);
          }
        }
        Err(e) => {
          error!("Failed to create dashboard for channel {}: {}", channel_name, e);
        }
      }
    }
  }
}
