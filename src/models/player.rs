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
    pub fn construct(discord_id: UserId, steam_id: Option<u64>) -> Player {
        Player {
            discord_id,
            steam_id,
            rank: None,
            role: None,
        }
    }

    pub fn set_rank(&mut self, rank: Option<Rank>) {
        info!("Setting rank for player {}: {:?}", self.discord_id, rank);
        self.rank = rank;
    }

    pub fn set_role(&mut self, role: Option<Role>) {
        info!("Setting role for player {}: {:?}", self.discord_id, role);
        self.role = role;
    }

    pub async fn get_name(&self, ctx: &Context) -> String {
        let name = &ctx.http.get_user(self.discord_id).await.unwrap();
        name.display_name().to_string()
    }
}
