# Database Refactoring Tasks

This document lists database schema issues that require larger refactoring or careful consideration.

## Critical Schema Mismatches

### 1. Users Table - Missing Columns in Code

**Issue**: The database schema has no `tag` or `elo` columns in the `users` table, but the code still expects them in some places.

**Current DB Schema**:

- `user_id` (PRIMARY KEY)
- `steam_id`
- `pm_hot_alert`
- `pm_queue_alert_threshold`
- `timeout`
- `vc_auto_join`
- `join_alert_*` (multiple columns)
- `vc_auto_leave`
- `leave_alert_*` (multiple columns)

**What's Missing**:

- No `tag` column (Discord username/tag)
- No `elo` column (moved to guild-specific `elo` table)

**Impact**:

- User repository methods that try to fetch/store tags need refactoring
- ELO is now guild-specific in the `elo` table (this is correct)
- Need to decide: fetch tags from Discord API on-demand or store them?

**Files Affected**:

- `src/database/repositories/user.rs` - Multiple methods reference `tag`
- `src/models/types.rs` - `Player` struct expects a tag

**Recommendation**:

- Remove all `tag` storage from database (already done in schema)
- Always fetch tags from Discord API or cache when needed
- Update `Player::default()` to accept empty string for tag initially

---

### 2. Config Table - Type Mismatch for default_rank

**Issue**: The `config` table stores `default_rank` as INTEGER, but code treats it as TEXT/String.

**Current DB Schema**:

```sql
default_rank INTEGER
```

**Code Expectation**:

- `config.rs` tries to read it as a string rank name
- Should store rank ID or rank name consistently

**Impact**:

- `ConfigRepository::get_config_map()` tries to convert INTEGER to string
- Rank lookups will fail or return wrong values

**Files Affected**:

- `src/database/repositories/config.rs`
- `src/database/migrations.rs` (line 67)

**Recommendation**:

- Decide: store rank as TEXT name (e.g., "Apprentice") or INTEGER ID?
- If TEXT: change migration to `default_rank TEXT`
- If INTEGER: change code to handle rank IDs and lookup from `ranks` table
- **Suggested**: Use TEXT for simplicity and consistency with `elo.rank` column

---

### 3. Groups Table - Missing Columns in Migrations

**Issue**: The database has columns that migrations don't verify or create.

**Missing from Migrations**:

- `team_balance_method` TEXT DEFAULT 'BCH'
- `dm_alert_enabled` INTEGER DEFAULT 0
- `dm_alert_users` TEXT DEFAULT '[]'

**Current State**:

- Migrations add these columns in `create_groups_table()` (lines 292-318)
- But `verify_groups()` doesn't check for them (line 322-332)

**Impact**:

- Schema validation will pass even if these columns are missing
- Runtime errors when trying to access these columns

**Files Affected**:

- `src/database/migrations.rs` - `verify_groups()` method

**Recommendation**:

- Add these columns to the `required_columns` list in `verify_groups()`
- Ensure all column additions are verified

---

### 4. Groups Table - Column Name Inconsistency

**Issue**: Database has `hot_timeout` but code uses `timeout`.

**DB Schema**:
`hot_timeout INTEGER`

**Code References**:

- Migrations use `timeout` (line 233, 260)
- Need to verify which name is actually in the database

**Impact**:

- Queries will fail if column name doesn't match
- Group timeout settings won't work

**Files Affected**:

- `src/database/migrations.rs`
- `src/database/repositories/group.rs`

**Recommendation**:

- Check actual database column name with `PRAGMA table_info(groups)`
- Standardize on one name (suggest `hot_timeout` to be explicit)
- Update all queries and migrations accordingly

---

## Migration Issues

### 5. Inconsistent Column Defaults in add_column Macro

**Issue**: The `add_column!` macro in migrations.rs has wrong default values for some columns.

**Examples** (lines 124-142):

```rust
add_column!(self, "users", "user_id", "INTEGER", "30");  // Wrong: user_id shouldn't default to 30
add_column!(self, "users", "steam_id", "INTEGER", "0");  // OK
add_column!(self, "users", "pm_queue_alert_threshold", "INTEGER", "3447003");  // Wrong: should be NULL or small number
add_column!(self, "users", "join_alert_img", "TEXT", "0");  // Wrong: should be NULL, not "0"
```

**Impact**:

- Incorrect default values when adding columns to existing tables
- Data corruption for existing users

