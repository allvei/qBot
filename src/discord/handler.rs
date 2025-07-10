//! # Discord Event Handler
//!
//! This module defines the main event handler for Discord events.
//! It delegates event handling to specialized functions in the events module.

use serenity::all::{Context, EventHandler, GatewayIntents, Message, Ready, VoiceState};
use serenity::async_trait;
use tracing::{error, info};

use crate::events;
use crate::error::AppResult;

/// Handler for Discord events
pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    /// Called when the bot receives a message
    async fn message(&self, ctx: Context, msg: Message) {
        if let Err(e) = events::handle_message(ctx, msg).await {
            error!("Error handling message event: {}", e);
        }
    }

    /// Called when a user's voice state changes
    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        if let Err(e) = events::handle_voice_state_update(ctx, old, new).await {
            error!("Error handling voice state update event: {}", e);
        }
    }

    /// Called when the bot connects to Discord
    async fn ready(&self, ctx: Context, ready: Ready) {
        if let Err(e) = events::handle_ready(ctx, ready).await {
            error!("Error handling ready event: {}", e);
        }
    }
}

/// Get the required gateway intents for the bot
///
/// # Returns
/// * `GatewayIntents` - The intents required by the bot
pub fn get_intents() -> GatewayIntents {
    GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::MESSAGE_CONTENT
}

/// Create a new Discord client with the appropriate handler and intents
///
/// # Arguments
/// * `token` - The Discord bot token
///
/// # Returns
/// * `AppResult<Client>` - The configured Discord client
pub async fn create_client(token: &str) -> AppResult<serenity::all::Client> {
    let intents = get_intents();
    
    let client = serenity::all::Client::builder(token, intents)
        .event_handler(Handler)
        .await
        .map_err(|e| {
            error!("Error creating Discord client: {}", e);
            crate::error::AppError::DiscordError(format!("Failed to create client: {}", e))
        })?;
    
    info!("Discord client created successfully");
    Ok(client)
}
