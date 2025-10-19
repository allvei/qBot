use serenity::all::{ CreateEmbed as CE, CreateEmbedFooter as CEF };
use anyhow::Result;
use serenity::all::*;
use tracing::{info, error};
use crate::models::command::ComponentContext as CC;
use crate::models::data::{Group, Session, SessionStatus, SessionPlayer};

/// Creates a dynamic dashboard embed based on current group state
///
/// * `group` - The group containing sessions and queue information
pub async fn dash_init(group: &Group) -> Result<CE> {
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
            
            // Display teams (first 4 players = Red, next 4 = Blue)
            let red_team = &current_session.pool[0..4];
            let blue_team = &current_session.pool[4..8];
            
            description.push_str("**🔴 Red Team:**\n");
            for (i, player) in red_team.iter().enumerate() {
                description.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
            }
            
            description.push_str("\n**🔵 Blue Team:**\n");
            for (i, player) in blue_team.iter().enumerate() {
                description.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
            }
            description.push_str("\n");
        } else {
            // Over quota - show current teams and queued players
            description.push_str("**🔥 MATCH READY! 🔥**\n");
            
            // Display teams (first 8 players split into teams)
            if players_in_queue >= 8 {
                let red_team = &current_session.pool[0..4];
                let blue_team = &current_session.pool[4..8];
                
                description.push_str("**🔴 Red Team:**\n");
                for (i, player) in red_team.iter().enumerate() {
                    description.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
                }
                
                description.push_str("\n**🔵 Blue Team:**\n");
                for (i, player) in blue_team.iter().enumerate() {
                    description.push_str(&format!("{}. <@{}>\n", i + 1, player.player.discord_id));
                }
            }
            
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
        for _session in hot_sessions {
            description.push_str("• Ready to start!\n");
        }
        description.push('\n');
    }
    
    // Show live matches
    if !live_sessions.is_empty() {
        description.push_str("**⚡ Live Matches:**\n");
        for _session in live_sessions {
            description.push_str("• Live\n");
        }
        description.push('\n');
    }
    
    if description.is_empty() {
        description = "*No active sessions. Join the queue to get started!*".to_string();
    }
    
    embed = embed.description(description);
    embed = embed.footer(CEF::new("Use the buttons below to manage the queue and matches"));
    
    Ok(embed)
}

