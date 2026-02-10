use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::time::Instant;
use serenity::all::{ChannelId, Context, CreateEmbed, CreateMessage};
use tokio::sync::Mutex;
use tracing::debug;

const ALERT_DELAY_SECS: u64 = 5;
const MAX_ALERTS_PER_WINDOW: usize = 4;
const WINDOW_SECS: u64 = 60;

type AlertKey = (u8, u8, u64); // (group_id, sg_id, user_id)

static RATE_LIMITER: LazyLock<Mutex<HashMap<AlertKey, VecDeque<Instant>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Schedule a join/leave alert to be sent after a delay, subject to rate limiting.
/// Returns immediately — the actual send happens in a spawned task.
pub fn schedule_alert(
    ctx: Context,
    channel: ChannelId,
    embed: CreateEmbed,
    group_id: u8,
    sg_id: u8,
    user_id: u64,
) {
    let key = (group_id, sg_id, user_id);

    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(ALERT_DELAY_SECS)).await;

        // Check and update rate limit
        {
            let mut limiter = RATE_LIMITER.lock().await;
            let timestamps = limiter.entry(key).or_insert_with(VecDeque::new);

            // Prune entries older than the window
            let cutoff = Instant::now() - std::time::Duration::from_secs(WINDOW_SECS);
            while timestamps.front().map_or(false, |t| *t < cutoff) {
                timestamps.pop_front();
            }

            if timestamps.len() >= MAX_ALERTS_PER_WINDOW {
                debug!("Rate limited alert for user {} in group {}/sg {}", user_id, group_id, sg_id);
                return;
            }

            timestamps.push_back(Instant::now());
        }

        let _ = channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await;
    });
}
