# Passtime.tf Discord PUG Bot

A comprehensive Discord bot for managing 4v4 pickup games (PUGs) for passtime.tf, built in Rust using the Serenity Discord library.

## Features

- **Queue Management**: Players can join/leave queue and check status
- **Automatic Team Generation**: Generates balanced 4v4 teams when 8 players are ready
- **Voice Channel Management**: Automatically moves players to team-specific voice channels
- **Match Lifecycle**: Complete match flow from queue → teams → confirmation → play → end
- **Admin Controls**: Bench players, configure bot settings, view logs
- **Role-based Permissions**: Separate permissions for runners and admins

## Commands

### Player Commands
- `/queue join` - Join the PUG queue
- `/queue leave` - Leave the PUG queue  
- `/queue status` - Check current queue status

### Runner Commands
- `/autogen` - Generate teams automatically from queue
- `/regen [match_id]` - Regenerate teams for a match
- `/confirm [match_id]` - Confirm generated teams and start match
- `/end <match_id>` - End a match and return players to queue

### Admin Commands
- `/bench <user>` - Bench a player (remove from queue)
- `/config [key] [value]` - View or modify bot configuration

## Setup

### Prerequisites
- Rust (latest stable version)
- Discord bot token
- Discord server with appropriate voice channels

### Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd pfpug
```

2. Install dependencies:
```bash
cargo build
```

3. Create environment file:
```bash
cp .env.example .env
```

4. Edit `.env` with your Discord bot token:
```
DISCORD_TOKEN=your_discord_bot_token_here
DATABASE_URL=sqlite:./pfpug.db
```

5. Run the bot:
```bash
cargo run
```

### Discord Server Setup

Your Discord server should have the following voice channels:
- **Queue** - Where players wait for games
- **RED Team** - RED team voice channel  
- **BLU Team** - BLU team voice channel
- **Server A/B/C** - Game server channels

Create the following roles:
- **Runner** - Can generate teams, confirm matches, end matches
- **Admin** - Full permissions including config and bench commands

### Configuration

Use `/config` command to set up channel IDs and role IDs:

```
/config guild_id <your_server_id>
/config queue_channel_id <queue_voice_channel_id>
/config red_channel_id <red_team_voice_channel_id>
/config blu_channel_id <blu_team_voice_channel_id>
/config server_a_channel_id <server_a_voice_channel_id>
/config log_channel_id <text_channel_for_logs>
/config runner_role_id <runner_role_id>
/config admin_role_id <admin_role_id>
```

## Workflow

1. **Queue Phase**: Players use `/queue join` to enter the queue
2. **Quota Notification**: When 8 players are ready, the bot notifies everyone
3. **Team Generation**: A runner uses `/autogen` to create teams
4. **Confirmation**: Runner uses `/confirm` to start the match and move players to voice channels
5. **Match Play**: Players play their 4v4 match
6. **Match End**: Runner uses `/end` to finish the match and return players to queue

## Database

The bot uses SQLite to store:
- User information (Discord ID, username, Steam ID)
- Queue sessions and status
- Match history and team compositions  
- Bot configuration settings

## Development

### Project Structure
```
src/
├── main.rs              # Bot initialization and event handling
├── database.rs          # Database layer and operations
├── models/              # Data models
│   ├── user.rs
│   ├── queue.rs  
│   ├── match_model.rs
│   └── config.rs
└── handlers/            # Command handlers
    ├── queue.rs         # Queue management commands
    ├── match_handler.rs # Team generation and match commands
    └── admin.rs         # Admin commands
```

### Building
```bash
cargo build --release
```

### Running Tests
```bash
cargo test
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

[Add your license here]

## Support

For support, join the passtime.tf Discord server or create an issue on GitHub.