**Recommendation**:

- Review all `add_column!` calls and fix default values
- Match defaults to the CREATE TABLE statement

---

### 6. Duplicate Column Definitions in Backup/Restore

**Issue**: Line 188-189 in migrations.rs has duplicate column names in INSERT query.

```rust
INSERT OR IGNORE INTO users (user_id, steam_id, pm_hot_alert, timeout, join_alert, 
    vc_auto_leave, join_alert_color, pm_queue_alert_threshold, join_alert, 
    join_alert_footer, join_alert_footer_img, join_alert_img, leave_alert, 
    leave_alert, leave_alert_footer, leave_alert_footer_img, leave_alert_img, elo)
```

**Duplicates**:

- `join_alert` appears twice
- `leave_alert` appears twice

**Impact**:

- SQL syntax error during table recreation
- Data migration will fail

**Recommendation**:

- Remove duplicate column names
- Ensure column list matches actual table schema

---

## Repository Method Issues

### 7. ConfigRepository - Outdated Methods

**Issue**: Several methods still assume old key-value schema.

**Affected Methods**:

- `set_config()` - uses `key`, `value` columns (line 55-66)
- `get_config_item()` - uses dynamic column selection with `?` (line 68-77)
- `delete_config()` - uses `key` column (line 79-84)
- `get_all_for_guild()` - returns `ConfigFormat` struct (line 86-93)
- Repository trait implementations (lines 103-127)

**Impact**:

- These methods won't work with new schema
- Need complete rewrite or removal

**Recommendation**:

- Create new methods for each config field and the matching column in the config table:
  - `get_runner_id()`, `set_runner_id()`
  - `get_admin_id()`, `set_admin_id()`
  - `get_active_elo()`, `set_active_elo()`
  - `get_default_rank()`, `set_default_rank()`

---

### 8. UserRepository - Tag Handling

**Issue**: Methods `upsert()` and `upsert_tag()` are now identical after removing tag storage.

**Current State**:

- Both methods do the same thing (insert/update user_id and steam_id)
- `upsert_tag()` name implies it handles tags, but it doesn't

**Impact**:

- Confusing API
- Redundant code

**Recommendation**:

- Remove `upsert_tag()` method
- Update all callers to use `upsert()` or `check_user()`
- Consider if tag fetching should be a separate concern

---

## Data Consistency Issues

### 9. ELO Data Split Between Tables

**Issue**: ELO data exists in both `users` table (old) and `elo` table (new).

**Current State**:

- `elo` table has guild-specific ELO (correct approach)
- Old `users.elo` column may still exist in some databases
- Code now uses `elo` table exclusively (correct)

**Impact**:

- Potential data inconsistency
- Migration path unclear for existing data

**Recommendation**:

- Drop `users.elo` column after migration

---

### 10. Rank Storage Inconsistency

**Issue**: Ranks stored differently in different tables.

**Current Storage**:

- `elo.rank` - TEXT (rank name like "Apprentice")
- `config.default_rank` - INTEGER (unclear what this represents)
- `ranks.role_id` - INTEGER (Discord role ID)

**Impact**:

- Confusion about what "rank" means in each context
- Type conversion errors

**Recommendation**:

- Standardize rank storage:
  - Use TEXT rank names everywhere for consistency
  - Store role_id separately in `ranks` table (already done)
  - Change `config.default_rank` to TEXT

---

## Summary of Actions Needed

### High Priority (Breaking Issues)

1. Fix `config.default_rank` type (INTEGER → TEXT)
2. Fix duplicate columns in migrations.rs line 188-189
3. Fix `add_column!` default values
4. Add missing columns to `verify_groups()`
5. Resolve `timeout` vs `hot_timeout` column name

### Medium Priority (Code Cleanup)

6. Refactor ConfigRepository methods for new schema
7. Remove redundant `upsert_tag()` method
8. Update `Player` struct initialization to handle missing tags
9. Remove `ConfigFormat` struct

### Low Priority (Documentation/Migration)

10. Document ELO migration from users table to elo table
11. Add migration script for existing data
12. Update README with new schema design

---

## Testing Checklist

After refactoring, test:

- [ ] User creation and retrieval
- [ ] Guild-specific ELO tracking
- [ ] Config get/set for all fields
- [ ] Group creation with all columns
- [ ] Rank assignment and retrieval
- [ ] Alert settings persistence
- [ ] Database validation passes
- [ ] No SQL errors in logs
