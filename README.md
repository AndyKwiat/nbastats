# nbastats

NBA 2026-27 regular-season stats app: every game and box score, league leaders with headshots,
standings, team/player advanced stats (PER, Win Shares, BPM, VORP, ORtg/DRtg, four factors…),
eight team rating systems (Elo, SRS, Sagarin-style, Massey, Colley, RPI, Bradley-Terry, Net) and charts.

- Data: ESPN public site API (`site.api.espn.com`), headshots/logos from ESPN's CDN.
- Backend: Rust (axum + rusqlite). `src/fetch.rs` ingests, `src/stats.rs` computes everything in memory.
- Frontend: vanilla JS SPA in `static/`.

```
DATA_DIR=./data ./target/release/app fetch      # pull/refresh into data/nba.db (--full re-pulls all box scores)
DATA_DIR=./data STATIC_DIR=./static PORT=8080 ./target/release/app   # serve
./deploy.sh                                      # build + deploy to radify (app: nbastats)
```

The server refreshes once on startup; `POST /api/refresh` (or the footer button) pulls new games.
It also refreshes daily at 03:00 US Pacific (`REFRESH_AT=HH:MM` to change, `off` to disable); `REFRESH_HOURS=N` adds an interval refresh.

## Betting lab (`#/betting`, `src/betting.rs`)

- Lines: DraftKings via ESPN (scoreboard for upcoming games incl. opening line; summary `pickcenter` for closing
  lines; core API for historical opening lines). Stored in `odds` (spread = home team's line).
- Training/backtest data: 2024-25 and 2025-26 regular seasons in `hist_games` (pulled once, ~2.5k ESPN calls).
- Models: naive, Elo, SRS (season / recency-weighted), opponent-adjusted efficiency + pace, ridge regression and a
  small MLP on game-time features, and an ensemble. Walk-forward by day: no game ever sees same-day or later results.
- Hyper-parameters tuned on 2024-25 only (`./target/release/app tune`); backtest vs lines on 2025-26 and 2026-27.
- `./target/release/app betting` prints the JSON. Lines are re-pulled every `LINES_MINUTES` (default 60).
