# Plan: ELO-Based Rank System

## Current State
- Ranks are hardcoded with fixed ELO values
- Players are assigned specific ranks
- ELO is derived from rank, not the other way around

## Proposed System

### 1. Rank Configuration Structure
```rust
// Define rank ranges with starting ELO
struct RankConfig {
    name: String,
    role_id: u64,           // Discord role ID
    min_elo: u32,           // Minimum ELO for this rank
    max_elo: Option<u32>,   // Maximum ELO (None for highest rank)
    starting_elo: u32,     // ELO when rank is assigned directly
}
```

### 2. Database Schema Changes
```sql
-- Add rank_configs table
CREATE TABLE rank_configs (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    role_id BIGINT NOT NULL UNIQUE,
    min_elo INTEGER NOT NULL,
    max_elo INTEGER,
    starting_elo INTEGER NOT NULL,
    guild_id BIGINT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Update players table
ALTER TABLE players ADD COLUMN manual_elo INTEGER; -- NULL = use rank-based ELO
```

### 3. New Admin Commands
- `/setelo <user> <elo>` - Set specific ELO for a player
- `/setrank <user> <rank>` - Assign rank with starting ELO
- `/addrank <name> <role> <min_elo> <starting_elo> [max_elo]` - Create new rank tier
- `/editrank <name> <field> <value>` - Modify existing rank
- `/removerank <name>` - Remove rank tier
- `/listranks` - Show all configured ranks with ranges

### 4. ELO → Rank Logic
```rust
fn get_rank_for_elo(elo: u32, guild_id: u64) -> Option<RankConfig> {
    // Find highest rank where elo >= min_elo and elo <= max_elo
    // Ranks are checked in descending order by min_elo
}
```

### 5. Discord Role Management
- Automatically assign/remove Discord roles based on ELO changes
- Batch role updates for efficiency
- Handle role hierarchy properly

### 6. Implementation Steps

**Phase 1: Database & Structure**
1. Create `rank_configs` table
2. Add `manual_elo` column to players
3. Create migration script

**Phase 2: Core Logic**
1. Implement `get_rank_for_elo()` function
2. Update ELO calculation to use manual_elo when set
3. Add rank range validation

**Phase 3: Admin Commands**
1. Implement `/setelo` command
2. Implement `/setrank` command
3. Implement rank management commands

**Phase 4: Discord Integration**
1. Automatic role assignment on ELO/rank changes
2. Role cleanup when ranks are removed
3. Audit logging for rank changes

**Phase 5: UI Updates**
1. Update dashboard to show ELO-based ranks
2. Add rank range display in settings
3. Update team balancing to use actual ELO values

### 7. Example Configuration
```
Rank Tiers:
- Novice: 0-24 ELO, starts at 15
- Advanced: 25-49 ELO, starts at 35  
- Expert: 50-74 ELO, starts at 60
- Master: 75+ ELO, starts at 80
```

### 8. Edge Cases
- Players with ELO above highest rank (keep highest rank)
- Rank deletions (reassign affected players)
- Role hierarchy conflicts
- Discord permission issues

Would you like me to start implementing this system?