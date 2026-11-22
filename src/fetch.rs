//! Pulls the regular season from ESPN's public site API into SQLite.
use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::time::Duration as StdDuration;

const BASE: &str = "https://site.api.espn.com/apis/site/v2/sports/basketball/nba";
pub const SEASON_YEAR: i64 = 2027; // ESPN labels 2026-27 as 2027
pub const SEASON_START: &str = "2026-10-20";

fn conf_div(abbr: &str) -> (&'static str, &'static str) {
    match abbr {
        "BOS" | "BKN" | "NY" | "PHI" | "TOR" => ("East", "Atlantic"),
        "CHI" | "CLE" | "DET" | "IND" | "MIL" => ("East", "Central"),
        "ATL" | "CHA" | "MIA" | "ORL" | "WSH" => ("East", "Southeast"),
        "DEN" | "MIN" | "OKC" | "POR" | "UTAH" => ("West", "Northwest"),
        "GS" | "LAC" | "LAL" | "PHX" | "SAC" => ("West", "Pacific"),
        "DAL" | "HOU" | "MEM" | "NO" | "SA" => ("West", "Southwest"),
        _ => ("?", "?"),
    }
}

struct Client {
    agent: ureq::Agent,
}

impl Client {
    fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(StdDuration::from_secs(30))
            .user_agent("nbastats/0.1")
            .build();
        Client { agent }
    }
    fn get(&self, url: &str) -> Result<Value> {
        let mut last = None;
        for attempt in 0..4 {
            match self.agent.get(url).call() {
                Ok(r) => return Ok(r.into_json()?),
                Err(e) => {
                    eprintln!("fetch {url} failed (attempt {attempt}): {e}");
                    last = Some(e);
                    std::thread::sleep(StdDuration::from_millis(800 * (attempt + 1)));
                }
            }
        }
        Err(last.unwrap()).with_context(|| url.to_string())
    }
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
fn i(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0) as i64,
        Value::String(x) => x.trim().trim_start_matches('+').parse::<f64>().unwrap_or(0.0) as i64,
        _ => 0,
    }
}
fn pair(v: &str) -> (i64, i64) {
    let mut it = v.split('-');
    let a = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let b = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (a, b)
}

pub fn run(conn: &mut Connection, full: bool) -> Result<()> {
    let c = Client::new();
    let t0 = std::time::Instant::now();
    fetch_teams(&c, conn)?;
    fetch_rosters(&c, conn)?;
    let start = NaiveDate::parse_from_str(SEASON_START, "%Y-%m-%d")?;
    fetch_schedule(&c, conn, start, Utc::now().date_naive() + Duration::days(7))?;
    let n = fetch_boxscores(&c, conn, full)?;
    ensure_derived(conn)?;
    if let Err(e) = fetch_history(&c, conn) {
        eprintln!("history fetch: {e:#}");
    }
    if let Err(e) = fetch_open_lines(&c, conn) {
        eprintln!("opening lines: {e:#}");
    }
    conn.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('updated', ?1)",
        params![Utc::now().to_rfc3339()],
    )?;
    eprintln!("fetch done: {n} box scores in {:?}", t0.elapsed());
    Ok(())
}

fn fetch_teams(c: &Client, conn: &Connection) -> Result<()> {
    let v = c.get(&format!("{BASE}/teams"))?;
    let teams = v["sports"][0]["leagues"][0]["teams"].as_array().cloned().unwrap_or_default();
    for t in teams {
        let t = &t["team"];
        let abbr = s(&t["abbreviation"]);
        let (conf, div) = conf_div(&abbr);
        conn.execute(
            "INSERT OR REPLACE INTO teams VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                s(&t["id"]),
                abbr,
                s(&t["name"]),
                s(&t["location"]),
                s(&t["displayName"]),
                s(&t["color"]),
                s(&t["alternateColor"]),
                s(&t["logos"][0]["href"]),
                conf,
                div
            ],
        )?;
    }
    Ok(())
}

