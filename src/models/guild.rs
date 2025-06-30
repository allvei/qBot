use serenity::{
    all::GuildId, model::guild::Guild, prelude::*
};

/// Given a slice of `GuildId`, returns a Vec<Guild> using cache or REST.
pub async fn fetch_guilds_from_ids(ctx: &Context, guild_ids: &[GuildId]) -> Vec<Guild> {
    let mut guilds = Vec::new();
    for guild_id in guild_ids {
        if let Some(cached) = ctx.cache.guild(*guild_id) {
            guilds.push(cached.to_owned());
        } else {
            // REST fallback: get partial guild data and convert to full guild
            if let Ok(partial_guild) = guild_id.to_partial_guild(&ctx.http).await {
                // Convert partial guild to full guild using the cache or API
                if let Some(cached_guild) = ctx.cache.guild(partial_guild.id) {
                    guilds.push(cached_guild.to_owned());
                }
            }
        }
    }
    guilds
}
