# Plan: egui Host Management Panel Integration

This plan restructures the bot to run tokio in a background thread while eframe/egui runs on the main thread, enabling a native GUI for real-time bot monitoring and management.

## Compatibility Findings (Code Analysis)

**Existing structures are compatible:**

- `Manager` is `#[derive(Clone)]` - can be cloned for snapshots
- All nested structs (`QGuild`, `Category`, `Format`, `Session`) are `Clone + Serialize + Deserialize`
- `tokio::sync::mpsc` already in use (DashboardUpdateQueue) - no need for crossbeam-channel
- `Application` already uses `Arc<Mutex<Manager>>` and `Arc<Database>` - ready for shared state
- `ShutdownHandler` uses oneshot channels - can integrate GUI shutdown trigger

**Key observations:**

- `Category` has `#[serde(skip)]` fields: `last_action`, `pending_vc_notification`, `last_ping_time` - these are runtime-only and should be excluded from snapshots
- `Context` is only used in tokio thread for Discord operations - should not be passed to GUI
- Current dashboard uses `DashboardUpdateQueue` with tokio mpsc - similar pattern can be used for GUI commands
- `main.rs` is very simple (13 lines) - easy to restructure for threading

**Simplifications possible:**

- Snapshot pattern can use direct Manager clone instead of separate snapshot structs (Option A)
- If performance issues arise, can switch to lightweight snapshot structs (Option B)

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Main Thread                           │
│              eframe::run() -> egui GUI                    │
│  ┌────────────┬────────────┬────────────┬────────────┐  │
│  │ Log Panel  │ Queue View │ Game View  │ Admin Panel│  │
│  └────────────┴────────────┴────────────┴────────────┘  │
└─────────────────────────────────────────────────────────┘
                           │
                           │ Arc<Mutex<GuiSharedState>>
                           │ (manager, db, log_buffer, cmd_tx)
                           ▼
┌─────────────────────────────────────────────────────────┐
│              Background Tokio Thread                      │
│  ┌────────────┬────────────┬────────────┬────────────┐  │
│  │ Discord    │ Database   │ Command    │ Logging    │  │
│  │ Gateway    │ Pool       │ Receiver   │ Subscriber │  │
│  └────────────┴────────────┴────────────┴────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Phase 1: Threading Model Restructure

### 1.1 Create `src/gui/` module

- `src/gui/mod.rs` - module exports
- `src/gui/state.rs` - `GuiSharedState` struct
- `src/gui/app.rs` - `MyApp` struct implementing `eframe::App`
- `src/gui/panels/` - submodules for each panel

### 1.2 Modify `src/main.rs`

- Remove `#[tokio::main]` from `main()`
- Create `run_bot()` async function that spawns tokio runtime
- `main()` becomes:
  1. Initialize `GuiSharedState`
  2. Spawn `run_bot()` in a background thread with `std::thread::spawn`
  3. Call `eframe::run_native()` with `MyApp` on main thread
  4. Handle graceful shutdown coordination

### 1.3 Modify `src/application.rs`

- Extract `Application::new()` and `Application::run()` into separate functions
- Make `Application::run()` accept `Arc<Mutex<Manager>>` and `Arc<Database>` from shared state
- Add shutdown signal channel that GUI can trigger

## Phase 2: Shared State Structure

### 2.1 `src/gui/state.rs` - `GuiSharedState`

```rust
pub struct GuiSharedState {
    pub manager: Arc<Mutex<Manager>>,
    pub db: Arc<Database>,
    pub log_buffer: Arc<Mutex<VecDeque<String>>>,  // Ring buffer for GUI log viewer
    pub cmd_tx: mpsc::Sender<GuiCommand>,          // Commands from GUI to bot (tokio::sync::mpsc)
    pub shutdown_tx: Option<oneshot::Sender<()>>,   // Shutdown signal
}
```

### 2.2 `src/gui/commands.rs` - `GuiCommand` enum

```rust
pub enum GuiCommand {
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
```

### 2.3 Command receiver in tokio thread

- Spawn a tokio task that receives from `cmd_rx`
- Execute commands with access to `manager`, `db`, and `Context`
- Command implementations in `src/gui/command_handler.rs` (25+ commands grouped by category)
- Log command execution results to both tracing and log_buffer

## Phase 3: Log Buffer Integration

### 3.1 Create custom tracing layer

- `src/gui/log_layer.rs` - `GuiLogLayer` implementing `tracing::Subscriber`
- Captures log events and pushes to `log_buffer` (ring buffer, max 1000 lines)
- Filter to `pf_pug_bot` logs only (exclude library spam)

### 3.2 Modify `src/util.rs::init_logging()`

- Add `GuiLogLayer` to subscriber stack
- Pass `Arc<Mutex<VecDeque<String>>>` to layer constructor