fn fetch_rosters(c: &Client, conn: &Connection) -> Result<()> {
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM teams")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for tid in ids {
        let v = match c.get(&format!("{BASE}/teams/{tid}/roster")) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("roster {tid}: {e}");
                continue;
            }
        };
        for a in v["athletes"].as_array().cloned().unwrap_or_default() {
            conn.execute(
                "INSERT INTO players(id,name,short_name,position,jersey,headshot,age,height,weight,exp,college,roster_team_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name, short_name=excluded.short_name,
                   position=excluded.position, jersey=excluded.jersey, headshot=excluded.headshot,
                   age=excluded.age, height=excluded.height, weight=excluded.weight, exp=excluded.exp,
                   college=excluded.college, roster_team_id=excluded.roster_team_id",
                params![
                    s(&a["id"]),
                    s(&a["displayName"]),
                    s(&a["shortName"]),
                    s(&a["position"]["abbreviation"]),
                    s(&a["jersey"]),
                    s(&a["headshot"]["href"]),
                    a["age"].as_i64(),
                    s(&a["displayHeight"]),
                    a["weight"].as_f64().map(|w| w as i64),
                    a["experience"]["years"].as_i64(),
                    s(&a["college"]["name"]),
                    tid
                ],
            )?;
        }
        std::thread::sleep(StdDuration::from_millis(120));
    }
    Ok(())
}

/// Light refresh: scoreboard for yesterday..tomorrow (statuses, scores, current lines).
pub fn run_lines(conn: &mut Connection) -> Result<()> {
    let c = Client::new();
    let today = (Utc::now() - Duration::hours(5)).date_naive();
    fetch_schedule(&c, conn, today - Duration::days(1), today + Duration::days(1))?;
    fetch_boxscores(&c, conn, false)?;
    Ok(())
}

