//! # Message Event Handler
//!
//! This module handles Discord message events, including commands and interactions.

use serenity::all::{Context, Message};
use tracing::{debug, error, info};

use crate::error::AppResult;

/// Handle message create events
///
/// This function is called when a new message is created in a channel.
/// It processes commands and other message-based interactions.
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The new message
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
pub async fn handle_message(ctx: Context, msg: Message) -> AppResult<()> {
    // Ignore messages from bots
    if msg.author.bot {
        return Ok(());
    }

    // Get the content of the message
    let content = &msg.content;
    
    // Check if the message is a command (starts with !)
    if content.starts_with('!') {
        let command = content.trim_start_matches('!');
        let command_parts: Vec<&str> = command.split_whitespace().collect();
        
        if !command_parts.is_empty() {
            match command_parts[0].to_lowercase().as_str() {
                "help" => {
                    handle_help_command(&ctx, &msg).await?;
                }
                "status" => {
                    handle_status_command(&ctx, &msg).await?;
                }
                // Add more command handlers here
                _ => {
                    debug!("Unknown command: {}", command_parts[0]);
                }
            }
        }
    }
    
    Ok(())
}

/// Handle the help command
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
async fn handle_help_command(ctx: &Context, msg: &Message) -> AppResult<()> {
    info!("Help command received from user {}", msg.author.id);
    
    let help_text = "
**PF PUG Bot Commands**
!help - Show this help message
!status - Show the current session status
!join - Join the queue
!leave - Leave the queue
";
    
    if let Err(e) = msg.channel_id.say(&ctx.http, help_text).await {
        error!("Failed to send help message: {}", e);
    }
    
    Ok(())
}

/// Handle the status command
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
async fn handle_status_command(ctx: &Context, msg: &Message) -> AppResult<()> {
    info!("Status command received from user {}", msg.author.id);
    
    // TODO: Implement status command logic
    // - Get the current session status for the relevant group
    // - Format and send the status message
    
    if let Err(e) = msg.channel_id.say(&ctx.http, "Status command not fully implemented yet.").await {
        error!("Failed to send status message: {}", e);
    }
    
    Ok(())
}
