// CHECK ME
use serde::{ Deserialize, Serialize };
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
        ),*
        $(,)?
    ) => {
        $(
            $(#[$meta])*
            pub const $const: u64 = $value;
        )*
    };
}

define_global_ids! {
  RUNNER_R_ID          => 1386951114225746040,
  ADMIN_R_ID           => 1386951155052974141,

  EU_BEGINNER_R_ID     => 1386989827307606107,
  EU_NEWCOMER_R_ID     => 1386951211109974066,
  EU_NOVICE_R_ID       => 1386951241539784827,
  EU_APPRENTICE_R_ID   => 1386951264117592097,
  EU_JOURNEYMAN_R_ID   => 1386951275056201820,
  EU_MASTER_R_ID       => 1386951316143734814,
  EU_MASTER_ELITE_R_ID => 1386951327711494204,
  EU_GRANDMASTER_R_ID  => 1386951360594837544,

  DASHBOARD_TC_ID    => 1385894822992281701,
  CHAT_TC_ID         => 1388643261543088208,
  QUEUE_TC_ID        => 1385893666010300436,
  RED_VC_ID          => 1385464431185494086,
  BLU_VC_ID          => 1385464563448680578,
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
    pub value:       Option<String>,
    pub description: Option<String>,
}

/// Bot configuration struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupConfig {
    pub guild_id:        u64,
    pub group_id:        u64,
    pub runner_r_id:     u64,
    pub admin_r_id:      u64,
    pub dashboard_tc_id: u64,
    pub queue_tc_id:     u64,
    pub queue_vc_id:     u64,
    pub log_tc_id:       u64,
    pub red_vc_id:       u64,
    pub blu_vc_id:       u64,
    pub join_timeout:    u16,
}

impl GroupConfig {
    pub fn new(
        guild_id:        u64,
        group_id:        u64,
        runner_r_id:     u64,
        admin_r_id:      u64,
        dashboard_tc_id: u64,
        queue_tc_id:     u64,
        queue_vc_id:     u64,
        log_tc_id:       u64,
        red_vc_id:       u64,
        blu_vc_id:       u64,
    ) -> Self {
        Self {
            guild_id,
            group_id,
            runner_r_id,
            admin_r_id,
            dashboard_tc_id,
            queue_tc_id,
            queue_vc_id,
            log_tc_id,
            red_vc_id,
            blu_vc_id,
            join_timeout: 120,
        }
    }

    pub fn empty(
        guild_id: u64,
        group_id: u64,
    ) -> Self {
        Self {
            guild_id,
            group_id,
            runner_r_id:     0,
            admin_r_id:      0,
            dashboard_tc_id: 0,
            queue_tc_id:     0,
            queue_vc_id:     0,
            log_tc_id:       0,
            red_vc_id:       0,
            blu_vc_id:       0,
            join_timeout:    120,
        }
    }
}
