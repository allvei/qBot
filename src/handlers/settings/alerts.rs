use serenity::all::{
  UserId as UI,
  GuildId as GI,
  CreateEmbed as CE,
  CreateEmbedFooter as CEF,
};
use serenity::prelude::Context;


/// Messages to replace description spam with
const SPAM_REPLACEMENT_MESSAGES: &[&str] = &[
  "If this is shart city, then I am the mayor",
  "*proceeds to triple-dribble in front of your goal and die*",
  "If it wasn't for xCape, I'd be GM already...",
  "@ me = free pocket medic",
  "idk, glop bomb is the best way to score",
  "#removemedic",
  "#justiceforsleepy",
  "#justiceforwerxify",
  "im so fucking ass",
  "Rawr x3 nuzzles how are you pounces on you you're so warm",
];
const FOOTER_SPAM_REPLACEMENT_MESSAGES: &[&str] = &["Mmmm, feet :3", "Go team!", "PUG PUG PUG!", "GG!", "qBot is best bot"];
const SANITIZE_ALERTS_ENABLED: bool = false;
const MAX_ALERT_NEWLINES: usize = 4;
const MAX_ALERT_CHARS: usize = 180;

/// Check if text exceeds alert message limits (max 4 newlines, 180 chars)
fn exceeds_alert_limits(text: &str) -> bool {
  text.matches('\n').count() > MAX_ALERT_NEWLINES || text.chars().count() > MAX_ALERT_CHARS
}

/// Process text and replace with a random message from `replacements` if limits exceeded
fn sanitize_text(text: &str, replacements: &[&str]) -> String {
  if SANITIZE_ALERTS_ENABLED && exceeds_alert_limits(text) {
    use rand::RngExt;
    let mut rng = rand::rng();
    let idx = rng.random_range(0..replacements.len());
    return replacements[idx].to_string();
  }
  text.to_string()
}

/// Build a join announcement embed (used for both actual announcements and previews)
pub async fn build_join_alert_embed(ctx: &Context, user_id: UI, guild_id: Option<GI>, settings: &crate::db::repo::UserSettings, rank_name: &str, fmt_name: Option<&str>) -> CE {
  // Get display name - try member nickname first, then user name, then user ID
  let display_name = if let Some(gid) = guild_id {
    // With guild context - try to get member for nickname
    let member = gid.member(&ctx.http, user_id).await.ok();
    if let Some(m) = member {
      m.display_name().to_string()
    } else {
      // Fallback to fetching user directly
      ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
    }
  } else {
    // For preview without guild context, fetch from HTTP API
    ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
  };

  // Build description with template support
  // If custom description is set (even if empty), use it; otherwise use default
  let description = match &settings.join_alert_desc {
    Some(custom_desc) if !custom_desc.trim().is_empty() => {
      // Sanitize newline spam only for actual announcements (not previews)
      let text_to_use = if guild_id.is_some() { sanitize_text(custom_desc, SPAM_REPLACEMENT_MESSAGES) } else { custom_desc.to_string() };

      // Replace template variables
      Some(text_to_use.replace("{user}", &format!("<@{}>", user_id)).replace("{rank}", rank_name).replace("{name}", &display_name))
    }
    Some(_) => None, // Empty string means no description
    None => None,
  };

  // Create embed with title showing nickname + "joined the queue"
  let mut embed = CE::new()
    .title(match fmt_name {
      Some(name) => format!("{display_name} joined the {name} queue"),
      None => format!("{display_name} joined the queue"),
    })
    .color(settings.join_alert_color);

  // Only add description if there is one
  if let Some(desc) = description {
    embed = embed.description(desc);
  }

  // Add custom footer
  if let Some(footer_text) = &settings.join_alert_footer {
    // Sanitize footer spam only for actual announcements (not previews)
    let footer_to_use = if guild_id.is_some() { sanitize_text(footer_text, FOOTER_SPAM_REPLACEMENT_MESSAGES) } else { footer_text.to_string() };

    let mut footer = CEF::new(footer_to_use);
    if let Some(footer_icon) = &settings.join_alert_footer_img {
      footer = footer.icon_url(footer_icon);
    }
    embed = embed.footer(footer);
  }

  // Add thumbnail
  if let Some(thumbnail) = &settings.join_alert_img {
    embed = embed.thumbnail(thumbnail);
  }

  embed
}

/// Build a leave announcement embed (used for both actual announcements and previews)
pub async fn build_leave_alert_embed(ctx: &Context, user_id: UI, guild_id: Option<GI>, settings: &crate::db::repo::UserSettings, fmt_name: Option<&str>) -> CE {
  // Get display name - try member nickname first, then user name, then user ID
  let display_name = if let Some(gid) = guild_id {
    // With guild context - try to get member for nickname
    let member = gid.member(&ctx.http, user_id).await.ok();
    if let Some(m) = member {
      m.display_name().to_string()
    } else {
      // Fallback to fetching user directly
      ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
    }
  } else {
    // For preview without guild context, fetch from HTTP API
    ctx.http.get_user(user_id).await.map(|u| u.name.clone()).unwrap_or_else(|_| user_id.to_string())
  };

  // Build description with template support
  // If custom description is set (even if empty), use it; otherwise use default
  let description = match &settings.leave_alert_desc {
    Some(custom_desc) if !custom_desc.trim().is_empty() => {
      // Sanitize newline spam only for actual announcements (not previews)
      let text_to_use = if guild_id.is_some() { sanitize_text(custom_desc, SPAM_REPLACEMENT_MESSAGES) } else { custom_desc.to_string() };

      // Replace template variables (no rank for leave)
      Some(text_to_use.replace("{user}", &format!("<@{}>", user_id)).replace("{name}", &display_name))
    }
    Some(_) => None, // Empty string means no description
    None => None,
  };

  // Create embed with title showing nickname + "left the queue"
  let mut embed = CE::new()
    .title(match fmt_name {
      Some(name) => format!("{display_name} left the {name} queue"),
      None => format!("{display_name} left the queue"),
    })
    .color(settings.join_alert_color);

  // Only add description if there is one
  if let Some(desc) = description {
    embed = embed.description(desc);
  }

  // Add custom footer if provided
  if let Some(footer_text) = &settings.leave_alert_footer {
    // Sanitize footer spam only for actual announcements (not previews)
    let footer_to_use = if guild_id.is_some() { sanitize_text(footer_text, FOOTER_SPAM_REPLACEMENT_MESSAGES) } else { footer_text.to_string() };

    let mut footer = CEF::new(footer_to_use);
    if let Some(footer_icon) = &settings.leave_alert_footer_img {
      footer = footer.icon_url(footer_icon);
    }
    embed = embed.footer(footer);
  }

  // Add custom thumbnail if provided
  if let Some(thumbnail) = &settings.leave_alert_img {
    embed = embed.thumbnail(thumbnail);
  }

  embed
}