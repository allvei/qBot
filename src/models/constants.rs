use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

macro_rules! define_global_ids {
    (
        $(
            $(#[$meta: meta])*
            $const: ident => $value: expr
        ),*
        $(,)?
    ) => {
        $(
            $(#[$meta])*
            pub const $const: u64 = $value;
        )*
    };
}

pub const DEFAULT_QUOTA: u8 = 8;

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

// ConfigFormat
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ConfigFormat {
    pub key: String,
    pub value: Option<String>,
    pub description: Option<String>,
}

/// `FileManager` struct provides cross-platform file operations.
pub struct FileManager;

impl FileManager {
    /// Checks if a file exists at the given path.
    /// 
    /// Returns `true` if the file exists, `false` otherwise.
    ///
    /// * `path` - The path to check.
    pub fn file_exists<P: AsRef<Path>>(path: P) -> bool {
        Path::new(path.as_ref()).exists()
    }

    /// Creates a new file at the given path.
    /// 
    /// Returns a `Result` containing `()` or an `anyhow::Error` if creation fails.
    ///
    /// * `path` - The path where the file should be created.
    pub fn create_file<P: AsRef<Path>>(path: P) -> Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(path)?;
        Ok(())
    }

    /// Normalizes a path for the current platform.
    /// 
    /// Returns a `PathBuf` with the normalized path.
    ///
    /// * `path` - The path to normalize.
    pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
        path.as_ref().to_path_buf()
    }
}
