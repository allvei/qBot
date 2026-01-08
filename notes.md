# Todo

## Testing

### Setup & Configuration

- `/setupadd`    - Create roles and group (full setup)
- `/roleadd`     - Create runner and admin roles
- `/groupadd`    - Create a new category with all group channels
- `/grouplink`   - Link existing channels to a group
- `/groupremove` - Remove a group

### Settings Menus

- `/settings` - Open personal settings menu
  - Toggle DM alerts
  - Toggle VC kick
  - Set timeout length
  - Edit join/leave alerts
- `/serversettings` - Open server settings menu (admin)
  - Toggle Dynamic ELO
  - Set runner role
  - Set admin role
- `/groupsettings` - Open group settings menu (runner)
  - Edit group name
  - Edit quota
  - Edit timeout
  - Edit connect info
- `/editplayer <user>` - Open player settings menu (admin)
  - Edit Steam ID
  - Edit ELO
  - Edit Rank

### Queue & Game Flow

- Dashboard "Add" button      - Join queue
- Dashboard "Leave" button    - Leave queue
- Dashboard "Settings" button - Open settings
- Queue fills to quota        - Session goes Hot
- Teams generated             - Players moved to team VCs
- Game ends                   - Players pulled back, session resets

### Player Management

- `/buffer <user>`     - Move player to start of queue
- `/fatkid <user>`     - Move player to end of queue
- `/clear`             - Clear all players from queue
- `/getplayerelo [user]` - View player ELO info

### Rank System

- `/rankadd <rank> <role>`        - Add Discord role to rank
- `/rankremove <rank> <role>`     - Remove Discord role from rank
- `/ranklist [rank]`              - List rank role mappings
- `/ranksetelo <rank_role> <elo>` - Set custom ELO for rank

### Timeout & Alerts

- Player timeout - Auto-remove after configured time
- Join alert     - Custom embed on player join
- Leave alert    - Custom embed on player leave
- DM alerts      - Notify player when game ready

### Edge Cases

- Player leaves during Hot phase
- Player disconnects from VC during game
- Multiple groups in same server
- Role permissions (runner vs admin vs regular user)

## Priority

- Edit groups via /serversettings
  - Edit a group
  - Add a new group
    - Create new channels OR link existing channels
  - Remove a group
- Group settings is only for admins
- Move roleadd and rolelink under server settings.

- Add right click user actions

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
