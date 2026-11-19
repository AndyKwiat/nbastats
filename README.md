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
Set `REFRESH_HOURS=N` in the app env for periodic refreshes.
