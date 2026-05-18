# qBot

A comprehensive Discord bot for managing PUGs, built with Rust using the Serenity Discord library.

## Why was this made over existing PUG bots?

- **To make it more efficient.**
  Other bots need Discord commands that are very clunky and inconvenient to use, I look to minimize the use of them by using interactive components such as buttons, or automatic common actions like generating teams, or automatically queueing players when they join a voice channel.
- **To make information compact and easy to access.**
  Other bots require commands to display info that could be spread information across multiple channels.
  I solved this by creating a dashboard that consolidates all information into one place.
- **To make it more personal for passtime.tf**
  Other bots were made to be more general for all types of games, I aimed to make ours more specific for passtime.tf.
- **To give room for more features.**
  We don't have control over the code of other bots. This would help us expand the bot with more features in the future. Check out the planned features below.

## Features

- **Queue Management**:          Players can join/leave queue and check status from the dashboard
- **Automatic Team Generation**: Generates balanced teams when queue is full
- **Voice Channel Management**:  Automatically moves players to team-specific voice channels when starting a game.
- **Match Lifecycle**:           Complete match flow from queue → teams → confirmation → play → end
- **Admin Controls**:            Player queue management, configure bot settings, view logs
- **Role-based Permissions**:    Separate permissions for runners and admins
- **Balancing**:                 Choose between ELO-based matchmaking or manual point-based balancing.

## Planned Features

### Major

- Integration with the TF2 game server.
  - Automatically assign players to the right teams.
  - Track server and player statistics.
  - Automatically move players back to the queue when a team wins.
- Class and region specific elo ratings.

### Minor

- Map voting and rotation.
- Methods for requesting substitutions when a player becomes unavailable.
- Pre-game class selection.
- Captain mode for manual team creation.
