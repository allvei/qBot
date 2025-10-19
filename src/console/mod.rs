use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::Mutex;
use serenity::prelude::Context;
use sqlx::{SqlitePool, Row};
use tracing::{info, error};

use crate::models::manager::Manager;
use crate::database::repositories::GroupRepository;

pub struct ConsoleHandler {
    manager: Arc<Mutex<Manager>>,
    database: SqlitePool,
    ctx: Option<Arc<Context>>,
}

impl ConsoleHandler {
    pub fn new(manager: Arc<Mutex<Manager>>, database: SqlitePool, ctx: Arc<Context>) -> Self {
        Self {
            manager,
            database,
            ctx: Some(ctx),
        }
    }

    pub fn new_without_context(manager: Arc<Mutex<Manager>>, database: SqlitePool) -> Self {
        Self {
            manager,
            database,
            ctx: None,
        }
    }

    pub async fn start_console_loop(&self) {
        info!("Console commands available: status, guilds, sessions, config <guild_id>, query <sql>, help, quit");
        
        loop {
            print!("pfpug> ");
            io::stdout().flush().unwrap();
            
            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    let input = input.trim();
                    if input.is_empty() {
                        continue;
                    }
                    
                    match self.handle_command(input).await {
                        Ok(should_quit) => {
                            if should_quit {
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Command error: {}", e);
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to read input: {}", e);
                    break;
                }
            }
        }
    }

    async fn handle_command(&self, input: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(false);
        }

        match parts[0].to_lowercase().as_str() {
            "status"   => self.cmd_status().await?,
            "guilds"   => self.cmd_list_guilds().await?,
            "sessions" => self.cmd_list_sessions().await?,
            "config"   => {
                match parts.len() {
                    2 => {
                        // Read config: config <guild_id>
                        self.cmd_print_config(parts[1]).await?;
                    },
                    4 => {
                        // Set config: config <guild_id> <key> <value>
                        self.cmd_set_config(parts[1], parts[2], parts[3]).await?;
                    },
                    _ => {
                        println!("Usage:");
                        println!("  config <guild_id>           - Show configuration");
                        println!("  config <guild_id> <key> <value> - Set configuration value");
                        println!("Examples:");
                        println!("  config TS1 dashboard 1385894822992281701");
                        println!("  config 1410654395229536268 session_quota 4");
                    }
                }
            },
            "create" => {
                if parts.len() < 7 {
                    println!("Usage: create <guild_id> <queue_channel> <dashboard_channel> <red_channel> <blue_channel> <quota>");
                    println!("Example: create 1383583686431080499 1388643261543088208 1385894822992281701 1385464431185494086 1385464563448680578 10");
                } else {
                    self.cmd_create_config(parts[1], parts[2], parts[3], parts[4], parts[5], parts[6]).await?;
                }
            },
            "query" => {
                if parts.len() < 2 {
                    println!("Usage: query <sql>");
                } else {
                    let sql = parts[1..].join(" ");
                    self.cmd_query_db(&sql).await?;
                }
            },
            "help" => self.cmd_help(),
            "quit" | "exit" => {
                println!("Shutting down console...");
                return Ok(true);
            },
            _ => {
                println!("Unknown command: {}. Type 'help' for available commands.", parts[0]);
            }
        }

        Ok(false)
    }

    async fn cmd_status(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = self.manager.lock().await;
        let total_servers = manager.servers.len();
        let mut total_sessions = 0;
        let mut active_sessions = 0;
        let mut total_players = 0;

        for server in &manager.servers {
            for group in &server.groups {
                total_sessions += group.sessions.len();
                for session in &group.sessions {
                    total_players += session.pool.len();
                    if session.status.is_active() {
                        active_sessions += 1;
                    }
                }
            }
        }

        println!("=== Bot Status ===");
        println!("Connected Guilds: {}", total_servers);
        println!("Total Sessions: {}", total_sessions);
        println!("Active Sessions: {}", active_sessions);
        println!("Total Players in Queue: {}", total_players);
        
        if let Some(ctx) = &self.ctx {
            println!("Bot User: {}", ctx.cache.current_user().name);
        } else {
            println!("Bot User: <Context not available>");
        }

        Ok(())
    }

