use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use serenity::all::Context;
use tracing::{info, warn};

/// Request to update a specific group's dashboard
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DashboardUpdateRequest {
    pub guild_id: u64,
    pub group_id: u64,
}

/// Dashboard update queue that batches updates to reduce API calls
pub struct DashboardUpdateQueue {
    sender: mpsc::UnboundedSender<DashboardUpdateRequest>,
}

impl Clone for DashboardUpdateQueue {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl DashboardUpdateQueue {
    /// Create a new dashboard update queue and spawn the batching task
    pub fn new(ctx: Context, manager: Arc<tokio::sync::Mutex<crate::models::Manager>>, database: Arc<crate::Database>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        
        // Spawn the batching task
        tokio::spawn(Self::batch_processor(receiver, ctx, manager, database));
        
        Self { sender }
    }
    
    /// Request a dashboard update for a specific group
    pub fn request_update(&self, guild_id: u64, group_id: u64) {
        let request = DashboardUpdateRequest { guild_id, group_id };
        if let Err(e) = self.sender.send(request) {
            warn!("Failed to queue dashboard update: {}", e);
        }
    }
    
    /// Background task that batches and processes dashboard updates
    /// 
    /// This uses a HashSet to automatically deduplicate update requests for the same group.
    /// Since dashboards show current state, only the latest update matters - all previous
    /// requests for the same group are redundant and automatically discarded.
    async fn batch_processor(
        mut receiver: mpsc::UnboundedReceiver<DashboardUpdateRequest>,
        ctx: Context,
        manager: Arc<tokio::sync::Mutex<crate::models::Manager>>,
        database: Arc<crate::Database>,
    ) {
        let batch_window = Duration::from_millis(200); // Wait 200ms to batch updates
        // HashSet automatically deduplicates - if 10 updates come in for the same group,
        // we only keep one entry and process it once with the current state
        let mut pending_updates: HashSet<DashboardUpdateRequest> = HashSet::new();
        
        loop {
            // Wait for the first update request
            match receiver.recv().await {
                Some(request) => {
                    pending_updates.insert(request);
                    
                    // Now wait for the batch window, collecting more updates
                    let deadline = tokio::time::Instant::now() + batch_window;
                    
                    loop {
                        match tokio::time::timeout_at(deadline, receiver.recv()).await {
                            Ok(Some(request)) => {
                                // Got another update, add it to the batch
                                pending_updates.insert(request);
                            }
                            Ok(None) => {
                                // Channel closed, process remaining and exit
                                Self::process_batch(&pending_updates, &ctx, manager.clone(), database.clone()).await;
                                return;
                            }
                            Err(_) => {
                                // Timeout - batch window expired, process the batch
                                break;
                            }
                        }
                    }
                    
                    // Process the batched updates
                    if !pending_updates.is_empty() {
                        // info!("Processing batch of {} dashboard updates", pending_updates.len());
                        Self::process_batch(&pending_updates, &ctx, manager.clone(), database.clone()).await;
                        pending_updates.clear();
                    }
                }
                None => {
                    // Channel closed, exit
                    return;
                }
            }
        }
    }
    
