//! # Voice State Event Handler
//!
//! This module handles Discord voice state update events, which occur when users
//! join, leave, or move between voice channels.

use serenity::all::{Context, VoiceState};
use tracing::{debug, info};

use crate::error::AppResult;
use crate::models::Group;
use crate::models::Server;

/// Handle voice state update events
///
/// This function is called when a user's voice state changes (joins/leaves/moves voice channels).
/// It manages player session assignments based on voice channel movements.
///
/// # Arguments
/// * `ctx` - The Discord context
/// * `old` - The previous voice state (if any)
/// * `new` - The new voice state
///
/// # Returns
/// * `AppResult<()>` - Success or failure with error context
pub async fn handle_voice_state_update(
    ctx: Context,
    old: Option<VoiceState>,
    new: VoiceState,
) -> AppResult<()> {
    let user_id = new.user_id.get();
    
    // Get channel IDs for comparison
    let old_channel_id = old.as_ref().and_then(|state| state.channel_id).map(|id| id.get());
    let new_channel_id = new.channel_id.map(|id| id.get());
    
    match (old_channel_id, new_channel_id) {
        // User joined a voice channel
        (None, Some(channel_id)) => {
            info!("User {} joined voice channel {}", user_id, channel_id);
            // TODO: Handle user joining a voice channel
            // - Check if channel is a queue channel
            // - If so, add user to the appropriate session
        }
        
        // User left a voice channel
        (Some(channel_id), None) => {
            info!("User {} left voice channel {}", user_id, channel_id);
            // TODO: Handle user leaving a voice channel
            // - Check if channel is a queue or team channel
            // - If so, remove user from the appropriate session
        }
        
        // User moved between voice channels
        (Some(old_id), Some(new_id)) if old_id != new_id => {
            info!("User {} moved from voice channel {} to {}", user_id, old_id, new_id);
            // TODO: Handle user moving between voice channels
            // - Check if either channel is a queue or team channel
            // - Update session assignments accordingly
        }
        
        // No relevant change
        _ => {
            debug!("Voice state update for user {} with no channel change", user_id);
        }
    }
    
    Ok(())
}

/// Find the appropriate group based on a voice channel ID
///
/// # Arguments
/// * `server` - The server to search in
/// * `channel_id` - The voice channel ID to find
///
/// # Returns
/// * `Option<&Group>` - The group if found, None otherwise
pub fn find_group_by_voice_channel(server: &Server, channel_id: u64) -> Option<&Group> {
    // Check queue channels
    if let Some(group) = server.find_group_by_queue_channel(channel_id) {
        return Some(group);
    }
    
    // Check team channels
    for group in &server.groups {
        for team_channel in &group.teams {
            if team_channel.red_vc_id == channel_id || team_channel.blu_vc_id == channel_id {
                return Some(group);
            }
        }
    }
    
    None
}
