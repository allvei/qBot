# Passtime.tf PUG Bot

A comprehensive Discord bot for managing PUGs for passtime.tf, built in Rust using the Serenity Discord library.

## Why was this made over Pugbot?

- **To make it more efficient.**
  Current Pugbot requires Discord commands that are very clunky and inconvenient to use, I look to minimize the use of them by using interactive components such as buttons, or automatic common actions like generating teams, or automatically queueing players when they join a voice channel.
- **To make information compact and easy to access.**
  Current Pugbot requires commands to access information or spreads information it across multiple channels.
  I solved this by creating a dashboard that consolidates all information into one place.
- **To make it more personal for passtime.tf**
  Current PUGbot was made to be more general for all types of games, I aimed to make ours more specific for passtime.tf.
- **To give room for more features.**
  We don't have control over the PUGbot code. This would help us expand the bot with more features in the future. Check out the planned features below.

## Features

- **Queue Management**:          Players can join/leave queue and check status from the dashboard
- **Automatic Team Generation**: Generates balanced 4v4 teams when queue is full
- **Voice Channel Management**:  Automatically moves players to team-specific voice channels
- **Match Lifecycle**:           Complete match flow from queue → teams → confirmation → play → end
- **Admin Controls**:            Buffer players, configure bot settings, view logs
- **Role-based Permissions**:    Separate permissions for runners and admins

## Planned Features

- Integration with the TF2 game server.
  - Automatically assign players to the right teams.
  - Track server and player statistics.
  - Automatically move players back to the queue when a team wins.
  - Send non-generated players automatically to spectator.
- Map voting and rotation.
- Methods for requesting substitutions when a player becomes unavailable.
- Pre-game class selection.
- Class specific elo ratings.
- Schedule based player queueing.
- Captain mode for manual team creation.
- Show the queue status in the voice channels name.
