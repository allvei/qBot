use anyhow::Result;
use qbot::{init_logging, Application};
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Main entry point for the PUG bot application.
/// Launches the egui management panel by default.
/// Pass `-nogui` to run headless (terminal only).
fn main() -> Result<()> {
  let nogui = std::env::args().any(|a| a == "-nogui" || a == "--nogui");

  if nogui {
    return run_headless();
  }

  run_gui()
}

/// Headless mode: tokio runtime on the main thread, no GUI.
fn run_headless() -> Result<()> {
  init_logging(None);

  let rt = tokio::runtime::Runtime::new()?;
  rt.block_on(async {
    let app = Application::new().await?;
    app.run().await
  })
}

/// GUI mode: egui on the main thread, bot in a background thread.
fn run_gui() -> Result<()> {
  // Initialize shared state components
  let log_buffer = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));
  let (cmd_tx, cmd_rx) = mpsc::channel::<qbot::gui::commands::GuiCommand>(100);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();

  // Initialize logging with GUI log buffer
  init_logging(Some(log_buffer.clone()));

  // Create manager and database for shared state
  let manager = Arc::new(Mutex::new(qbot::Manager::default()));
  let db = Arc::new(tokio::runtime::Runtime::new().expect("Failed to create tokio runtime").block_on(async { qbot::Database::new("sqlite:./qbot.db").await.unwrap() }));

  // Create shared state for GUI
  let shared_state = Arc::new(qbot::gui::state::GuiSharedState::new(manager.clone(), db.clone(), log_buffer, cmd_tx, shutdown_tx));

  // Clone for bot thread
  let latest_manager_bot = shared_state.latest_manager.clone();
  let user_search_results_bot = shared_state.user_search_results.clone();
  let user_guild_data_bot = shared_state.user_guild_data.clone();
  let guild_config_cache_bot = shared_state.guild_config_cache.clone();
  let system_message_channel_guilds_bot = shared_state.system_message_channel_guilds.clone();
  let community_updates_channel_guilds_bot = shared_state.community_updates_channel_guilds.clone();
  let shared_state_bot = shared_state.clone();

  // Spawn tokio runtime in background thread
  let bot_thread = thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    rt.block_on(async {
      let app = Application::new_with_shared(manager, db).await.unwrap();
      let app = app
        .with_cmd_rx(cmd_rx)
        .with_latest_manager(latest_manager_bot)
        .with_user_search_results(user_search_results_bot)
        .with_user_guild_data(user_guild_data_bot)
        .with_guild_config_cache(guild_config_cache_bot)
        .with_system_message_channel_guilds(system_message_channel_guilds_bot)
        .with_community_updates_channel_guilds(community_updates_channel_guilds_bot)
        .with_gui_shutdown(shutdown_rx)
        .with_shared_state(shared_state_bot);
      if let Err(e) = app.run().await {
        eprintln!("Bot error: {}", e);
      }
    });
  });

  // Run eframe GUI on main thread
  let mut native_options = eframe::NativeOptions::default();

  // Configure small windowed mode
  native_options.viewport.inner_size = Some(egui::vec2(900.0, 650.0));
  native_options.viewport.min_inner_size = Some(egui::vec2(600.0, 400.0));

  // Force X11 backend on Linux to avoid Wayland display errors on long-running sessions
  #[cfg(target_os = "linux")]
  {
    native_options.event_loop_builder = Some(Box::new(|builder| {
      use winit::platform::x11::EventLoopBuilderExtX11;
      builder.with_x11();
    }));
  }

  let result = eframe::run_native(
    "qBot Host Management Panel",
    native_options,
    Box::new(|cc| {
      // Configure custom font
      let mut fonts = egui::FontDefinitions::default();

      // Try to load JetBrainsMonoNL Nerd Font Mono from system
      let font_paths = ["fonts/JetBrainsMonoNLNerdFont-Regular.ttf"];

      let mut font_loaded = false;
      for path in &font_paths {
        if let Ok(font_data) = std::fs::read(path) {
          fonts.font_data.insert("JetBrainsMonoNLNerdFontMono".to_owned(), egui::FontData::from_owned(font_data).into());
          fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "JetBrainsMonoNLNerdFontMono".to_owned());
          fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "JetBrainsMonoNLNerdFontMono".to_owned());
          font_loaded = true;
          break;
        }
      }

      if !font_loaded {
        eprintln!("Warning: JetBrainsMonoNL Nerd Font Mono not found, using default font");
      }

      // Add Phosphor icons
      fonts.font_data.insert("phosphor".into(), std::sync::Arc::new(egui_phosphor::Variant::Regular.font_data()));
      fonts.families.insert(egui::FontFamily::Name("phosphor".into()), vec!["Ubuntu-Light".into(), "phosphor".into()]);

      cc.egui_ctx.set_fonts(fonts);

      Ok(Box::new(qbot::gui::app::MyApp::new(shared_state)))
    }),
  );

  // Wait for bot thread to finish
  bot_thread.join().unwrap();

  result.map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}
