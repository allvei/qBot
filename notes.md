# Notes

- Each server, a b and c should have their own red and blue channels, eg, red_a, blue_a, red_b, blue_b, red_c, blue_c
- Autogen should not be a command, but a function, it will only exist in the backend. Each time a player joins a queue, it is run and the teams are shuffled.
- Bench logs should only be displayed within the queue embed like this, using columns:
  ```txt
  RED
  P1 | 20 | Benched by [admin]
  P2 | 50 |
  P3 | 30 | Benched by [admin]
  P4 | 85 |

  BLU
  P1 | 20 |
  P2 | 50 |
  P3 | 30 |
  P4 | 85 |
  ```
  No logs are actually generated
- QueueType isn't needed, this should be flexible and configured based only on channel ID and roles that can access the channel.
  Each queue will have it's counterpart queue text channel where users will use the command to add to that specific q.
  eg. Newbie pugs VC and #newbie-add text channel
  Journey pugs VC and #journey-add text channel
  These channels are also generated through the config
- Rename internal Queue to session, each game will have this loop flow:
  waiting <-> hot <-> pushing <-> playing <-> pulling -> waiting
  Undoing progression will be possible with a command.
  Each session will then have it's own ID.
- if using the embed msg, there should be a queue toggle button, but using commands there should be /join and /leave
- rename regen to shuffle
- rename match to session
- rename confirm to accept