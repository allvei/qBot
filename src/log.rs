use tracing::{info, warn};
use serenity::all::{Context, UserId};

// Import guild_name function from constants
use crate::models::constants::guild_name;

/// Get user tag for logging purposes - tries database first, then Discord API
pub async fn get_user_tag(ctx: &Context, user_id: UserId, db: &crate::Database) -> String {
    // Try database first (most efficient)
    if let Ok(player) = db.get_user(user_id, ctx).await {
        if !player.tag.is_empty() {
            return player.tag;
        } else {
            warn!("User {} found in database but tag is empty, falling back to Discord API", user_id);
        }
    } else {
        warn!("User {} not found in database, falling back to Discord API", user_id);
    }
    
    // Fallback to Discord API
    match ctx.http.get_user(user_id).await {
        Ok(user) => user.tag(),
        Err(e) => {
            warn!("Failed to get user {} from Discord API: {}, using user ID as fallback", user_id, e);
            user_id.to_string()
        }
    }
}

pub fn log_queue_toggle(guild_name: &str, category_name: &str, tag: &str, queue_type: QueueToggleType, pool_size: Option<(usize, usize)>, sg_name: Option<&str>, position: Option<usize>) {
    let (action, source) = match queue_type {
        QueueToggleType::BJ => ("joined", None),
        QueueToggleType::BL => ("left",   None),
        QueueToggleType::VJ => ("joined", Some("VC")),
        QueueToggleType::VL => ("left",   Some("VC")),
    };

    let pos_part = position.map(|p| format!("#{}", p)).unwrap_or_default();

    let prefix = log_prefix_format(guild_name, category_name, sg_name.unwrap_or(""));
    
    match (pool_size, source) {
        (Some((current, quota)), Some(src)) => info!("{} {} {} {} ({}) [{}/{}]", prefix, pos_part, tag, action, src, current, quota),
        (Some((current, quota)), None)       => info!("{} {} {} {} [{}/{}]",     prefix, pos_part, tag, action, current, quota),
        (None, Some(src))                    => info!("{} {} {} {} ({})",       prefix, pos_part, tag, action, src),
        (None, None)                         => info!("{} {} {} {}",             prefix, pos_part, tag, action),
    }
}

/// Generate log prefix in format [GUILD_NAME/CATEGORY_NAME/FORMAT_NAME]
pub fn log_prefix_category(guild_name: &str, category_name: &str) -> String {
    format!("[{}/{}]", guild_name, category_name)
}

/// Generate log prefix in format [GUILD_NAME/CATEGORY_NAME/FORMAT_NAME]
pub fn log_prefix_format(guild_name: &str, category_name: &str, format_name: &str) -> String {
    let sg_suffix = if format_name.is_empty() { 
        "".to_string() 
    } else { 
        format!("/{}", format_name) 
    };
    format!("[{}/{}{}]", guild_name, category_name, sg_suffix)
}

/// Generate log prefix from Context and IDs
pub async fn log_prefix_from_context(ctx: &Context, guild_id: serenity::all::GuildId, category_name: &str, format_name: &str) -> String {
    let guild_name = guild_name(ctx, guild_id);
    log_prefix_format(&guild_name, category_name, format_name)
}

pub enum QueueToggleType {
    BJ, // Button Join
    BL, // Button Leave
    VJ, // Voice Join
    VL, // Voice Leave
}
