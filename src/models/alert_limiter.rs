use serenity::all::{ChannelId, Context, CreateMessage, GuildId, UserId};
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::debug;

const ALERT_DELAY_SECS: u64 = 5;
const MAX_ALERTS_PER_WINDOW: usize = 4;
const WINDOW_SECS: u64 = 60;

/// Whether the player joined or left
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertType {
  Join,
  Leave,
}

/// A buffered alert intent (not yet rendered into an embed)
#[derive(Debug, Clone)]
struct AlertIntent {
  alert_type: AlertType,
  fmt_name: Option<String>,
  rank_name: String,
}

/// Key for categorying alerts: (category_id, user_id)
/// Alerts across formats for the same user in the same category get combined.
type BufferKey = (u8, u64);

type RateLimitKey = (u8, u8, u64); // (category_id, fmt_id, user_id)

struct PendingAlert {
  intents: Vec<AlertIntent>,
  ctx: Context,
  channel: ChannelId,
  guild_id: GuildId,
  user_id: UserId,
  db: std::sync::Arc<crate::Database>,
}

static PENDING: LazyLock<Mutex<HashMap<BufferKey, PendingAlert>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

static RATE_LIMITER: LazyLock<Mutex<HashMap<RateLimitKey, VecDeque<Instant>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Schedule a join/leave alert intent. Multiple intents for the same user+category
/// within the buffer window get combined or cancelled before sending.
pub fn schedule_alert(
  ctx: Context,
  channel: ChannelId,
  guild_id: GuildId,
  user_id: UserId,
  db: std::sync::Arc<crate::Database>,
  category_id: u8,
  fmt_id: u8,
  alert_type: AlertType,
  fmt_name: Option<String>,
  rank_name: String,
) {
  let buffer_key = (category_id, user_id.get());

  tokio::spawn(async move {
    let first_intent = {
      let mut pending = PENDING.lock().await;
      let entry = pending.entry(buffer_key);

      match entry {
        std::collections::hash_map::Entry::Occupied(mut occ) => {
          occ.get_mut().intents.push(AlertIntent { alert_type, fmt_name, rank_name });
          false // not the first, timer already running
        }
        std::collections::hash_map::Entry::Vacant(vac) => {
          vac.insert(PendingAlert { intents: vec![AlertIntent { alert_type, fmt_name, rank_name }], ctx, channel, guild_id, user_id, db });
          true // first intent, we need to start the timer
        }
      }
    };

    if !first_intent {
      return; // timer already running from the first intent
    }

    // Wait for the buffer window
    tokio::time::sleep(tokio::time::Duration::from_secs(ALERT_DELAY_SECS)).await;

    // Take the buffered intents
    let alert = {
      let mut pending = PENDING.lock().await;
      pending.remove(&buffer_key)
    };

    let Some(alert) = alert else { return };

    // Resolve intents: cancel opposing join/leave pairs per format
    let mut net: HashMap<Option<String>, i8> = HashMap::new(); // fmt_name -> net (+1 join, -1 leave)
    let mut rank_name = String::new();
    for intent in &alert.intents {
      let counter = net.entry(intent.fmt_name.clone()).or_insert(0);
      match intent.alert_type {
        AlertType::Join => *counter += 1,
        AlertType::Leave => *counter -= 1,
      }
      if !intent.rank_name.is_empty() {
        rank_name = intent.rank_name.clone();
      }
    }

    // Collect remaining joins and leaves
    let mut join_names: Vec<Option<String>> = Vec::new();
    let mut leave_names: Vec<Option<String>> = Vec::new();
    for (fmt_name, count) in &net {
      if *count > 0 {
        join_names.push(fmt_name.clone());
      } else if *count < 0 {
        leave_names.push(fmt_name.clone());
      }
      // count == 0 means join+leave cancelled out
    }

    // If nothing remains, the player toggled and net effect is zero
    if join_names.is_empty() && leave_names.is_empty() {
      debug!("Alert cancelled for user {} in category {} (join/leave cancelled out)", alert.user_id, category_id);
      return;
    }

    // Rate limit check (use first sg or 0)
    let rate_fmt = fmt_id;
    let rate_key = (category_id, rate_fmt, alert.user_id.get());
    {
      let mut limiter = RATE_LIMITER.lock().await;
      let timestamps = limiter.entry(rate_key).or_insert_with(VecDeque::new);
      let cutoff = Instant::now() - std::time::Duration::from_secs(WINDOW_SECS);
      while timestamps.front().map_or(false, |t| *t < cutoff) {
        timestamps.pop_front();
      }
      if timestamps.len() >= MAX_ALERTS_PER_WINDOW {
        debug!("Rate limited alert for user {} in category {}", alert.user_id, category_id);
        return;
      }
      timestamps.push_back(Instant::now());
    }

    // Build and send embeds
    let settings = match alert.db.users.get_prefs(alert.user_id).await {
      Ok(s) => s,
      Err(_) => return,
    };

    use crate::handlers::settings::{build_join_alert_embed, build_leave_alert_embed};

    if !join_names.is_empty() {
      let combined = combine_fmt_names(&join_names);
      let embed = build_join_alert_embed(&alert.ctx, alert.user_id, Some(alert.guild_id), &settings, &rank_name, combined.as_deref()).await;
      let _ = alert.channel.send_message(&alert.ctx.http, CreateMessage::new().embed(embed)).await;
    }

    if !leave_names.is_empty() {
      let combined = combine_fmt_names(&leave_names);
      let embed = build_leave_alert_embed(&alert.ctx, alert.user_id, Some(alert.guild_id), &settings, combined.as_deref()).await;
      let _ = alert.channel.send_message(&alert.ctx.http, CreateMessage::new().embed(embed)).await;
    }
  });
}

/// Combine format names: ["4v4", "3v3"] -> "4v4 & 3v3"
fn combine_fmt_names(names: &[Option<String>]) -> Option<String> {
  let concrete: Vec<&str> = names.iter().filter_map(|n| n.as_deref()).collect();
  if concrete.is_empty() {
    None
  } else {
    Some(concrete.join(" & "))
  }
}
