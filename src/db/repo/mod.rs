pub mod user;
pub mod category;
pub mod config;
pub mod elo;
pub mod rank;
pub mod team;

pub use user::{UserRepository, UserSettings, is_valid_user_text};
pub use category::CategoryRepository;
pub use config::ConfigRepository;
pub use elo::{EloRepository, GuildElo};
pub use rank::{RankRepository, GuildRank};
pub use team::TeamRepository;

use anyhow::Result;
use async_trait::async_trait;

/// Base repository trait that all repositories implement
#[async_trait]
pub trait Repository<T, ID> {
    async fn create(   &self, entity: &T) -> Result<T>;
    async fn get_by_id(&self, id:     ID) -> Result<T>;
    async fn update(   &self, entity: &T) -> Result<T>;
    async fn delete(   &self, id:     ID) -> Result<()>;
}