### 3.3 Log panel in egui

- Display last N lines with scrolling
- Filter by log level (INFO, WARN, ERROR)
- Search/filter text input
- Copy to clipboard button

## Phase 4: GUI Panels (Iterative Implementation)

### 4.1 Log Panel (v1 - easiest)

- Read from `log_buffer` under lock
- Display lines with syntax highlighting (ANSI color codes)
- Auto-scroll to bottom toggle
- Clear buffer button

### 4.2 Queue/Game View (v2 - core value)

- Iterate `manager.qguilds` → `Category` → `Format` → `Session`
- Display tree view with collapsible nodes
- For each session: status, player count, players with ELO, teams
- Show concurrent games separately (Game 1, Game 2, etc.)
- Show voice state indicators (in VC, missing)

### 4.3 Admin Panel (v3 - actions)

Organized by category with input fields and confirmation dialogs:

**Queue Management:**

- Force-end game (select guild/category/format/session)
- Clear queue (select format)
- Add player (user_id input)
- Reorder queue (user_id + position input)
- Remove player (user_id input)
- Move player between sessions (for concurrent games)
- Force session state (dropdown: Idle/Hot/Push/Live/Pull)
- Reset session timer
- Force team regeneration
- Swap teams

**Debugging/Development:**

- Dump state to log (select guild)
- Toggle debug mode (select category, checkbox)
- Test Discord API (ping button)
- View session details (raw JSON display)

**Recovery from Bugs:**

- Clear all team VCs (select category)
- Reset category state (full reset)
- Remove orphaned sessions
- Fix player VC state (user_id input)
- Clear pending team switches
- Reset voice state tracking
- Recover from database (reload category from DB)

**Testing/Load Testing:**

- Add dummy players (count + optional role_id)
- Simulate game flow (auto-run Hot→Push→Live→Pull)
- Trigger concurrent games (count input)
- Test balance methods (compare BCH vs Average)
- Force quota met (trigger hot_fmt regardless)
- Simulate VC timeout

**Voice Channel Management:**

- Move player to VC (user_id + channel_id)
- Kick from VC (user_id)
- Sync VC state (resync in_queue_vc flags)

**Common UI elements:**

- Input fields for parameters (user_id, quota, position, etc.)
- Confirmation dialogs for destructive actions
- Result feedback (success/error messages)
- Command history/log

### 4.4 Settings Panel (v4 - optional)

- Edit config values live
- View database connection status
- View active Discord gateway connection status

## Phase 5: Data Snapshot Pattern (Critical for Performance)

### 5.1 Simplified snapshot approach

**Finding:** `Manager` is already `#[derive(Clone)]`, and all nested structs (`QGuild`, `Category`, `Format`, `Session`) are `Clone + Serialize + Deserialize`. We can simplify by:

**Option A (Simpler):** Just clone `Manager` periodically

- Periodic task (every 100ms) clones Manager under lock
- Stores clone in `GuiSharedState.latest_manager: Arc<RwLock<Option<Manager>>>`
- GUI reads via read lock (non-blocking for concurrent reads)

**Option B (Lightweight):** Create snapshot structs with only GUI-relevant data

- Exclude `Category` fields with `#[serde(skip)]`: `last_action`, `pending_vc_notification`, `last_ping_time`
- Exclude Discord IDs that aren't needed for display
- Smaller memory footprint, faster clone

**Recommendation:** Start with Option A for simplicity, optimize to Option B if performance issues arise.

### 5.2 Snapshot generation in tokio thread

- Periodic task (every 100ms) clones Manager state under lock
- Stores in `GuiSharedState.latest_manager`
- Manager lock only held briefly during clone

### 5.3 GUI reads snapshot instead of Manager

- GUI reads `latest_manager` via read lock
- Prevents GUI from blocking bot operations
- If using Option A, the clone is fast enough that lock contention is minimal

## Phase 6: Dependencies

### 6.1 Add to `Cargo.toml`

```toml
[dependencies]
eframe = "0.31"        # egui framework
egui = "0.31"         # core library (included in eframe, but explicit for types)
# Note: tokio::sync::mpsc already in use, no crossbeam-channel needed
```

### 6.2 Update `[bin]` section

- Keep existing `pf_pug_bot` binary (for headless mode)
- Add optional feature flag `gui` for GUI mode
- Or just always build GUI (simpler for now)

## Phase 7: Testing & Refinement

### 7.1 Test thread safety

- Verify `Manager` locks are held briefly
- Test concurrent GUI reads + bot writes
- Check for deadlocks

### 7.2 Test command execution

- Verify commands from GUI execute correctly
- Test error handling (invalid user_id, etc.)
- Verify results log to GUI

