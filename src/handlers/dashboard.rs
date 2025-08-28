use serenity::all::{ CreateEmbed as CE, CreateEmbedFooter as CEF };
use anyhow::Result;
use tracing::info;
use crate::models::command::ComponentContext as CC;
use crate::models::session::{Group, Session, SessionStatus};



/// Creates an embed for displaying information on the dashboard.
///
/// * `title` - The title of the embed.
/// * `description` - The description of the embed.
/// * `footer` - The footer text for the embed.
pub async fn create_dashboard(title: &str,description: Option<&str>,footer: Option<&str>) -> CE {
    CE::new()
        .title(title)
        .description(description.unwrap_or(""))
        .footer(CEF::new(footer.unwrap_or("")))
}

/// Creates a dynamic dashboard embed based on current group state
///
/// * `group` - The group containing sessions and queue information
pub async fn create_dynamic_dashboard(group: &Group) -> CE {
    let mut embed = CE::new().title("PUG Dashboard");
    
    // Get current sessions by status
    let idle_sessions: Vec<&Session> = group.sessions.iter().filter(|s| s.status == SessionStatus::Idle).collect();
    let hot_sessions:  Vec<&Session> = group.sessions.iter().filter(|s| s.status == SessionStatus::Hot) .collect();
    let live_sessions: Vec<&Session> = group.sessions.iter().filter(|s| s.status == SessionStatus::Live).collect();
    
    let mut description = String::new();
    
    // Show current queue status
    if let Some(current_session) = idle_sessions.first() {
        let players_in_queue = current_session.pool.len();
        let quota = group.quota as usize;
        
        description.push_str(&format!("**📋 Current Queue ({}/{})**\n", players_in_queue, quota));
        
        if players_in_queue == 0 {
            description.push_str("*No players in queue. Be the first to join!*\n\n");
        } else if players_in_queue < quota {
            // Show current players
            description.push_str("**Players:**\n");
            for (i, player) in current_session.pool.iter().enumerate() {
                description.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
            }
            description.push_str(&format!("\n*Need {} more players to start*\n\n", quota - players_in_queue));
        } else if players_in_queue == quota {
            // Show teams when quota is met
            description.push_str("**🔥 READY TO START! 🔥**\n");
            // TODO: Add red_team and blu_team fields to Session struct
            // For now, show placeholder text
            description.push_str("*Teams not yet generated. Click Shuffle to create teams.*\n\n");
        } else {
            // Over quota - show current teams and queued players
            description.push_str("**🔥 MATCH READY! 🔥**\n");
            // TODO: Add red_team and blu_team fields to Session struct
            description.push_str("*Teams not yet generated. Click Shuffle to create teams.*\n");
            
            // Show queued players for next session
            let extra_players = &current_session.pool[quota..];
            if !extra_players.is_empty() {
                description.push_str(&format!("\n**⏳ Queued for Next ({}):**\n", extra_players.len()));
                for player in extra_players {
                    description.push_str(&format!("• <@{}>\n", player.player.discord_id));
                }
            }
            description.push('\n');
        }
    } else {
        description.push_str("**📋 Queue Status**\n*No active sessions. Join the queue to get started!*\n\n");
    }
    
    // Show hot sessions (waiting to start)
    if !hot_sessions.is_empty() {
        description.push_str("**🔥 Ready Sessions:**\n");
        for session in hot_sessions {
            description.push_str(&format!("• Session {} - Ready to start!\n", session.session_id));
        }
        description.push('\n');
    }
    
    // Show live matches
    if !live_sessions.is_empty() {
        description.push_str("**⚡ Live Matches:**\n");
        for session in live_sessions {
            description.push_str(&format!("• Session {} - Match in progress\n", session.session_id));
        }
        description.push('\n');
    }
    
    if description.is_empty() {
        description = "*No active sessions. Join the queue to get started!*".to_string();
    }
    
    embed = embed.description(description);
    embed = embed.footer(CEF::new("Use the buttons below to manage the queue and matches"));
    
    embed
}

