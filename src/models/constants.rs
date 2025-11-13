use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::types::Rank;

pub const DEFAULT_QUOTA: u8 = 8;
pub const DEFAULT_RANK: Rank = Rank::Apprentice;

// Note: Runner and Admin role IDs are now configured per guild via database during setup.
// The hardcoded values below are kept for reference only (passtime.tf server roles).
// Use the /setup command to configure roles for each guild with dropdown menus.
// pub const RUNNER_R_ID: u64 = 1386951114225746040;
// pub const ADMIN_R_ID: u64 = 1386951155052974141;

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
