mod db;
mod fetch;
mod stats;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tower_http::{compression::CompressionLayer, services::ServeDir, services::ServeFile};

struct AppState {
    db_path: PathBuf,
    model: RwLock<Arc<stats::Model>>,
    refreshing: AtomicBool,
}
type S = Arc<AppState>;

fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()))
}

fn rebuild(st: &AppState) -> anyhow::Result<()> {
    let conn = db::open(&st.db_path)?;
    let m = stats::build(&conn)?;
    *st.model.write().unwrap() = Arc::new(m);
    Ok(())
}

fn spawn_refresh(st: S, full: bool) -> bool {
    if st.refreshing.swap(true, Ordering::SeqCst) {
        return false;
    }
    std::thread::spawn(move || {
        let res = db::open(&st.db_path).and_then(|mut c| fetch::run(&mut c, full));
        if let Err(e) = res {
            eprintln!("refresh failed: {e:#}");
        }
        if let Err(e) = rebuild(&st) {
            eprintln!("rebuild failed: {e:#}");
        }
        st.refreshing.store(false, Ordering::SeqCst);
    });
    true
}

fn json_str(s: &str) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], s.to_string()).into_response()
}

async fn league(State(s): State<S>) -> Response {
    let m = s.model.read().unwrap().clone();
    let mut v: Value = serde_json::from_str(&m.league_json).unwrap_or(json!({}));
    v["refreshing"] = json!(s.refreshing.load(Ordering::SeqCst));
    Json(v).into_response()
}
async fn teams(State(s): State<S>) -> Response {
    json_str(&s.model.read().unwrap().teams_json)
}
async fn players(State(s): State<S>) -> Response {
    json_str(&s.model.read().unwrap().players_json)
}
async fn games(State(s): State<S>) -> Response {
    json_str(&s.model.read().unwrap().games_json)
}
async fn elo(State(s): State<S>) -> Response {
    json_str(&s.model.read().unwrap().elo_json)
}
async fn refresh(State(s): State<S>) -> Response {
    let started = spawn_refresh(s, false);
    Json(json!({ "started": started })).into_response()
}

fn rows_to_json(stmt: &mut rusqlite::Statement, p: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Value>> {
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows = stmt.query(p)?;
    let mut out = vec![];
    while let Some(r) = rows.next()? {
        let mut m = serde_json::Map::new();
        for (i, n) in names.iter().enumerate() {
            let v = match r.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(x) => json!(x),
                rusqlite::types::ValueRef::Real(x) => json!(x),
                rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                rusqlite::types::ValueRef::Blob(_) => Value::Null,
            };
            m.insert(n.clone(), v);
        }
        out.push(Value::Object(m));
    }
    Ok(out)
}

