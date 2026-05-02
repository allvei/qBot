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
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

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
            let app = app.with_cmd_rx(cmd_rx).with_latest_manager(latest_manager_bot).with_gui_shutdown(shutdown_rx);
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
    
    let result = eframe::run_native(
        "qBot Host Management Panel",
        native_options,
        Box::new(|cc| {
            // Configure custom font
            let mut fonts = egui::FontDefinitions::default();
            
            // Try to load JetBrainsMonoNL Nerd Font Mono from system
            // First, check if we can load it from common system font paths
            let font_paths = [
                "fonts/JetBrainsMonoNLNerdFont-Regular.ttf",
            ];
            
            let mut font_loaded = false;
            for path in &font_paths {
                if let Ok(font_data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "JetBrainsMonoNLNerdFontMono".to_owned(),
                        egui::FontData::from_owned(font_data).into(),
                    );
                    fonts
                        .families
                        .entry(egui::FontFamily::Monospace)
                        .or_default()
                        .insert(0, "JetBrainsMonoNLNerdFontMono".to_owned());
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .insert(0, "JetBrainsMonoNLNerdFontMono".to_owned());
                    font_loaded = true;
                    break;
                }
            }
            
            if !font_loaded {
                eprintln!("Warning: JetBrainsMonoNL Nerd Font Mono not found, using default font");
            }
            
            // Add Phosphor icons
            fonts.font_data.insert(
                "phosphor".into(),
                std::sync::Arc::new(egui_phosphor::Variant::Regular.font_data()),
            );
            fonts.families.insert(
                egui::FontFamily::Name("phosphor".into()),
                vec!["Ubuntu-Light".into(), "phosphor".into()],
            );
            
            cc.egui_ctx.set_fonts(fonts);
            
            Ok(Box::new(pf_pug_bot::gui::app::MyApp::new(shared_state)))
        }),
    );

    // Wait for bot thread to finish
    bot_thread.join().unwrap();

    result.map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}
