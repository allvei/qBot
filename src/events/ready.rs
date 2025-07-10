//! # Ready Event Handler
//!
//! This module handles the Discord ready event, which occurs when the bot
//! successfully connects to Discord and is ready to receive events.

use serenity::all::{Context, Ready};
use tracing::info;

use crate::error::AppResult;

/// Handle the ready event
///
/// This function is called when the bot successfully connects to Discord.
/// It performs initialization tasks and logs connection information.
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `ready` - The ready event data
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
pub async fn handle_ready(ctx: Context, ready: Ready) -> AppResult<()> {
    let bot_user = &ready.user;
    info!("Connected as {}#{}", bot_user.name, bot_user.discriminator);
    info!("Serving {} guilds", ready.guilds.len());
    
    // Log the guilds the bot is connected to
    for guild in &ready.guilds {
        info!("Connected to guild: {}", guild.id());
    }
    
    // Initialize any data structures or state needed for the bot
    initialize_bot_state(&ctx).await?;
    
    Ok(())
}

/// Initialize bot state after connecting
///
/// # Arguments
/// * `ctx` - The Discord context
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
async fn initialize_bot_state(ctx: &Context) -> AppResult<()> {
    // TODO: Initialize any necessary bot state
    // - Load configuration
    // - Initialize data structures
    // - Set up scheduled tasks
    
    info!("Bot state initialized successfully");
    Ok(())
}