/// Updates the dashboard message with current group state using EditMessage::embed
/// Only edits existing messages - does not create new ones
///
/// * `group` - The group containing current session data
/// * `ctx` - Serenity context for sending messages
/// * `channel_id` - Channel ID where dashboard should be updated
pub async fn dash_update(group: &Group, ctx: &serenity::all::Context, channel_id: u64) -> Result<()> {
    use serenity::all::{ChannelId, CreateActionRow, EditMessage, MessageId};
    
    let channel    = ChannelId::new(channel_id);
    let embed      = dash_init(group).await.unwrap();
    let buttons    = group.create_dashboard_buttons();
    let action_row = CreateActionRow::Buttons(buttons);
    
    // Get the stored dashboard message ID
    let msg_id = group.dashboard.msg.get();
    
    if msg_id != 1 {
        // Only edit existing dashboard message using EditMessage::embed
        let message_id = MessageId::new(msg_id);
        info!("Attempting to edit existing dashboard message: {}", message_id);
        
        match channel.edit_message(
            &ctx.http,
            message_id,
            EditMessage::new()
                .embed(embed)
                .components(vec![action_row])
        ).await {
            Ok(_) => {
                info!("Dashboard message {} updated successfully via EditMessage::embed", message_id);
                Ok(())
            },
            Err(e) => {
                error!("Failed to edit dashboard message {}: {:?}", message_id, e);
                Err(anyhow::anyhow!("Failed to edit dashboard message: {:?}", e))
            }
        }
    } else {
        // No existing message ID - cannot update without a valid message ID;
        Err(anyhow::anyhow!("No valid dashboard message ID found ({}), cannot update dashboard", msg_id))
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
    let user = cc.component.user.id;
    let channel = cc.component.channel_id;
    
    // Get group from database as base configuration
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    // Get player info or create a new one
    let player = match cc.db.get_user(user).await {
        Ok(player) => {
            info!("Found user in db!");
            player
        }
        Err(_) => {
            info!("Creating new user in db!");
            cc.db.new_user(user).await?
        }
    };

    let mut queue_count = 0;
    let mut already_in_queue = false;
    
    // Scope the manager lock to avoid Send issues
    {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
        // Check if we have idle sessions
        match group.get_sessions_by_status(&SessionStatus::Idle).len() {
            0 => {
                info!("No idle sessions found, creating a new session");
                group.create_session();
            },
            1 => {
                info!("Found one existing idle session");
            },
            n => {
                return Err(anyhow::anyhow!("Found more than one idle session ({}). This is unexpected.", n));
            },
        }

        // Check if player is already in session
        if group.get_user_session(user).is_some() {
            info!("Player {} is already in a session", player.discord_id);
            already_in_queue = true;
        } else {
            // Add player to the session
            if let Some(session) = group.sessions.last_mut() {
                if session.status == SessionStatus::Idle {
                    session.pool.push(SessionPlayer::construct(player));
                    queue_count = session.pool.len();
                    info!("Added player to session. Queue now has {} players", queue_count);
                }
            }
        }
    } // Manager lock is dropped here
    
    if already_in_queue {
        cc.create_bot_reply("You are already in the queue!").await?;
    } else {
        cc.create_bot_reply(&format!("✅ Joined the queue! ({}/12 players)", queue_count)).await?;
        
        // Update dashboard to reflect new state
        dash_update(cc, channel).await?;
    }

    Ok(())
}

/// Handles the leave queue button
async fn leave_queue(cc: &CC<'_>) -> Result<()> {
    let user = cc.component.user.id;
    let channel = cc.component.channel_id;
    
    // Get group from database as base configuration
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    let mut found = false;
    let mut queue_count = 0;
    
    // Scope the manager lock to avoid Send issues
    {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
        // Find and remove player from any session
        for session in &mut group.sessions {
            if session.status == SessionStatus::Idle {
                let initial_len = session.pool.len();
                session.pool.retain(|p| p.player.discord_id != user);
                if session.pool.len() < initial_len {
                    found = true;
                    queue_count = session.pool.len();
                    info!("Removed player from session. Queue now has {} players", queue_count);
                    break;
                }
            }
        }
    } // Manager lock is dropped here
    
    if found {
        cc.create_bot_reply(&format!("❌ Left the queue! ({}/12 players)", queue_count)).await?;
        
        // Update dashboard to reflect new state
        dash_update(cc, channel).await?;
    } else {
        cc.create_bot_reply("You are not in the queue!").await?;
    }
    
    Ok(())
}

/// Handles the shuffle teams button
async fn shuffle(cc: &CC<'_>, session_id: Option<String>) -> Result<()> {
    let channel = cc.component.channel_id;
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    let mut shuffled = false;
    
    // Scope the manager lock to avoid Send issues
    {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
        // Find the session to shuffle
        if let Some(session) = group.sessions.iter_mut().find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8) {
            // Shuffle the players using rand crate
            use rand::seq::SliceRandom;
            session.pool.shuffle(&mut rand::thread_rng());
            shuffled = true;
            info!("Teams shuffled for session with {} players", session.pool.len());
        }
    } // Manager lock is dropped here
    
    if shuffled {
        cc.create_bot_reply("🔀 Teams shuffled! Check the dashboard for new team assignments.").await?;
        
        // Update dashboard to show shuffled teams
        dash_update(cc, channel).await?;
    } else {
        cc.create_bot_reply("❌ No session ready for shuffling. Need at least 8 players in queue.").await?;
    }
    
    Ok(())
}

/// Handles the start match button
async fn start(cc: &CC<'_>, session_id: Option<String>) -> Result<()> {
    let channel = cc.component.channel_id;
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    let mut match_started = false;
    
    // Scope the manager lock to avoid Send issues
    {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
        // Find the session to start
        if let Some(session) = group.sessions.iter_mut().find(|s| s.status == SessionStatus::Idle && s.pool.len() >= 8) {
            // Change session status to Hot (ready to start)
            session.status = SessionStatus::Hot;
            match_started = true;
            info!("Match started for session with {} players", session.pool.len());
        }
    } // Manager lock is dropped here
    
    if match_started {
        cc.create_bot_reply("🔥 Match started! Teams are now ready to play.").await?;
        
        // Update dashboard to show match status
        dash_update(cc, channel).await?;
    } else {
        cc.create_bot_reply("❌ No session ready to start. Need at least 8 players and shuffled teams.").await?;
    }
    
    Ok(())
}

/// Handles the end match button
async fn end(cc: &CC<'_>, session_id: Option<String>) -> Result<()> {
    let channel = cc.component.channel_id;
    let base_group = cc.db.get_group_by_channel(channel).await?;
    
    let mut match_ended = false;
    
    // Scope the manager lock to avoid Send issues
    {
        let mut manager = match cc.manager.lock() {
            Ok(manager) => manager,
            Err(poisoned) => {
                error!("Manager mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        };
        let group = manager.get_or_create_group(channel, base_group);
        
        // Find active sessions to end
        for session in &mut group.sessions {
            if session.status == SessionStatus::Hot || session.status == SessionStatus::Live {
                // Clear the session and reset to idle
                session.pool.clear();
                session.status = SessionStatus::Idle;
                match_ended = true;
                info!("Match ended and session reset");
                break;
            }
        }
    } // Manager lock is dropped here
    
    if match_ended {
        cc.create_bot_reply("✅ Match ended! Session has been reset and is ready for new players.").await?;
        
        // Update dashboard to show reset state
        dash_update(cc, channel).await?;
    } else {
        cc.create_bot_reply("❌ No active match to end.").await?;
    }
    
    Ok(())
}
