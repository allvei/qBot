-- Fix config table foreign key to reference ranks(id) instead of ranks(role_id)

-- First, drop the existing foreign key constraint
-- SQLite doesn't support ALTER TABLE DROP CONSTRAINT directly, so we need to recreate the table

-- Create a new config table with correct foreign key
CREATE TABLE config_new (
    guild_id      INTEGER NOT NULL,
    runner_id     INTEGER,
    admin_id      INTEGER,
    default_rank  INTEGER,
    active_elo    INTEGER,
    CONSTRAINT "CONFIG_PK" PRIMARY KEY("guild_id"),
    FOREIGN KEY("default_rank") REFERENCES "ranks"("id") ON DELETE SET NULL
);

-- Copy data from old table to new table
INSERT INTO config_new (guild_id, runner_id, admin_id, default_rank, active_elo)
SELECT guild_id, runner_id, admin_id, default_rank, active_elo FROM config;

-- Drop the old table
DROP TABLE config;

-- Rename the new table to the original name
ALTER TABLE config_new RENAME TO config;

-- Note: Any default_rank values that were referencing role_id will now be invalid
-- and will be set to NULL. The server admin will need to reconfigure the default rank.
