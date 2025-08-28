pub mod user;
pub mod group;
pub mod config;

pub use user::UserRepository;
pub use group::GroupRepository;
pub use config::ConfigRepository;

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
