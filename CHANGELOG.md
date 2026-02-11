__12.02.26__
## Users & Admins
__Added__
- Auto-end games when all players leave team voice channels.
- Enhanced team voice channel management with flexible cleanup policies.
- Comprehensive server settings system with toggle states and modular menus.
- Text sanitization for alert messages to prevent spam and improve readability.

__Improved__
- Settings menu now uses modular components for better maintainability.
- Better error handling and user feedback across all settings interfaces.
- Enhanced logging with guild context for better debugging.
- Improved team voice channel cleanup with force parameter for destroy policies.

__Fixed__
- Removed UNIQUE constraints from team voice channels to allow flexible channel reuse.
- Fixed formatting issues in various UI components and logging messages.
- Resolved compilation errors in edit command logging.

## Developers
__Added__
- Team VC auto-end event handling in main event loop.
- Force parameter for team VC cleanup to support different destroy policies.
- Helper methods for identifying team channels and checking VC emptiness.

__Refactored__
- Complete overhaul of settings system with modular, reusable components.
- Enhanced database migration system for safer schema changes.
- Improved error handling and logging throughout the application.
- Simplified embeds model and removed redundant functionality.

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
