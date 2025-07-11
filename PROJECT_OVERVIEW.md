# Passtime.tf Discord PUG Bot - Project Overview

## Introduction

This project is a Discord bot built in Rust for managing 4v4 pickup games (PUGs) for the Passtime.tf community. The bot facilitates the complete lifecycle of pickup games, from queue management to team formation, match execution, and completion. It's designed specifically for the Passtime game mode in Team Fortress 2, providing automated team balancing and voice channel management.

## Architecture Overview

The codebase follows a modular architecture with clear separation of concerns, adhering to Rust's ownership model and leveraging async/await patterns through Tokio. The application is structured around several key components:

1. **Core Domain Models**: Defined in the `models` directory, these represent the fundamental entities in the system (players, sessions, groups).

2. **Database Layer**: Provides persistent storage through SQLite and abstracts database operations.

3. **Discord Integration**: Handles interactions with the Discord API through the Serenity library.

4. **Event System**: Manages the flow of events through the application.

5. **Command Handlers**: Process Discord slash commands and execute the appropriate business logic.

## Project Structure

The codebase is organized as follows:

```text
pfpug/
├── src/                    # Source code directory
│   ├── discord/            # Discord-specific functionality
│   │   ├── commands.rs     # Command registration and routing
│   │   ├── events.rs       # Discord event handlers
│   │   ├── interactions.rs # Interaction response handling
│   │   └── mod.rs          # Module exports
│   ├── events/             # Event handling system
│   │   ├── dispatcher.rs   # Event dispatch logic
│   │   ├── handlers.rs     # Event handler implementations
│   │   ├── listeners.rs    # Event listener registration
│   │   └── mod.rs          # Module exports
│   ├── handlers/           # Command and interaction handlers
│   │   ├── admin.rs        # Admin command implementations
│   │   ├── common.rs       # Shared handler utilities
│   │   ├── player.rs       # Player command implementations
│   │   ├── runner.rs       # Runner command implementations
│   │   ├── session.rs      # Session management handlers
│   │   └── mod.rs          # Module exports
│   ├── models/             # Core data models
│   │   ├── command.rs      # Command definitions and metadata
│   │   ├── common.rs       # Shared types and utilities
│   │   ├── config.rs       # Configuration management
│   │   ├── file.rs         # File operations and utilities
│   │   ├── group.rs        # Group/division management
│   │   ├── manager.rs      # Central coordinator for components
│   │   ├── mod.rs          # Module exports
│   │   ├── player.rs       # Player data and operations
│   │   ├── server.rs       # Server-specific functionality
│   │   └── session.rs      # Game session management
│   ├── database.rs         # Database interactions and schema
│   ├── error.rs            # Error types and handling
│   ├── events.rs           # Event definitions and types
│   ├── lib.rs              # Library exports and initialization
│   └── main.rs             # Application entry point and bootstrap
├── .env                    # Environment variables (not in version control)
├── .env.example            # Example environment configuration
├── Cargo.toml              # Rust package manifest
├── Cargo.lock              # Dependency lock file
├── CODE_REVIEW.md          # Code review notes and feedback
├── TODO.md                 # Planned features and improvements
└── README.md               # Project documentation
```

## Core Domain Models

### Player (`models/player.rs`)

The `Player` struct represents a user participating in PUG games:

```rust
pub struct Player {
    /// Discord user ID of the player
    pub discord_id: u64,
    /// Steam ID64 of the player (optional)
    pub steam_id:   Option<u64>,
    /// Discord guild/server ID where this player belongs
    pub guild_id:   u64,
    /// Historical record of sessions this player has participated in
    pub session:    Vec<Option<Session>>,
    /// Backreference to the current active session ID
    pub session_id: Option<u16>,
    /// Backreference to the current group ID
    pub group_id:   Option<u64>,
    /// Player's skill rank
    pub rank:       Option<Rank>,
    /// Player's preferred role
    pub role:       Option<Role>,
    /// Whether the player is in a buffered state
    pub buffered:   bool,
    /// Current team assignment (Red or Blue)
    pub team:       Option<Team>,
}
```

Key features:

- **Player Identity**: Tracks both Discord and Steam identifiers
- **Session Tracking**: Maintains references to current and historical sessions
- **Rank System**: Implements a skill-based ranking system (`Beginner` through `Grandmaster`)
- **Role Management**: Supports different permission roles (`Runner`, `Admin`)
- **Team Assignment**: Tracks which team a player is assigned to during matches
- **Buffer Status**: Allows priority queuing for certain players

### Session (`models/session.rs`)

The `Session` struct represents an active or pending game session:

```rust
pub struct Session {
    /// Unique identifier for this session
    pub id: u16,
    /// ID of the owning group (backreference)
    pub group_id: u64,
    /// Current status of the session (Idle, Hot, Push, Live, Pull)
    pub status: SessionStatus,
    /// Collection of players currently in this session
    pub pool: Vec<Player>,
}
```