    async fn cmd_list_guilds(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("=== Connected Guilds ===");
        
        // List guilds from database (more reliable and Send-safe)
        let group_repo = GroupRepository::new(self.database.clone());
        match sqlx::query("SELECT DISTINCT guild_id FROM groups").fetch_all(&self.database).await {
            Ok(rows) => {
                for row in rows {
                    if let Ok(guild_id) = row.try_get::<i64, _>("guild_id") {
                        println!("Guild ID: {}", guild_id);
                        
                        match group_repo.get_groups_for_guild(guild_id as u64).await {
                            Ok(groups) => {
                                println!("  ✅ {} group(s) configured", groups.len());
                                for group in groups {
                                    println!("    - Group {}: Queue Channel {}", group.group_id, group.channels.queue);
                                }
                            },
                            Err(e) => {
                                println!("  ❌ Error checking groups: {}", e);
                            }
                        }
                    }
                }
            },
            Err(e) => {
                println!("Error querying database: {}", e);
            }
        }

        Ok(())
    }

    async fn cmd_list_sessions(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = self.manager.lock().await;
        println!("=== Active Sessions ===");
        
        let mut session_count = 0;
        for server in &manager.servers {
            for group in &server.groups {
                for (_i, session) in group.sessions.iter().enumerate() {
                    session_count += 1;
                    println!("Session {}: Guild {} - Group {} - {} players - Status: {:?}", 
                        session_count, server.guild_id, group.group_id, session.pool.len(), session.status);
                    
                    if !session.pool.is_empty() {
                        print!("  Players: ");
                        for (j, player) in session.pool.iter().enumerate() {
                            if j > 0 { print!(", "); }
                            print!("{}", player.player.discord_id);
                        }
                        println!();
                    }
                }
            }
        }
        
        if session_count == 0 {
            println!("No active sessions found.");
        }

        Ok(())
    }

    async fn resolve_guild_id(&self, guild_identifier: &str) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        // Try parsing as numeric ID first
        if let Ok(guild_id) = guild_identifier.parse::<u64>() {
            return Ok(guild_id);
        }

