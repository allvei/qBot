# v0.14.0

__21.05.26__

## Users & Admins

__Added__

- __Auto-leave queue on disconnect__ - New user preference to automatically leave the queue when disconnecting from the voice channel. Found in `/prefs` menu.
- __GUI user management__ - Desktop app now includes a panel for viewing and managing server users with guild activity indicators.
- __Guild-scoped deletion__ - Admins can delete all bot data for a specific server through the desktop app.

__Improved__

- __Dashboard ELO display__ - ELO values now consistently respect the dynamic ELO setting across all dashboard views.
- __Runner menu flow__ - ELO confirmation handling and runner menu interactions work more smoothly.
- __GUI integration__ - Desktop app now runs as part of the main application for live updates and better sync with Discord.

__Fixed__

- __Score submission freezing__ - Fixed a deadlock that could cause the bot to freeze when submitting match scores.
- __Player lookup reliability__ - Players with missing rank data now default to Unranked instead of causing lookup issues.

## Developers

__Added__

- __User search__ - Added user search functionality for easier player lookup.
- __Guild deletion helper__ - Added helper for complete guild data removal.

__Refactored__

- __Settings naming__ - Renamed "server settings" and "user settings" for clearer separation.
- __Code formatting__ - Standardized formatting with 2-space indentation across the entire codebase.

# v0.13.0

__18.05.26__

## Users & Admins

__Added__

- __Dynamic ELO admin controls__ - Server admins can now view and edit player dynamic ELO ratings through the `/edit` command. Shows current dynamic ELO value alongside regular ELO with an edit button for manual adjustments.
- __Skill selection for new players__ - First-time queue joiners are prompted to select their skill tier (Beginner, Intermediate, Expert, Veteran) to set their initial dynamic ELO rating.
- __Hiatus boost system__ - Players returning after inactivity receive increased ELO mobility, making it easier to recover their rating after a break.
- __Dynamic ELO system__ - Player ratings now adjust after each match based on team performance and opponent strength. Live toggle in guild config enables or disables the feature.
- __Match result reporting__ - Runners can report match scores through the dashboard, with ELO adjustments automatically calculated based on the result.
- __Match result correction__ - Incorrect match results can be changed through the runner menu, with automatic ELO re-calculation to correct player ratings.
- __ELO privacy option__ - Server setting to hide ELO values from dashboards while still using them internally for team balancing.
- __GUI admin panel__ - Complete desktop interface for managing queues, sessions, and testing. Includes buttons for all queue operations, session state controls, and recovery tools.
- __GUI queue panel__ - Desktop interface showing all categories and formats with live updates. Right-click players for quick actions like Remove, Buffer, or Fatkid.
- __Copy to clipboard__ - Right-click on guilds or players in the GUI to copy their identifiers. Hover tooltips show additional information.
- __Live dashboard sync__ - GUI actions immediately update Discord dashboard embeds, keeping both interfaces in sync.

__Improved__

- __Queue rank handling__ - Rank assignment now works even when Discord access is missing or not configured, falling back to the lowest rank automatically.
- __Admin access__ - Admins can now use runner menu commands and buttons without needing explicit runner access.
- __Channel creation reliability__ - Voice channels can now be created even when the bot lacks permission overwrite capabilities, preventing setup failures.
- __Dashboard responsiveness__ - Discord dashboard updates instantly after any GUI action, removing previous delay.
- __Player move logging__ - Consolidated log messages for player movement between voice channels for clearer tracking.

__Fixed__

- __Match ending deadlock__ - Fixed bot freezing when attempting to end a game through the dashboard. Runners can now reliably end matches without the console becoming unresponsive.
- __Queue button handling__ - Fixed double-acknowledgment errors when joining queue through dashboard buttons.
- __Balance menu routing__ - Guild config balance selection menu now correctly routes to its handler.
- __Unmigrated player ELO__ - Players without existing ELO data now default to 1500 instead of causing errors.

## Developers

__Added__

