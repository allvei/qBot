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
