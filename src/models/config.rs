use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::models::Channels;

// Example usage:
// define_global_ids! {
//     ID_RUNNER => 1386951114225746040,
//     ID_ADMIN  => 1386951155052974141,
// }
macro_rules! define_global_ids {
    (
        $(
            $(#[$meta:meta])*
            $const:ident => $value:expr
        ),* $(,)?
    ) => {
        $(
            $(#[$meta])*
            pub const $const: u64 = $value;
        )*
    }
}

define_global_ids! {
  ID_RUNNER          => 1386951114225746040,
  ID_ADMIN           => 1386951155052974141,

  ID_EU_BEGINNER     => 1386989827307606107,
  ID_EU_NEWCOMER     => 1386951211109974066,
  ID_EU_NOVICE       => 1386951241539784827,
  ID_EU_APPRENTICE   => 1386951264117592097,
  ID_EU_JOURNEYMAN   => 1386951275056201820,
  ID_EU_MASTER       => 1386951316143734814,
  ID_EU_MASTER_ELITE => 1386951327711494204,
  ID_EU_GRANDMASTER  => 1386951360594837544,

  ID_NA_BEGINNER     => 0,
  ID_NA_NEWCOMER     => 0,
  ID_NA_NOVICE       => 0,
  ID_NA_APPRENTICE   => 0,
  ID_NA_JOURNEYMAN   => 0,
  ID_NA_MASTER       => 0,
  ID_NA_MASTER_ELITE => 0,
  ID_NA_GRANDMASTER  => 0,
}

/// Configuration key-value pair struct.
// #[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConfigFormat {
    pub key:         String,
    pub value:       String,
    pub description: Option<String>,
}

/// Bot configuration struct.
// #[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub queue_channel_id:     u64,
    pub log_channel_id:       u64,
    pub queue_quota:          u8,
    pub confirmation_timeout: u64,
    pub runner_role_id:       u64,
    pub admin_role_id:        u64,
    pub queue_channel:        u64,
    pub buffer_channel:       u64,
    pub red_channel:          u64,
    pub blue_channel:         u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            queue_channel_id:     0,
            log_channel_id:       0,
            queue_quota:          8,
            confirmation_timeout: 120,
            runner_role_id:       0,
            admin_role_id:        0,
            queue_channel:        0,
            buffer_channel:       0,
            red_channel:          0,
            blue_channel:         0,
        }
    }
}
