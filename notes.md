# Todo

## Priority

- Add remove group button
- Add a special method for creating selection menus:
  - Use a list if const LIST_THRESHOLD => options.len()
  - Use buttons if options.len() < LIST_THRESHOLD
  - Auto select option if only one option
- Make pretty:
  - Dashboard
  - Preferences
  - Menu
  - Match summary
- Dashboard
  - Show time since start
  - Don't show time left if in vc.
  - Before quitting app, mark bot as offline
  - Connect info
- Notifications
  - DM when game is close to ready
- Queue
  - Prevent multiple fatkids, count +1 on a player for each time
- Rank name = role name
- Config bool for role management (auto fix, auto assign to new players, etc)
- Dynamic team channels

## Secondary

- logs.tf API data
- New features:
  - Bait command/button
    - cancels current game and removes self from the queue
    - if a player is in the queue with the same or similar elo already, instead of cancelling current game, just sub the player.
  - Substitute command
    - in game command requires 3/4 of the team to agree to get a sub
    - bot sends a ping to request a sub
- Scheduled add up with options for length and delay.
- 4 team gen
- Make server config keys into config command arguments rather than values part of the key argument allowing to autocomplete the command.
- Autopings is 50/50, people want personalisation, but at the same time I can see areas where this would benefit, so yes
- Game server integration
  - Map pool voting in dashboard
  - Map pool and trends
  - Assign players to their teams
    - Display game info in the dashboard
    - Track game statistics
    - Discord and steam link
  - New elo system
  - Command to list stats
  - Captain team generation
- Burger!
  [11:39]Kafri: can you make it so if a burger adds up the bot reacts their add
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
