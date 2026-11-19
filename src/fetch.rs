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
    fetch_schedule(&c, conn)?;
    let n = fetch_boxscores(&c, conn, full)?;
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

fn fetch_schedule(c: &Client, conn: &Connection) -> Result<()> {
    let start = NaiveDate::parse_from_str(SEASON_START, "%Y-%m-%d")?;
    let end = Utc::now().date_naive() + Duration::days(7);
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
        "SELECT id FROM games WHERE status='STATUS_FINAL' AND fetched=0 ORDER BY date"
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
    tx.execute("UPDATE games SET fetched=1 WHERE id=?1", [gid])?;
    Ok(())
}