fn fnum(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(x) => {
            let t = x.trim().trim_start_matches(|ch: char| ch == 'o' || ch == 'u' || ch == '+');
            if t.eq_ignore_ascii_case("even") || t.eq_ignore_ascii_case("pk") {
                Some(0.0)
            } else {
                t.parse().ok()
            }
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn upsert_odds(
    conn: &Connection,
    gid: &str,
    provider: &str,
    spread: Option<f64>,
    total: Option<f64>,
    mlh: Option<f64>,
    mla: Option<f64>,
    open: [Option<f64>; 4],
) -> Result<()> {
    let mi = |x: Option<f64>| x.map(|v| v as i64);
    conn.execute(
        "INSERT INTO odds(game_id,provider,spread,total,ml_home,ml_away,spread_open,total_open,ml_home_open,ml_away_open,updated)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(game_id) DO UPDATE SET
           provider=COALESCE(excluded.provider,provider), spread=COALESCE(excluded.spread,spread),
           total=COALESCE(excluded.total,total), ml_home=COALESCE(excluded.ml_home,ml_home),
           ml_away=COALESCE(excluded.ml_away,ml_away), spread_open=COALESCE(spread_open,excluded.spread_open),
           total_open=COALESCE(total_open,excluded.total_open), ml_home_open=COALESCE(ml_home_open,excluded.ml_home_open),
           ml_away_open=COALESCE(ml_away_open,excluded.ml_away_open), updated=excluded.updated",
        params![
            gid,
            if provider.is_empty() { None } else { Some(provider) },
            spread,
            total,
            mi(mlh),
            mi(mla),
            open[0],
            open[1],
            mi(open[2]),
            mi(open[3]),
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

/// Lines from a scoreboard competition (`competitions[0].odds[0]`), upcoming games only.
fn store_scoreboard_odds(conn: &Connection, gid: &str, o: &Value) -> Result<()> {
    if o.is_null() || fnum(&o["overUnder"]).is_none() && fnum(&o["spread"]).is_none() {
        return Ok(());
    }
    upsert_odds(
        conn,
        gid,
        &s(&o["provider"]["name"]),
        fnum(&o["spread"]),
        fnum(&o["overUnder"]),
        fnum(&o["moneyline"]["home"]["close"]["odds"]),
        fnum(&o["moneyline"]["away"]["close"]["odds"]),
        [
            fnum(&o["pointSpread"]["home"]["open"]["line"]),
            fnum(&o["total"]["over"]["open"]["line"]),
            fnum(&o["moneyline"]["home"]["open"]["odds"]),
            fnum(&o["moneyline"]["away"]["open"]["odds"]),
        ],
    )
}

/// Closing lines from a game summary's `pickcenter`. Always writes a row (possibly empty) so the
/// game isn't re-requested just for odds.
fn store_pickcenter(conn: &Connection, gid: &str, v: &Value) -> Result<()> {
    let pc = v["pickcenter"].as_array().cloned().unwrap_or_default();
    let o = pc.iter().find(|o| fnum(&o["spread"]).is_some() || fnum(&o["overUnder"]).is_some());
    match o {
        Some(o) => upsert_odds(
            conn,
            gid,
            &s(&o["provider"]["name"]),
            fnum(&o["spread"]),
            fnum(&o["overUnder"]),
            fnum(&o["homeTeamOdds"]["moneyLine"]),
            fnum(&o["awayTeamOdds"]["moneyLine"]),
            [None; 4],
        ),
        None => upsert_odds(conn, gid, "", None, None, None, None, [None; 4]),
    }
}

fn fetch_schedule(c: &Client, conn: &Connection, start: NaiveDate, end: NaiveDate) -> Result<()> {
    let mut d = start;
    while d <= end {
        let url = format!("{BASE}/scoreboard?dates={}", d.format("%Y%m%d"));
        let v = c.get(&url)?;
        for e in v["events"].as_array().cloned().unwrap_or_default() {
            if e["season"]["year"].as_i64() != Some(SEASON_YEAR) || e["season"]["type"].as_i64() != Some(2) {
                continue;
            }
            let comp = &e["competitions"][0];
            let mut home = (String::new(), 0i64);
            let mut away = (String::new(), 0i64);
            let mut periods = 0;
            for t in comp["competitors"].as_array().cloned().unwrap_or_default() {
                let x = (s(&t["team"]["id"]), i(&t["score"]));
                periods = periods.max(t["linescores"].as_array().map(|l| l.len()).unwrap_or(0));
                if s(&t["homeAway"]) == "home" {
                    home = x
                } else {
                    away = x
                }
            }
            let status = s(&e["status"]["type"]["name"]);
            conn.execute(
                "INSERT INTO games(id,date,season_type,status,home_id,away_id,home_score,away_score,periods,venue,city,attendance,neutral,fetched)
                 VALUES (?1,?2,2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0)
                 ON CONFLICT(id) DO UPDATE SET date=excluded.date,status=excluded.status,home_id=excluded.home_id,
                   away_id=excluded.away_id,home_score=excluded.home_score,away_score=excluded.away_score,
                   periods=excluded.periods,venue=excluded.venue,city=excluded.city,
                   attendance=excluded.attendance,neutral=excluded.neutral",
                params![
                    s(&e["id"]),
                    s(&e["date"]),
                    status,
                    home.0,
                    away.0,
                    home.1,
                    away.1,
                    periods as i64,
                    s(&comp["venue"]["fullName"]),
                    format!("{}, {}", s(&comp["venue"]["address"]["city"]), s(&comp["venue"]["address"]["state"])),
                    i(&comp["attendance"]),
                    comp["neutralSite"].as_bool().unwrap_or(false) as i64
                ],
            )?;
            if status == "STATUS_SCHEDULED" {
                store_scoreboard_odds(conn, &s(&e["id"]), &comp["odds"][0])?;
            }
        }
        d += Duration::days(1);
        std::thread::sleep(StdDuration::from_millis(100));
    }
    Ok(())
}

fn fetch_boxscores(c: &Client, conn: &mut Connection, full: bool) -> Result<usize> {
    let q = if full {
        "SELECT id FROM games WHERE status='STATUS_FINAL' ORDER BY date"
    } else {
        "SELECT id FROM games WHERE status='STATUS_FINAL' AND (fetched=0 OR id NOT IN (SELECT game_id FROM odds) OR id NOT IN (SELECT game_id FROM raw_summaries)) ORDER BY date"
    };
    let ids: Vec<String> = conn.prepare(q)?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let total = ids.len();
    for (n, gid) in ids.iter().enumerate() {
        let v = match c.get(&format!("{BASE}/summary?event={gid}")) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("summary {gid}: {e}");
                continue;
            }
        };
        let tx = conn.transaction()?;
        if let Err(e) = store_summary(&tx, gid, &v) {
            eprintln!("parse {gid}: {e}");
            continue;
        }
        tx.commit()?;
        if n % 20 == 0 {
            eprintln!("box scores {}/{}", n + 1, total);
        }
        std::thread::sleep(StdDuration::from_millis(150));
    }
    Ok(total)
}

fn store_summary(tx: &rusqlite::Transaction, gid: &str, v: &Value) -> Result<()> {
    let comp = &v["header"]["competitions"][0];
    let mut ls: std::collections::HashMap<String, (bool, Vec<i64>)> = Default::default();
    for t in comp["competitors"].as_array().cloned().unwrap_or_default() {
        let lines: Vec<i64> = t["linescores"]
            .as_array()
            .map(|a| a.iter().map(|x| i(&x["displayValue"])).collect())
            .unwrap_or_default();
        ls.insert(s(&t["team"]["id"]), (s(&t["homeAway"]) == "home", lines));
    }
    let teams = v["boxscore"]["teams"].as_array().cloned().unwrap_or_default();
    if teams.len() != 2 {
        anyhow::bail!("no team box");
    }
    let ids: Vec<String> = teams.iter().map(|t| s(&t["team"]["id"])).collect();
    tx.execute("DELETE FROM team_games WHERE game_id=?1", [gid])?;
    tx.execute("DELETE FROM player_games WHERE game_id=?1", [gid])?;
    for (k, t) in teams.iter().enumerate() {
        let tid = &ids[k];
        let opp = &ids[1 - k];
        let mut st: std::collections::HashMap<String, String> = Default::default();
        for x in t["statistics"].as_array().cloned().unwrap_or_default() {
            st.insert(s(&x["name"]), s(&x["displayValue"]));
        }
        let g = |k: &str| st.get(k).map(|x| i(&Value::String(x.clone()))).unwrap_or(0);
        let p = |k: &str| st.get(k).map(|x| pair(x)).unwrap_or((0, 0));
        let (fgm, fga) = p("fieldGoalsMade-fieldGoalsAttempted");
        let (tpm, tpa) = p("threePointFieldGoalsMade-threePointFieldGoalsAttempted");
        let (ftm, fta) = p("freeThrowsMade-freeThrowsAttempted");
        let (home, lines) = ls.get(tid).cloned().unwrap_or((false, vec![]));
        let pts = 2 * fgm + tpm + ftm;
        tx.execute(
            "INSERT INTO team_games VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
            params![
                gid, tid, opp, home as i64, pts, fgm, fga, tpm, tpa, ftm, fta,
                g("offensiveRebounds"), g("defensiveRebounds"), g("totalRebounds"), g("assists"),
                g("steals"), g("blocks"), g("turnovers"), g("teamTurnovers"), g("totalTurnovers"),
                g("fouls"), g("fastBreakPoints"), g("pointsInPaint"), g("turnoverPoints"),
                g("largestLead"), serde_json::to_string(&lines)?
            ],
        )?;
    }
    for pt in v["boxscore"]["players"].as_array().cloned().unwrap_or_default() {
        let tid = s(&pt["team"]["id"]);
        let block = &pt["statistics"][0];
        let keys: Vec<String> = block["keys"].as_array().map(|a| a.iter().map(s).collect()).unwrap_or_default();
        for a in block["athletes"].as_array().cloned().unwrap_or_default() {
            let ath = &a["athlete"];
            let pid = s(&ath["id"]);
            let stats: Vec<String> = a["stats"].as_array().map(|x| x.iter().map(s).collect()).unwrap_or_default();
            let get = |k: &str| -> String {
                keys.iter().position(|x| x == k).and_then(|ix| stats.get(ix).cloned()).unwrap_or_default()
            };
            let n = |k: &str| i(&Value::String(get(k)));
            let (fgm, fga) = pair(&get("fieldGoalsMade-fieldGoalsAttempted"));
            let (tpm, tpa) = pair(&get("threePointFieldGoalsMade-threePointFieldGoalsAttempted"));
            let (ftm, fta) = pair(&get("freeThrowsMade-freeThrowsAttempted"));
            let dnp = a["didNotPlay"].as_bool().unwrap_or(false) || stats.is_empty();
            tx.execute(
                "INSERT OR REPLACE INTO player_games VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
                params![
                    gid, pid, tid, a["starter"].as_bool().unwrap_or(false) as i64, dnp as i64,
                    s(&a["reason"]), n("minutes"), n("points"), fgm, fga, tpm, tpa, ftm, fta,
                    n("offensiveRebounds"), n("defensiveRebounds"), n("rebounds"), n("assists"),
                    n("steals"), n("blocks"), n("turnovers"), n("fouls"), n("plusMinus")
                ],
            )?;
            // make sure every player who appears in a box score exists
            tx.execute(
                "INSERT INTO players(id,name,short_name,position,jersey,headshot,roster_team_id) VALUES (?1,?2,?3,?4,?5,?6,NULL)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    pid,
                    s(&ath["displayName"]),
                    s(&ath["shortName"]),
                    s(&ath["position"]["abbreviation"]),
                    s(&ath["jersey"]),
                    s(&ath["headshot"]["href"])
                ],
            )?;
        }
    }
    store_pickcenter(tx, gid, v)?;
    store_raw(tx, gid, v)?;
    store_plays(tx, gid, v)?;
    derive_game(tx, gid, v)?;
    tx.execute("UPDATE games SET fetched=1 WHERE id=?1", [gid])?;
    Ok(())
}

/// Opening lines for finished games (ESPN core API), once per game.
fn fetch_open_lines(c: &Client, conn: &Connection) -> Result<()> {
    let ids: Vec<String> = conn
        .prepare(
            "SELECT o.game_id FROM odds o
             WHERE o.spread IS NOT NULL AND o.spread_open IS NULL
               AND o.game_id NOT IN (SELECT game_id FROM odds_open_checked)
               AND (o.game_id IN (SELECT id FROM hist_games) OR o.game_id IN (SELECT id FROM games WHERE status='STATUS_FINAL'))",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let total = ids.len();
    for (n, gid) in ids.iter().enumerate() {
        let url = format!("https://sports.core.api.espn.com/v2/sports/basketball/leagues/nba/events/{gid}/competitions/{gid}/odds");
        let v = match c.get(&url) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("open odds {gid}: {e}");
                continue;
            }
        };
        let items = v["items"].as_array().cloned().unwrap_or_default();
        if let Some(o) = items.iter().find(|o| !o["homeTeamOdds"]["open"]["pointSpread"]["american"].is_null()) {
            conn.execute(
                "UPDATE odds SET spread_open=?2, total_open=?3, ml_home_open=?4, ml_away_open=?5 WHERE game_id=?1",
                params![
                    gid,
                    fnum(&o["homeTeamOdds"]["open"]["pointSpread"]["american"]),
                    fnum(&o["open"]["total"]["american"]),
                    fnum(&o["homeTeamOdds"]["open"]["moneyLine"]["american"]).map(|x| x as i64),
                    fnum(&o["awayTeamOdds"]["open"]["moneyLine"]["american"]).map(|x| x as i64),
                ],
            )?;
        }
        conn.execute("INSERT OR IGNORE INTO odds_open_checked VALUES (?1)", [gid])?;
        if n % 100 == 0 {
            eprintln!("opening lines {}/{}", n + 1, total);
        }
        std::thread::sleep(StdDuration::from_millis(60));
    }
    Ok(())
}

/// Prior regular seasons: (ESPN season year, first day, last day).
const HIST: &[(i64, &str, &str)] = &[(2025, "2024-10-22", "2025-04-13"), (2026, "2025-10-21", "2026-04-12")];

/// One-time pull of prior seasons' scores, team box basics and closing lines (for model training/backtests).
fn fetch_history(c: &Client, conn: &mut Connection) -> Result<()> {
    for &(year, a, b) in HIST {
        let key = format!("hist_sched_{year}");
        let done: Option<String> = conn.query_row("SELECT value FROM meta WHERE key=?1", [&key], |r| r.get(0)).ok();
        if done.is_some() {
            continue;
        }
        eprintln!("history: schedule {year}");
        let mut d = NaiveDate::parse_from_str(a, "%Y-%m-%d")?;
        let end = NaiveDate::parse_from_str(b, "%Y-%m-%d")?;
        while d <= end {
            let v = c.get(&format!("{BASE}/scoreboard?dates={}", d.format("%Y%m%d")))?;
            for e in v["events"].as_array().cloned().unwrap_or_default() {
                if e["season"]["year"].as_i64() != Some(year)
                    || e["season"]["type"].as_i64() != Some(2)
                    || s(&e["status"]["type"]["name"]) != "STATUS_FINAL"
                {
                    continue;
                }
                let comp = &e["competitions"][0];
                let (mut h, mut aw) = ((String::new(), 0), (String::new(), 0));
                for t in comp["competitors"].as_array().cloned().unwrap_or_default() {
                    let x = (s(&t["team"]["id"]), i(&t["score"]));
                    if s(&t["homeAway"]) == "home" {
                        h = x
                    } else {
                        aw = x
                    }
                }
                conn.execute(
                    "INSERT OR IGNORE INTO hist_games(id,season,date,home_id,away_id,home_score,away_score,neutral)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![s(&e["id"]), year, s(&e["date"]), h.0, aw.0, h.1, aw.1,
                        comp["neutralSite"].as_bool().unwrap_or(false) as i64],
                )?;
            }
            d += Duration::days(1);
            std::thread::sleep(StdDuration::from_millis(80));
        }
        conn.execute("INSERT OR REPLACE INTO meta(key,value) VALUES(?1,'1')", [&key])?;
    }
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM hist_games WHERE fetched=0 ORDER BY date")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let total = ids.len();
    for (n, gid) in ids.iter().enumerate() {
        let v = match c.get(&format!("{BASE}/summary?event={gid}")) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("hist summary {gid}: {e}");
                continue;
            }
        };
        let tx = conn.transaction()?;
        let comp = &v["header"]["competitions"][0];
        let mut home_of: std::collections::HashMap<String, bool> = Default::default();
        let mut periods = 4;
        for t in comp["competitors"].as_array().cloned().unwrap_or_default() {
            home_of.insert(s(&t["team"]["id"]), s(&t["homeAway"]) == "home");
            periods = periods.max(t["linescores"].as_array().map(|l| l.len()).unwrap_or(4));
        }
        for t in v["boxscore"]["teams"].as_array().cloned().unwrap_or_default() {
            let mut st: std::collections::HashMap<String, String> = Default::default();
            for x in t["statistics"].as_array().cloned().unwrap_or_default() {
                st.insert(s(&x["name"]), s(&x["displayValue"]));
            }
            let g = |k: &str| st.get(k).map(|x| i(&Value::String(x.clone()))).unwrap_or(0);
            let (fgm, fga) = st.get("fieldGoalsMade-fieldGoalsAttempted").map(|x| pair(x)).unwrap_or((0, 0));
            let (_, fta) = st.get("freeThrowsMade-freeThrowsAttempted").map(|x| pair(x)).unwrap_or((0, 0));
            let p = if *home_of.get(&s(&t["team"]["id"])).unwrap_or(&false) { "h" } else { "a" };
            tx.execute(
                &format!(
                    "UPDATE hist_games SET {p}_fgm=?2,{p}_fga=?3,{p}_fta=?4,{p}_oreb=?5,{p}_dreb=?6,{p}_tov=?7 WHERE id=?1"
                ),
                params![gid, fgm, fga, fta, g("offensiveRebounds"), g("defensiveRebounds"), g("totalTurnovers").max(g("turnovers"))],
            )?;
        }
        store_pickcenter(&tx, gid, &v)?;
        tx.execute("UPDATE hist_games SET fetched=1, periods=?2 WHERE id=?1", params![gid, periods as i64])?;
        tx.commit()?;
        if n % 50 == 0 {
            eprintln!("history box scores {}/{}", n + 1, total);
        }
        std::thread::sleep(StdDuration::from_millis(100));
    }
    Ok(())
}

