# Balanced Composite Heuristic (BCH)

## Overview

**Balanced Composite Heuristic (BCH)** is a team balancing method designed to fairly distribute players into two teams by considering three key statistical metrics:

- **Average (mean) ELO**
- **Median ELO**
- **Standard Deviation (spread) of ELO**

BCH evaluates all possible team splits and selects the one with the lowest combined score of these differences between the two teams, resulting in the most balanced match based on skill level and distribution.

---

## Why BCH?

Traditional methods like ABBAABBA or ABABABAB use drafting orders to approximate balance. However, they do not consider actual numerical distribution and are prone to:

- Outlier stacking
- Unbalanced spread of skill
- Inflexibility to different skill profiles

BCH directly evaluates balance using measurable criteria.

---

## How BCH Works

### Step 1: Generate All Valid Team Splits

- For `n` players (even number), generate all unique ways to split into two equal teams.
- This is \(C(n, n/2) / 2\) combinations.

### Step 2: Evaluate Each Split

For each team split (Team A and Team B):

1. **Calculate average (mean)** ELO for both teams
2. **Calculate median** ELO for both teams
3. **Calculate standard deviation** of ELO for both teams

### Step 3: Score the Split

Compute the absolute differences between the two teams:

```
avg_diff = |avg(team_a) - avg(team_b)|
med_diff = |median(team_a) - median(team_b)|
std_diff = |stddev(team_a) - stddev(team_b)|

score = avg_diff + med_diff + std_diff
```

This gives each team split a score. The lower the score, the better the balance.

### Step 4: Pick the Best Split

Choose the split with the lowest total score.

---

## Practical Tips

- **Inputs**: Player list with their ELO values.
- **Output**: Two teams with balanced skill.
- **Performance**: For 20+ players, full evaluation can be slow. Use sampling, cutoff thresholds, or heuristics.

---

## Implementation Guide

### Data Structure Example (Python)

```python
players = {
    "Player1": 75,
    "Player2": 40,
    "Player3": 90,
    "Player4": 60,
    ...
}
```

### Core Functions

```python
from itertools import combinations
import statistics

# Calculate stats
def team_stats(team, players):
    elos = [players[p] for p in team]
    return statistics.mean(elos), statistics.median(elos), statistics.stdev(elos)

# Score a team split
def score_split(team_a, team_b, players):
    avg_a, med_a, std_a = team_stats(team_a, players)
    avg_b, med_b, std_b = team_stats(team_b, players)

    score = abs(avg_a - avg_b) + abs(med_a - med_b) + abs(std_a - std_b)
    return score

# Find best team split
def find_best_split(players):
    player_names = list(players.keys())
    best = None
    lowest_score = float('inf')

    for team_a in combinations(player_names, len(player_names) // 2):
        team_b = [p for p in player_names if p not in team_a]
        score = score_split(team_a, team_b, players)
        if score < lowest_score:
            best = (team_a, team_b)
            lowest_score = score

    return best, lowest_score
```

---

## When to Use BCH

- Competitive matches
- Tournaments or custom games
- Any scenario where team fairness matters more than speed

## When to Avoid BCH

- Real-time matchmaking with large player pools
- Games where roles/synergies matter more than numerical skill

---

## Alternatives & Integration

| Method   | Pros                       | Cons                        |
| -------- | -------------------------- | --------------------------- |
| **BCH**  | Most balanced, data-driven | Slower for large groups     |
| ABBAABBA | Fast, intuitive            | Doesn’t consider ELO values |
| ABABABAB | Easy to implement          | Prone to outlier stacking   |

For >20 players:

- Use BCH on a **sampled subset** of combinations.
- Consider **hybrid approach**: try ABBA/ABAB, then fallback to BCH if needed.

---

## Summary

The **Balanced Composite Heuristic** is a robust, flexible, and fair method for team balancing using multiple statistical indicators. It outperforms traditional draft methods in many scenarios, especially when player skill distribution is uneven.

For best results: use BCH where fairness is critical, and optimize or fallback when speed is a priority.

