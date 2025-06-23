/// Queue command handlers module
/// 
/// * `handle_queue_command` - Handles the `/queue` command
/// * `handle_queue_status_command` - Handles the `/queue status` command
/// * `trigger_quota_notification` - Triggers a quota notification if needed
/// 
pub mod queue;

/// Admin command handlers module
/// 
/// * `handle_buffer_command` - Handles the `/buffer` command, guarantees the player a spot in the next match.
/// * `handle_config_command` - Handles the /config command, which allows admins to modify bot configuration.
pub mod admin;
/// Session handlers module
pub mod session;
