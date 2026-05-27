//! Commands that can be sent from GUI to bot

use crate::models::SessionStatus;

/// Commands that can be sent from the GUI to the tokio thread
pub enum GuiCommand {
  // Snapshot Control
  RefreshSnapshot,

  // Queue Management
  ForceEndGame { guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize },
  ClearQueue { guild_id: u64, category_id: u8, fmt_id: u8 },
  AddPlayer { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64 },
  RemovePlayer { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64 },
  DeletePlayerFromDb { guild_id: u64, user_id: u64 },
  BufferPlayer { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64 },
  FatkidPlayer { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64 },
  ReorderQueue { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64, new_position: usize },
  MovePlayerBetweenSessions { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64, from_session: usize, to_session: usize },
  ForceSessionState { guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize, new_state: SessionStatus },
  ResetSessionTimer { guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize },
  ForceTeamRegeneration { guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize },
  SwapTeams { guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize },

  // Debugging/Development
  DumpStateToLog { guild_id: u64 },
  ToggleDebugMode { guild_id: u64, category_id: u8, enabled: bool },
  TestDiscordApi,
  ViewSessionDetails { guild_id: u64, category_id: u8, fmt_id: u8, session_index: usize },

  // System Control
  GracefulRestart,
  GracefulShutdown,

  // Recovery from Bugs
  ClearAllTeamVCs { guild_id: u64, category_id: u8 },
  ResetCategoryState { guild_id: u64, category_id: u8 },
  RemoveOrphanedSessions { guild_id: u64, category_id: u8 },
  FixPlayerVCState { guild_id: u64, category_id: u8, user_id: u64 },
  ClearPendingTeamSwitches { guild_id: u64, category_id: u8, fmt_id: u8 },
  ResetVoiceStateTracking { guild_id: u64 },
  RecoverFromDatabase { guild_id: u64, category_id: u8 },

  // Testing/Load Testing
  AddDummyPlayers { guild_id: u64, category_id: u8, fmt_id: u8, count: usize, role_id: Option<u64> },
  SimulateGameFlow { guild_id: u64, category_id: u8, fmt_id: u8 },
  TriggerConcurrentGames { guild_id: u64, category_id: u8, fmt_id: u8, count: usize },
  TestBalanceMethods { guild_id: u64, category_id: u8, fmt_id: u8 },
  ForceQuotaMet { guild_id: u64, category_id: u8, fmt_id: u8 },
  SimulateVCTimeout { guild_id: u64, category_id: u8, fmt_id: u8 },

  // Voice Channel Management
  MovePlayerToVC { guild_id: u64, user_id: u64, channel_id: u64 },
  KickFromVC { guild_id: u64, user_id: u64 },
  SyncVCState { guild_id: u64, category_id: u8 },

  // User Management
  QueryUsers { search_term: String },
  UpdateUserTag { user_id: u64, tag: String },
  UpdateUserSteamId { user_id: u64, steam_id: Option<u64> },
  UpdateUserQueueExpiration { user_id: u64, queue_expiration: u8 },
  GetUserGuildData { user_id: u64 },
  UpdateUserElo { user_id: u64, guild_id: u64, elo: u16 },
  UpdateUserDynamicElo { user_id: u64, guild_id: u64, dynamic_elo: Option<u16> },

  // Config Management
  LoadGuildConfig { guild_id: u64 },
  UpdateGuildConfigBool { guild_id: u64, column: String, value: bool },
  UpdateGuildConfigInt { guild_id: u64, column: String, value: i64 },
  UpdateGuildConfigText { guild_id: u64, column: String, value: String },

  // System Messages
  SendSystemMessage { guild_id: Option<u64>, message: String },
  ValidateSystemMessageChannels,

  // Community Updates
  SendCommunityUpdate { guild_id: Option<u64>, message: String },
  ValidateCommunityUpdatesChannels,
}

