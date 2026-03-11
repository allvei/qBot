use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serenity::all::{Context, GuildId, VoiceState};

/// Resolve a guild's display name from cache, with a fallback.
pub fn guild_name(ctx: &Context, guild_id: GuildId) -> String {
  ctx.cache.guild(guild_id).map(|g| g.name.clone()).unwrap_or_else(|| "Unknown".to_string())
}

pub const DEFAULT_ALERT_COLOR: u32 = 3447003;
pub const DEFAULT_ACTIVE_ELO: bool = false;
pub const DEFAULT_QUOTA: u8 = 8;
pub const DEFAULT_HOT_JOIN_TIMEOUT: u16 = 300; // Seconds for players to join VC when queue goes hot
pub const CLEANUP_INTERVAL_SECS: u64 = 60; // Check every minute
pub const INACTIVITY_TIMEOUT_SECS: u64 = 600; // 10m
pub const DEFAULT_TIMEOUT: u8 = 120; // 2h default user queue expiry
pub const MAX_TIMEOUT: u8 = 240; // 4h max user queue expiry
pub const MIN_TIMEOUT: u8 = 30; // 30m min user queue expiry
pub const MAX_MATCH_SCORE: u8 = 5; // Maximum score per team in a match

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

#[derive(Debug, PartialEq)]
pub enum VoiceStateUpdate {
  Connected,
  Reconnected,
  Disconnected,
  Moved,
}

impl VoiceStateUpdate {
  pub fn get(old: &Option<VoiceState>, new: &VoiceState) -> VoiceStateUpdate {
    match old {
      Some(old_state) => {
        if old_state.channel_id == new.channel_id {
          VoiceStateUpdate::Reconnected
        } else if old_state.channel_id.is_none() {
          VoiceStateUpdate::Connected
        } else {
          VoiceStateUpdate::Moved
        }
      }
      None => {
        if new.channel_id.is_none() {
          VoiceStateUpdate::Disconnected
        } else {
          VoiceStateUpdate::Connected
        }
      }
    }
  }
}
