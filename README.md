# nbastats

A self-hosted NBA stats site for the 2026-27 regular season, built from scratch in Rust and vanilla JS.
It pulls every game from ESPN's public API, stores box scores and play-by-play in SQLite, and computes
everything else itself.

**What it does**

- Every game and box score, standings, league leaders with headshots, team and player pages.
- Advanced stats computed from box scores with Basketball-Reference formulas: PER, Win Shares, BPM, VORP,
  ORtg/DRtg, four factors.
- On/off-court ratings, stints and RAPM (ridge-regularized adjusted plus-minus) derived from play-by-play,
  plus teammate with/without splits.
- Eight team rating systems: Elo, SRS, Sagarin-style, Massey, Colley, RPI, Bradley-Terry and Net.
- A betting lab that compares DraftKings lines with eight prediction models and runs a walk-forward backtest.
- Interactive charts; mobile-friendly.

**How it's built**

- Backend: Rust (axum + rusqlite, tokio). `src/fetch.rs` ingests, `src/stats.rs` computes the model in memory,
  `src/onoff.rs` / `src/rapm.rs` handle play-by-play, `src/betting.rs` runs the models and backtest.
- Frontend: a dependency-free vanilla JS single-page app in `static/`.
- Data: ESPN public site API (`site.api.espn.com`); headshots and logos from ESPN's CDN.
- Ships as one static musl binary plus static files and a seed database.

## Running it

```
cargo build --release
DATA_DIR=./data ./target/release/app fetch      # pull/refresh into data/nba.db (--full re-pulls all box scores)
DATA_DIR=./data STATIC_DIR=./static PORT=8080 ./target/release/app   # serve
./deploy.sh                                      # build + deploy (uses radify, my personal deploy tool)
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

## Stored data (SQLite, `data/nba.db`)

Each finished game is downloaded once and kept:

- `raw_summaries`: ESPN's full summary response, gzip JSON (`fetch::load_raw`), with everything ESPN sends
  (box score, play-by-play, officials, win probability, odds, ...)
- `plays`: play-by-play, one row per event (`t` = seconds elapsed, `p1..p3` = participant ids, `x/y` shot location)
- Derived from play-by-play: `stints` (same ten players on court) and `player_onoff`. These are recomputed from
  `raw_summaries` whenever `fetch::DERIVE_VERSION` is bumped, with no network calls. Run `app derive --all` to force it.

## License

MIT, see [LICENSE](LICENSE).
