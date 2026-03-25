# v0.12.0
__25.03.26__
## Users & Admins
__Added__
- **Runner menu enhancements** - Comprehensive runner controls with dedicated menu for match management, player administration, and queue operations.
- **Ping button** - Dashboard button to ping all players in queue voice channel for immediate attention.
- **VC join notifications** - Real-time alerts when players join queue voice channel during active sessions.
- **Forced score reporting** - Optional per-category setting to require score submission before ending matches. End button becomes 'End & log score' with backup 'End without score' option in runner menu.
- **Per-player timeout system** - Individual scheduled timeouts for each player with automatic cleanup and queue removal.

__Improved__
- **Dashboard interactions** - Duplicate button presses now prevented with 2-second cooldown per session. Start/End actions include format name in logs for clarity.
- **Member lookup performance** - Three-tier fallback pattern: Cache (fastest) → Database → Discord API (slowest). Reduces API calls and improves reliability during Discord outages.
- **Match notifications** - Queue-full pings now only sent to players not already in queue voice channel, eliminating redundant notifications for players already present.
- **Parallel user movement** - Moving users between voice channels now happens in parallel making it instant.

__Fixed__
- **Queue leave during matches** - Players can no longer leave queue during live matches via Leave button. Must find substitutes manually.
- **Team voice channel handling** - Players moved by bot from queue VC to team VC no longer incorrectly removed from queue. Prevents premature player ejection after match start.

# v0.11.0
__11.03.26__
## Users & Admins
__Added__
- **Fatkid immunity system** - Players who finish a match are protected from being skipped when rejoining queue. First 2 games grant automatic immunity, then 3-day cooldown or until all others have immunity. Ensures fair rotation when queue exceeds capacity.
- **Help button** - New Help button on dashboard provides clear explanation of queue mechanics, match flow, and bot features.

__Improved__
- **Queue re-entry after matches** - When players finish a match and queue is full, bot now intelligently selects who gets added based on immunity status rather than random selection.
- **Log clarity** - Dashboard creation and updates now show proper guild/category prefixes in logs for easier debugging.

## Developers
__Added__
- **Fatkid immunity tracking** - Dedicated database table and repository for tracking player immunity status separately from ELO data.
- **Player immunity info** - Combined immunity check function reduces database queries by 50% with single call returning both immunity status and level.

__Refactored__
- **Player selection logic** - Extracted 80-line fatkid immunity selection into dedicated function for better testability and maintainability.
- **Dashboard footer buttons** - Centralized Preferences/Runner Menu/Help button generation into single reusable function.
- **Match recording** - Extracted match database recording into standalone function, reducing dash_end from 45 lines to 8-line function call.
- **Code organization** - Improved separation of concerns across immunity system, dashboard components, and match recording.

# v0.10.0
__11.03.26__
## Users & Admins
__Added__
- **Match score reporting** - Runners can now report final scores on match history messages using a Report Score button. Scores appear at the top of the match summary.
- **Team switch detection** - Bot now detects when two players swap teams during a live match and automatically updates their assignments after 2 minutes.

__Improved__
- **Matched team order to in-game** - Blue first, red second.
- **Player visibility** - All players with team assignments now show correctly on dashboard. Previously, new players joining after someone left weren't displayed (showing 4v3 instead of 4v4).
- **Server settings info** - Main settings screen now shows all config options for guidance.

__Fixed__
- **Command parsing** - Fixed /fatkid and /buffer commands not parsing user mentions correctly.
- **Orphaned team channels** - Players in orphaned team channels are now moved back to queue voice channel during cleanup.
- **Timeout behavior** - Players in team channels are no longer removed by timeout system, only players in queue.
- **Post-game notifications** - Players who just finished a match are no longer pinged when queue fills up. Only players who were waiting in queue get notified.

## Developers
__Added__
- **File logging system** - Daily rotating log files in logs/ directory with structured output for debugging and auditing.
- **Log filtering** - Separate console and file log levels to reduce noise from Discord and HTTP libraries.
- **Team switch tracking** - New pending_team_switch field in Session to track and validate team swaps.
- **Modular settings system** - Reorganized settings into logical modules (menu, ui, server, categories, ranks, player_admin, alerts, core, utils).

