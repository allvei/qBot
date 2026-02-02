-- Migration script to fix the elo table foreign key constraint
-- This script should be run on existing databases to fix the foreign key mismatch

-- Step 1: Create a backup of the existing elo table
CREATE TABLE elo_backup AS SELECT * FROM elo;

-- Step 2: Drop the existing elo table (this will also drop the foreign key constraint)
DROP TABLE elo;

-- Step 3: Recreate the elo table with the correct foreign key constraint
CREATE TABLE elo (
    id        INTEGER PRIMARY KEY,
    guild_id  INTEGER NOT NULL,
    user_id   INTEGER NOT NULL,
    elo       INTEGER NOT NULL DEFAULT 50,
    rank      INTEGER NOT NULL,
    games     INTEGER NOT NULL DEFAULT 0,
    wins      INTEGER NOT NULL DEFAULT 0,
    UNIQUE(guild_id, user_id),
    FOREIGN KEY (rank)    REFERENCES ranks(id) ON DELETE SET NULL,
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

-- Step 4: Restore data from backup, converting rank names to rank IDs
-- This assumes that the rank column currently stores rank names as strings
INSERT INTO elo (id, guild_id, user_id, elo, rank, games, wins)
SELECT 
    eb.id,
    eb.guild_id,
    eb.user_id,
    eb.elo,
    COALESCE(r.id, 0) as rank_id,  -- Use 0 if rank not found (will be NULL due to FK constraint)
    eb.games,
    eb.wins
FROM elo_backup eb
LEFT JOIN ranks r ON r.name = eb.rank AND r.guild_id = eb.guild_id;

-- Step 5: Clean up the backup table
DROP TABLE elo_backup;

-- Step 6: Update any records that couldn't find a matching rank (rank_id = 0)
-- Set them to NULL or a default rank
UPDATE elo SET rank = NULL WHERE rank = 0;
