pub mod alert_limiter;
pub mod constants;
pub mod dashboard;
pub mod dm_tracker;
pub mod manager;
pub mod server;
pub mod session;
pub mod setup_state;
pub mod types;
pub mod colours;
pub mod buttons;
pub mod embeds;

pub use constants::*;
pub use dashboard::*;
pub use dm_tracker::*;
pub use manager::*;
pub use server::*;
pub use session::*;
pub use setup_state::*;
pub use types::*;
pub use colours::*;
pub use buttons::*;
pub use embeds::*;

// TypeMapKey for DashboardUpdateQueue (needed globally across crate)
use std::sync::Arc;
use serenity::prelude::TypeMapKey;
use tokio::sync::Mutex;

pub struct DashboardQueueKey;
impl TypeMapKey for DashboardQueueKey {
    type Value = Arc<Mutex<DashboardUpdateQueue>>;
}

pub struct GuildKey;
impl TypeMapKey for GuildKey {
    type Value = Arc<Mutex<crate::models::Manager>>;
}

pub struct DmTrackerKey;
impl TypeMapKey for DmTrackerKey {
    type Value = Arc<DmMessageTracker>;
}