impl GuiCommand {
  /// Returns the affected guild ID for state-mutating commands, or None for read-only commands.
  pub fn guild_id(&self) -> Option<u64> {
    match self {
      GuiCommand::RefreshSnapshot => None,
      GuiCommand::TestDiscordApi => None,
      GuiCommand::QueryUsers { .. } => None,
      GuiCommand::UpdateUserTag { .. } => None,
      GuiCommand::UpdateUserSteamId { .. } => None,
      GuiCommand::UpdateUserQueueExpiration { .. } => None,
      GuiCommand::GetUserGuildData { .. } => None,
      GuiCommand::LoadGuildConfig { .. } => None,
      GuiCommand::UpdateGuildConfigBool { .. } => None,
      GuiCommand::UpdateGuildConfigInt { .. } => None,
      GuiCommand::UpdateGuildConfigText { .. } => None,
      GuiCommand::SendSystemMessage { .. } => None,
      GuiCommand::ValidateSystemMessageChannels => None,
      GuiCommand::SendCommunityUpdate { .. } => None,
      GuiCommand::ValidateCommunityUpdatesChannels => None,
      GuiCommand::DumpStateToLog { .. } => None,
      GuiCommand::ViewSessionDetails { .. } => None,
      GuiCommand::TestBalanceMethods { .. } => None,
      GuiCommand::ToggleDebugMode { .. } => None,
      GuiCommand::GracefulRestart => None,
      GuiCommand::GracefulShutdown => None,
      _ => match self {
        GuiCommand::ForceEndGame { guild_id, .. } => Some(*guild_id),
        GuiCommand::ClearQueue { guild_id, .. } => Some(*guild_id),
        GuiCommand::AddPlayer { guild_id, .. } => Some(*guild_id),
        GuiCommand::RemovePlayer { guild_id, .. } => Some(*guild_id),
        GuiCommand::DeletePlayerFromDb { guild_id, .. } => Some(*guild_id),
        GuiCommand::BufferPlayer { guild_id, .. } => Some(*guild_id),
        GuiCommand::FatkidPlayer { guild_id, .. } => Some(*guild_id),
        GuiCommand::ReorderQueue { guild_id, .. } => Some(*guild_id),
        GuiCommand::MovePlayerBetweenSessions { guild_id, .. } => Some(*guild_id),
        GuiCommand::ForceSessionState { guild_id, .. } => Some(*guild_id),
        GuiCommand::ResetSessionTimer { guild_id, .. } => Some(*guild_id),
        GuiCommand::ForceTeamRegeneration { guild_id, .. } => Some(*guild_id),
        GuiCommand::SwapTeams { guild_id, .. } => Some(*guild_id),
        GuiCommand::ForceQuotaMet { guild_id, .. } => Some(*guild_id),
        GuiCommand::AddDummyPlayers { guild_id, .. } => Some(*guild_id),
        GuiCommand::SimulateGameFlow { guild_id, .. } => Some(*guild_id),
        GuiCommand::SimulateVCTimeout { guild_id, .. } => Some(*guild_id),
        GuiCommand::TriggerConcurrentGames { guild_id, .. } => Some(*guild_id),
        GuiCommand::ResetCategoryState { guild_id, .. } => Some(*guild_id),
        GuiCommand::RemoveOrphanedSessions { guild_id, .. } => Some(*guild_id),
        GuiCommand::ClearPendingTeamSwitches { guild_id, .. } => Some(*guild_id),
        GuiCommand::FixPlayerVCState { guild_id, .. } => Some(*guild_id),
        GuiCommand::ResetVoiceStateTracking { guild_id } => Some(*guild_id),
        GuiCommand::RecoverFromDatabase { guild_id, .. } => Some(*guild_id),
        GuiCommand::MovePlayerToVC { guild_id, .. } => Some(*guild_id),
        GuiCommand::KickFromVC { guild_id, .. } => Some(*guild_id),
        GuiCommand::SyncVCState { guild_id, .. } => Some(*guild_id),
        GuiCommand::ClearAllTeamVCs { guild_id, .. } => Some(*guild_id),
        GuiCommand::UpdateUserElo { guild_id, .. } => Some(*guild_id),
        GuiCommand::UpdateUserDynamicElo { guild_id, .. } => Some(*guild_id),
        _ => None,
      },
    }
  }
}
