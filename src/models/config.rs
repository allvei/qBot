// CHECK ME
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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

  ID_DASHBOARD       => 1385894822992281701,
  ID_CHAT            => 1388643261543088208,
  ID_QUEUE           => 1385893666010300436,
  ID_RED             => 1385464431185494086,
  ID_BLU             => 1385464563448680578,
  // ID_NA_BEGINNER     => 0,
  // ID_NA_NEWCOMER     => 0,
  // ID_NA_NOVICE       => 0,
  // ID_NA_APPRENTICE   => 0,
  // ID_NA_JOURNEYMAN   => 0,
  // ID_NA_MASTER       => 0,
  // ID_NA_MASTER_ELITE => 0,
  // ID_NA_GRANDMASTER  => 0,
}

/// Configuration key-value pair struct.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConfigFormat {
    pub key:         String,
    pub value:       String,
    pub description: Option<String>,
}

/// Bot configuration struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub i_guild:      u64,
    pub i_runner:     u64,
    pub i_admin:      u64,
    pub ic_queue:     u64,
    pub ic_log:       u64,
    pub ic_buffer:    u64,
    pub ic_red:       u64,
    pub ic_blue:      u64,
    pub join_timeout: u64,
}

impl Config {
    pub fn new(
        i_guild: u64,
        i_runner: u64,
        i_admin: u64,
        ic_queue: u64,
        ic_log: u64,
        ic_buffer: u64,
        ic_red: u64,
        ic_blue: u64,
    ) -> Self {
        Self {
            i_guild,
            i_runner,
            i_admin,
            ic_queue,
            ic_log,
            ic_buffer,
            ic_red,
            ic_blue,
            join_timeout: 120,
        }
    }
}
