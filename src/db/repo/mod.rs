pub mod category;
pub mod config;
pub mod elo;
pub mod fatkid;
pub mod guilds;
pub mod r#match;
pub mod rank;
pub mod team;
pub mod user;
pub mod user_server_prefs;

pub use category::CategoryRepository;
pub use config::ConfigRepository;
pub use elo::{EloRepository, GuildElo};
pub use fatkid::FatkidRepository;
pub use guilds::GuildRepository;
pub use r#match::{MatchPlayerInsert, MatchRecord, MatchRepo, PlayerStats};
pub use rank::{GuildRank, RankRepository};
pub use team::TeamRepository;
pub use user::{is_valid_user_text, PlayerRepository, UserPreferences};
pub use user_server_prefs::UserServerPrefsRepository;

use anyhow::Result;
use async_trait::async_trait;

/// Base repository trait that all repositories implement
#[async_trait]
pub trait Repository<T, ID> {
  async fn create(&self, entity: &T) -> Result<T>;
  async fn get_by_id(&self, id: ID) -> Result<T>;
  async fn update(&self, entity: &T) -> Result<T>;
  async fn delete(&self, id: ID) -> Result<()>;
}