// ---------- raw storage + derived tables ----------

/// Bump when the play-by-play derivations (on/off, stints, ...) change; every game is then
/// recomputed from the stored raw responses on the next refresh, with no network calls.
pub const DERIVE_VERSION: i64 = 1;

/// Keep ESPN's full summary response for every game, gzip-compressed, so future analyses never
/// need to download it again.
fn store_raw(tx: &rusqlite::Transaction, gid: &str, v: &Value) -> Result<()> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&serde_json::to_vec(v)?)?;
    tx.execute(
        "INSERT OR REPLACE INTO raw_summaries(game_id, fetched_at, json_gz) VALUES (?1, ?2, ?3)",
        params![gid, Utc::now().to_rfc3339(), enc.finish()?],
    )?;
    Ok(())
}

pub fn load_raw(conn: &Connection, gid: &str) -> Result<Value> {
    use std::io::Read;
    let blob: Vec<u8> = conn.query_row("SELECT json_gz FROM raw_summaries WHERE game_id=?1", [gid], |r| r.get(0))?;
    let mut s = String::new();
    flate2::read::GzDecoder::new(&blob[..]).read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

/// One row per play-by-play event, in a flat, query-friendly form.
fn store_plays(tx: &rusqlite::Transaction, gid: &str, v: &Value) -> Result<()> {
    use crate::onoff::{clock_secs, period_len, period_start};
    tx.execute("DELETE FROM plays WHERE game_id=?1", [gid])?;
    let mut st = tx.prepare(
        "INSERT OR REPLACE INTO plays (game_id, seq, period, clock, t, type_id, type_text, text, team_id,
           p1, p2, p3, scoring, score_value, shooting, points_attempted, home_score, away_score, x, y, wallclock)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
    )?;
    for p in v["plays"].as_array().cloned().unwrap_or_default() {
        let period = p["period"]["number"].as_i64().unwrap_or(0);
        let clock = s(&p["clock"]["displayValue"]);
        let t = if period > 0 {
            Some(period_start(period) + (period_len(period) - clock_secs(&clock)).clamp(0.0, period_len(period)))
        } else {
            None
        };
        let parts: Vec<String> = p["participants"].as_array().map(|a| a.iter().map(|x| s(&x["athlete"]["id"])).collect()).unwrap_or_default();
        let part = |k: usize| parts.get(k).cloned().filter(|x| !x.is_empty());
        // ESPN uses huge sentinel values for plays without a court location
        let coord = |c: &Value| c.as_f64().filter(|x| x.abs() < 1000.0);
        st.execute(params![
            gid,
            s(&p["sequenceNumber"]).parse::<i64>().unwrap_or(0),
            period,
            clock,
            t,
            s(&p["type"]["id"]),
            s(&p["type"]["text"]),
            s(&p["text"]),
            s(&p["team"]["id"]),
            part(0),
            part(1),
            part(2),
            p["scoringPlay"].as_bool().unwrap_or(false) as i64,
            p["scoreValue"].as_i64().unwrap_or(0),
            p["shootingPlay"].as_bool().unwrap_or(false) as i64,
            p["pointsAttempted"].as_i64().unwrap_or(0),
            p["homeScore"].as_i64(),
            p["awayScore"].as_i64(),
            coord(&p["coordinate"]["x"]),
            coord(&p["coordinate"]["y"]),
            s(&p["wallclock"]),
        ])?;
    }
    Ok(())
}

