use anyhow::Result;
use pf_pug_bot::{init_logging, Application};

/// Main entry point for the PUG bot application.
/// Uses the Application struct for all initialization and setup.
#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    
    let app = Application::new().await?;
    app.run().await
}