async fn game_box(State(s): State<S>, Path(id): Path<String>) -> Response {
    let res = (|| -> anyhow::Result<Value> {
        let c = db::open(&s.db_path)?;
        let g = rows_to_json(&mut c.prepare("SELECT * FROM games WHERE id=?1")?, &[&id])?;
        let tg = rows_to_json(&mut c.prepare("SELECT * FROM team_games WHERE game_id=?1")?, &[&id])?;
        let pg = rows_to_json(
            &mut c.prepare(
                "SELECT pg.*, p.name, p.short_name, p.position, p.headshot FROM player_games pg
                 LEFT JOIN players p ON p.id=pg.player_id WHERE game_id=?1
                 ORDER BY pg.team_id, pg.starter DESC, pg.dnp, pg.min DESC",
            )?,
            &[&id],
        )?;
        Ok(json!({ "game": g.first(), "teams": tg, "players": pg }))
    })();
    match res {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn player_log(State(s): State<S>, Path(id): Path<String>) -> Response {
    let res = (|| -> anyhow::Result<Value> {
        let c = db::open(&s.db_path)?;
        let log = rows_to_json(
            &mut c.prepare(
                "SELECT pg.*, g.date, g.home_id, g.away_id, g.home_score, g.away_score, g.periods
                 FROM player_games pg JOIN games g ON g.id=pg.game_id
                 WHERE pg.player_id=?1 AND g.status='STATUS_FINAL' ORDER BY g.date",
            )?,
            &[&id],
        )?;
        Ok(json!({ "log": log }))
    })();
    match res {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn seed_db(db_path: &std::path::Path) {
    if db_path.exists() {
        return;
    }
    let mut cands = vec![PathBuf::from("seed/nba.db")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent().and_then(|p| p.parent()) {
            cands.insert(0, root.join("seed/nba.db"));
        }
    }
    for c in cands {
        if c.exists() {
            match std::fs::copy(&c, db_path) {
                Ok(_) => eprintln!("seeded database from {}", c.display()),
                Err(e) => eprintln!("seed copy failed: {e}"),
            }
            return;
        }
    }
}

/// US Pacific UTC offset (hours) for a local calendar date: PDT (-7) from the 2nd Sunday of March
/// through the day before the 1st Sunday of November, else PST (-8). Valid for times after 2 am.
fn pacific_offset(d: chrono::NaiveDate) -> i64 {
    use chrono::{Datelike, NaiveDate};
    let nth_sunday = |month: u32, n: u32| {
        let first = NaiveDate::from_ymd_opt(d.year(), month, 1).unwrap();
        let skip = (7 - first.weekday().num_days_from_sunday()) % 7;
        first + chrono::Duration::days((skip + 7 * (n - 1)) as i64)
    };
    if d >= nth_sunday(3, 2) && d < nth_sunday(11, 1) { -7 } else { -8 }
}

/// Next UTC instant at which Pacific wall-clock time is h:m.
fn next_pacific(now: chrono::DateTime<chrono::Utc>, h: u32, m: u32) -> chrono::DateTime<chrono::Utc> {
    let today = (now + chrono::Duration::hours(-8)).date_naive();
    for k in 0..3 {
        let d = today + chrono::Duration::days(k);
        let local = d.and_hms_opt(h, m, 0).unwrap();
        let utc = (local - chrono::Duration::hours(pacific_offset(d))).and_utc();
        if utc > now {
            return utc;
        }
    }
    now + chrono::Duration::hours(24)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    #[test]
    fn schedule() {
        let t = |s: &str| chrono::Utc.datetime_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        // Nov 20 2026 (PST): 3am PST = 11:00 UTC
        assert_eq!(next_pacific(t("2026-11-20 05:00"), 3, 0), t("2026-11-20 11:00"));
        assert_eq!(next_pacific(t("2026-11-20 11:01"), 3, 0), t("2026-11-21 11:00"));
        // Summer (PDT): 3am PDT = 10:00 UTC
        assert_eq!(next_pacific(t("2027-04-10 12:00"), 3, 0), t("2027-04-11 10:00"));
        // DST starts Mar 14 2027; ends Nov 7 2027
        assert_eq!(next_pacific(t("2027-03-13 12:00"), 3, 0), t("2027-03-14 10:00"));
        assert_eq!(next_pacific(t("2027-11-06 12:00"), 3, 0), t("2027-11-07 11:00"));
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("nba.db");

    if args.get(1).map(|s| s.as_str()) == Some("fetch") {
        let full = args.iter().any(|a| a == "--full");
        let mut c = db::open(&db_path)?;
        fetch::run(&mut c, full)?;
        let m = stats::build(&c)?;
        eprintln!("model built; updated {}", m.updated);
        return Ok(());
    }

    seed_db(&db_path);
    let conn = db::open(&db_path)?;
    let model = stats::build(&conn)?;
    drop(conn);
    let st: S = Arc::new(AppState { db_path, model: RwLock::new(Arc::new(model)), refreshing: AtomicBool::new(false) });

    // Refresh once on start (picks up any games since the seed), and every REFRESH_HOURS if set.
    if std::env::var("NO_STARTUP_REFRESH").is_err() {
        spawn_refresh(st.clone(), false);
    }
    if let Some(h) = std::env::var("REFRESH_HOURS").ok().and_then(|x| x.parse::<u64>().ok()).filter(|h| *h > 0) {
        let st2 = st.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(h * 3600));
            spawn_refresh(st2.clone(), false);
        });
    }

    // Daily refresh at a fixed US Pacific time (default 03:00; REFRESH_AT=HH:MM to change, "off" to disable).
    let at = std::env::var("REFRESH_AT").unwrap_or_else(|_| "03:00".into());
    if at != "off" {
        if let Some((h, m)) = at.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?))) {
            let st3 = st.clone();
            std::thread::spawn(move || loop {
                let next = next_pacific(chrono::Utc::now(), h, m);
                eprintln!("next scheduled refresh: {next}");
                loop {
                    let left = (next - chrono::Utc::now()).num_seconds();
                    if left <= 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(left.min(600) as u64));
                }
                eprintln!("scheduled refresh starting");
                spawn_refresh(st3.clone(), false);
                std::thread::sleep(std::time::Duration::from_secs(90));
            });
        }
    }

    let static_dir =PathBuf::from(std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".into()));
    let index = static_dir.join("index.html");
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/league", get(league))
        .route("/api/teams", get(teams))
        .route("/api/players", get(players))
        .route("/api/games", get(games))
        .route("/api/elo", get(elo))
        .route("/api/game/{id}", get(game_box))
        .route("/api/player/{id}/log", get(player_log))
        .route("/api/refresh", post(refresh))
        .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index)))
        .layer(CompressionLayer::new())
        .with_state(st);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
        eprintln!("listening on 127.0.0.1:{port}");
        axum::serve(listener, app).await?;
        Ok(())
    })
}
