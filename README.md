# Passtime.tf Discord PUG Bot

A comprehensive Discord bot for managing 4v4 pickup games (PUGs) for passtime.tf, built in Rust using the Serenity Discord library.

## Features

- **Queue Management**:          Players can join/leave queue and check status
- **Automatic Team Generation**: Generates balanced 4v4 teams when 8 players are ready
- **Voice Channel Management**:  Automatically moves players to team-specific voice channels
- **Match Lifecycle**:           Complete match flow from queue → teams → confirmation → play → end
- **Admin Controls**:            Buffer players, configure bot settings, view logs
- **Role-based Permissions**:    Separate permissions for runners and admins

## Commands

### Player Commands
- `/join`                 - Join the PUG queue
- `/leave`                - Leave the PUG queue

### Runner Commands
- `/shuffle [match_id]`   - Regenerate teams for a match
- `/accept [match_id]`    - Confirm generated teams and start match
- `/end [match_id]`       - End a match and return players to queue

### Admin Commands
- `/buffer [user]`        - Buffer a player (guarantee a spot in the current session)
- `/config [key] [value]` - View or modify bot configuration