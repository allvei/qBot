// CHECKED

use serde::{
    Deserialize,
    Serialize,
};
use serenity::all::{Context, UserId};
use sqlx::prelude::FromRow;
use tracing::info;

use crate::models::pug::Rank;
use crate::models::server::Role;

/// User data structure representing a player in the system
#[derive(Debug, Clone, Copy, Serialize, Deserialize, FromRow)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub discord_id: UserId,
    pub steam_id:   Option<u64>,
    pub rank:       Option<Rank>,
    pub role:       Option<Role>,
}

impl Player {
    pub fn add(discord_id: UserId, steam_id: Option<u64>) -> Player {
        Player {
            discord_id,
            steam_id,
            rank:     None,
            role:     None,
        }
    }

    pub fn set_steam(&mut self, steam_id: Option<u64>) {
        self.steam_id = steam_id;
    }

    pub fn set_rank(&mut self, rank: Option<Rank>) {
        self.rank = rank;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        self.role = role;
    }

    pub async fn get_name(&self, ctx: &Context) -> String {
        let name = &ctx.http.get_user(self.discord_id).await.unwrap();
        name.display_name().to_string()
    }
}
