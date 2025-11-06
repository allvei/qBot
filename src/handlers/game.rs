// Combined game handlers
use std::sync::Arc;

use anyhow::{ anyhow, Result };
use rand::rng;
use rand::seq::SliceRandom;
use serenity::all::{
    ChannelId,
    Context,
    CreateEmbed as CE,
    CreateEmbedFooter as CEF,
    CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
    EditMember,
    GuildId,
};
use tracing::info;

use crate::handlers::player::check_role;
use crate::models::server::*;
use crate::models::game::*;
use crate::database::Database;
use crate::models::command::{CommandContext};

/// Splits the players into two teams.
///
/// * `players` - The players to split into teams.
pub fn split_into_teams(players: &[GamePlayer]) -> (Vec<GamePlayer>, Vec<GamePlayer>) {
    let mut rng = rng();
    let mut player_list: Vec<GamePlayer> = players.to_vec();
    player_list.shuffle(&mut rng);
    let team_size = player_list.len() / 2;
    let team1 = player_list[0..team_size].to_vec();
    let team2 = player_list[team_size..].to_vec();
    (team1, team2)
}


/// Moves players back to the queue channel.
///
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `group`      - The group containing game and queue info.
/// * `game`    - The game with players to move.
/// * `guild_id`   - The ID of the guild where the game is taking place.
async fn move_players_to_queue_channel(
    ctx:      &Context,
    _db:      &Arc<Database>,
    group:    &Group,
    game:  &Games,
    guild_id: GuildId
) -> Result<()> {
    // Check if queue channel is configured
    if group.channels.queue.get() != 0 {
        for player in &game.pool {
            let user_id = player.player.discord_id;
            // Try to move the user back to queue
            let _ = ctx.http.edit_member(
                guild_id,
                user_id,
                &EditMember::new().voice_channel(ChannelId::new(group.channels.queue_vc.get())),
                Some("Moving player back to queue voice channel")
            ).await;
        }
    }
    Ok(())
}

/// Moves players to their respective team channels.
///
/// * `ctx`        - Ref to the Serenity context.
/// * `db`         - Ref to the database.
/// * `group`      - The group containing team channel information.
/// * `game`    - The game with assigned teams.
/// * `guild_id`   - The ID of the guild where the game is taking place.
async fn move_players_to_team_channels(
    ctx:      &Context,
    _db:      &Arc<Database>,
    group:    &Group,
    game:  &Games,
    guild_id: GuildId
) -> Result<()> {
    // Get red/blue voice channel IDs from the first team in the group
    if group.channels.teams.is_empty() {
        return Err(anyhow!("No team channels configured for this group"));
    }
    let redvc = group.channels.teams[0].red_vc;
    let bluvc = group.channels.teams[0].blu_vc;

    // Move players to red/blu voice channels
    for player in &game.pool {
        if let Some(team) = &player.team {
            let target_channel = match team {
                Team::Unassigned => continue,
                Team::Red        => redvc,
                Team::Blu        => bluvc,
            };
            let user_id = player.player.discord_id;
            if let Ok(mut member) = guild_id.member(&ctx.http, user_id).await {
                let _ = member.edit(
                    &ctx.http,
                    EditMember::new().voice_channel(target_channel)
                ).await;
            }
        }
    }

    Ok(())
}