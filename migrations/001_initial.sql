-- Initial database schema for PUG bot
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    discord_id TEXT UNIQUE NOT NULL,
    steam_id TEXT,
    username TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS queue_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    queue_type TEXT NOT NULL DEFAULT 'default', -- 'nowbie' or 'journey'
    joined_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'waiting', -- 'waiting', 'in_match', 'benched'
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS matches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    match_uuid TEXT UNIQUE NOT NULL,
    red_team_channel_id TEXT,
    blu_team_channel_id TEXT,
    server_channel TEXT, -- A, B, or C
    status TEXT NOT NULL DEFAULT 'forming', -- 'forming', 'confirmed', 'in_progress', 'ended'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    confirmed_at DATETIME,
    ended_at DATETIME,
    confirmed_by TEXT -- Discord ID of admin who confirmed
);

CREATE TABLE IF NOT EXISTS match_players (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    team TEXT NOT NULL, -- 'RED' or 'BLU'
    FOREIGN KEY (match_id) REFERENCES matches (id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    description TEXT
);

-- Insert default configuration
INSERT OR REPLACE INTO config (key, value, description) VALUES
    ('guild_id', '', 'Discord server ID'),
    ('queue_channel_id', '', 'Main queue voice channel ID'),
    ('red_channel_id', '', 'RED team voice channel ID'),
    ('blu_channel_id', '', 'BLU team voice channel ID'),
    ('server_a_channel_id', '', 'Server A voice channel ID'),
    ('server_b_channel_id', '', 'Server B voice channel ID'),
    ('server_c_channel_id', '', 'Server C voice channel ID'),
    ('log_channel_id', '', 'Text channel for match logs'),
    ('queue_size', '8', 'Number of players needed for a match'),
    ('confirmation_timeout', '120', 'Seconds to wait for confirmation'),
    ('runner_role_id', '', 'Discord role ID for match runners'),
    ('admin_role_id', '', 'Discord role ID for admins');

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_queue_sessions_status ON queue_sessions(status);
CREATE INDEX IF NOT EXISTS idx_queue_sessions_queue_type ON queue_sessions(queue_type);
CREATE INDEX IF NOT EXISTS idx_matches_status ON matches(status);
CREATE INDEX IF NOT EXISTS idx_users_discord_id ON users(discord_id);
