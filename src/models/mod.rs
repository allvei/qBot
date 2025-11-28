pub mod constants;
pub mod dashboard;
pub mod dm_tracker;
pub mod manager;
pub mod server;
pub mod session;
pub mod setup_state;
pub mod types;
pub mod colours;

pub use constants::*;
pub use dashboard::*;
pub use dm_tracker::*;
pub use manager::*;
pub use server::*;
pub use session::*;
pub use setup_state::*;
pub use types::*;
pub use colours::*;

// TypeMapKey for DashboardUpdateQueue (needed globally across crate)
use std::sync::Arc;
use serenity::prelude::TypeMapKey;

pub struct DashboardQueueKey;
impl TypeMapKey for DashboardQueueKey {
    type Value = Arc<DashboardUpdateQueue>;
}

pub struct DmTrackerKey;
impl TypeMapKey for DmTrackerKey {
    type Value = Arc<DmMessageTracker>;
}