- __Inactivity tracking__ - Players now have last-game timestamps stored, enabling the hiatus boost calculation for returning players.
- __Dynamic ELO admin editing__ - Admin interface for viewing and modifying player dynamic ELO values directly through Discord commands.
- __Skill selection system__ - New player onboarding flow with tier-based initial ELO assignment.
- __Double-end protection__ - Guard against duplicate match result submissions preventing race conditions when ending games.
- __Dynamic ELO calculation__ - Complete ELO adjustment system with configurable K-factor, decay rate, and minimum/maximum bounds.
- __Match result tracking__ - Data storage for match outcomes and ELO changes with rollback capability.
- __GUI command system__ - Full command handler for desktop interface with queue, session, testing, and recovery operations.
- __Live ELO toggle__ - Runtime toggle for dynamic ELO system without requiring restart.
- __Headless mode__ - Command-line flag (-nogui) to run bot without desktop GUI.

__Refactored__

- __Guild config storage__ - Moved guild_name and team_balance_method to appropriate data storage for better data organization.
- __Queue panel structure__ - GUI queue panel restructured to show categories in main content instead of sidebar.
- __Logging consolidation__ - Player movement logs unified across different code paths for consistency.
- __Dynamic ELO constants__ - Configuration values extracted to dedicated constants module.

# v0.12.0

__25.03.26__

## Users & Admins

__Added__

- __Runner menu enhancements__ - Comprehensive runner controls with dedicated menu for match management, player administration, and queue operations.
- __Ping button__ - Dashboard button to ping all players in queue voice channel for immediate attention.
- __VC join notifications__ - Real-time alerts when players join queue voice channel during active sessions.
- __Forced score reporting__ - Optional per-category setting to require score submission before ending matches. End button becomes 'End & log score' with backup 'End without score' option in runner menu.

__Improved__

- __Dashboard interactions__ - Duplicate button presses now prevented with 2-second cooldown per session. Start/End actions include format name in logs for clarity.
- __Member lookup performance__ - Three-tier fallback pattern: Cache (fastest) → Database → Discord API (slowest). Reduces API calls and improves reliability during Discord outages.
- __Match notifications__ - Queue-full pings now only sent to players not already in queue voice channel, eliminating redundant notifications for players already present.
- __Parallel user movement__ - Moving users between voice channels now happens in parallel making it instant.

__Fixed__

- __Queue leave during matches__ - Players can no longer leave queue during live matches via Leave button. Must find substitutes manually.
- __Team voice channel handling__ - Players moved by bot from queue VC to team VC no longer incorrectly removed from queue. Prevents premature player ejection after match start.

# v0.11.0

__11.03.26__

## Users & Admins

__Added__

- __Fatkid immunity system__ - Players who finish a match are protected from being skipped when rejoining queue. First 2 games grant automatic immunity, then 3-day cooldown or until all others have immunity. Ensures fair rotation when queue exceeds capacity.
- __Help button__ - New Help button on dashboard provides clear explanation of queue mechanics, match flow, and bot features.

__Improved__

- __Queue re-entry after matches__ - When players finish a match and queue is full, bot now intelligently selects who gets added based on immunity status rather than random selection.
- __Log clarity__ - Dashboard creation and updates now show proper guild/category prefixes in logs for easier debugging.

## Developers

__Added__

- __Fatkid immunity tracking__ - Dedicated database table and repository for tracking player immunity status separately from ELO data.
- __Player immunity info__ - Combined immunity check function reduces database queries by 50% with single call returning both immunity status and level.

__Refactored__

- __Player selection logic__ - Extracted 80-line fatkid immunity selection into dedicated function for better testability and maintainability.
- __Dashboard footer buttons__ - Centralized Preferences/Runner Menu/Help button generation into single reusable function.
- __Match recording__ - Extracted match database recording into standalone function, reducing dash_end from 45 lines to 8-line function call.
- __Code organization__ - Improved separation of concerns across immunity system, dashboard components, and match recording.

# v0.10.0

__11.03.26__

## Users & Admins

__Added__

- __Match score reporting__ - Runners can now report final scores on match history messages using a Report Score button. Scores appear at the top of the match summary.
- __Team switch detection__ - Bot now detects when two players swap teams during a live match and automatically updates their assignments after 2 minutes.

