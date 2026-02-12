# v0.5.0
__ 12.02.26__
## Users & Admins
__Added__
- **Subgroups support** - Groups can now contain multiple subgroups, each with their own quota, sessions, and settings.
- **Dynamic team voice channels** - Automatic creation and cleanup of team voice channels based on configurable policies.
- **Rank-gated categories** - Restrict channel category visibility by rank with permission overwrites.
- **Automatic game ending** - Games now end when all players leave team voice channels.
- **ELO-Rank independent mode** - Option to decouple ELO from ranks for more flexible player management.
- **Group creation wizard** - Create new groups with custom names, categories, and quotas through an intuitive modal interface.
- **Complete group removal** - Delete entire groups including all channels and categories with detailed feedback.
- **Alert rate limiting** - Join/leave alerts are now buffered and rate-limited to prevent spam.
- **Offline mode indicators** - Dashboard now shows when bot is offline and hides buttons during downtime.

__Improved__
- **Multi-subgroup dashboard** - Dashboard now displays and manages multiple subgroups within a group.
- **Team voice channel lifecycle** - Configurable policies for when to create/destroy team channels (on join, on hot, on game start & on leave, after pull, after timeout).
- **Queue timeout refresh** - Re-joining via dashboard button now refreshes your queue timeout.
- **Better setup flow** - Enhanced 7-step setup process with queue voice channel configuration.
- **Player ELO validation** - Automatic ELO normalization based on Discord rank ranges.
- **Cross-group player tracking** - See in-game status across all subgroups on dashboards.

__Fixed__
- **Rank role matching** - Better handling of duplicate rank names in dropdown menus.
- **ELO wipe** - Fixed player ELO data being wiped on startup.
- **Player settings** - Fixed rank determination to check Discord roles before database.
- **Steam profile links** - Now hidden when no Steam ID is configured.

## New Commands
- **/migrate** - Bulk-assign ELO to all members with a specific Discord role.

## Developers
__Added__
- Subgroup system with per-subgroup session management.
- ConfigToggle system for declarative configuration options.
- Alert limiter module for rate-limiting notifications.
- DM tracker for proper cleanup of direct messages.
- Bulk update request system for dashboard efficiency.
- Pre-push Git hook to enforce changelog and version updates.

__Refactored__
- Complete settings system redesign with modular components.
- Database schema updates for subgroups and team VC persistence.
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
  - Automatically removes old rank role and assigns new rank role on confirmation
- Rank selection dropdown in player settings for easier rank management.

__Improved__
- Player settings now show rank selection dropdown instead of manual entry.
- Steam profile links now hide when no Steam ID is configured (shows "Not set").
- Dashboard updates immediately when admin changes player ELO or rank.
- Better error handling for missing ranks and configuration issues.

__Fixed__
- Player rank now correctly determined from Discord roles instead of stale database entries.
- Players without database records can now be edited via `/edit`.
- Missing default rank configuration now shows helpful error message instead of crashing.
- ELO and rank assignment using invalid database entries resulting in incorrect values.
- Role checking logic for runner and admin permissions.

## Developers
__Added__
- Discord tag caching system for improved performance and reduced API calls.
- Pre-push Git hook to enforce changelog and version updates.

__Improved__
- ELO validation system ensures values stay within Discord rank ranges.
- ELO normalization system validates player ELO against Discord rank ranges.
- Player settings now check Discord roles first before falling back to database.

__Fixed__
- Foreign key mismatch in config table for default rank reference.
- Database schema issues with foreign key constraints.

__Refactored__
- Database schema restructured for better foreign key integrity.
- Player.rank changed to Option<Rank> for safer null handling.
- Removed redundant id column from users table.
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
- ELO and rank assignment using invalid database entries resulting in incorrect ELO values.
- Users no longer time out when in voice.

## Known Issues
- Auto-leave and auto-join are currently being overridden by voice verification.
  This means despite having disabled auto-leave queue, you would still be removed.
