pub mod alert_limiter;
pub mod buttons;
pub mod colors;
pub mod constants;
pub mod dashboard;
pub mod dm_tracker;
pub mod dynamic_elo;
pub mod embeds;
pub mod fatkid_immunity;
pub mod manager;
pub mod queue_expiration_scheduler;
pub mod server;
pub mod session;
pub mod setup_state;
pub mod types;

pub use buttons::*;
pub use colors::*;
pub use constants::*;
pub use dashboard::*;
pub use dm_tracker::*;
pub use embeds::*;
pub use manager::*;
pub use queue_expiration_scheduler::*;
pub use server::*;
pub use session::*;
pub use setup_state::*;
pub use types::*;

// TypeMapKey for DashboardUpdateQueue (needed globally across crate)
use serenity::prelude::TypeMapKey;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DashboardQueueKey;
impl TypeMapKey for DashboardQueueKey {
  type Value = Arc<Mutex<DashboardUpdateQueue>>;
}

pub struct DmTrackerKey;
impl TypeMapKey for DmTrackerKey {
  type Value = Arc<DmMessageTracker>;
}