Key features:

- **Lifecycle Management**: Sessions progress through multiple states (`Idle`, `Hot`, `Push`, `Live`, `Pull`)
- **Player Pool**: Maintains the collection of players in the current session
- **Team Generation**: Implements a balanced team generation algorithm using a snake draft pattern
- **Group Association**: Each session belongs to a specific group/division

### Group (`models/group.rs`)

The `Group` struct represents a division or category of PUG games:

```rust
pub struct Group {
    /// Guild ID this group belongs to
    pub guild_id: u64,
    /// Dashboard channel ID
    pub dashboard: u64,
    /// Chat channel ID
    pub chat: u64,
    /// Queue voice channel ID
    pub queue: u64,
    /// Team channels (red/blue)
    pub teams: Vec<TeamChannels>,
    /// Collection of sessions in this group
    pub session: Vec<Session>,
    /// Session increment counter
    pub session_increment: u16,
    /// Maximum number of sessions allowed
    pub session_quota: u8,
}
```

Key features:

- **Channel Management**: Tracks all relevant Discord channels for the group
- **Session Collection**: Maintains multiple concurrent sessions
- **Team Channels**: Manages voice channels for team coordination
- **Session Quotas**: Enforces limits on the number of concurrent sessions

## Database System

The database layer is implemented in `database.rs` and provides a clean abstraction over SQLite operations:

```rust
pub struct Database {
    pool: SqlitePool,
}
```

Key features:

- **Connection Pooling**: Uses SQLx's connection pool for efficient database access
- **Schema Management**: Automatically initializes and migrates database schema
- **CRUD Operations**: Provides methods for creating, reading, updating, and deleting entities
- **Configuration Storage**: Manages bot configuration in the database

The database schema includes tables for:

- `users`: Stores player information and statistics
- `groups`: Stores group/division configurations
- `config`: Stores bot configuration key-value pairs

## Team Balancing Algorithm

### Balanced Composite Heuristic (BCH)

**Balanced Composite Heuristic (BCH)** is a team balancing method designed to fairly distribute players into two teams by considering three key statistical metrics:

- **Average (mean) ELO**
- **Median ELO**
- **Standard Deviation (spread) of ELO**

BCH evaluates all possible team splits and selects the one with the lowest combined score of these differences between the two teams, resulting in the most balanced match based on skill level and distribution.

### Why BCH?

Traditional methods like ABBAABBA or ABABABAB use drafting orders to approximate balance. However, they do not consider actual numerical distribution and are prone to:

- Outlier stacking
- Unbalanced spread of skill
- Inflexibility to different skill profiles

BCH directly evaluates balance using measurable criteria.

### How BCH Works

#### Step 1: Generate All Valid Team Splits

- For `n` players (even number), generate all unique ways to split into two equal teams.
- This is \(C(n, n/2) / 2\) combinations.

#### Step 2: Evaluate Each Split

For each team split (Team A and Team B):

1. **Calculate average (mean)** ELO for both teams
2. **Calculate median** ELO for both teams
3. **Calculate standard deviation** of ELO for both teams

#### Step 3: Score the Split

Compute the absolute differences between the two teams:

```python
avg_diff = |avg(team_a) - avg(team_b)|
med_diff = |median(team_a) - median(team_b)|
std_diff = |stddev(team_a) - stddev(team_b)|

score = avg_diff + med_diff + std_diff
```

This gives each team split a score. The lower the score, the better the balance.

#### Step 4: Pick the Best Split

Choose the split with the lowest total score.

## Discord Integration

The bot integrates deeply with Discord through the Serenity library, providing:

1. **Slash Commands**: Modern command interface with auto-completion
2. **Voice Channel Management**: Automatically moves players between team channels
3. **Role-Based Permissions**: Restricts commands based on Discord roles
4. **Embeds and Rich Messages**: Provides formatted feedback and status updates
5. **Event Handling**: Responds to Discord events like member joins/leaves

## Command System

Commands are organized by permission level and implemented in the `handlers` directory:

### Player Commands

- `/join` - Join the PUG queue
  - Adds the player to the current session's pool
  - Creates player record if they don't exist in the database
  - Notifies other players that someone has joined

- `/leave` - Leave the PUG queue
  - Removes the player from the current session
  - Updates the queue status message
  - Cancels any team assignments if applicable

### Runner Commands

- `/shuffle [match_id]` - Regenerate teams for a match
  - Reruns the team balancing algorithm
  - Updates team assignments for all players
  - Sends a new team preview message

- `/accept [match_id]` - Confirm generated teams and start match
  - Changes session status to `Push`
  - Moves players to their team voice channels
  - Updates the match status in the dashboard

