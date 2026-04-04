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
    ReorderQueue { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64, new_position: usize },
    RemovePlayer { guild_id: u64, category_id: u8, fmt_id: u8, user_id: u64 },
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
}
