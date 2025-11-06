# Todo

## Testing

- Moving players
  - Move to team channels
  - Move to queue channel
- Shuffle
- Start match
- End match

## Priority

- Reply if conf not generated.
- Functional dashboard
  - Buttons:
    - Shuffle players
    - Start match
    - End match
  - Automatically update to list current players in the queue, if quota met, generate teams and show gen result
  - If over quota, list other players as queued for next game
  - List current matches and their status
- First come first serve system, person with the longest time spent in queue has position.
- New team generation algorithm
- Command to handle roles - ?
- During setup or group creation, offer to create the group channels.

## Secondary

- New features:
  - Bait command/button
    - cancels current game and removes self from the queue
    - if a player is in the queue with the same or similar elo already, instead of cancelling current game, just sub the player.
  - Substitute command
    - in game command requires 3/4 of the team to agree to get a sub
    - bot sends a ping to request a sub
- Scheduled add up with options for length and delay.
- 4 team gen
- Different elo for different classes
- Map pool look trends
- Add terminal commands:
  - Status
  - List guilds
  - List games
  - Query DB
  - Print config for a guild
- Make server config keys into config command arguments rather than values part of the key argument allowing to autocomplete the command.
- Autopings is 50/50, people want personalisation, but at the same time I can see areas where this would benefit, so yes
- Map pool and trends
- Map pool voting in dashboard
- Game server integration
  - Assign players to their teams
    - Display game info in the dashboard
    - Track game statistics
    - Discord and steam link
  - New elo system
  - Command to list stats
  - Captain team generation
- Dashboard
  - on guild_create
  - for every group in guild
  - check if dashboard exists
    - how?
    - search for message id
- Setup command that uses dropdowns etc to pick the channels and roles for a group
- Burger!
  [11:39]Kafri: can you make it so if a burger adds up the bot 🍔 reacts their add
  [11:39]Kafri: or pings them with "BURGER!!!"
  [11:39]Kafri: it would be awesome
  rare chance! 0.05

## Aligning with Russ and his website

- Using his database, needs tables to be expanded
- Web can display roles and user data.
- Discord roles are synchronised with the game server roles.
- Endpoints and auth for web to access data.
- Webhooks.
- Relay server data to the server, no A2S or RCON needed.
- SDR gets new IP on each launch, need to find where to read the ip from to allow players to connect.
