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

//
// Queue functions
//

/// `/join` and `/leave`
pub async fn queue<'a>(cc: &'a CommandContext<'a>, mut guild: Server) -> Result<()> {
    info!("Processing queue command from user {}", cc.intax.user.id);
    let client_id  = cc.intax.user.id;
    let channel_id = cc.intax.channel_id;
    
    // Get group of current channel
    let group = guild.get_group(channel_id).unwrap();
    
    // Check if we have idle games
    match group.get_games_by_status(&GameStatus::Idle).len() {
        0 => {
            info!("No idle games found, creating a new game");
            group.create_game();
        },
        1 => {
            info!("Found one existing idle game");
        },
        n => {
            return Err(anyhow!("Found more than one idle game ({}). This is unexpected. ", n));
        },
    }

    // Get player info or create a new one
    let player = match cc.db.get_user(client_id).await {
        Ok(player) => {
            player
        }
        Err(_) => {
            info!("Creating new user in db!");
            cc.db.new_user(client_id).await?
        }
    };

    // Check if player is already in game
    if group.get_user_game(player.discord_id).is_ok() {
        info!("Player {} is already in a game", player.discord_id);
        return Ok(());
    }


    info!("Command processed successfully, sending response");
    Ok(())
}

/// `/status`
pub async fn status<'a>(cc: &'a CommandContext<'a>, mut guild: Server) -> Result<()> {
    info!("Processing queue status command");
    // Get active group with game
    let group = guild.get_group(cc.intax.channel_id).unwrap();

    // If group has no games or game pool, return empty count
    let count = if group.games.is_empty() {
        0
    } else {
        group.games.last().expect("No active game found").pool.len()
    };

    let description = if count == 0 {
        "Queue is empty. Use `/join` to join!".to_string()
    } else {
        let mut parts = vec![format!("**{} players in queue:**\n", count)];
        // Ensure we have a game and access its pool
        if let Some(game) = group.games.last() {
            for (i, member) in game.pool.iter().enumerate() {
                // Use discord_id as display name if needed
                let name = format!("user_{}", member.player.discord_id);
                parts.push(format!("{}.{}", i + 1, name));
            }
        }
        parts.join("\n")
    };

    let embed = CE::new()
        .title("Queue Status")
        .description(description)
        .footer(CEF::new(format!("Queue: {}/8", count)));

    let response = CIR::Message(
        CIRM::new().embed(embed).ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;
    Ok(())
}

/// `/shuffle`
pub async fn shuffle(cc: &CommandContext<'_>, mut guild: Server) -> Result<()> {
    info!("Processing shuffle command");
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.create_bot_reply("Only runners can shuffle teams!").await?;
        return Ok(());
    }

    // Get active group with game
    let group = guild.get_group(cc.intax.channel_id).unwrap();

    if group.games.is_empty() {
        cc.create_bot_reply("No active games.").await?;
        return Ok(());
    }

    let game = group.games.last().unwrap();

    if game.pool.len() < 8 {
        cc.create_bot_reply(
            &format!("Not enough players in game. Need {} more.", 8 - game.pool.len())
        ).await?;
        return Ok(());
    }

    // Collect players and split into teams (synchronous shuffle so no !Send types live across await)
    let (mut red_team, mut blu_team) = split_into_teams(&game.pool);
    let mut updated_group = group.clone();

    // Assign teams using GamePlayer's team method
    for sp in &mut red_team {
        sp.team(Team::Red);
    }
    for sp in &mut blu_team {
        sp.team(Team::Blu);
    }

    // Update pool with new team assignments
    updated_group.games.last_mut().unwrap().pool.clear();
    updated_group.games.last_mut().unwrap().pool.extend(red_team.into_iter());
    updated_group.games.last_mut().unwrap().pool.extend(blu_team.into_iter());

    updated_group.games.last_mut().unwrap().status = GameStatus::Hot;
    // TODO: Persist updated_group changes to DB if needed (no update_group method exists)
    // You may need to implement this in your database layer.

    let red_team_names: Vec<String> = updated_group.games.last().unwrap().pool.iter().filter(|sp| sp.team == Some(Team::Red)).map(|sp| format!("<@{}>", sp.player.discord_id)).collect();
    let blu_team_names: Vec<String> = updated_group.games.last().unwrap().pool.iter().filter(|sp| sp.team == Some(Team::Blu)).map(|sp| format!("<@{}>", sp.player.discord_id)).collect();

    let embed = CE::new()
        .title("Teams Generated!")
        .description(
            format!(
                "**Game ID:** `{}`\n\n**🔴 RED Team:**\n{}\n\n**🔵 BLU Team:**\n{}",
                stringify!(updated_group.game.last().unwrap().id),
                red_team_names.join("\n"),
                blu_team_names.join("\n")
            )
        )
        .footer(CEF::new("Use /accept to confirm teams"));

    let response = CIR::Message(
        CIRM::new().embed(embed).ephemeral(true)
    );

    cc.intax.create_response(&cc.ctx.http, response).await?;

    Ok(())
}

/// `/accept`
pub async fn accept(cc: &CommandContext<'_>, mut guild: Server) -> Result<()> {
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.create_bot_reply("Only runners can accept games!").await?;
        return Ok(());
    }

    // Get the group for the current channel
    let channel_id = cc.intax.channel_id;
    let group = guild.get_group(channel_id).unwrap();

    match group.get_games_by_status(&GameStatus::Hot).len() {
        0 => {
            cc.create_bot_reply("No hot games found in this group.").await?;
            return Ok(());
        },
        1 => {
            info!("Found one existing hot game");
        },
        n => {
            return Err(anyhow!("Found more than one hot game ({}). This is unexpected. ", n));
        },
    };

    let target_game = &mut group.get_games_by_status(&GameStatus::Hot)[0];
    
    // Update game status to Push
    target_game.status = GameStatus::Push;

    cc.create_bot_reply("Game accepted! Players moved to team channels.").await?;

    Ok(())
}

/// `/end`
///
/// * `cc` - The command context.
pub async fn end(cc: &CommandContext<'_>, mut guild: Server) -> Result<()> {
    info!("Processing end command");
    // Check permissions
    if !check_role(cc, &Role::Runner).await? {
        cc.create_bot_reply("Only runners can end games!").await?;
        return Ok(());
    }

    let group = guild.get_group(cc.intax.channel_id).unwrap();

    // Find the game that the current user is participating in
    let user_id = cc.intax.user.id;
    let game_index = group.games.iter().position(|s| {
        s.pool.iter().any(|player| player.player.discord_id == user_id)
    });

    if let Some(index) = game_index {
        // Set the game status to Pull (ended)
        group.games[index].status = GameStatus::Pull;

        // TODO: Persist group changes to DB if needed (no update_group method exists)
        // You may need to implement this in your database layer.

        // Move players to queue channel if we're in a guild
        if let Some(guild_id) = cc.intax.guild_id {
            move_players_to_queue_channel(
                cc.ctx,
                &cc.db,
                &group,
                &group.games[index],
                guild_id
            ).await?;
        }
    } else {
        cc.create_bot_reply("You are not currently participating in any game.").await?;
        return Ok(());
    }

    cc.create_bot_reply("Game has been ended. Players will be moved back to queue.").await?;

    Ok(())
}
