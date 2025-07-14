//! # Discord Commands
//!
//! This module defines Discord command handlers and utilities.
//! It provides functions for processing and responding to user commands.

use serenity::all::{Context, Message};
use tracing::{debug, error, info};

use crate::error::AppResult;
use crate::models::session::SessionStatus;

/// Command response type for standardized command handling
pub enum CommandResponse {
    /// Text response to be sent to the channel
    Text(String),
    /// Embed response with title and description
    Embed {
        title:       String,
        description: String,
        color:       Option<(u8, u8, u8)>,
    },
    /// No response needed
    None,
}

/// Process a command message
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
/// * `command` - The command string (without prefix)
///
/// # Returns
/// * `AppResult<CommandResponse>` - The response to send
pub async fn process_command(
    ctx: &Context,
    msg: &Message,
    command: &str,
) -> AppResult<CommandResponse> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(CommandResponse::None);
    }

    match parts[0].to_lowercase().as_str() {
        "help" => handle_help_command(ctx, msg, &parts[1..]).await,
        "status" => handle_status_command(ctx, msg, &parts[1..]).await,
        "join" => handle_join_command(ctx, msg, &parts[1..]).await,
        "leave" => handle_leave_command(ctx, msg, &parts[1..]).await,
        _ => {
            debug!("Unknown command: {}", parts[0]);
            Ok(CommandResponse::Text(format!("Unknown command: `{}`", parts[0])))
        }
    }
}

/// Handle the help command
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
/// * `args` - Command arguments
///
/// # Returns
/// * `AppResult<CommandResponse>` - The response to send
async fn handle_help_command(
    _ctx: &Context,
    _msg: &Message,
    _args: &[&str],
) -> AppResult<CommandResponse> {
    info!("Processing help command");

    Ok(CommandResponse::Embed {
        title:       "PF PUG Bot Commands".to_string(),
        description: "
**Basic Commands**
`!help` - Show this help message
`!status` - Show the current session status

**Queue Commands**
`!join` - Join the queue
`!leave` - Leave the queue

**Admin Commands**
`!start` - Start a session (admin only)
`!end` - End a session (admin only)
"
        .to_string(),
        color:       Some((0, 128, 255)),
    })
}

/// Handle the status command
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
/// * `args` - Command arguments
///
/// # Returns
/// * `AppResult<CommandResponse>` - The response to send
async fn handle_status_command(
    _ctx: &Context,
    msg: &Message,
    _args: &[&str],
) -> AppResult<CommandResponse> {
    info!("Processing status command from user {}", msg.author.id);

    // TODO: Implement status command logic
    // - Get the current session status for the relevant group
    // - Format and send the status message

    Ok(CommandResponse::Text("Status command not fully implemented yet.".to_string()))
}

/// Handle the join command
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
/// * `args` - Command arguments
///
/// # Returns
/// * `AppResult<CommandResponse>` - The response to send
async fn handle_join_command(
    _ctx: &Context,
    msg: &Message,
    _args: &[&str],
) -> AppResult<CommandResponse> {
    info!("Processing join command from user {}", msg.author.id);

    // TODO: Implement join command logic
    // - Add the user to the appropriate session

    Ok(CommandResponse::Text("Join command not fully implemented yet.".to_string()))
}

/// Handle the leave command
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `msg` - The message containing the command
/// * `args` - Command arguments
///
/// # Returns
/// * `AppResult<CommandResponse>` - The response to send
async fn handle_leave_command(
    _ctx: &Context,
    msg: &Message,
    _args: &[&str],
) -> AppResult<CommandResponse> {
    info!("Processing leave command from user {}", msg.author.id);

    // TODO: Implement leave command logic
    // - Remove the user from the appropriate session

    Ok(CommandResponse::Text("Leave command not fully implemented yet.".to_string()))
}

/// Format session status for display
///
/// # Arguments
/// * `status` - The session status
///
/// # Returns
/// * `&'static str` - Human-readable status string
pub fn format_session_status(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Idle => "Idle - Waiting for players",
        SessionStatus::Hot => "Hot - Ready to start",
        SessionStatus::Push => "Push - Moving players to teams",
        SessionStatus::Live => "Live - Game in progress",
        SessionStatus::Pull => "Pull - Game ending",
    }
}