__Refactored__
- **Settings architecture** - Complete restructure of settings system with AsSettingsMenu trait and organized component structure.
- **Naming conventions** - Standardized category_id to ctg_id across codebase for consistency.
- **Shutdown logging** - Improved dashboard offline messages with consistent prefix formatting.
- **Queue logging** - Simplified log_queue_toggle to use Format, Player, and Action for cleaner code.
- **Timestamp utilities** - Renamed now() to timestamp_now() for better clarity.

# v0.9.0
__ 19.02.26__
## Users & Admins
__Fixed__
- **Team voice channel protection** - Properly track team voice channels to avoid deleting unrelated channels.

## Developers
__Added__
- **Team channel tracking** - Voice channels are now tracked in persistent storage for better management
- **Settings abstraction helpers** - Comprehensive macros and helper functions to reduce code duplication

__Refactored__
- **Settings system** - Major code reduction with reusable patterns for modal interactions and data fetching
- **Directory structure** - Shortened directory names to reduce log width and improve readability
- **Error handling** - Consistent functional style patterns across settings components

# v0.7.0
__ 16.02.26__
## Users & Admins
__Fixed__
- **Queue display updates** - Player count now shows correctly immediately after joining queue.
- **Alert message formatting** - Fixed text handling that could cause overly long alert messages to break display.

## Developers
__Added__
- Updated rand dependency to version 0.10.0 for better speed and security.
- Error response helper functions to reduce code duplication across settings handlers.

__Refactored__
- Complete terminology change from "groups" to "categories" throughout the entire codebase for better consistency with user-facing features.
- Consolidated error response handling in settings component with reusable helper functions.
- Improved log formatting system with better subgroup support and clearer prefixes.

# v0.6.0
__ 12.02.26__
## Users & Admins
__Added__
- **/remove** - Remove all players from queue, or remove a specific player when used with a user mention.

## New Commands
- **/remove** - Remove all players from queue, or remove a specific player when used with a user mention.

## Developers
__Refactored__
- Updated internal terminology from "groups" to "categories" throughout the codebase for better consistency with user-facing features.

# v0.5.0
__ 12.02.26__
## Users & Admins
__Added__
- **Formats support** - Categories can now contain multiple formats, each with their own quota, sessions, and settings.
- **Dynamic team voice channels** - Automatic creation and cleanup of team voice channels based on configurable policies.
- **Rank-gated categories** - Restrict channel category visibility by rank with access overwrites.
- **Automatic game ending** - Games now end when all players leave team voice channels.
- **ELO-Rank independent mode** - Option to decouple ELO from ranks for more flexible player management.
- **Category creation wizard** - Create new categories with custom names, categories, and quotas through an intuitive menu system.
- **Complete category removal** - Delete entire categories including all channels and categories with detailed feedback.
- **Alert rate limiting** - Join/leave alerts are now buffered and rate-limited to prevent spam.
- **Offline mode indicators** - Dashboard now shows when bot is offline and hides buttons during downtime.

__Improved__
- **Multi-format dashboard** - Dashboard now displays and manages multiple formats within a category.
- **Team voice channel lifecycle** - Configurable policies for when to create/destroy team channels (on join, on hot, on game start & on leave, after pull, after timeout).
- **Queue timeout refresh** - Re-joining via dashboard button now refreshes your queue timeout.
- **Better setup flow** - Enhanced 7-step setup process with queue voice channel configuration.
- **Player ELO validation** - Automatic ELO normalization based on Discord rank ranges.
- **Cross-category player tracking** - See in-game status across all formats on dashboards.

__Fixed__
- **Rank access matching** - Better handling of duplicate rank names in dropdown menus.
- **ELO wipe** - Fixed player ELO data being wiped on startup.
- **Player settings** - Fixed rank determination to check Discord roles before data storage.
- **Steam profile links** - Now hidden when no Steam identifier is configured.

## New Commands
- **/migrate** - Bulk-assign ELO to all members with a specific Discord access.

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