        // Try resolving by guild name from database
        match sqlx::query("SELECT DISTINCT guild_id FROM groups").fetch_all(&self.database).await {
            Ok(rows) => {
                for row in rows {
                    if let Ok(guild_id) = row.try_get::<i64, _>("guild_id") {
                        // Check if we can get guild name from context (if available)
                        if let Some(ctx) = &self.ctx {
                            if let Some(guild) = ctx.cache.guild(serenity::model::id::GuildId::new(guild_id as u64)) {
                                if guild.name.to_lowercase() == guild_identifier.to_lowercase() {
                                    return Ok(guild_id as u64);
                                }
                            }
                        }
                    }
                }
                Err(format!("Guild '{}' not found", guild_identifier).into())
            },
            Err(e) => Err(format!("Database error: {}", e).into())
        }
    }

    async fn cmd_print_config(&self, guild_identifier: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        
        let group_repo = GroupRepository::new(self.database.clone());
        match group_repo.get_groups_for_guild(guild_id).await {
            Ok(groups) => {
                if groups.is_empty() {
                    println!("No configuration found for guild ID: {}", guild_id);
                } else {
                    println!("=== Configuration for Guild {} ===", guild_id);
                    for group in groups {
                        println!("Group ID: {}", group.group_id);
                        println!("  Dashboard Channel: {}", group.dashboard.channel_id);
                        println!("  Chat Channel: N/A");
                        println!("  Queue Channel: {}", group.channels.queue);
                        println!("  Dashboard Message ID: {}", group.dashboard.msg);
                        if let Some(teams) = group.channels.teams.first() {
                            println!("  Red Team Channel: {}", teams.red_vc);
                            println!("  Blue Team Channel: {}", teams.blu_vc);
                        } else {
                            println!("  Red Team Channel: N/A");
                            println!("  Blue Team Channel: N/A");
                        }
                        println!("  Session Quota: {}", group.quota);
                        println!("  Timeout: {} minutes", group.timeout);
                        println!();
                    }
                }
            },
            Err(e) => {
                println!("Error retrieving configuration: {}", e);
            }
        }

        Ok(())
    }

    async fn cmd_set_config(&self, guild_identifier: &str, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        
        // Get existing groups for this guild
        let group_repo = GroupRepository::new(self.database.clone());
        let groups = group_repo.get_groups_for_guild(guild_id).await?;
        
        if groups.is_empty() {
            println!("No configuration found for guild ID: {}. Cannot set values.", guild_id);
            return Ok(());
        }

        // For now, update the first group (most common case)
        let group = &groups[0];
        let queue_id = group.channels.queue.get();

        match key.to_lowercase().as_str() {
            "dashboard" => {
                let dashboard_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid dashboard channel ID: {}", value))?;
                
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    dashboard_id,
                    0, // chat (not used)
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    group.quota as u8,
                ).await {
                    Ok(_) => println!("✅ Updated dashboard channel to {} for guild {}", dashboard_id, guild_id),
                    Err(e) => println!("❌ Failed to update dashboard: {}", e),
                }
            },
            "session_quota" | "quota" => {
                let quota: u8 = value.parse()
                    .map_err(|_| format!("Invalid session quota: {}", value))?;
                
                if quota == 0 || quota > 20 {
                    println!("❌ Session quota must be between 1 and 20");
                    return Ok(());
                }
                
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    group.dashboard.channel_id.get(),
                    0, // chat (not used)
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    quota,
                ).await {
                    Ok(_) => println!("✅ Updated session quota to {} for guild {}", quota, guild_id),
                    Err(e) => println!("❌ Failed to update session quota: {}", e),
                }
            },
            "red" | "red_team" => {
                let red_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid red team channel ID: {}", value))?;
                
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    group.dashboard.channel_id.get(),
                    0, // chat (not used)
                    red_id,
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    group.quota as u8,
                ).await {
                    Ok(_) => println!("✅ Updated red team channel to {} for guild {}", red_id, guild_id),
                    Err(e) => println!("❌ Failed to update red team channel: {}", e),
                }
            },
            "blue" | "blu" | "blue_team" => {
                let blue_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid blue team channel ID: {}", value))?;
                
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    group.dashboard.channel_id.get(),
                    0, // chat (not used)
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    blue_id,
                    group.quota as u8,
                ).await {
                    Ok(_) => println!("✅ Updated blue team channel to {} for guild {}", blue_id, guild_id),
                    Err(e) => println!("❌ Failed to update blue team channel: {}", e),
                }
            },
            _ => {
                println!("❌ Unknown configuration key: {}", key);
                println!("Available keys: dashboard, session_quota, red, blue");
            }
        }

        Ok(())
    }

    async fn cmd_create_config(&self, guild_id_str: &str, queue_channel: &str, dashboard_channel: &str, red_channel: &str, blue_channel: &str, quota_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id: u64 = guild_id_str.parse()
            .map_err(|_| format!("Invalid guild ID: {}", guild_id_str))?;
        let queue_id: u64 = queue_channel.parse()
            .map_err(|_| format!("Invalid queue channel ID: {}", queue_channel))?;
        let dashboard_id: u64 = dashboard_channel.parse()
            .map_err(|_| format!("Invalid dashboard channel ID: {}", dashboard_channel))?;
        let red_id: u64 = red_channel.parse()
            .map_err(|_| format!("Invalid red channel ID: {}", red_channel))?;
        let blue_id: u64 = blue_channel.parse()
            .map_err(|_| format!("Invalid blue channel ID: {}", blue_channel))?;
        let quota: u8 = quota_str.parse()
            .map_err(|_| format!("Invalid quota: {}", quota_str))?;

        if quota == 0 || quota > 20 {
            println!("❌ Session quota must be between 1 and 20");
            return Ok(());
        }

        let group_repo = GroupRepository::new(self.database.clone());
        
        // Check if group already exists
        match group_repo.get_groups_for_guild(guild_id).await {
            Ok(groups) => {
                if !groups.is_empty() {
                    println!("❌ Guild {} already has {} group configuration(s)", guild_id, groups.len());
                    println!("Use 'config' command to modify existing configurations");
                    return Ok(());
                }
            },
            Err(e) => {
                println!("⚠️  Error checking existing groups: {}", e);
            }
        }

        // Create new group configuration
        match group_repo.update_group(
            queue_id,
            guild_id,
            dashboard_id,
            0, // chat (not used)
            red_id,
            blue_id,
            quota,
        ).await {
            Ok(_) => {
                println!("✅ Created new group configuration for guild {}", guild_id);
                println!("   Queue Channel: {}", queue_id);
                println!("   Dashboard Channel: {}", dashboard_id);
                println!("   Red Team Channel: {}", red_id);
                println!("   Blue Team Channel: {}", blue_id);
                println!("   Session Quota: {}", quota);
            },
            Err(e) => {
                println!("❌ Failed to create group configuration: {}", e);
            }
        }

        Ok(())
    }

    async fn cmd_query_db(&self, sql: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Only allow SELECT queries for safety
        if !sql.trim().to_lowercase().starts_with("select") {
            println!("Only SELECT queries are allowed for safety.");
            return Ok(());
        }

        match sqlx::query(sql).fetch_all(&self.database).await {
            Ok(rows) => {
                println!("Query executed successfully. Rows returned: {}", rows.len());
                for (i, row) in rows.iter().enumerate() {
                    println!("Row {}: {} columns", i + 1, row.len());
                    // Print column values if possible
                    for col_idx in 0..row.len() {
                        if let Ok(value) = row.try_get::<String, _>(col_idx) {
                            println!("  Column {}: {}", col_idx, value);
                        } else if let Ok(value) = row.try_get::<i64, _>(col_idx) {
                            println!("  Column {}: {}", col_idx, value);
                        } else {
                            println!("  Column {}: <unprintable>", col_idx);
                        }
                    }
                }
            },
            Err(e) => {
                println!("Query error: {}", e);
            }
        }

        Ok(())
    }

    fn cmd_help(&self) {
        println!("=== Available Commands ===");
        println!("status                          - Show bot status and statistics");
        println!("guilds                          - List all connected guilds and their configurations");
        println!("sessions                        - List all active sessions and players");
        println!("config <guild_id>               - Print configuration for a specific guild");
        println!("config <guild_id> <key> <value> - Set configuration value");
        println!("create <guild_id> <queue> <dashboard> <red> <blue> <quota> - Create new group configuration");
        println!("query <sql>                     - Execute a SELECT query on the database");
        println!("help                            - Show this help message");
        println!("quit/exit                       - Shutdown the console (bot will continue running)");
        println!();
        println!("=== Config Command Examples ===");
        println!("config TS1 dashboard 1385894822992281701");
        println!("config 1410654395229536268 session_quota 4");
        println!("config TS1 red 1385464431185494086");
        println!("config TS1 blue 1385464563448680578");
        println!();
        println!("=== Create Command Example ===");
        println!("create 1383583686431080499 1388643261543088208 1385894822992281701 1385464431185494086 1385464563448680578 10");
        println!();
        println!("Available config keys: dashboard, session_quota, red, blue");
    }
}