- `/end [match_id]` - End a match and return players to queue
  - Changes session status to `Pull`
  - Moves players back to the queue channel
  - Records match statistics
  - Prepares for the next match

### Admin Commands

- `/buffer [user]` - Buffer a player (guarantee a spot in the current session)
  - Sets the player's buffered status
  - Ensures they'll be included in the next team generation
  - Updates the buffer list in the dashboard

- `/config [key] [value]` - View or modify bot configuration
  - Updates configuration in the database
  - Supports changing channel IDs, role IDs, and other settings
  - Validates input values before saving

## Session Lifecycle

A PUG session progresses through several states, each with specific behaviors. The state transitions are fully event-driven, automatically handling player movement, team assignments, and notifications:

1. **Idle**: Initial state, waiting for players to join the queue
   - Players can freely join and leave either by using commands or by joining/leaving the queue channel
   - No teams are generated yet
   - Session monitors the player count to determine when to transition to Hot

2. **Hot**: Enough players have joined to form teams
   - Team generation algorithm runs automatically using the BCH algorithm
   - Preview of teams is displayed in the dashboard channel with mentions
   - Players are notified that the session is ready to start
   - Runners can shuffle or accept teams

3. **Push**: Teams are confirmed, moving players to team channels
   - Players are automatically moved to their assigned team voice channels
   - Team assignments are finalized and stored
   - Match status is updated in the dashboard with a visual indicator
   - Notifications are sent to relevant channels that the match is starting

4. **Live**: The match is in progress
   - Players remain in their team channels
   - Bot sends notifications to dashboard and chat channels
   - Match status is updated to indicate the game is in progress
   - Bot monitors for match completion

5. **Pull**: Match is ending, returning players to queue
   - Players are automatically moved back to the queue channel
   - Team assignments are reset for all players
   - Match statistics are recorded for future reference
   - Notifications are sent to dashboard and chat channels
   - New session begins or continues in Idle state

Each state transition is implemented as an asynchronous method that handles all related actions, ensuring that the session state, player locations, team assignments, and notifications are always synchronized.

## Error Handling

The error handling system is defined in `error.rs` and provides:

1. **Custom Error Types**: Domain-specific error types for better context
2. **Error Propagation**: Uses the `?` operator extensively for clean error handling
3. **Logging Integration**: Errors are automatically logged with appropriate context
4. **User-Friendly Messages**: Errors are translated to user-friendly Discord messages

The system uses `thiserror` for defining error types and `anyhow` for error propagation:

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Discord API error: {0}")]
    DiscordError(String),
    
    #[error("Session error: {0}")]
    SessionError(String),
    
    #[error("Player error: {0}")]
    PlayerError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type AppResult<T> = Result<T, AppError>;
```

## Configuration System

The bot's configuration is managed through:

1. **Environment Variables**: Basic configuration in `.env` file
2. **Database Storage**: Dynamic configuration stored in the `config` table
3. **In-Memory Cache**: Frequently accessed config values are cached
4. **Discord Commands**: Admins can update config via `/config` command

Key configuration values include:

- Discord token and application ID
- Guild/server ID
- Channel IDs for queue, dashboard, teams
- Role IDs for permissions
- Session quotas and limits

## Logging and Monitoring

The application uses the `tracing` crate for structured logging:

1. **Log Levels**: Different verbosity levels (error, warn, info, debug, trace)
2. **Contextual Information**: Logs include context like session IDs and player IDs
3. **Performance Metrics**: Key operations are timed and logged
4. **Discord Integration**: Critical logs can be sent to a Discord channel

## Development Workflow

1. **Local Development**:
   - Run with `cargo run` for local testing
   - Environment variables in `.env` control configuration
   - SQLite database provides persistence

2. **Testing**:
   - Unit tests for core business logic
   - Integration tests for database operations
   - Manual testing through Discord interactions

3. **Deployment**:
   - Build with `cargo build --release`
   - Deploy as a standalone binary
   - Configure through environment variables or database

## Future Development

Planned features and improvements are documented in `TODO.md`, including:

1. **Enhanced Team Balancing**: More sophisticated algorithms for team formation
2. **Statistics Tracking**: Detailed player and match statistics
3. **Web Dashboard**: Browser-based interface for administration
4. **Multi-Server Support**: Better handling of multiple Discord servers
5. **Integration with Game Servers**: Automatic server configuration and map rotation

## Security Considerations

1. **Token Management**: Discord token is stored securely in environment variables
2. **Permission System**: Commands are restricted based on Discord roles
3. **Input Validation**: All user inputs are validated before processing
4. **Error Handling**: Errors are handled gracefully without exposing sensitive information

## Conclusion

The Passtime.tf Discord PUG Bot is a comprehensive solution for managing pickup games, with a focus on team balance, automation, and user experience. Its modular architecture allows for easy extension and maintenance, while the Rust implementation provides safety and performance benefits.
