use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    Member,
    Runner,
    Admin,
}

/// User data structure representing a player in the system
/// 
/// * `discord_id` - Discord user ID
/// * `steam_id64` - Steam 64-bit ID
/// * `elo`        - User's Elo rating
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub discord_id: u64,
    pub steam_id64: u64,
    pub elo:        u8,
    pub role:       Role,
}

impl Player {
    pub fn new(discord_id: u64) -> Player {
        Player {
            discord_id,
            steam_id64: 0,
            elo:        0,
            role:       Role::Member,
        }
    }
}