/// Updates the dashboard message with current group state
///
/// * `group` - The group containing current session data
/// * `ctx` - Serenity context for sending messages
/// * `channel_id` - Channel ID where dashboard should be updated
pub async fn update_dashboard(group: &Group, ctx: &serenity::all::Context, channel_id: u64) -> Result<()> {
    use serenity::all::{ChannelId, CreateMessage, CreateActionRow};
    
    let channel = ChannelId::new(channel_id);
    let embed = create_dynamic_dashboard(group).await;
    let buttons = group.create_dashboard_buttons();
    let action_row = CreateActionRow::Buttons(buttons);
    
    // For now, send a new message. In the future, this should edit the existing dashboard message
    match channel.send_message(
        &ctx.http,
        CreateMessage::new()
            .embed(embed)
            .components(vec![action_row])
    ).await {
        Ok(_) => {
            info!("Dashboard updated successfully");
            Ok(())
        },
        Err(e) => {
            tracing::error!("Failed to update dashboard: {:?}", e);
            Err(anyhow::anyhow!("Failed to update dashboard: {:?}", e))
        }
    }
}

/// Handles button interaction events from the dashboard
/// 
/// Processes all button interactions in a modular way
///
/// * `cc` - The component context with button information
pub async fn handle_button_interaction(cc: &CC<'_>) -> Result<()> {
    let custom_id = &cc.component.data.custom_id;
    
    // Log the button click
    info!("Button clicked: {}", custom_id);
    
    // Split the custom_id to extract action and optional session ID
    // Format: "action:session_id" or just "action"
    let parts: Vec<&str> = custom_id.split(':').collect();
    let action = parts[0];
    let session_id = parts.get(1).map(|s| s.to_string());
    
    match action {
        "join" => join_queue(cc).await,
        "leave" => leave_queue(cc).await,
        "shuffle" => shuffle(cc, session_id).await,
        "start" => start(cc, session_id).await,
        "end" => end(cc, session_id).await,
        _ => {
            cc.create_bot_reply(&format!("Unknown button action: {}", action)).await?;
            Ok(())
        }
    }
}

/// Handles the join queue button
async fn join_queue(cc: &CC<'_>) -> Result<()> {
    // TODO: Integrate with existing queue join functionality
    // For now, provide feedback that the button was clicked
    cc.create_bot_reply("🎮 Joining queue... (Integration with queue system pending)").await?;
    
    // TODO: After joining, update the dashboard to reflect new state
    // This would call update_dashboard() with the updated group state
    
    Ok(())
}

/// Handles the leave queue button
async fn leave_queue(cc: &CC<'_>) -> Result<()> {
    // TODO: Integrate with existing queue leave functionality
    // For now, provide feedback that the button was clicked
    cc.create_bot_reply("👋 Leaving queue... (Integration with queue system pending)").await?;
    
    // TODO: After leaving, update the dashboard to reflect new state
    // This would call update_dashboard() with the updated group state
    
    Ok(())
}

/// Handles the shuffle teams button
async fn shuffle(cc: &CC<'_>, session_id: Option<String>) -> Result<()> {
    // If we have a session ID, use it, otherwise use the latest session
    if let Some(id) = session_id {
        cc.create_bot_reply(&format!("Shuffling teams for session {}...", id)).await?
    } else {
        cc.create_bot_reply("Shuffling teams for latest session...").await?
    }
    
    // TODO: Implement actual team shuffling functionality
    Ok(())
}

/// Handles the start match button
async fn start(cc: &CC<'_>, session_id: Option<String>) -> Result<()> {
    // This is equivalent to accepting the teams
    if let Some(id) = session_id {
        cc.create_bot_reply(&format!("Starting match for session {}...", id)).await?
    } else {
        cc.create_bot_reply("Starting match for latest session...").await?
    }
    
    // TODO: Implement actual start match functionality
    Ok(())
}

/// Handles the end match button
async fn end(cc: &CC<'_>, session_id: Option<String>) -> Result<()> {
    if let Some(id) = session_id {
        cc.create_bot_reply(&format!("Ending match for session {}...", id)).await?
    } else {
        cc.create_bot_reply("Ending match for latest session...").await?
    }
    
    // TODO: Implement actual end match functionality
    Ok(())
}