    /// Process a batch of dashboard updates
    async fn process_batch(
        updates: &HashSet<DashboardUpdateRequest>,
        ctx: &Context,
        manager: Arc<tokio::sync::Mutex<crate::models::Manager>>,
        database: Arc<crate::Database>,
    ) {
        // Process updates concurrently (Discord allows multiple requests in parallel)
        let mut tasks = Vec::new();
        
        for update in updates {
            let ctx = ctx.clone();
            let manager = manager.clone();
            let database = database.clone();
            let guild_id = update.guild_id;
            let group_id = update.group_id;
            
            // Spawn a task for each dashboard update
            let task = tokio::spawn(async move {
                // Acquire lock briefly to get CURRENT dashboard data
                // This ensures we always show the latest state, regardless of how many
                // update requests were queued - they all get collapsed into this one update
                let (channel_id, dashboard_channel_id, message_id, embed, buttons, guild_name) = {
                    let mut manager_lock = manager.lock().await;
                    
                    let server = match manager_lock.get_server(serenity::all::GuildId::new(guild_id)) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("Failed to get server for dashboard update: {}", e);
                            return;
                        }
                    };
                    
                    let guild_name = server.guild_name.clone();
                    
                    let group = match server.groups.iter_mut().find(|g| g.group_id == group_id as u8) {
                        Some(g) => g,
                        None => {
                            warn!("[{}] Failed to find group {} for dashboard update", guild_name, group_id);
                            return;
                        }
                    };
                    
                    // Refresh player ranks from Discord to ensure dashboard shows current ranks
                    // This prevents desync when players are promoted while sitting in queue
                    group.refresh_player_ranks(&ctx, serenity::all::GuildId::new(guild_id), &database).await;
                    
                    // Validate VC status to ensure accurate display of who is in voice chat
                    // This prevents desync where flags don't match Discord's actual voice states
                    group.validate_vc_status(&ctx, serenity::all::GuildId::new(guild_id)).await;
                    
                    // Get dashboard message info
                    let channel_id = group.channels.dashboard;
                    let dashboard_channel_id = channel_id.get();
                    let message_id = group.dashboard_msg;
                    
                    // Generate dashboard content
                    let (embed, buttons) = match group.build_dashboard_content().await {
                        Ok(content) => content,
                        Err(e) => {
                            warn!("[{}] Failed to build dashboard content for group {}: {}", guild_name, group_id, e);
                            return;
                        }
                    };
                    
                    (channel_id, dashboard_channel_id, message_id, embed, buttons, guild_name)
                }; // Release lock here
                
                // Update the dashboard message WITHOUT holding any locks
                use serenity::all::EditMessage;
                let channel_name = channel_id.name(&ctx.http).await.unwrap_or_else(|_| format!("#{}", channel_id));
                match channel_id.edit_message(&ctx.http, message_id, EditMessage::new().embed(embed.clone()).components(buttons.clone())).await {
                    Ok(_) => {
                        info!("[{}] Updated dashboard in #{}", guild_name, channel_name);
                    }
                    Err(e) => {
                        // Check if message was deleted (404 error)
                        if e.to_string().contains("404") || e.to_string().contains("Unknown Message") {
                            warn!("[{}] Dashboard message was deleted in #{}, recreating...", guild_name, channel_name);
                            
                            // Recreate the dashboard message
                            use serenity::all::CreateMessage;
                            match channel_id.send_message(&ctx.http, CreateMessage::new().embed(embed).components(buttons)).await {
                                Ok(new_msg) => {
                                    info!("[{}] Recreated dashboard in #{}", guild_name, channel_name);
                                    
                                    // Update the stored message ID in memory
                                    let mut manager_lock = manager.lock().await;
                                    if let Ok(server) = manager_lock.get_server(serenity::all::GuildId::new(guild_id)) {
                                        if let Some(group) = server.groups.iter_mut().find(|g| g.group_id == group_id as u8) {
                                            group.dashboard_msg = new_msg.id;
                                        }
                                    }
                                    drop(manager_lock);
                                    
                                    // Persist to database
                                    if let Err(e) = database.groups.update_dashboard_msg(guild_id, dashboard_channel_id, new_msg.id.get()).await {
                                        warn!("Failed to update dashboard message ID in database: {}", e);
                                    }
                                }
                                Err(create_err) => {
                                    warn!("[{}] Failed to recreate dashboard in #{}: {}", guild_name, channel_name, create_err);
                                }
                            }
                        } else {
                            warn!("[{}] Failed to update dashboard in #{}: {}", guild_name, channel_name, e);
                        }
                    }
                }
            });
            
            tasks.push(task);
        }
        
        // Wait for all updates to complete
        for task in tasks {
            let _ = task.await;
        }
    }
}