__Improved__

- __Matched team order to in-game__ - Blue first, red second.
- __Player visibility__ - All players with team assignments now show correctly on dashboard. Previously, new players joining after someone left weren't displayed (showing 4v3 instead of 4v4).
- __Guild config info__ - Main settings screen now shows all config options for guidance.

__Fixed__

- __Command parsing__ - Fixed /fatkid and /buffer commands not parsing user mentions correctly.
- __Orphaned team channels__ - Players in orphaned team channels are now moved back to queue voice channel during cleanup.
- __Timeout behavior__ - Players in team channels are no longer removed by timeout system, only players in queue.
- __Post-game notifications__ - Players who just finished a match are no longer pinged when queue fills up. Only players who were waiting in queue get notified.

## Developers

__Added__

- __File logging system__ - Daily rotating log files in logs/ directory with structured output for debugging and auditing.
- __Log filtering__ - Separate console and file log levels to reduce noise from Discord and HTTP libraries.
- __Team switch tracking__ - New pending_team_switch field in Session to track and validate team swaps.
- __Modular settings system__ - Reorganized settings into logical modules (menu, ui, server, categories, ranks, player_admin, alerts, core, utils).

__Refactored__

- __Settings architecture__ - Complete restructure of settings system with AsSettingsMenu trait and organized component structure.
- __Naming conventions__ - Standardized category_id to ctg_id across codebase for consistency.
- __Shutdown logging__ - Improved dashboard offline messages with consistent prefix formatting.
- __Queue logging__ - Simplified log_queue_toggle to use Format, Player, and Action for cleaner code.
- __Timestamp utilities__ - Renamed now() to timestamp_now() for better clarity.

# v0.9.0

__19.02.26__

## Users & Admins

__Fixed__

- __Team voice channel protection__ - Properly track team voice channels to avoid deleting unrelated channels.

## Developers

__Added__

- __Team channel tracking__ - Voice channels are now tracked in persistent storage for better management
- __Settings abstraction helpers__ - Comprehensive macros and helper functions to reduce code duplication

__Refactored__

- __Settings system__ - Major code reduction with reusable patterns for modal interactions and data fetching
- __Directory structure__ - Shortened directory names to reduce log width and improve readability
- __Error handling__ - Consistent functional style patterns across settings components

# v0.7.0

__16.02.26__

## Users & Admins

__Fixed__

- __Queue display updates__ - Player count now shows correctly immediately after joining queue.
- __Alert message formatting__ - Fixed text handling that could cause overly long alert messages to break display.

## Developers

__Added__

- Updated rand dependency to version 0.10.0 for better speed and security.
- Error response helper functions to reduce code duplication across settings handlers.

__Refactored__

- Complete terminology change from "groups" to "categories" throughout the entire codebase for better consistency with user-facing features.
- Consolidated error response handling in settings component with reusable helper functions.
- Improved log formatting system with better subgroup support and clearer prefixes.

# v0.6.0

__12.02.26__

## Users & Admins

__Added__

- __/remove__ - Remove all players from queue, or remove a specific player when used with a user mention.

## New Commands

- __/remove__ - Remove all players from queue, or remove a specific player when used with a user mention.

## Developers

__Refactored__

- Updated internal terminology from "groups" to "categories" throughout the codebase for better consistency with user-facing features.

# v0.5.0

__12.02.26__

## Users & Admins

__Added__

- __Formats support__ - Categories can now contain multiple formats, each with their own quota, sessions, and settings.
- __Dynamic team voice channels__ - Automatic creation and cleanup of team voice channels based on configurable policies.
- __Rank-gated categories__ - Restrict channel category visibility by rank with access overwrites.
- __Automatic game ending__ - Games now end when all players leave team voice channels.
- __ELO-Rank independent mode__ - Option to decouple ELO from ranks for more flexible player management.
- __Category creation wizard__ - Create new categories with custom names, categories, and quotas through an intuitive menu system.
- __Complete category removal__ - Delete entire categories including all channels and categories with detailed feedback.
- __Alert rate limiting__ - Join/leave alerts are now buffered and rate-limited to prevent spam.
- __Offline mode indicators__ - Dashboard now shows when bot is offline and hides buttons during downtime.

