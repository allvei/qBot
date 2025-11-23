use std::sync::Arc;
use std::collections::HashMap;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter, MatchingBracketHighlighter};
use rustyline::hint::HistoryHinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Editor};
use rustyline::Helper;
use serenity::all::UserId;
use serenity::prelude::Context;
use sqlx::Row;
use tokio::sync::Mutex;
use tracing::error;

use pf_pug_bot::Database;
use pf_pug_bot::models::{Manager, Team};

/// Command definition with all metadata for auto-generated documentation
struct Command {
    name:        &'static str,
    description: &'static str,
    usage:       Vec<&'static str>,
    examples:    Vec<&'static str>,
    category:    CommandCategory,
}

#[derive(PartialEq, Eq, Hash)]
enum CommandCategory {
    Core,
    Config,
    Testing,
    System,
}

impl CommandCategory {
    fn name(&self) -> &'static str {
        match self {
            CommandCategory::Core    => "Core Commands",
            CommandCategory::Config  => "Configuration",
            CommandCategory::Testing => "Testing Commands",
            CommandCategory::System  => "System",
        }
    }
}

struct CommandRegistry {
    commands: HashMap<&'static str, Command>,
}

impl CommandRegistry {
    fn new() -> Self {
        let mut commands = HashMap::new();

        // Core commands
        commands.insert("status", Command {
            name:        "status",
            description: "Show bot status and statistics",
            usage:       vec!["status"],
            examples:    vec!["status"],
            category:    CommandCategory::Core,
        });

        commands.insert("guilds", Command {
            name:        "guilds",
            description: "List all connected guilds and their configurations",
            usage:       vec!["guilds"],
            examples:    vec!["guilds"],
            category:    CommandCategory::Core,
        });

        commands.insert("games", Command {
            name:        "games",
            description: "List all active games and players",
            usage:       vec!["games"],
            examples:    vec!["games"],
            category:    CommandCategory::Core,
        });

        // Config commands
        commands.insert("config", Command {
            name:        "config",
            description: "View or modify guild and group configurations",
            usage: vec![
                "config <guild_id>",
                "config <guild_id> <group_id> <key> <value>",
            ],
            examples: vec![
                "config TS1                  # Show all configs",
                "config TS1 0 quota 8        # Set quota to 8",
                "config TS1 0 timeout 120    # Set timeout",
            ],
            category: CommandCategory::Config,
        });

        commands.insert("create", Command {
            name: "create",
            description: "Create a new group configuration",
            usage: vec!["create <guild_id> <queue_ch> <dashboard_ch> <red_ch> <blue_ch> <quota>"],
            examples: vec![
                "create 1383583686431080499 1388643261543088208 1385894822992281701 1385464431185494086 1385464563448680578 10",
            ],
            category: CommandCategory::Config,
        });

        commands.insert("query", Command {
            name: "query",
            description: "Execute a query on the database",
            usage: vec!["query <sql>"],
            examples: vec![
                "query SELECT * FROM groups",
                "query SELECT * FROM players WHERE discord_id = 123456",
            ],
            category: CommandCategory::Config,
        });

        // Testing commands
        commands.insert("forcegen", Command {
            name: "forcegen",
            description: "Force team generation for the current queue",
            usage: vec!["forcegen <guild_id> <group_id>"],
            examples: vec![
                "forcegen TS1 0",
                "forcegen 1383583686431080499 0",
            ],
            category: CommandCategory::Testing,
        });

        commands.insert("fakeplayer", Command {
            name: "fakeplayer",
            description: "Add fake players to a queue for testing",
            usage: vec![
                "fakeplayer <guild_name> <count>",
                "fakeplayer <guild_id> <group_id> <count>",
            ],
            examples: vec![
                "fakeplayer TS1 8            # Add 8 to group 0",
                "fakeplayer 1383583686431080499 0 8",
            ],
            category: CommandCategory::Testing,
        });

        commands.insert("testnotify", Command {
            name: "testnotify",
            description: "Test the notify method (sends notification to queue chat)",
            usage: vec!["testnotify <guild_id> <group_id>"],
            examples: vec!["testnotify TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("showqueue", Command {
            name: "showqueue",
            description: "Display queue with player details and timestamps",
            usage: vec!["showqueue <guild_id> <group_id>"],
            examples: vec!["showqueue TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("showteams", Command {
            name: "showteams",
            description: "Display team compositions with stats (if generated)",
            usage: vec!["showteams <guild_id> <group_id>"],
            examples: vec!["showteams TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("clearqueue", Command {
            name: "clearqueue",
            description: "Remove all players from the queue",
            usage: vec!["clearqueue <guild_id> <group_id>"],
            examples: vec!["clearqueue TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("removeplayer", Command {
            name: "removeplayer",
            description: "Remove a player from the queue by index (0-based)",
            usage: vec!["removeplayer <guild_id> <group_id> <index>"],
            examples: vec!["removeplayer TS1 0 2  # Remove player at index 2"],
            category: CommandCategory::Testing,
        });

        commands.insert("forcehot", Command {
            name: "forcehot",
            description: "Force session to Hot status (bypass quota check)",
            usage: vec!["forcehot <guild_id> <group_id>"],
            examples: vec!["forcehot TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("forcepush", Command {
            name: "forcepush",
            description: "Force push players to team channels and set Live",
            usage: vec!["forcepush <guild_id> <group_id>"],
            examples: vec!["forcepush TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("forcepull", Command {
            name: "forcepull",
            description: "Force pull players back to queue and reset to Idle",
            usage: vec!["forcepull <guild_id> <group_id>"],
            examples: vec!["forcepull TS1 0"],
            category: CommandCategory::Testing,
        });

        commands.insert("simulate", Command {
            name: "simulate",
            description: "Simulate complete game cycle (fill → hot → push → live → pull → idle)",
            usage: vec!["simulate <guild_id> <group_id>"],
            examples: vec!["simulate TS1 0"],
            category: CommandCategory::Testing,
        });

        // System commands
        commands.insert("help", Command {
            name: "help",
            description: "Show all available commands",
            usage: vec!["help", "help -v"],
            examples: vec!["help", "help -v  # Show verbose output with usage and examples"],
            category: CommandCategory::System,
        });

        commands.insert("quit", Command {
            name: "quit",
            description: "Shutdown the console (bot continues running)",
            usage: vec!["quit"],
            examples: vec!["quit"],
            category: CommandCategory::System,
        });

        commands.insert("exit", Command {
            name: "exit",
            description: "Shutdown the console (bot continues running)",
            usage: vec!["exit"],
            examples: vec!["exit"],
            category: CommandCategory::System,
        });

        Self { commands }
    }

    fn get_command_names(&self) -> Vec<String> {
        self.commands.keys().map(|&s| s.to_string()).collect()
    }

    fn print_help(&self, verbose: bool) {
        if verbose {
            // Verbose output with full details
            println!("=== Console Command Reference ===");
            println!();

            let categories = vec![
                CommandCategory::Core,
                CommandCategory::Config,
                CommandCategory::Testing,
                CommandCategory::System,
            ];

            for category in categories {
                println!("## {}", category.name());
                println!();

                let mut category_commands: Vec<_> = self.commands.values()
                    .filter(|cmd| cmd.category == category)
                    .collect();
                category_commands.sort_by_key(|cmd| cmd.name);

                for cmd in category_commands {
                    println!("### {}", cmd.name);
                    println!("    {}", cmd.description);
                    println!();
                    println!("    Usage:");
                    for usage in &cmd.usage {
                        println!("      {}", usage);
                    }
                    if !cmd.examples.is_empty() {
                        println!();
                        println!("    Examples:");
                        for example in &cmd.examples {
                            println!("      {}", example);
                        }
                    }
                    println!();
                }
            }

            println!("Available config keys: quota, timeout, dashboard, red, blue");
        } else {
            // Compact output - just command names and descriptions
            println!("=== Available Commands ===");
            println!();

            let categories = vec![
                CommandCategory::Core,
                CommandCategory::Config,
                CommandCategory::Testing,
                CommandCategory::System,
            ];

            for category in categories {
                println!("{}:", category.name());

                let mut category_commands: Vec<_> = self.commands.values()
                    .filter(|cmd| cmd.category == category)
                    .collect();
                category_commands.sort_by_key(|cmd| cmd.name);

                for cmd in category_commands {
                    println!("  {} - {}", cmd.name, cmd.description);
                }
                println!();
            }

            println!("Use 'help -v' for detailed usage and examples");
        }
    }
}

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

    fn highlight_char(&self, line: &str, pos: usize, forced: CmdKind) -> bool {
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
    fn new(registry: &CommandRegistry) -> Self {
        Self {
            commands: registry.get_command_names(),
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
    manager:  Arc<Mutex<Manager>>,
    database: Arc<Database>,
    ctx:      Option<Arc<Context>>,
    registry: CommandRegistry,
}

impl ConsoleHandler {
    pub fn new(manager: Arc<Mutex<Manager>>, database: Arc<Database>, ctx: Arc<Context>) -> Self {
        Self {
            manager,
            database,
            ctx: Some(ctx),
            registry: CommandRegistry::new(),
        }
    }

    pub async fn start_console_loop(&self) {
        let command_names: Vec<_> = self.registry.get_command_names();
        println!("Console ready. Commands: {}", command_names.join(", "));

        let config = Config::builder()
            .completion_type(CompletionType::List)
            .auto_add_history(true)
            .build();

        let helper = CommandHelper {
            completer: CommandCompleter::new(&self.registry),
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
                    continue;
                },
                Err(ReadlineError::Eof) => {
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
                        if let Some(cmd) = self.registry.commands.get("config") {
                            println!("Usage:");
                            for usage in &cmd.usage {
                                println!("  {}", usage);
                            }
                            if !cmd.examples.is_empty() {
                                println!("Examples:");
                                for example in &cmd.examples {
                                    println!("  {}", example);
                                }
                            }
                        }
                    }
                }
            },
            "create" => {
                if parts.len() < 7 {
                    if let Some(cmd) = self.registry.commands.get("create") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                        if !cmd.examples.is_empty() {
                            println!("Examples:");
                            for example in &cmd.examples {
                                println!("  {}", example);
                            }
                        }
                    }
                } else {
                    self.cmd_create_config(parts[1], parts[2], parts[3], parts[4], parts[5], parts[6]).await?;
                }
            },
            "query" => {
                if parts.len() < 2 {
                    if let Some(cmd) = self.registry.commands.get("query") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                        if !cmd.examples.is_empty() {
                            println!("Examples:");
                            for example in &cmd.examples {
                                println!("  {}", example);
                            }
                        }
                    }
                } else {
                    let sql = parts[1..].join(" ");
                    self.cmd_query_db(&sql).await?;
                }
            },
            "forcegen" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("forcegen") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                        if !cmd.examples.is_empty() {
                            println!("Examples:");
                            for example in &cmd.examples {
                                println!("  {}", example);
                            }
                        }
                    }
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
                        if let Some(cmd) = self.registry.commands.get("fakeplayer") {
                            println!("Usage:");
                            for usage in &cmd.usage {
                                println!("  {}", usage);
                            }
                            if !cmd.examples.is_empty() {
                                println!("Examples:");
                                for example in &cmd.examples {
                                    println!("  {}", example);
                                }
                            }
                        }
                    }
                }
            },
            "testnotify" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("testnotify") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                        if !cmd.examples.is_empty() {
                            println!("Examples:");
                            for example in &cmd.examples {
                                println!("  {}", example);
                            }
                        }
                    }
                } else {
                    self.cmd_test_notify(parts[1], parts[2]).await?;
                }
            },
            "showqueue" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("showqueue") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_show_queue(parts[1], parts[2]).await?;
                }
            },
            "showteams" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("showteams") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_show_teams(parts[1], parts[2]).await?;
                }
            },
            "clearqueue" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("clearqueue") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_clear_queue(parts[1], parts[2]).await?;
                }
            },
            "removeplayer" => {
                if parts.len() < 4 {
                    if let Some(cmd) = self.registry.commands.get("removeplayer") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_remove_player(parts[1], parts[2], parts[3]).await?;
                }
            },
            "forcehot" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("forcehot") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_force_hot(parts[1], parts[2]).await?;
                }
            },
            "forcepush" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("forcepush") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_force_push(parts[1], parts[2]).await?;
                }
            },
            "forcepull" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("forcepull") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_force_pull(parts[1], parts[2]).await?;
                }
            },
            "simulate" => {
                if parts.len() < 3 {
                    if let Some(cmd) = self.registry.commands.get("simulate") {
                        println!("Usage:");
                        for usage in &cmd.usage {
                            println!("  {}", usage);
                        }
                    }
                } else {
                    self.cmd_simulate_cycle(parts[1], parts[2]).await?;
                }
            },
            "help" => {
                let verbose = parts.len() > 1 && (parts[1] == "-v" || parts[1] == "--verbose");
                self.registry.print_help(verbose);
            },
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
        match sqlx::query("SELECT DISTINCT guild_id FROM groups").fetch_all(self.database.pool()).await {
            Ok(rows) => {
                for row in rows {
                    if let Ok(guild_id) = row.try_get::<i64, _>("guild_id") {
                        println!("Guild ID: {}", guild_id);

                        match self.database.groups.get_groups_for_guild(guild_id as u64).await {
                            Ok(groups) => {
                                println!("  {} group(s) configured", groups.len());
                                for group in groups {
                                    println!("    - Group {}: Queue Channel {}", group.group_id, group.channels.queue_chat);
                                }
                            },
                            Err(e) => {
                                println!("  Error checking groups: {}", e);
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
                        game_count, server.guild_id, group.group_id, game.pool.len(), game.status);

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
        match sqlx::query("SELECT DISTINCT guild_id FROM groups").fetch_all(self.database.pool()).await {
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

        match self.database.groups.get_groups_for_guild(guild_id).await {
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

        let groups = self.database.groups.get_groups_for_guild(guild_id).await?;

        let group = groups.iter().find(|g| g.group_id == group_id)
            .ok_or(format!("Group {} not found for guild {}", group_id, guild_id))?;

        let queue_id = group.channels.queue_chat.get();

        match key.to_lowercase().as_str() {
            "quota" | "game_quota" => {
                let quota: u8 = value.parse()
                    .map_err(|_| format!("Invalid quota value: {}", value))?;

                if quota == 0 || quota > 20 {
                    println!("Quota must be between 1 and 20");
                    return Ok(());
                }

                // Update database
                match self.database.groups.update_group(
                    queue_id,
                    guild_id,
                    group.channels.dashboard.get(),
                    0,
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    quota,
                ).await {
                    Ok(_) => {
                        println!("Updated quota to {} for guild {} group {}", quota, guild_id, group_id);

                        // Update in-memory manager and dashboard
                        if let Some(ctx) = &self.ctx {
                            let mut manager = self.manager.lock().await;
                            if let Ok(group) = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id) {
                                group.quota = quota;

                                // Update dashboard to reflect new quota
                                group.queue_dash_update(ctx, guild_id).await;
                            }
                        } else {
                            println!("  Context not available, dashboard not updated");
                        }
                    },
                    Err(e) => println!("Failed to update quota: {}", e),
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
                        println!("Updated timeout to {} minutes for guild {} group {}", timeout, guild_id, group_id);

                        // Update dashboard
                        group.queue_dash_update(ctx, guild_id).await;
                    }
                } else {
                    println!("Context not available");
                }
            },
            "dashboard" => {
                let dashboard_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid dashboard channel ID: {}", value))?;

                match self.database.groups.update_group(
                    queue_id,
                    guild_id,
                    dashboard_id,
                    0,
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    group.quota as u8,
                ).await {
                    Ok(_) => println!("Updated dashboard channel to {} for guild {}", dashboard_id, guild_id),
                    Err(e) => println!("Failed to update dashboard: {}", e),
                }
            },
            "red" | "red_team" => {
                let red_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid red team channel ID: {}", value))?;

                match self.database.groups.update_group(
                    queue_id,
                    guild_id,
                    group.channels.dashboard.into(),
                    0,
                    red_id,
                    group.channels.teams.first().map(|t| t.blu_vc.get()).unwrap_or(0),
                    group.quota as u8,
                ).await {
                    Ok(_)  => println!("Updated red team channel to {} for guild {}", red_id, guild_id),
                    Err(e) => println!("Failed to update red team channel: {}", e),
                }
            },
            "blue" | "blu" | "blue_team" => {
                let blue_id: u64 = value.parse()
                    .map_err(|_| format!("Invalid blue team channel ID: {}", value))?;

                match self.database.groups.update_group(
                    queue_id,
                    guild_id,
                    group.channels.dashboard.into(),
                    0,
                    group.channels.teams.first().map(|t| t.red_vc.get()).unwrap_or(0),
                    blue_id,
                    group.quota as u8,
                ).await {
                    Ok(_) => println!("Updated blue team channel to {} for guild {}", blue_id, guild_id),
                    Err(e) => println!("Failed to update blue team channel: {}", e),
                }
            },
            _ => {
                println!("Unknown configuration key: {}", key);
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
            println!("Game quota must be between 1 and 20");
            return Ok(());
        }

        // Check if group already exists
        match self.database.groups.get_groups_for_guild(guild_id).await {
            Ok(groups) => {
                if !groups.is_empty() {
                    println!("Guild {} already has {} group configuration(s)", guild_id, groups.len());
                    println!("Use 'config' command to modify existing configurations");
                    return Ok(());
                }
            },
            Err(e) => {
                println!("  Error checking existing groups: {}", e);
            }
        }

        // Create new group configuration
        match self.database.groups.update_group(
            queue_id,
            guild_id,
            dashboard_id,
            0,
            red_id,
            blue_id,
            quota,
        ).await {
            Ok(_) => {
                println!("Created new group configuration for guild {}", guild_id);
                println!("   Queue Channel: {}",     queue_id);
                println!("   Dashboard Channel: {}", dashboard_id);
                println!("   Red Team Channel: {}",  red_id);
                println!("   Blue Team Channel: {}", blue_id);
                println!("   Game Quota: {}",        quota);
            },
            Err(e) => {
                println!("Failed to create group configuration: {}", e);
            }
        }

        Ok(())
    }

    async fn cmd_query_db(&self, sql: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match sqlx::query(sql).fetch_all(self.database.pool()).await {
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
                println!("Forcing team generation for guild {} group {} with {} players...", guild_id, group_id, session.pool.len());
                group.generate_teams(ctx, serenity::model::id::GuildId::new(guild_id), None).await;
                println!("Teams generated successfully!");

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
                println!("No active queue found for guild {} group {}", guild_id, group_id);
            }
        } else {
            println!("Context not available. Cannot generate teams without Discord context.");
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
            println!("Player count must be between 1 and 20");
            return Ok(());
        }

        let mut manager = self.manager.lock().await;
        let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

        // Get the idle session
        if let Ok(session) = group.get_queue().await {
            println!("Adding {} fake player(s) to guild {} group {}...", count, guild_id, group_id);

            // Generate fake player IDs starting from a high number to avoid conflicts
            let base_id = 9000000000000000000_u64;
            for i in 0..count {
                let fake_id = UserId::new(base_id + i as u64);
                // Use Novice rank as default for test players
                let fake_player = pf_pug_bot::Player::add(fake_id, Some(format!("FakePlayer{}", i)), None);
                session.add_player(fake_player, pf_pug_bot::Rank::Novice);
            }

            println!("Added {} fake player(s). Total players in queue: {}", count, session.pool.len());

            // Update dashboard if context is available
            if let Some(ctx) = &self.ctx {
                group.queue_dash_update(ctx, guild_id).await;
            }
        } else {
            println!("Failed to get queue for guild {} group {}", guild_id, group_id);
        }

        Ok(())
    }

    async fn cmd_test_notify(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        if let Some(ctx) = &self.ctx {
            let mut manager = self.manager.lock().await;
            let server = manager.servers.iter_mut().find(|s| s.guild_id == guild_id)
                .ok_or(format!("Server not found for guild ID: {}", guild_id))?;

            let group = server.groups.iter_mut().find(|g| g.group_id == group_id)
                .ok_or(format!("Group {} not found for guild {}", group_id, guild_id))?;

            println!("Testing notify method for guild {} group {}...", guild_id, group_id);
            group.notify(ctx, serenity::model::id::GuildId::new(guild_id)).await;
            println!("Notify method called successfully!");
            println!("   Check the queue chat channel for the notification message.");
        } else {
            println!("Context not available. Cannot test notify without Discord context.");
        }

        Ok(())
    }

    async fn cmd_show_queue(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        let mut manager = self.manager.lock().await;
        let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
        let quota = group.quota;

        if let Ok(session) = group.get_queue().await {
            println!("\n=== Queue for Guild {} Group {} ===", guild_id, group_id);
            println!("Status: {:?}", session.status);
            println!("Players in queue: {}/{}", session.pool.len(), quota);

            if session.pool.is_empty() {
                println!("  (No players in queue)");
            } else {
                println!("\nPlayers:");
                for (i, player) in session.pool.iter().enumerate() {
                    let team_str = match player.team {
                        Some(Team::Red) => " [RED]",
                        Some(Team::Blu) => " [BLU]",
                        Some(Team::Unassigned) | None => "",
                    };
                    println!("  {}. {}{}", i, player.player.discord_id, team_str);
                }
            }
        } else {
            println!("No active queue found for guild {} group {}", guild_id, group_id);
        }

        Ok(())
    }

    async fn cmd_show_teams(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        let mut manager = self.manager.lock().await;
        let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

        if let Ok(session) = group.get_queue().await {
            let red_players: Vec<_> = session.pool.iter()
                .filter(|p| p.team == Some(Team::Red))
                .collect();
            let blu_players: Vec<_> = session.pool.iter()
                .filter(|p| p.team == Some(Team::Blu))
                .collect();

            if red_players.is_empty() && blu_players.is_empty() {
                println!("No teams have been generated yet.");
                println!("   Use 'forcegen' to generate teams.");
            } else {
                println!("\n=== Teams for Guild {} Group {} ===", guild_id, group_id);

                println!("\nRed Team ({} players):", red_players.len());
                for p in red_players {
                    let elo = p.player.rank.map(|r| r.elo()).unwrap_or(30);
                    println!("  - {} (ELO: {})", p.player.discord_id, elo);
                }

                println!("\nBlue Team ({} players):", blu_players.len());
                for p in blu_players {
                    let elo = p.player.rank.map(|r| r.elo()).unwrap_or(30);
                    println!("  - {} (ELO: {})", p.player.discord_id, elo);
                }
            }
        } else {
            println!("No active queue found for guild {} group {}", guild_id, group_id);
        }

        Ok(())
    }

    async fn cmd_clear_queue(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        let mut manager = self.manager.lock().await;
        let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

        if let Ok(session) = group.get_queue().await {
            let player_count = session.pool.len();
            session.pool.clear();
            println!("Cleared {} player(s) from the queue", player_count);

            // Update dashboard if context is available
            if let Some(ctx) = &self.ctx {
                group.queue_dash_update(ctx, guild_id).await;
            }
        } else {
            println!("No active queue found for guild {} group {}", guild_id, group_id);
        }

        Ok(())
    }

    async fn cmd_remove_player(&self, guild_identifier: &str, group_id_str: &str, index_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;
        let index: usize = index_str.parse()
            .map_err(|_| format!("Invalid index: {}", index_str))?;

        let mut manager = self.manager.lock().await;
        let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

        if let Ok(session) = group.get_queue().await {
            if index >= session.pool.len() {
                println!("Index {} is out of bounds. Queue has {} player(s)", index, session.pool.len());
            } else {
                let removed_player = session.pool.remove(index);
                println!("Removed player {} from position {}", removed_player.player.discord_id, index);

                // Update dashboard if context is available
                if let Some(ctx) = &self.ctx {
                    group.queue_dash_update(ctx, guild_id).await;
                }
            }
        } else {
            println!("No active queue found for guild {} group {}", guild_id, group_id);
        }

        Ok(())
    }

    async fn cmd_force_hot(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        if let Some(ctx) = &self.ctx {
            let mut manager = self.manager.lock().await;
            let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

            if let Ok(_session) = group.get_queue().await {
                println!("Forcing session to Hot status...");
                group.hot(ctx, Some(serenity::model::id::GuildId::new(guild_id)), None, Some(self.manager.clone())).await?;
                println!("Session is now Hot!");
                println!("   Teams have been generated and players have been notified.");
            } else {
                println!("No active queue found for guild {} group {}", guild_id, group_id);
            }
        } else {
            println!("Context not available. Cannot force hot without Discord context.");
        }

        Ok(())
    }

    async fn cmd_force_push(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        if let Some(ctx) = &self.ctx {
            let mut manager = self.manager.lock().await;
            let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

            println!("Forcing push to team channels...");
            match group.push(ctx, serenity::model::id::GuildId::new(guild_id)).await {
                Ok(_) => {
                    println!("Players pushed to team channels!");
                    println!("   Session is now Live.");
                },
                Err(e) => {
                    println!("Failed to push players: {}", e);
                }
            }
        } else {
            println!("Context not available. Cannot force push without Discord context.");
        }

        Ok(())
    }

    async fn cmd_force_pull(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        if let Some(ctx) = &self.ctx {
            let mut manager = self.manager.lock().await;
            let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;

            println!("Forcing pull back to queue...");
            match group.pull(ctx, serenity::model::id::GuildId::new(guild_id), &self.database, Some(self.manager.clone())).await {
                Ok(_) => {
                    println!("Players pulled back to queue!");
                    println!("   Session reset to Idle.");
                },
                Err(e) => {
                    println!("Failed to pull players: {}", e);
                }
            }
        } else {
            println!("Context not available. Cannot force pull without Discord context.");
        }

        Ok(())
    }

    async fn cmd_simulate_cycle(&self, guild_identifier: &str, group_id_str: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild_id = self.resolve_guild_id(guild_identifier).await?;
        let group_id: u8 = group_id_str.parse()
            .map_err(|_| format!("Invalid group ID: {}", group_id_str))?;

        if let Some(ctx) = &self.ctx {
            println!("\nStarting complete game cycle simulation...\n");

            // Step 1: Add fake players to fill quota
            {
                let mut manager = self.manager.lock().await;
                let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
                let quota = group.quota as usize;

                if group.sessions.is_empty() {
                    let _ = group.create_session();
                }

                if let Ok(session) = group.get_queue().await {
                    let current_count = session.pool.len();
                    if current_count < quota {
                        let needed = quota - current_count;
                        println!("1️⃣  Adding {} fake player(s) to reach quota ({})...", needed, quota);

                        let base_id = 9000000000000000000_u64;
                        for i in 0..needed {
                            let fake_id = UserId::new(base_id + i as u64);
                            // Use Novice rank as default for test players
                            let fake_player = pf_pug_bot::Player::add(fake_id, Some(format!("FakePlayer{}", i)), None);
                            session.add_player(fake_player, pf_pug_bot::Rank::Novice);
                        }
                        println!("   Queue now has {} players\n", session.pool.len());
                    } else {
                        println!("1️⃣  Queue already has {} players (quota: {})\n", current_count, quota);
                    }
                }
            }

            // Step 2: Force Hot
            println!("2️⃣  Setting session to Hot...");
            {
                let mut manager = self.manager.lock().await;
                let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
                group.hot(ctx, Some(serenity::model::id::GuildId::new(guild_id)), None, Some(self.manager.clone())).await?;
                println!("   Session is Hot, teams generated\n");
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // Step 3: Push to team channels
            println!("3️⃣  Pushing players to team channels...");
            {
                let mut manager = self.manager.lock().await;
                let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
                match group.push(ctx, serenity::model::id::GuildId::new(guild_id)).await {
                    Ok(_) => println!("   Players pushed, session is Live\n"),
                    Err(e) => println!("     Push failed: {}\n", e),
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // Step 4: Pull back to queue
            println!("4️⃣  Pulling players back to queue...");
            {
                let mut manager = self.manager.lock().await;
                let group = manager.get_group_by_id(serenity::model::id::GuildId::new(guild_id), group_id)?;
                match group.pull(ctx, serenity::model::id::GuildId::new(guild_id), &self.database, Some(self.manager.clone())).await {
                    Ok(_) => println!("   Players pulled back, session reset to Idle\n"),
                    Err(e) => println!("     Pull failed: {}\n", e),
                }
            }

            println!("Complete game cycle simulation finished!\n");
        } else {
            println!("Context not available. Cannot simulate cycle without Discord context.");
        }

        Ok(())
    }

}
