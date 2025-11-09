use std::sync::Arc;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{Highlighter, MatchingBracketHighlighter};
use rustyline::hint::HistoryHinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Editor};
use rustyline::Helper;
use serenity::all::UserId;
use serenity::prelude::Context;
use sqlx::{Row, SqlitePool};
use tokio::sync::Mutex;
use tracing::{error, info};

use pf_pug_bot::database::repositories::GroupRepository;
use pf_pug_bot::models::{Manager, Team};

struct CommandHelper {
    completer: CommandCompleter,
    highlighter: MatchingBracketHighlighter,
    hinter: HistoryHinter,
}

impl Helper for CommandHelper {}

impl Completer for CommandHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Highlighter for CommandHelper {
    fn highlight<'l>(&self, line: &'l str, pos: usize) -> std::borrow::Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_char(&self, line: &str, pos: usize, forced: bool) -> bool {
        self.highlighter.highlight_char(line, pos, forced)
    }
}

impl rustyline::hint::Hinter for CommandHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &rustyline::Context<'_>) -> Option<String> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Validator for CommandHelper {}

struct CommandCompleter {
    commands: Vec<String>,
}

impl CommandCompleter {
    fn new() -> Self {
        Self {
            commands: vec![
                "status".to_string(),
                "guilds".to_string(),
                "games".to_string(),
                "config".to_string(),
                "create".to_string(),
                "query".to_string(),
                "forcegen".to_string(),
                "fakeplayer".to_string(),
                "testnotify".to_string(),
                "help".to_string(),
                "quit".to_string(),
                "exit".to_string(),
            ],
        }
    }
}

impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let line = &line[..pos];
        let mut candidates = Vec::new();

        // Only complete the first word (command name)
        if !line.contains(' ') {
            for cmd in &self.commands {
                if cmd.starts_with(line) {
                    candidates.push(Pair {
                        display: cmd.clone(),
                        replacement: cmd.clone(),
                    });
                }
            }
        }

        Ok((0, candidates))
    }
}

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
        info!("Console commands available: status, guilds, games, config, create, query, forcegen, fakeplayer, testnotify, help, quit");
        info!("Use Tab for autocompletion, Up/Down arrows for command history");
        
        let config = Config::builder()
            .completion_type(CompletionType::List)
            .auto_add_history(true)
            .build();

        let helper = CommandHelper {
            completer: CommandCompleter::new(),
            highlighter: MatchingBracketHighlighter::new(),
            hinter: HistoryHinter::new(),
        };

        let mut rl = Editor::with_config(config).unwrap();
        rl.set_helper(Some(helper));
        
        // Load history if it exists
        let _ = rl.load_history(".pf_pug_bot_history");
        
        loop {
            let readline = rl.readline("pf_pug_bot> ");
            match readline {
                Ok(line) => {
                    let input = line.trim();
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
                Err(ReadlineError::Interrupted) => {
                    info!("CTRL-C");
                    continue;
                },
                Err(ReadlineError::Eof) => {
                    info!("CTRL-D");
                    break;
                },
                Err(err) => {
                    error!("Error reading line: {:?}", err);
                    break;
                }
            }
        }
        
        // Save history
        let _ = rl.save_history(".pf_pug_bot_history");
    }

    async fn handle_command(&self, input: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(false);
        }

        match parts[0].to_lowercase().as_str() {
            "status"   => self.cmd_status().await?,
            "guilds"   => self.cmd_list_guilds().await?,
            "games" => self.cmd_list_games().await?,
            "config"   => {
                match parts.len() {
                    2 => {
                        // Read config: config <guild_id>
                        self.cmd_print_config(parts[1]).await?;
                    },
                    5 => {
                        // Set config: config <guild_id> <group_id> <key> <value>
                        self.cmd_set_config(parts[1], parts[2], parts[3], parts[4]).await?;
                    },
                    _ => {
                        println!("Usage:");
                        println!("  config <guild_id>                         - Show configuration");
                        println!("  config <guild_id> <group_id> <key> <value> - Set group configuration");
                        println!("Examples:");
                        println!("  config TS1 0 quota 8");
                        println!("  config TS1 0 timeout 120");
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
            "forcegen" => {
                if parts.len() < 3 {
                    println!("Usage: forcegen <guild_id> <group_id>");
                    println!("Example: forcegen 1383583686431080499 0");
                } else {
                    self.cmd_force_generate_teams(parts[1], parts[2]).await?;
                }
            },
            "fakeplayer" => {
                match parts.len() {
                    3 => {
                        // fakeplayer <guild_name> <count> - defaults to group 0
                        self.cmd_add_fake_players(parts[1], "0", parts[2]).await?;
                    },
                    4 => {
                        // fakeplayer <guild_id> <group_id> <count>
                        self.cmd_add_fake_players(parts[1], parts[2], parts[3]).await?;
                    },
                    _ => {
                        println!("Usage:");
                        println!("  fakeplayer <guild_name> <count>              - Add fake players to group 0");
                        println!("  fakeplayer <guild_id> <group_id> <count>     - Add fake players to specific group");
                        println!("Examples:");
                        println!("  fakeplayer TS1 8");
                        println!("  fakeplayer 1383583686431080499 0 8");
                    }
                }
            },
            "testnotify" => {
                if parts.len() < 3 {
                    println!("Usage: testnotify <guild_id> <group_id>");
                    println!("Example: testnotify 1383583686431080499 0");
                } else {
                    self.cmd_test_notify(parts[1], parts[2]).await?;
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
        let mut total_games = 0;
        let mut active_games = 0;
        let mut total_players = 0;

        for server in &manager.servers {
            for group in &server.groups {
                total_games += group.sessions.len();
                for game in &group.sessions {
                    total_players += game.pool.len();
                    if game.is_active() {
                        active_games += 1;
                    }
                }
            }
        }

        println!("=== Bot Status ===");
        println!("Connected Guilds: {}", total_servers);
        println!("Total Games: {}", total_games);
        println!("Active Games: {}", active_games);
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
                                    println!("    - Group {}: Queue Channel {}", group.group_id, group.channels.queue_chat);
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

    async fn cmd_list_games(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = self.manager.lock().await;
        println!("=== Active Games ===");
        
        let mut game_count = 0;
        for server in &manager.servers {
            for group in &server.groups {
                for game in group.sessions.iter() {
                    game_count += 1;
                    println!("Game {}: Guild {} - Group ID {} - {} players - Status: {:?}", 
                        game_count, server.guild_id.get(), group.group_id, game.pool.len(), game.status);
                    
                    if !game.pool.is_empty() {
                        print!("  Players: ");
                        for (j, player) in game.pool.iter().enumerate() {
                            if j > 0 { print!(", "); }
                            print!("{}", player.player.discord_id);
                        }
                        println!();
                    }
                }
            }
        }
        
        if game_count == 0 {
            println!("No active games found.");
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
                        println!("Group ID: {}"              , group.group_id);
                        println!("  Dashboard Channel: {}"   , group.channels.dashboard);
                        println!("  Chat Channel: {}"        , group.channels.queue_chat);
                        println!("  Queue Channel: {}"       , group.channels.queue_vc);
                        println!("  Dashboard Message ID: {}", group.dashboard_msg);
                        if let Some(teams) = group.channels.teams.first() {
                            println!("  Red Team Channel: {}", teams.red_vc);
                            println!("  Blue Team Channel: {}", teams.blu_vc);
                        } else {
                            println!("  Red Team Channel: N/A");
                            println!("  Blue Team Channel: N/A");
                        }
                        println!("  Game Quota: {}", group.quota);
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

    async fn cmd_set_config(&self, guild_identifier: &str, group_id_str: &str, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;
        
        let group_repo = GroupRepository::new(self.database.clone());
        let groups = group_repo.get_groups_for_guild(guild_id).await?;
        
        let group = groups.iter().find(|g| g.group_id == group_id)
            .ok_or(format!("Group {} not found for guild {}", group_id, guild_id))?;
        
        let queue_id = group.channels.queue_chat.get();

        match key.to_lowercase().as_str() {
            "quota" | "game_quota" => {
                let quota: u8 = value.parse()
                    .map_err(|_| format!("Invalid quota value: {}", value))?;
                
                if quota == 0 || quota > 20 {
                    println!("❌ Quota must be between 1 and 20");
                    return Ok(());
                }
                
                // Update database
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    group.channels.dashboard.get(),
                    0, // chat (not used)
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    quota,
                ).await {
                    Ok(_) => {
                        println!("✅ Updated quota to {} for guild {} group {}", quota, guild_id, group_id);
                        
                        // Update in-memory manager and dashboard
                        if let Some(ctx) = &self.ctx {
                            let mut manager = self.manager.lock().await;
                            if let Ok(group) = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id) {
                                group.quota = quota;
                                
                                // Update dashboard to reflect new quota
                                if let Err(e) = group.dash_update(ctx).await {
                                    println!("⚠️  Failed to update dashboard: {}", e);
                                } else {
                                    println!("✅ Dashboard updated successfully");
                                }
                            }
                        } else {
                            println!("⚠️  Context not available, dashboard not updated");
                        }
                    },
                    Err(e) => println!("❌ Failed to update quota: {}", e),
                }
            },
            "timeout" => {
                let timeout: u16 = value.parse()
                    .map_err(|_| format!("Invalid timeout value: {}", value))?;
                
                // Update in-memory manager
                if let Some(ctx) = &self.ctx {
                    let mut manager = self.manager.lock().await;
                    if let Ok(group) = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id) {
                        group.timeout = timeout;
                        println!("✅ Updated timeout to {} minutes for guild {} group {}", timeout, guild_id, group_id);
                        
                        // Update dashboard
                        if let Err(e) = group.dash_update(ctx).await {
                            println!("⚠️  Failed to update dashboard: {}", e);
                        }
                    }
                } else {
                    println!("❌ Context not available");
                }
            },
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
            "red" | "red_team" => {
                let red_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid red team channel ID: {}", value))?;
                
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    group.channels.dashboard.into(),
                    0, // chat (not used)
                    red_id,
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    group.quota as u8,
                ).await {
                    Ok(_)  => println!("✅ Updated red team channel to {} for guild {}", red_id, guild_id),
                    Err(e) => println!("❌ Failed to update red team channel: {}", e),
                }
            },
            "blue" | "blu" | "blue_team" => {
                let blue_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid blue team channel ID: {}", value))?;
                
                match group_repo.update_group(
                    queue_id,
                    guild_id,
                    group.channels.dashboard.into(),
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
                println!("Available keys: quota, timeout, dashboard, red, blue");
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
            println!("❌ Game quota must be between 1 and 20");
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
                println!("   Game Quota: {}", quota);
            },
            Err(e) => {
                println!("❌ Failed to create group configuration: {}", e);
            }
        }

        Ok(())
    }

    async fn cmd_query_db(&self, sql: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    async fn cmd_force_generate_teams(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;
        
        if let Some(ctx) = &self.ctx {
            let mut manager = self.manager.lock().await;
            let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
            
            // Check if there's a session with enough players
            if let Ok(session) = group.get_queue().await {                
                println!("🔄 Forcing team generation for guild {} group {} with {} players...", guild_id, group_id, session.pool.len());
                group.generate_teams(ctx).await;
                println!("✅ Teams generated successfully!");
                
                // Show the teams
                if let Ok(session) = group.get_queue().await {
                    println!("\n=== Generated Teams ===");
                    let red_players: Vec<_> = session.pool.iter()
                        .filter(|p| p.team == Some(Team::Red))
                        .collect();
                    let blu_players: Vec<_> = session.pool.iter()
                        .filter(|p| p.team == Some(Team::Blu))
                        .collect();
                    
                    println!("Red Team ({} players):", red_players.len());
                    for p in red_players {
                        println!("  - {}", p.player.discord_id);
                    }
                    
                    println!("\nBlue Team ({} players):", blu_players.len());
                    for p in blu_players {
                        println!("  - {}", p.player.discord_id);
                    }
                }
            } else {
                println!("❌ No active queue found for guild {} group {}", guild_id, group_id);
            }
        } else {
            println!("❌ Context not available. Cannot generate teams without Discord context.");
        }
        
        Ok(())
    }

    async fn cmd_add_fake_players(&self, guild_identifier: &str, group_id_str: &str, count_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;
        let count: usize = count_str.parse()
            .map_err(|_| format!("Invalid player count: {}", count_str))?;
        
        if count == 0 || count > 20 {
            println!("❌ Player count must be between 1 and 20");
            return Ok(());
        }
        
        let mut manager = self.manager.lock().await;
        let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
        
        // Ensure there's a session
        if group.sessions.is_empty() {
            group.create_session();
        }
        
        // Get the idle session
        if let Ok(session) = group.get_queue().await {
            println!("🔄 Adding {} fake player(s) to guild {} group {}...", count, guild_id, group_id);
            
            // Generate fake player IDs starting from a high number to avoid conflicts
            let base_id = 9000000000000000000_u64;
            for i in 0..count {
                let fake_id = UserId::new(base_id + i as u64);
                session.add_player(fake_id);
            }
            
            println!("✅ Added {} fake player(s). Total players in queue: {}", count, session.pool.len());
            
            // Update dashboard if context is available
            if let Some(ctx) = &self.ctx {
                if let Err(e) = group.dash_update(ctx).await {
                    println!("⚠️  Failed to update dashboard: {}", e);
                }
            }
        } else {
            println!("❌ Failed to get queue for guild {} group {}", guild_id, group_id);
        }
        
        Ok(())
    }

    async fn cmd_test_notify(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;
        
        if let Some(ctx) = &self.ctx {
            let manager = self.manager.lock().await;
            let server = manager.servers.iter().find(|s| s.guild_id.get() == guild_id)
                .ok_or(format!("Server not found for guild ID: {}", guild_id))?;
            
            let group = server.groups.iter().find(|g| g.group_id == group_id)
                .ok_or(format!("Group {} not found for guild {}", group_id, guild_id))?;
            
            println!("🔄 Testing notify method for guild {} group {}...", guild_id, group_id);
            group.notify(ctx).await;
            println!("✅ Notify method called successfully!");
            println!("   Check the queue chat channel for the notification message.");
        } else {
            println!("❌ Context not available. Cannot test notify without Discord context.");
        }
        
        Ok(())
    }

    fn cmd_help(&self) {
        println!("=== Available Commands ===");
        println!("status                                            - Show bot status and statistics");
        println!("guilds                                            - List all connected guilds and their configurations");
        println!("games                                             - List all active games and players");
        println!("config <guild_id>                                 - Print configuration for a specific guild");
        println!("config <guild_id> <group_id> <key> <value>        - Set group configuration");
        println!("create <guild_id> <queue> <dashboard> <red> <blue> <quota> - Create new group configuration");
        println!("query <sql>                                       - Execute a SELECT query on the database");
        println!();
        println!("=== Testing Commands ===");
        println!("forcegen   <guild_id> <group_id>                  - Force team generation for the current queue");
        println!("fakeplayer <guild_name> <count>                   - Add fake players to group 0");
        println!("fakeplayer <guild_id> <group_id> <count>          - Add fake players to specific group");
        println!("testnotify <guild_id> <group_id>                  - Test the notify method (sends notification to queue chat)");
        println!();
        println!("help                                              - Show this help message");
        println!("quit/exit                                         - Shutdown the console (bot will continue running)");
        println!();
        println!("=== Config Command Examples ===");
        println!("config TS1                         # Show all group configs for guild");
        println!("config TS1 0 quota 8               # Set quota to 8 for group 0");
        println!("config TS1 0 timeout 120           # Set timeout to 120 minutes");
        println!();
        println!("=== Create Command Example ===");
        println!("create 1383583686431080499 1388643261543088208 1385894822992281701 1385464431185494086 1385464563448680578 10");
        println!();
        println!("=== Testing Command Examples ===");
        println!("forcegen   TS1 0                   # Force team generation");
        println!("fakeplayer TS1 8                   # Add 8 fake players to group 0");
        println!("fakeplayer 1383583686431080499 0 8 # Add 8 fake players to specific group");
        println!("testnotify TS1 0                   # Test notify method");
        println!();
        println!("Available config keys: quota, timeout, dashboard, red, blue");
    }
}
