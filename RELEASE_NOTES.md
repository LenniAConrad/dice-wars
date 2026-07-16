# Dice Wars v1.1.0

Balance release. Online players must both update (older versions are told so).

## Fairness

- **The starting deal is fair now.** Maps are dealt so every player's biggest
  connected cluster is comparable — no more winning the game at map generation.
- **Equal territory counts.** Extra regions become lakes so the map always
  divides evenly among the players; the host no longer gets a spare territory.
- **Random first player.** Who moves first is decided by the map seed, not by
  who hosts.

## Smarter bots

- All bots (except Easy) now lean on whoever is running away with the game,
  harder the more of the map the leader holds — games stay contested longer.
- Bots consider what they leave behind: no more emptying a territory next to a
  big enemy stack for a marginal capture, and they prefer conquests that knit
  their territory together.
- Normal bots also reinforce their frontlines now (previously Hard only).

## Map

- Bigger playfield (30x22 cells, ~50 territories) with more lakes and more
  ragged coastlines.

## Downloads

- **Linux**: `dice-wars-v1.1.0-linux-x86_64.tar.gz`
- **Windows**: `dice-wars-v1.1.0-windows-x86_64.zip`