/// Tables computed from play-by-play (on/off totals, stints). Pure function of the raw response.
pub fn derive_game(tx: &rusqlite::Transaction, gid: &str, v: &Value) -> Result<()> {
    let (onoff, stints) = crate::onoff::compute(v);
    tx.execute("DELETE FROM player_onoff WHERE game_id=?1", [gid])?;
    tx.execute("DELETE FROM stints WHERE game_id=?1", [gid])?;
    for (k, st) in stints.iter().enumerate() {
        tx.execute(
            "INSERT INTO stints VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![gid, k as i64, st.sec, st.home.join(","), st.away.join(","), st.hpts, st.apts],
        )?;
    }
    for (pid, o) in onoff {
        tx.execute(
            "INSERT OR REPLACE INTO player_onoff VALUES (?1,?2,?3,?4,?5,?6)",
            params![gid, pid, o.team, o.sec, o.pf, o.pa],
        )?;
    }
    tx.execute("INSERT OR REPLACE INTO derived(game_id, version) VALUES (?1, ?2)", params![gid, DERIVE_VERSION])?;
    Ok(())
}

/// Recompute derived tables from stored raw responses for any game on an older derive version.
pub fn ensure_derived(conn: &mut Connection) -> Result<usize> {
    let ids: Vec<String> = conn
        .prepare(
            "SELECT r.game_id FROM raw_summaries r LEFT JOIN derived d ON d.game_id = r.game_id
             WHERE d.version IS NULL OR d.version < ?1",
        )?
        .query_map([DERIVE_VERSION], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for gid in &ids {
        let v = load_raw(conn, gid)?;
        let tx = conn.transaction()?;
        derive_game(&tx, gid, &v)?;
        tx.commit()?;
    }
    if !ids.is_empty() {
        eprintln!("re-derived {} games from stored play-by-play", ids.len());
    }
    Ok(ids.len())
}
