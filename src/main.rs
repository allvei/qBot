use anyhow::Result;
use pf_pug_bot::{init_logging, Application};
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use tokio::sync::{mpsc, Mutex, oneshot};

/// Main entry point for the PUG bot application.
/// Runs tokio in a background thread and eframe GUI on the main thread.
fn main() -> Result<()> {
    // Initialize shared state components
    let log_buffer = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));
    let (cmd_tx, cmd_rx) = mpsc::channel::<pf_pug_bot::gui::commands::GuiCommand>(100);
    let (shutdown_tx, _shutdown_rx) = oneshot::channel();

    // TODO: Implement GUI shutdown trigger (Phase 1.3, Phase 7.1)
    // The shutdown_tx is passed to GuiSharedState but not currently used by the GUI
    // Add a shutdown button in Settings panel that calls shutdown_tx.send()

    // Initialize logging with GUI log buffer
    init_logging(Some(log_buffer.clone()));

    // Create manager and database for shared state
    let manager = Arc::new(Mutex::new(pf_pug_bot::Manager::default()));
    let db = Arc::new(tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(async { pf_pug_bot::Database::new("sqlite:./pf_pug_bot.db").await.unwrap() }));

    // Create shared state for GUI
    let shared_state = Arc::new(pf_pug_bot::gui::state::GuiSharedState::new(
        manager.clone(),
        db.clone(),
        log_buffer,
        cmd_tx,
        shutdown_tx,
    ));

    // Clone for bot thread
    let latest_manager_bot = shared_state.latest_manager.clone();

    // Spawn tokio runtime in background thread
    let bot_thread = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        rt.block_on(async {
            let app = Application::new_with_shared(manager, db).await.unwrap();
            let app = app.with_cmd_rx(cmd_rx).with_latest_manager(latest_manager_bot);
            if let Err(e) = app.run().await {
                eprintln!("Bot error: {}", e);
            }
        });
    });

    // Run eframe GUI on main thread
    let native_options = eframe::NativeOptions::default();
    let result = eframe::run_native(
        "qBot Host Management Panel",
        native_options,
        Box::new(|_cc| {
            Ok(Box::new(pf_pug_bot::gui::app::MyApp::new(shared_state)))
        }),
    );

    // Wait for bot thread to finish
    bot_thread.join().unwrap();

    result.map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}