### 7.3 Performance profiling

- Measure lock contention
- Optimize snapshot generation if needed
- Adjust log buffer size

## File Changes Summary

**New files:**

- `src/gui/mod.rs`
- `src/gui/state.rs`
- `src/gui/app.rs`
- `src/gui/commands.rs` - GuiCommand enum and command handler
- `src/gui/command_handler.rs` - Implementation of all 25+ commands
- `src/gui/log_layer.rs`
- `src/gui/panels/mod.rs`
- `src/gui/panels/log.rs`
- `src/gui/panels/queue.rs`
- `src/gui/panels/admin.rs`

**Modified files:**

- `src/main.rs` - restructure main() to spawn tokio thread
- `src/application.rs` - extract run() to accept shared state
- `src/util.rs` - add GuiLogLayer to init_logging()
- `Cargo.toml` - add eframe dependencies

**No changes needed:**

- All existing bot logic (models, handlers, etc.)
- Database schema
- Discord event handlers

## Estimated Lines of Code

**New files:**

- `src/gui/mod.rs` - 30 lines (module exports)
- `src/gui/state.rs` - 70 lines (GuiSharedState struct)
- `src/gui/app.rs` - 400 lines (MyApp eframe::App implementation, main GUI loop)
- `src/gui/commands.rs` - 70 lines (GuiCommand enum with 25+ variants)
- `src/gui/command_handler.rs` - 900 lines (25+ command implementations, ~35 lines each)
- `src/gui/log_layer.rs` - 130 lines (custom tracing Subscriber layer)
- `src/gui/panels/mod.rs` - 25 lines (panel module exports)
- `src/gui/panels/log.rs` - 200 lines (log viewer with scrolling/filtering)
- `src/gui/panels/queue.rs` - 400 lines (tree view of guilds/categories/formats/sessions)
- `src/gui/panels/admin.rs` - 500 lines (admin UI with 25+ command buttons/inputs)

**Modified files:**

- `src/main.rs` - +40 lines (restructure to spawn tokio thread, was 13 lines)
- `src/application.rs` - +80 lines (extract run(), add shutdown coordination)
- `src/util.rs` - +30 lines (add GuiLogLayer to init_logging)
- `Cargo.toml` - +5 lines (add eframe/egui dependencies)

**Total new LOC: ~2,765 lines**
**Total modified LOC: ~155 lines**
**Grand total: ~2,920 lines**

## Estimated Effort

- Phase 1 (Threading): 4 hours
- Phase 2 (Shared State): 2 hours
- Phase 3 (Log Integration): 2 hours
- Phase 4 (GUI Panels): 12 hours (expanded admin panel with 25+ commands)
- Phase 5 (Snapshot Pattern): 2 hours (simplified approach)
- Phase 6 (Dependencies): 0.5 hours
- Phase 7 (Testing): 4 hours (more commands to test)

**Total: ~26.5 hours (3-4 days)**

## Risks & Mitigations

1. **Lock contention** - GUI holding Manager lock too long
   - Mitigation: Snapshot pattern, minimize lock duration
   - Finding: Manager is Clone, so clone is fast; RwLock allows concurrent reads

2. **Context not Send** - `serenity::Context` may not be thread-safe
   - Mitigation: Don't pass Context to GUI; snapshot only needed data
   - Finding: Context is only needed in tokio thread for Discord operations

3. **Shutdown coordination** - GUI and bot threads both need to exit cleanly
   - Mitigation: oneshot channels, proper join() on bot thread
   - Finding: Existing ShutdownHandler uses oneshot, can integrate GUI trigger

4. **egui learning curve** - Immediate mode GUI paradigm
   - Mitigation: Start with simple panels, iterate incrementally

5. **Category runtime fields** - `#[serde(skip)]` fields shouldn't be in snapshots
   - Fields: `last_action`, `pending_vc_notification`, `last_ping_time`
   - Mitigation: If using Option B snapshot structs, exclude these fields

6. **Command implementation complexity** - 25+ admin commands to implement
   - Mitigation: Implement commands incrementally, group by category
   - Some commands (SimulateGameFlow, TriggerConcurrentGames) may require new helper functions

## Implementation Order

1. Phase 1 + Phase 2 (threading + shared state) - get architecture working
2. Phase 3 (log integration) - test data flow
3. Phase 4.1 (log panel) - first visible GUI
4. Phase 5 (snapshot pattern) - performance optimization
5. Phase 4.2 (queue/game view) - core feature
6. Phase 4.3 (admin panel) - implement commands incrementally by category:
   - Queue Management commands (highest priority)
   - Recovery commands (for debugging)
   - Voice Channel Management
   - Debugging/Development
   - Testing/Load Testing
7. Phase 7 (testing/refinement)
