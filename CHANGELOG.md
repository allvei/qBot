__05.02.26__
## Users
__Added__
- Discord tag caching for improved performance and reduced API calls.
- Offline indicator on dashboard when bot is shut down.
- Disabled dashboard buttons when bot is offline to prevent interactions during downtime.

__Improved__
- Player settings now show rank selection dropdown instead of manual entry.
- Steam profile links now hide when no Steam ID is configured (shows "Not set").
- Dashboard updates immediately when admin changes player ELO or rank.
- ELO validation ensures values stay within Discord rank ranges.

__Fixed__
- Player rank now correctly determined from Discord roles (source of truth) instead of stale database entries.
- Players without database records can now be edited via `/editplayer`.
- Missing default rank configuration now shows helpful error message instead of crashing.
- Foreign key mismatch in config table for default rank reference.
- ELO and rank assignment using invalid database entries resulting in incorrect values.

## Admins
__Added__
- Confirmation prompt when changing player ELO outside their current rank range.
  - Shows current vs new rank and ELO values
  - Displays which Discord roles will be updated
  - Automatically removes old rank role and assigns new rank role on confirmation
- Rank selection dropdown in player settings for easier rank management.
- Dashboard update notifications when player ELO/rank changes.

__Improved__
- ELO normalization system validates player ELO against Discord rank ranges.
- Player settings now check Discord roles first before falling back to database.
- Better error handling for missing ranks and configuration issues.

__Fixed__
- Database schema issues with foreign key constraints.
- Role checking logic for runner and admin permissions.
- Player rank assignment now properly uses Option<Rank> throughout codebase.

## Technical
__Refactored__
- Database schema restructured for better foreign key integrity.
- Player.rank changed to Option<Rank> for safer null handling.
- Removed redundant id column from users table.
- Cleaned up deprecated configuration methods.
- Enabled foreign key constraints in SQLite.

__14.01.26__
## Users
__Added__
- DM notifications for when queue is almost full.
- Display "Offline" on dashboard when bot is shut down.
- Display when a game started on dashboard.
__Improved__
- More consistent and cleaner UI.
__Fixed__
- ELO and rank assignment using invalid database entries resulting in incorrect ELO values.
- Users no longer time out when in voice
## Admins
__Added__
- Guild-specific ELO with ranks
__Improved__
- ELO assignment beyond 100
## Known issues
- Auto-leave and auto-join are currently being overridden by voice verification
  This means despite having disabled auto-leave queue, you would still be removed.