__Improved__

- __Multi-format dashboard__ - Dashboard now displays and manages multiple formats within a category.
- __Team voice channel lifecycle__ - Configurable policies for when to create/destroy team channels (on join, on hot, on game start & on leave, after pull, after timeout).
- __Queue timeout refresh__ - Re-joining via dashboard button now refreshes your queue timeout.
- __Better setup flow__ - Enhanced 7-step setup process with queue voice channel configuration.
- __Player ELO validation__ - Automatic ELO normalization based on Discord rank ranges.
- __Cross-category player tracking__ - See in-game status across all formats on dashboards.

__Fixed__

- __Rank access matching__ - Better handling of duplicate rank names in dropdown menus.
- __ELO wipe__ - Fixed player ELO data being wiped on startup.
- __Player settings__ - Fixed rank determination to check Discord roles before data storage.
- __Steam profile links__ - Now hidden when no Steam identifier is configured.

## New Commands

- __/migrate__ - Bulk-assign ELO to all members with a specific Discord access.

## Developers

__Added__

- Format system with per-format session management.
- ConfigToggle system for declarative configuration options.
- Alert limiter component for rate-limiting notifications.
- DM tracker for proper cleanup of direct messages.
- Bulk update request system for dashboard efficiency.
- Pre-push Git hook to enforce changelog and version updates.

__Refactored__

- Complete settings system redesign with modular components.
- Data structure updates for formats and team VC persistence.
- Admin command consolidation with data-driven wizard steps.
- Enhanced error handling throughout the application.
- Improved voice state handling and rank assignment.

__05.02.26__

## Users & Admins

__Added__

- Offline indicator on dashboard when bot is shut down.
- Disabled dashboard buttons when bot is offline to prevent interactions during downtime.
- Confirmation prompt when changing player ELO outside their current rank range.
  - Shows current vs new rank and ELO values
  - Displays which Discord roles will be updated
  - Automatically removes old rank access and assigns new rank access on confirmation
- Rank selection dropdown in player settings for easier rank management.

__Improved__

- Player settings now show rank selection dropdown instead of manual entry.
- Steam profile links now hide when no Steam identifier is configured (shows "Not set").
- Dashboard updates immediately when admin changes player ELO or rank.
- Better error handling for missing ranks and configuration issues.

__Fixed__

- Player rank now correctly determined from Discord access instead of stale data storage entries.
- Players without data storage records can now be edited via `/edit`.
- Missing default rank configuration now shows helpful error message instead of crashing.
- ELO and rank assignment using invalid data storage entries resulting in incorrect values.
- Access checking logic for runner and admin control.

## Developers

__Added__

- Discord tag caching system for improved speed and reduced system calls.
- Pre-push Git hook to enforce changelog and version updates.

__Improved__

- ELO validation system ensures values stay within Discord rank ranges.
- ELO normalization system validates player ELO against Discord rank ranges.
- Player settings now check Discord permissions first before falling back to data storage.

__Fixed__

- Foreign key mismatch in config data storage for default rank reference.
- Data structure issues with foreign key constraints.

__Refactored__

- Data structure restructured for better foreign key integrity.
- Player.rank changed to Option<Rank> for safer null handling.
- Removed redundant identifier column from users data storage.
- Cleaned up deprecated configuration methods.
- Enabled foreign key constraints in SQLite.

__14.01.26__

## Users & Admins

__Added__

- DM notifications for when queue is almost full.
- Display "Offline" on dashboard when bot is shut down.
- Display when a game started on dashboard.
- Guild-specific ELO with ranks.

__Improved__

- More consistent and cleaner UI.
- ELO assignment beyond 100.

__Fixed__

- ELO and rank assignment using invalid data storage entries resulting in incorrect ELO values.
- Users no longer time out when in voice.

## Known Issues

- Auto-leave and auto-join are currently being overridden by voice verification.
  This means despite having disabled auto-leave queue, you would still be removed.
