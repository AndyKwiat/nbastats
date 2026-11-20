use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS teams (
  id TEXT PRIMARY KEY, abbr TEXT, name TEXT, location TEXT, display_name TEXT,
  color TEXT, alt_color TEXT, logo TEXT, conf TEXT, div TEXT
);
CREATE TABLE IF NOT EXISTS players (
  id TEXT PRIMARY KEY, name TEXT, short_name TEXT, position TEXT, jersey TEXT,
  headshot TEXT, age INTEGER, height TEXT, weight INTEGER, exp INTEGER, college TEXT,
  roster_team_id TEXT
);
CREATE TABLE IF NOT EXISTS games (
  id TEXT PRIMARY KEY, date TEXT, season_type INTEGER, status TEXT,
  home_id TEXT, away_id TEXT, home_score INTEGER, away_score INTEGER,
  periods INTEGER, venue TEXT, city TEXT, attendance INTEGER, neutral INTEGER,
  fetched INTEGER DEFAULT 0
);
CREATE TABLE IF NOT EXISTS team_games (
  game_id TEXT, team_id TEXT, opp_id TEXT, home INTEGER,
  pts INTEGER, fgm INTEGER, fga INTEGER, tpm INTEGER, tpa INTEGER, ftm INTEGER, fta INTEGER,
  oreb INTEGER, dreb INTEGER, reb INTEGER, ast INTEGER, stl INTEGER, blk INTEGER,
  tov INTEGER, team_tov INTEGER, total_tov INTEGER, pf INTEGER,
  fast_break INTEGER, paint INTEGER, pts_off_to INTEGER, largest_lead INTEGER,
  linescore TEXT,
  PRIMARY KEY (game_id, team_id)
);
CREATE TABLE IF NOT EXISTS player_games (
  game_id TEXT, player_id TEXT, team_id TEXT, starter INTEGER, dnp INTEGER, reason TEXT,
  min INTEGER, pts INTEGER, fgm INTEGER, fga INTEGER, tpm INTEGER, tpa INTEGER,
  ftm INTEGER, fta INTEGER, oreb INTEGER, dreb INTEGER, reb INTEGER, ast INTEGER,
  stl INTEGER, blk INTEGER, tov INTEGER, pf INTEGER, pm INTEGER,
  PRIMARY KEY (game_id, player_id)
);
CREATE INDEX IF NOT EXISTS pg_player ON player_games(player_id);
-- betting lines (one provider per game; spread is the HOME team's line, negative = home favoured)
CREATE TABLE IF NOT EXISTS odds (
  game_id TEXT PRIMARY KEY, provider TEXT, spread REAL, total REAL, ml_home INTEGER, ml_away INTEGER,
  spread_open REAL, total_open REAL, ml_home_open INTEGER, ml_away_open INTEGER, updated TEXT
);
CREATE TABLE IF NOT EXISTS odds_open_checked (game_id TEXT PRIMARY KEY);
-- prior regular seasons (training data for the betting models only)
CREATE TABLE IF NOT EXISTS hist_games (
  id TEXT PRIMARY KEY, season INTEGER, date TEXT, home_id TEXT, away_id TEXT,
  home_score INTEGER, away_score INTEGER, periods INTEGER, neutral INTEGER,
  h_fgm INTEGER, h_fga INTEGER, h_fta INTEGER, h_oreb INTEGER, h_dreb INTEGER, h_tov INTEGER,
  a_fgm INTEGER, a_fga INTEGER, a_fta INTEGER, a_oreb INTEGER, a_dreb INTEGER, a_tov INTEGER,
  fetched INTEGER DEFAULT 0
);
"#,
    )?;
    Ok(conn)
}
