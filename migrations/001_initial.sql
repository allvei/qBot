-- Initial database schema for PUG bot
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    discord_id TEXT UNIQUE NOT NULL,
    steam_id64 TEXT,
    username TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS queue_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    channel_id TEXT NOT NULL, -- Discord channel ID where queue was joined
    joined_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_uuid TEXT UNIQUE NOT NULL,
    red_team_channel_id TEXT,
    blu_team_channel_id TEXT,
    server_channel TEXT, -- A, B, or C
    status TEXT NOT NULL DEFAULT 'hot', -- 'hot', 'push', 'live', 'pull'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    accepted_at DATETIME,
    ended_at DATETIME,
    accepted_by TEXT -- Discord ID of runner who accepted
);

CREATE TABLE IF NOT EXISTS session_players (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    team TEXT NOT NULL, -- 'RED' or 'BLU'
    is_benched BOOLEAN NOT NULL DEFAULT FALSE,
    benched_by TEXT, -- Discord ID of admin who benched
    benched_at DATETIME,
    FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    description TEXT
);

-- Insert default configuration with server-specific team channels
INSERT OR REPLACE INTO config (key, value, description) VALUES
    ('guild_id', '', 'Discord server ID'),
    ('queue_channel_id', '', 'Main queue voice channel ID'),
    ('log_channel_id', '', 'Text channel for session logs'),
    ('queue_size', '8', 'Number of players needed for a session'),
    ('confirmation_timeout', '120', 'Seconds to wait for confirmation'),
    ('runner_role_id', '', 'Discord role ID for session runners'),
    ('admin_role_id', '', 'Discord role ID for admins'),
    -- Server A channels
    ('red_a_channel_id', '', 'RED team voice channel ID for Server A'),
    ('blu_a_channel_id', '', 'BLU team voice channel ID for Server A'),
    -- Server B channels
    ('red_b_channel_id', '', 'RED team voice channel ID for Server B'),
    ('blu_b_channel_id', '', 'BLU team voice channel ID for Server B'),
    -- Server C channels
    ('red_c_channel_id', '', 'RED team voice channel ID for Server C'),
    ('blu_c_channel_id', '', 'BLU team voice channel ID for Server C'),

-- Indexes for performance

CREATE INDEX IF NOT EXISTS idx_queue_sessions_channel_id ON queue_sessions(channel_id);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_users_discord_id ON users(discord_id);
