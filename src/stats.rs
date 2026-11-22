//! Turns raw box scores into season aggregates, advanced stats and team ratings.
use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Default, Clone, Copy, Debug)]
pub struct Bx {
    pub pts: f64,
    pub fgm: f64,
    pub fga: f64,
    pub tpm: f64,
    pub tpa: f64,
    pub ftm: f64,
    pub fta: f64,
    pub oreb: f64,
    pub dreb: f64,
    pub reb: f64,
    pub ast: f64,
    pub stl: f64,
    pub blk: f64,
    pub tov: f64,
    pub pf: f64,
    pub min: f64,
}

impl Bx {
    fn add(&mut self, o: &Bx) {
        self.pts += o.pts;
        self.fgm += o.fgm;
        self.fga += o.fga;
        self.tpm += o.tpm;
        self.tpa += o.tpa;
        self.ftm += o.ftm;
        self.fta += o.fta;
        self.oreb += o.oreb;
        self.dreb += o.dreb;
        self.reb += o.reb;
        self.ast += o.ast;
        self.stl += o.stl;
        self.blk += o.blk;
        self.tov += o.tov;
        self.pf += o.pf;
        self.min += o.min;
    }
}

fn d(a: f64, b: f64) -> f64 {
    if b.abs() < 1e-9 {
        0.0
    } else {
        a / b
    }
}
fn r(x: f64, n: i32) -> Value {
    if !x.is_finite() {
        return Value::Null;
    }
    let p = 10f64.powi(n);
    json!((x * p).round() / p)
}

/// Possession estimate (Basketball-Reference formula), from both sides of the ledger.
fn poss(t: &Bx, o: &Bx) -> f64 {
    let side = |a: &Bx, b: &Bx| a.fga + 0.4 * a.fta - 1.07 * d(a.oreb, a.oreb + b.dreb) * (a.fga - a.fgm) + a.tov;
    0.5 * (side(t, o) + side(o, t))
}

#[derive(Clone)]
pub struct Game {
    pub id: String,
    pub date: String,
    pub day: String,
    pub status: String,
    pub home: String,
    pub away: String,
    pub hs: i64,
    pub aws: i64,
    pub periods: i64,
    pub neutral: bool,
    pub venue: String,
    pub city: String,
    pub attendance: i64,
}

struct TeamInfo {
    id: String,
    abbr: String,
    name: String,
    location: String,
    display: String,
    color: String,
    alt: String,
    logo: String,
    conf: String,
    div: String,
}

#[derive(Default)]
struct TeamAgg {
    b: Bx,
    ob: Bx,
    poss: f64,
    gp: f64,
    w: i64,
    l: i64,
    hw: i64,
    hl: i64,
    aw: i64,
    al: i64,
    cw: i64,
    cl: i64,
    dw: i64,
    dl: i64,
    ow: i64,
    ol: i64,
    fb: f64,
    paint: f64,
    pot: f64,
    ofb: f64,
    opaint: f64,
    results: Vec<(String, bool, i64)>, // day, win, margin
    close_w: i64,
    close_l: i64,
}

#[derive(Default, Clone)]
struct Stint {
    b: Bx,
    gp: i64,
    gs: i64,
    pm: f64,
    gmsc: f64,
    dd: i64,
    td: i64,
    hi_pts: f64,
    hi_reb: f64,
    hi_ast: f64,
    last_day: String,
    oo: OnOffAcc,
}

/// Team results with a player on vs off the court (only games he played in).
#[derive(Default, Clone, Copy)]
struct OnOffAcc {
    on_sec: f64,
    on_pf: f64,
    on_pa: f64,
    on_poss: f64,
    off_sec: f64,
    off_pf: f64,
    off_pa: f64,
    off_poss: f64,
}
impl OnOffAcc {
    fn add(&mut self, o: &OnOffAcc) {
        self.on_sec += o.on_sec;
        self.on_pf += o.on_pf;
        self.on_pa += o.on_pa;
        self.on_poss += o.on_poss;
        self.off_sec += o.off_sec;
        self.off_pf += o.off_pf;
        self.off_pa += o.off_pa;
        self.off_poss += o.off_poss;
    }
}

pub struct Model {
    pub updated: String,
    pub teams_json: String,
    pub players_json: String,
    pub games_json: String,
    pub league_json: String,
    pub elo_json: String,
    /// possessions per second of game clock, per game (for stint-based splits)
    pub poss_rate: HashMap<String, f64>,
}

/// Ridge strength for RAPM (in possessions). Chosen by 5-fold cross-validation by game
/// (run with RAPM_CV=1 to print the table).
const RAPM_LAMBDA: f64 = 8000.0;
/// Pure RAPM (no box-score prior) cross-validates best with a lighter penalty.
const RAPM0_LAMBDA: f64 = 2000.0;

fn load_stints(conn: &Connection, rate: &HashMap<String, f64>) -> Result<Vec<crate::rapm::StintRow>> {
    let mut st = conn.prepare("SELECT game_id, sec, home, away, hpts, apts FROM stints")?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, f64>(4)?, r.get::<_, f64>(5)?))
    })?;
    let mut out = vec![];
    for x in rows {
        let (g, sec, h, a, hp, ap) = x?;
        let Some(rt) = rate.get(&g) else { continue };
        let split = |s: &str| s.split(',').filter(|x| !x.is_empty()).map(String::from).collect::<Vec<_>>();
        out.push(crate::rapm::StintRow { game: g, poss: sec * rt, home: split(&h), away: split(&a), hpts: hp, apts: ap });
    }
    Ok(out)
}

/// Solve A x = b (dense, partial pivoting).
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).max_by(|&x, &y| a[x][c].abs().partial_cmp(&a[y][c].abs()).unwrap()).unwrap();
        a.swap(c, p);
        b.swap(c, p);
        let piv = a[c][c];
        if piv.abs() < 1e-12 {
            continue;
        }
        for r_ in (c + 1)..n {
            let f = a[r_][c] / piv;
            if f == 0.0 {
                continue;
            }
            for k in c..n {
                a[r_][k] -= f * a[c][k];
            }
            b[r_] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let mut s = b[c];
        for k in (c + 1)..n {
            s -= a[c][k] * x[k];
        }
        x[c] = d(s, a[c][c]);
    }
    x
}

/// Least-squares ratings with home-court term: margin = r_home - r_away + h (sum r = 0).
fn margin_ratings(n: usize, games: &[(usize, usize, f64, f64, bool)]) -> (Vec<f64>, f64) {
    // games: (home, away, margin(home-away), weight, neutral)
    let m = n + 1; // last var = home court
    let mut a = vec![vec![0.0; m]; m];
    let mut b = vec![0.0; m];
    for &(h, aw, mg, w, neutral) in games {
        let mut row = vec![0.0; m];
        row[h] = 1.0;
        row[aw] = -1.0;
        if !neutral {
            row[n] = 1.0;
        }
        for i in 0..m {
            if row[i] == 0.0 {
                continue;
            }
            for j in 0..m {
                a[i][j] += w * row[i] * row[j];
            }
            b[i] += w * row[i] * mg;
        }
    }
    // replace first equation by sum-to-zero constraint
    a[0] = (0..m).map(|j| if j < n { 1.0 } else { 0.0 }).collect();
    b[0] = 0.0;
    let x = solve(a, b);
    (x[..n].to_vec(), x[n])
}

fn eastern_day(iso: &str) -> String {
    // ESPN dates are UTC "2026-10-21T01:50Z"; shift 5h to get the US calendar date.
    let fmt = if iso.len() == 17 { "%Y-%m-%dT%H:%MZ" } else { "%Y-%m-%dT%H:%M:%SZ" };
    match chrono::NaiveDateTime::parse_from_str(iso, fmt) {
        Ok(t) => (t - chrono::Duration::hours(5)).format("%Y-%m-%d").to_string(),
        Err(_) => iso.chars().take(10).collect(),
    }
}

pub fn build(conn: &Connection) -> Result<Model> {
    let updated: String = conn
        .query_row("SELECT value FROM meta WHERE key='updated'", [], |r| r.get(0))
        .unwrap_or_default();

    // ---------- load ----------
    let mut teams: Vec<TeamInfo> = conn
        .prepare("SELECT id,abbr,name,location,display_name,color,alt_color,logo,conf,div FROM teams")?
        .query_map([], |r| {
            Ok(TeamInfo {
                id: r.get(0)?,
                abbr: r.get(1)?,
                name: r.get(2)?,
                location: r.get(3)?,
                display: r.get(4)?,
                color: r.get(5)?,
                alt: r.get(6)?,
                logo: r.get(7)?,
                conf: r.get(8)?,
                div: r.get(9)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    teams.retain(|t| t.conf != "?");
    teams.sort_by(|a, b| a.display.cmp(&b.display));
    let tix: HashMap<String, usize> = teams.iter().enumerate().map(|(i, t)| (t.id.clone(), i)).collect();
    let nt = teams.len();

    let mut games: Vec<Game> = conn
        .prepare("SELECT id,date,status,home_id,away_id,home_score,away_score,periods,neutral,venue,city,attendance FROM games ORDER BY date")?
        .query_map([], |r| {
            let date: String = r.get(1)?;
            Ok(Game {
                id: r.get(0)?,
                day: eastern_day(&date),
                date,
                status: r.get(2)?,
                home: r.get(3)?,
                away: r.get(4)?,
                hs: r.get(5)?,
                aws: r.get(6)?,
                periods: r.get::<_, Option<i64>>(7)?.unwrap_or(4),
                neutral: r.get::<_, i64>(8)? != 0,
                venue: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
                city: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                attendance: r.get::<_, Option<i64>>(11)?.unwrap_or(0),
            })
        })?
        .collect::<Result<_, _>>()?;
    games.retain(|g| tix.contains_key(&g.home) && tix.contains_key(&g.away));

    struct Tg {
        b: Bx,
        fb: f64,
        paint: f64,
        pot: f64,
    }
    let mut tgs: HashMap<(String, String), Tg> = HashMap::new();
    {
        let mut st = conn.prepare(
            "SELECT game_id,team_id,pts,fgm,fga,tpm,tpa,ftm,fta,oreb,dreb,reb,ast,stl,blk,total_tov,pf,fast_break,paint,pts_off_to FROM team_games",
        )?;
        let rows = st.query_map([], |r| {
            let f = |i: usize| -> rusqlite::Result<f64> { Ok(r.get::<_, Option<i64>>(i)?.unwrap_or(0) as f64) };
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                Tg {
                    b: Bx {
                        pts: f(2)?,
                        fgm: f(3)?,
                        fga: f(4)?,
                        tpm: f(5)?,
                        tpa: f(6)?,
                        ftm: f(7)?,
                        fta: f(8)?,
                        oreb: f(9)?,
                        dreb: f(10)?,
                        reb: f(11)?,
                        ast: f(12)?,
                        stl: f(13)?,
                        blk: f(14)?,
                        tov: f(15)?,
                        pf: f(16)?,
                        min: 0.0,
                    },
                    fb: f(17)?,
                    paint: f(18)?,
                    pot: f(19)?,
                },
            ))
        })?;
        for x in rows {
            let (k, v) = x?;
            tgs.insert(k, v);
        }
    }

    // finals that have box scores for both sides
    let finals: Vec<&Game> = games
        .iter()
        .filter(|g| {
            g.status == "STATUS_FINAL"
                && tgs.contains_key(&(g.id.clone(), g.home.clone()))
                && tgs.contains_key(&(g.id.clone(), g.away.clone()))
        })
        .collect();
    let game_by_id: HashMap<&str, &Game> = games.iter().map(|g| (g.id.as_str(), g)).collect();
    let game_min = |g: &Game| 240.0 + 25.0 * ((g.periods.max(4) - 4) as f64);

    // ---------- team aggregates ----------
    let mut ta: Vec<TeamAgg> = (0..nt).map(|_| TeamAgg::default()).collect();
    let mut lg = Bx::default();
    let mut lg_poss = 0.0;
    let mut game_poss: HashMap<String, f64> = HashMap::new();
    for g in &finals {
        let hi = tix[&g.home];
        let ai = tix[&g.away];
        let mut hb = tgs[&(g.id.clone(), g.home.clone())].b;
        let mut ab = tgs[&(g.id.clone(), g.away.clone())].b;
        hb.min = game_min(g);
        ab.min = game_min(g);
        let p = poss(&hb, &ab);
        game_poss.insert(g.id.clone(), p);
        for (me, opp, mb, ob, home) in [(hi, ai, hb, ab, true), (ai, hi, ab, hb, false)] {
            let tg = &tgs[&(g.id.clone(), teams[me].id.clone())];
            let otg = &tgs[&(g.id.clone(), teams[opp].id.clone())];
            let t = &mut ta[me];
            t.b.add(&mb);
            t.ob.add(&ob);
            t.poss += p;
            t.gp += 1.0;
            t.fb += tg.fb;
            t.paint += tg.paint;
            t.pot += tg.pot;
            t.ofb += otg.fb;
            t.opaint += otg.paint;
            let margin = (mb.pts - ob.pts) as i64;
            let win = margin > 0;
            if win { t.w += 1 } else { t.l += 1 }
            if home {
                if win { t.hw += 1 } else { t.hl += 1 }
            } else if win {
                t.aw += 1
            } else {
                t.al += 1
            }
            if teams[me].conf == teams[opp].conf {
                if win { t.cw += 1 } else { t.cl += 1 }
            } else if win {
                t.ow += 1
            } else {
                t.ol += 1
            }
            if teams[me].div == teams[opp].div {
                if win { t.dw += 1 } else { t.dl += 1 }
            }
            if margin.abs() <= 5 {
                if win { t.close_w += 1 } else { t.close_l += 1 }
            }
            t.results.push((g.day.clone(), win, margin));
            lg.add(&mb);
            lg_poss += p;
        }
    }
    let lg_pace = 48.0 * d(lg_poss, lg.min / 5.0);
    let lg_ortg = 100.0 * d(lg.pts, lg_poss);
    let lg_ppg = d(lg.pts, ta.iter().map(|t| t.gp).sum::<f64>());
    let lg_3par = d(lg.tpa, lg.fga);
    let lg_ts = d(lg.pts, 2.0 * (lg.fga + 0.44 * lg.fta));

    // ---------- ratings ----------
    let idx_games: Vec<(usize, usize, f64, bool, String)> = finals
        .iter()
        .map(|g| (tix[&g.home], tix[&g.away], (g.hs - g.aws) as f64, g.neutral, g.day.clone()))
        .collect();
    let ng = idx_games.len().max(1);

    // Elo (FiveThirtyEight-style: K=20, 100 pt home court, margin-of-victory multiplier)
    let mut elo = vec![1500.0; nt];
    let mut elo_hist: Vec<Vec<Value>> = (0..nt).map(|_| vec![json!([crate::fetch::SEASON_START, 1500])]).collect();
    for (h, a, m, neutral, day) in &idx_games {
        let hca = if *neutral { 0.0 } else { 100.0 };
        let diff = elo[*h] + hca - elo[*a];
        let ph = 1.0 / (1.0 + 10f64.powf(-diff / 400.0));
        let res = if *m > 0.0 { 1.0 } else { 0.0 };
        let wdiff = if *m > 0.0 { diff } else { -diff };
        let mult = (m.abs() + 3.0).powf(0.8) / (7.5 + 0.006 * wdiff);
        let shift = 20.0 * mult * (res - ph);
        elo[*h] += shift;
        elo[*a] -= shift;
        elo_hist[*h].push(json!([day, elo[*h].round()]));
        elo_hist[*a].push(json!([day, elo[*a].round()]));
    }

    // SRS
    let mut mov = vec![0.0; nt];
    let mut opps: Vec<Vec<usize>> = vec![vec![]; nt];
    for (h, a, m, _, _) in &idx_games {
        mov[*h] += m;
        mov[*a] -= m;
        opps[*h].push(*a);
        opps[*a].push(*h);
    }
    for i in 0..nt {
        mov[i] = d(mov[i], opps[i].len() as f64);
    }
    let mut srs = mov.clone();
    for _ in 0..500 {
        let next: Vec<f64> = (0..nt)
            .map(|i| mov[i] + d(opps[i].iter().map(|&o| srs[o]).sum::<f64>(), opps[i].len() as f64))
            .collect();
        let mean = next.iter().sum::<f64>() / nt as f64;
        srs = next.iter().map(|x| x - mean).collect();
    }

    // Massey (pure margin least squares with home court)
    let mg: Vec<(usize, usize, f64, f64, bool)> = idx_games.iter().map(|(h, a, m, n, _)| (*h, *a, *m, 1.0, *n)).collect();
    let (massey, massey_hca) = margin_ratings(nt, &mg);

    // Sagarin-style "predictor": margins capped at ±20 (diminishing returns for blowouts),
    // recent games weighted more (linearly 0.6 → 1.0 across the season).
    let sg: Vec<(usize, usize, f64, f64, bool)> = idx_games
        .iter()
        .enumerate()
        .map(|(k, (h, a, m, n, _))| (*h, *a, m.clamp(-20.0, 20.0), 0.6 + 0.4 * (k as f64 / ng as f64), *n))
        .collect();
    let (sag_pred, _) = margin_ratings(nt, &sg);

    // Bradley-Terry (win/loss only, like Sagarin's ELO-CHESS) via gradient ascent with a light prior
    let mut bt = vec![0.0f64; nt];
    let mut bt_h = 0.0f64;
    for _ in 0..3000 {
        let mut grad = vec![0.0; nt];
        let mut gh = 0.0;
        for (h, a, m, n, _) in &idx_games {
            let hc = if *n { 0.0 } else { bt_h };
            let p = 1.0 / (1.0 + (-(bt[*h] - bt[*a] + hc)).exp());
            let y = if *m > 0.0 { 1.0 } else { 0.0 };
            grad[*h] += y - p;
            grad[*a] -= y - p;
            if !*n {
                gh += y - p;
            }
        }
        for i in 0..nt {
            grad[i] -= 0.15 * bt[i];
            bt[i] += 0.05 * grad[i];
        }
        bt_h += 0.01 * gh / ng as f64;
        let mean = bt.iter().sum::<f64>() / nt as f64;
        for x in bt.iter_mut() {
            *x -= mean;
        }
    }
    // Sagarin-style composite: blend predictor (points) with W/L rating mapped to points
    let bt_pts: Vec<f64> = bt.iter().map(|x| x * 400.0 / std::f64::consts::LN_10 / 28.0).collect();
    let sagarin: Vec<f64> = (0..nt).map(|i| 0.5 * sag_pred[i] + 0.5 * bt_pts[i]).collect();

    // Colley matrix (W/L only, schedule-adjusted)
    let mut cm = vec![vec![0.0; nt]; nt];
    let mut cb = vec![1.0; nt];
    for i in 0..nt {
        cm[i][i] = 2.0;
    }
    for (h, a, m, _, _) in &idx_games {
        cm[*h][*h] += 1.0;
        cm[*a][*a] += 1.0;
        cm[*h][*a] -= 1.0;
        cm[*a][*h] -= 1.0;
        let (w, l) = if *m > 0.0 { (*h, *a) } else { (*a, *h) };
        cb[w] += 0.5;
        cb[l] -= 0.5;
    }
    let colley = solve(cm, cb);

    // RPI = .25 WP + .50 OWP + .25 OOWP
    let mut wl: Vec<Vec<(usize, bool)>> = vec![vec![]; nt];
    for (h, a, m, _, _) in &idx_games {
        wl[*h].push((*a, *m > 0.0));
        wl[*a].push((*h, *m < 0.0));
    }
    let wp: Vec<f64> = (0..nt).map(|i| d(wl[i].iter().filter(|x| x.1).count() as f64, wl[i].len() as f64)).collect();
    let wp_excl = |o: usize, excl: usize| {
        let gs: Vec<&(usize, bool)> = wl[o].iter().filter(|x| x.0 != excl).collect();
        d(gs.iter().filter(|x| x.1).count() as f64, gs.len() as f64)
    };
    let owp: Vec<f64> = (0..nt)
        .map(|i| d(wl[i].iter().map(|(o, _)| wp_excl(*o, i)).sum::<f64>(), wl[i].len() as f64))
        .collect();
    let oowp: Vec<f64> = (0..nt)
        .map(|i| d(wl[i].iter().map(|(o, _)| owp[*o]).sum::<f64>(), wl[i].len() as f64))
        .collect();
    let rpi: Vec<f64> = (0..nt).map(|i| 0.25 * wp[i] + 0.5 * owp[i] + 0.25 * oowp[i]).collect();

    // ---------- team output ----------
    let rank_of = |vals: &Vec<f64>, higher_better: bool| -> Vec<usize> {
        let mut ix: Vec<usize> = (0..vals.len()).collect();
        ix.sort_by(|&a, &b| {
            let o = vals[a].partial_cmp(&vals[b]).unwrap_or(std::cmp::Ordering::Equal);
            if higher_better { o.reverse() } else { o }
        });
        let mut rk = vec![0; vals.len()];
        for (k, &i) in ix.iter().enumerate() {
            rk[i] = k + 1;
        }
        rk
    };
    let net: Vec<f64> = (0..nt)
        .map(|i| 100.0 * d(ta[i].b.pts - ta[i].ob.pts, ta[i].poss))
        .collect();
    let pyth: Vec<f64> = (0..nt)
        .map(|i| {
            let (p, o) = (ta[i].b.pts.powf(14.0), ta[i].ob.pts.powf(14.0));
            d(p, p + o)
        })
        .collect();
    let rank_sets = [
        rank_of(&elo, true),
        rank_of(&srs, true),
        rank_of(&sagarin, true),
        rank_of(&massey, true),
        rank_of(&colley, true),
        rank_of(&rpi, true),
        rank_of(&bt, true),
        rank_of(&net, true),
    ];
    let comp_score: Vec<f64> = (0..nt)
        .map(|i| rank_sets.iter().map(|r| r[i] as f64).sum::<f64>() / rank_sets.len() as f64)
        .collect();
    let comp_rank = rank_of(&comp_score, false);

    let mut team_out = vec![];
    for (i, t) in teams.iter().enumerate() {
        let a = &ta[i];
        let gp = a.gp.max(1.0);
        let b = &a.b;
        let o = &a.ob;
        let pg = |x: f64| r(x / gp, 1);
        let mut last10: Vec<&(String, bool, i64)> = a.results.iter().rev().take(10).collect();
        last10.reverse();
        let l10w = last10.iter().filter(|x| x.1).count();
        let mut streak = String::new();
        if let Some(last) = a.results.last() {
            let n = a.results.iter().rev().take_while(|x| x.1 == last.1).count();
            streak = format!("{}{}", if last.1 { "W" } else { "L" }, n);
        }
        let ortg = 100.0 * d(b.pts, a.poss);
        let drtg = 100.0 * d(o.pts, a.poss);
        let mut m = Map::new();
        m.insert("id".into(), json!(t.id));
        m.insert("abbr".into(), json!(t.abbr));
        m.insert("name".into(), json!(t.name));
        m.insert("location".into(), json!(t.location));
        m.insert("display".into(), json!(t.display));
        m.insert("color".into(), json!(t.color));
        m.insert("alt".into(), json!(t.alt));
        m.insert("logo".into(), json!(t.logo));
        m.insert("conf".into(), json!(t.conf));
        m.insert("div".into(), json!(t.div));
        m.insert("gp".into(), json!(a.gp));
        m.insert("w".into(), json!(a.w));
        m.insert("l".into(), json!(a.l));
        m.insert("pct".into(), r(d(a.w as f64, a.gp), 3));
        m.insert("home".into(), json!(format!("{}-{}", a.hw, a.hl)));
        m.insert("away".into(), json!(format!("{}-{}", a.aw, a.al)));
        m.insert("confrec".into(), json!(format!("{}-{}", a.cw, a.cl)));
        m.insert("divrec".into(), json!(format!("{}-{}", a.dw, a.dl)));
        m.insert("cw".into(), json!(a.cw));
        m.insert("cl".into(), json!(a.cl));
        m.insert("close".into(), json!(format!("{}-{}", a.close_w, a.close_l)));
        m.insert("l10".into(), json!(format!("{}-{}", l10w, last10.len() - l10w)));
        m.insert("streak".into(), json!(streak));
        m.insert("form".into(), json!(last10.iter().map(|x| if x.1 { 1 } else { 0 }).collect::<Vec<_>>()));
        m.insert("ppg".into(), pg(b.pts));
        m.insert("oppg".into(), pg(o.pts));
        m.insert("diff".into(), r((b.pts - o.pts) / gp, 1));
        for (k, v, ov) in [
            ("fgm", b.fgm, o.fgm),
            ("fga", b.fga, o.fga),
            ("tpm", b.tpm, o.tpm),
            ("tpa", b.tpa, o.tpa),
            ("ftm", b.ftm, o.ftm),
            ("fta", b.fta, o.fta),
            ("oreb", b.oreb, o.oreb),
            ("dreb", b.dreb, o.dreb),
            ("reb", b.reb, o.reb),
            ("ast", b.ast, o.ast),
            ("stl", b.stl, o.stl),
            ("blk", b.blk, o.blk),
            ("tov", b.tov, o.tov),
            ("pf", b.pf, o.pf),
        ] {
            m.insert(k.into(), pg(v));
            m.insert(format!("o_{k}"), pg(ov));
        }
        m.insert("fb".into(), pg(a.fb));
        m.insert("paint".into(), pg(a.paint));
        m.insert("pot".into(), pg(a.pot));
        m.insert("o_fb".into(), pg(a.ofb));
        m.insert("o_paint".into(), pg(a.opaint));
        m.insert("fgp".into(), r(d(b.fgm, b.fga), 3));
        m.insert("tpp".into(), r(d(b.tpm, b.tpa), 3));
        m.insert("ftp".into(), r(d(b.ftm, b.fta), 3));
        m.insert("o_fgp".into(), r(d(o.fgm, o.fga), 3));
        m.insert("o_tpp".into(), r(d(o.tpm, o.tpa), 3));
        m.insert("pace".into(), r(48.0 * d(a.poss, b.min / 5.0), 1));
        m.insert("ortg".into(), r(ortg, 1));
        m.insert("drtg".into(), r(drtg, 1));
        m.insert("net".into(), r(ortg - drtg, 1));
        m.insert("ts".into(), r(d(b.pts, 2.0 * (b.fga + 0.44 * b.fta)), 3));
        m.insert("o_ts".into(), r(d(o.pts, 2.0 * (o.fga + 0.44 * o.fta)), 3));
        m.insert("efg".into(), r(d(b.fgm + 0.5 * b.tpm, b.fga), 3));
        m.insert("tovp".into(), r(d(b.tov, b.fga + 0.44 * b.fta + b.tov), 3));
        m.insert("orbp".into(), r(d(b.oreb, b.oreb + o.dreb), 3));
        m.insert("ftr".into(), r(d(b.ftm, b.fga), 3));
        m.insert("o_efg".into(), r(d(o.fgm + 0.5 * o.tpm, o.fga), 3));
        m.insert("o_tovp".into(), r(d(o.tov, o.fga + 0.44 * o.fta + o.tov), 3));
        m.insert("drbp".into(), r(d(b.dreb, b.dreb + o.oreb), 3));
        m.insert("o_ftr".into(), r(d(o.ftm, o.fga), 3));
        m.insert("tpar".into(), r(d(b.tpa, b.fga), 3));
        m.insert("o_tpar".into(), r(d(o.tpa, o.fga), 3));
        m.insert("astp".into(), r(d(b.ast, b.fgm), 3));
        m.insert("ast_to".into(), r(d(b.ast, b.tov), 2));
        m.insert("stlp".into(), r(100.0 * d(b.stl, a.poss), 1));
        m.insert("blkp".into(), r(100.0 * d(b.blk, o.fga - o.tpa), 1));
        m.insert("pyth_w".into(), r(pyth[i] * a.gp, 1));
        m.insert("pyth_pct".into(), r(pyth[i], 3));
        m.insert("luck".into(), r(a.w as f64 - pyth[i] * a.gp, 1));
        m.insert("elo".into(), r(elo[i], 0));
        m.insert("srs".into(), r(srs[i], 2));
        m.insert("mov".into(), r(mov[i], 2));
        m.insert("sos".into(), r(srs[i] - mov[i], 2));
        m.insert("massey".into(), r(massey[i], 2));
        m.insert("sagarin".into(), r(sagarin[i], 2));
        m.insert("sag_pred".into(), r(sag_pred[i], 2));
        m.insert("bt".into(), r(1.0 / (1.0 + (-bt[i]).exp()), 3));
        m.insert("colley".into(), r(colley[i], 3));
        m.insert("rpi".into(), r(rpi[i], 4));
        m.insert("owp".into(), r(owp[i], 3));
        m.insert("oowp".into(), r(oowp[i], 3));
        m.insert("composite".into(), r(comp_score[i], 2));
        m.insert("power_rank".into(), json!(comp_rank[i]));
        let names = ["elo", "srs", "sagarin", "massey", "colley", "rpi", "bt", "net"];
        for (k, rs) in names.iter().zip(rank_sets.iter()) {
            m.insert(format!("rk_{k}"), json!(rs[i]));
        }
        team_out.push(Value::Object(m));
    }

    // ---------- players ----------
    struct PInfo {
        name: String,
        short: String,
        pos: String,
        jersey: String,
        headshot: String,
        age: Option<i64>,
        height: String,
        weight: Option<i64>,
        exp: Option<i64>,
        college: String,
        roster_team: Option<String>,
    }
    let pinfo: HashMap<String, PInfo> = conn
        .prepare("SELECT id,name,short_name,position,jersey,headshot,age,height,weight,exp,college,roster_team_id FROM players")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                PInfo {
                    name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    short: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    pos: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    jersey: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    headshot: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    age: r.get(6)?,
                    height: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    weight: r.get(8)?,
                    exp: r.get(9)?,
                    college: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    roster_team: r.get(11)?,
                },
            ))
        })?
        .collect::<Result<_, _>>()?;

    let mut onoff: HashMap<(String, String), (f64, f64, f64)> = HashMap::new();
    {
        let mut st = conn.prepare("SELECT game_id,player_id,sec,pf,pa FROM player_onoff")?;
        let rows = st.query_map([], |r| Ok(((r.get::<_, String>(0)?, r.get::<_, String>(1)?), (r.get(2)?, r.get(3)?, r.get(4)?))))?;
        for x in rows {
            let (k, v) = x?;
            onoff.insert(k, v);
        }
    }
    let mut stints: HashMap<(String, String), Stint> = HashMap::new();
    // per game top scorers for the games list
    let mut top: HashMap<(String, String), (String, f64, f64, f64)> = HashMap::new();
    {
        let mut st = conn.prepare(
            "SELECT game_id,player_id,team_id,starter,dnp,min,pts,fgm,fga,tpm,tpa,ftm,fta,oreb,dreb,reb,ast,stl,blk,tov,pf,pm FROM player_games",
        )?;
        let mut rows = st.query([])?;
        while let Some(rw) = rows.next()? {
            let gid: String = rw.get(0)?;
            let Some(g) = game_by_id.get(gid.as_str()) else { continue };
            if g.status != "STATUS_FINAL" {
                continue;
            }
            let dnp: i64 = rw.get(4)?;
            let min: i64 = rw.get(5)?;
            if dnp != 0 || min <= 0 {
                continue;
            }
            let f = |i: usize| -> rusqlite::Result<f64> { Ok(rw.get::<_, Option<i64>>(i)?.unwrap_or(0) as f64) };
            let b = Bx {
                min: min as f64,
                pts: f(6)?,
                fgm: f(7)?,
                fga: f(8)?,
                tpm: f(9)?,
                tpa: f(10)?,
                ftm: f(11)?,
                fta: f(12)?,
                oreb: f(13)?,
                dreb: f(14)?,
                reb: f(15)?,
                ast: f(16)?,
                stl: f(17)?,
                blk: f(18)?,
                tov: f(19)?,
                pf: f(20)?,
            };
            let pid: String = rw.get(1)?;
            let tid: String = rw.get(2)?;
            let s = stints.entry((pid.clone(), tid.clone())).or_default();
            s.b.add(&b);
            s.gp += 1;
            s.gs += rw.get::<_, i64>(3)?;
            s.pm += f(21)?;
            if let (Some(&(sec, pf, pa)), Some(&gposs), Some(tg)) =
                (onoff.get(&(gid.clone(), pid.clone())), game_poss.get(&gid), tgs.get(&(gid.clone(), tid.clone())))
            {
                let gsec = game_min(g) / 5.0 * 60.0;
                let rate = gposs / gsec;
                let tpts = tg.b.pts;
                let opts = (g.hs + g.aws) as f64 - tpts;
                let sec = sec.min(gsec);
                s.oo.add(&OnOffAcc {
                    on_sec: sec,
                    on_pf: pf,
                    on_pa: pa,
                    on_poss: sec * rate,
                    off_sec: gsec - sec,
                    off_pf: (tpts - pf).max(0.0),
                    off_pa: (opts - pa).max(0.0),
                    off_poss: (gsec - sec) * rate,
                });
            }
            s.gmsc += b.pts + 0.4 * b.fgm - 0.7 * b.fga - 0.4 * (b.fta - b.ftm) + 0.7 * b.oreb + 0.3 * b.dreb + b.stl
                + 0.7 * b.ast + 0.7 * b.blk - 0.4 * b.pf - b.tov;
            let tens = [b.pts, b.reb, b.ast, b.stl, b.blk].iter().filter(|x| **x >= 10.0).count();
            if tens >= 2 {
                s.dd += 1;
            }
            if tens >= 3 {
                s.td += 1;
            }
            s.hi_pts = s.hi_pts.max(b.pts);
            s.hi_reb = s.hi_reb.max(b.reb);
            s.hi_ast = s.hi_ast.max(b.ast);
            if g.day > s.last_day {
                s.last_day = g.day.clone();
            }
            let e = top.entry((gid.clone(), tid.clone())).or_insert((String::new(), -1.0, 0.0, 0.0));
            if b.pts > e.1 {
                *e = (pid.clone(), b.pts, b.reb, b.ast);
            }
        }
    }

    // advanced per stint
    struct Adv {
        per_u: f64, // unadjusted, pace-adjusted PER (pre-normalization)
        usg: f64,
        astp: f64,
        tovp: f64,
        orbp: f64,
        drbp: f64,
        trbp: f64,
        stlp: f64,
        blkp: f64,
        ortg: f64,
        totposs: f64,
        drtg: f64,
        ows: f64,
        dws: f64,
        bpm_raw: f64,
        tm_min_share: f64, // MP / (TmMP/5)
    }
    let factor = (2.0 / 3.0) - (0.5 * d(lg.ast, lg.fgm)) / (2.0 * d(lg.fgm, lg.ftm));
    let vop = d(lg.pts, lg.fga - lg.oreb + lg.tov + 0.44 * lg.fta);
    let drbp_lg = d(lg.reb - lg.oreb, lg.reb);
    let lg_pts_poss = d(lg.pts, lg_poss);

    let mut adv: HashMap<(String, String), Adv> = HashMap::new();
    for ((pid, tid), s) in &stints {
        let Some(&ti) = tix.get(tid) else { continue };
        let t = &ta[ti];
        let tb = &t.b;
        let ob = &t.ob;
        let b = &s.b;
        let mp = b.min;
        if mp <= 0.0 {
            continue;
        }
        let tm_mp = tb.min;
        let tm_pace = 48.0 * d(t.poss, tm_mp / 5.0);
        // PER
        let tm_ast_fg = d(tb.ast, tb.fgm);
        let uper = (1.0 / mp)
            * (b.tpm + (2.0 / 3.0) * b.ast + (2.0 - factor * tm_ast_fg) * b.fgm
                + b.ftm * 0.5 * (1.0 + (1.0 - tm_ast_fg) + (2.0 / 3.0) * tm_ast_fg)
                - vop * b.tov
                - vop * drbp_lg * (b.fga - b.fgm)
                - vop * 0.44 * (0.44 + 0.56 * drbp_lg) * (b.fta - b.ftm)
                + vop * (1.0 - drbp_lg) * (b.reb - b.oreb)
                + vop * drbp_lg * b.oreb
                + vop * b.stl
                + vop * drbp_lg * b.blk
                - b.pf * (d(lg.ftm, lg.pf) - 0.44 * d(lg.fta, lg.pf) * vop));
        let per_u = d(lg_pace, tm_pace) * uper;

        let share = d(mp, tm_mp / 5.0);
        let usg = 100.0 * d((b.fga + 0.44 * b.fta + b.tov) * (tm_mp / 5.0), mp * (tb.fga + 0.44 * tb.fta + tb.tov));
        let astp = 100.0 * d(b.ast, share * tb.fgm - b.fgm);
        let tovp = 100.0 * d(b.tov, b.fga + 0.44 * b.fta + b.tov);
        let orbp = 100.0 * d(b.oreb * (tm_mp / 5.0), mp * (tb.oreb + ob.dreb));
        let drbp = 100.0 * d(b.dreb * (tm_mp / 5.0), mp * (tb.dreb + ob.oreb));
        let trbp = 100.0 * d(b.reb * (tm_mp / 5.0), mp * (tb.reb + ob.reb));
        let stlp = 100.0 * d(b.stl * (tm_mp / 5.0), mp * t.poss);
        let blkp = 100.0 * d(b.blk * (tm_mp / 5.0), mp * (ob.fga - ob.tpa));

        // Offensive rating (Dean Oliver, as on Basketball-Reference)
        let q_ast = share * (1.14 * d(tb.ast - b.ast, tb.fgm))
            + d((tb.ast / tm_mp) * mp * 5.0 - b.ast, (tb.fgm / tm_mp) * mp * 5.0 - b.fgm) * (1.0 - share);
        let fg_part = b.fgm * (1.0 - 0.5 * d(b.pts - b.ftm, 2.0 * b.fga) * q_ast);
        let ast_part = 0.5 * d((tb.pts - tb.ftm) - (b.pts - b.ftm), 2.0 * (tb.fga - b.fga)) * b.ast;
        let ft_part = (1.0 - (1.0 - d(b.ftm, b.fta)).powi(2)) * 0.4 * b.fta;
        let tm_sc_poss = tb.fgm + (1.0 - (1.0 - d(tb.ftm, tb.fta)).powi(2)) * tb.fta * 0.4;
        let tm_play = d(tm_sc_poss, tb.fga + tb.fta * 0.4 + tb.tov);
        let tm_orbp = d(tb.oreb, tb.oreb + (ob.reb - ob.oreb));
        let tm_orb_w = d((1.0 - tm_orbp) * tm_play, (1.0 - tm_orbp) * tm_play + tm_orbp * (1.0 - tm_play));
        let orb_part = b.oreb * tm_orb_w * tm_play;
        let sc_poss = (fg_part + ast_part + if b.fta > 0.0 { ft_part } else { 0.0 })
            * (1.0 - d(tb.oreb, tm_sc_poss) * tm_orb_w * tm_play)
            + orb_part;
        let fgx_poss = (b.fga - b.fgm) * (1.0 - 1.07 * tm_orbp);
        let ftx_poss = if b.fta > 0.0 { (1.0 - d(b.ftm, b.fta)).powi(2) * 0.4 * b.fta } else { 0.0 };
        let tot_poss = sc_poss + fgx_poss + ftx_poss + b.tov;
        let pprod_fg = 2.0 * (b.fgm + 0.5 * b.tpm) * (1.0 - 0.5 * d(b.pts - b.ftm, 2.0 * b.fga) * q_ast);
        let pprod_ast = 2.0 * d(tb.fgm - b.fgm + 0.5 * (tb.tpm - b.tpm), tb.fgm - b.fgm)
            * 0.5
            * d((tb.pts - tb.ftm) - (b.pts - b.ftm), 2.0 * (tb.fga - b.fga))
            * b.ast;
        let pprod_orb = b.oreb * tm_orb_w * tm_play * d(tb.pts, tm_sc_poss);
        let pprod = (pprod_fg + pprod_ast + b.ftm) * (1.0 - d(tb.oreb, tm_sc_poss) * tm_orb_w * tm_play) + pprod_orb;
        let ortg = 100.0 * d(pprod, tot_poss);

        // Defensive rating
        let dorp = d(ob.oreb, ob.oreb + tb.dreb);
        let dfgp = d(ob.fgm, ob.fga);
        let fmwt = d(dfgp * (1.0 - dorp), dfgp * (1.0 - dorp) + (1.0 - dfgp) * dorp);
        let stops1 = b.stl + b.blk * fmwt * (1.0 - 1.07 * dorp) + b.dreb * (1.0 - fmwt);
        let stops2 = (d(ob.fga - ob.fgm - tb.blk, tm_mp) * fmwt * (1.0 - 1.07 * dorp) + d(ob.tov - tb.stl, tm_mp)) * mp
            + d(b.pf, tb.pf) * 0.4 * ob.fta * (1.0 - d(ob.ftm, ob.fta)).powi(2);
        let stops = stops1 + stops2;
        let stop_pct = d(stops * tm_mp, t.poss * mp);
        let tm_drtg = 100.0 * d(ob.pts, t.poss);
        let d_pts_sc = d(ob.pts, ob.fgm + (1.0 - (1.0 - d(ob.ftm, ob.fta)).powi(2)) * ob.fta * 0.4);
        let drtg = tm_drtg + 0.2 * (100.0 * d_pts_sc * (1.0 - stop_pct) - tm_drtg);

        // Win shares
        let mppw = 0.32 * lg_ppg * d(tm_pace, lg_pace);
        let ows = d(pprod - 0.92 * lg_pts_poss * tot_poss, mppw);
        let dws = d(d(mp, tm_mp) * t.poss * (1.08 * lg_pts_poss - drtg / 100.0), mppw);

        // Box Plus/Minus 1.0 (raw; team adjustment applied below)
        let ts = d(b.pts, 2.0 * (b.fga + 0.44 * b.fta));
        let tm_ts = d(tb.pts, 2.0 * (tb.fga + 0.44 * tb.fta));
        let tpar = d(b.tpa, b.fga);
        let mpg = mp / s.gp as f64;
        let tov_f = tovp / 100.0;
        let bpm_raw = 0.123391 * mpg + 0.119597 * orbp - 0.151287 * drbp + 1.255644 * stlp + 0.531838 * blkp
            - 0.305868 * astp
            - 0.921292 * usg * tov_f
            + 0.711217 * usg * (1.0 - tov_f) * (2.0 * (ts - tm_ts) + 0.017022 * astp + 0.297639 * (tpar - lg_3par) - 0.213485)
            + 0.725930 * (astp * trbp).max(0.0).sqrt();

        adv.insert(
            (pid.clone(), tid.clone()),
            Adv {
                per_u,
                usg,
                astp,
                tovp,
                orbp,
                drbp,
                trbp,
                stlp,
                blkp,
                ortg,
                totposs: tot_poss,
                drtg,
                ows,
                dws,
                bpm_raw,
                tm_min_share: share,
            },
        );
    }
    // PER normalization: league average = 15
    let (mut wsum, mut msum) = (0.0, 0.0);
    for (k, a) in &adv {
        let mp = stints[k].b.min;
        wsum += a.per_u * mp;
        msum += mp;
    }
    let per_scale = 15.0 / d(wsum, msum).max(1e-9);
    // BPM team adjustment: minute-weighted BPM sum = 1.2 * team net rating
    let mut bpm_adj: HashMap<String, f64> = HashMap::new();
    for (i, t) in teams.iter().enumerate() {
        let sum: f64 = adv
            .iter()
            .filter(|((_, tid), _)| tid == &t.id)
            .map(|(_, a)| a.bpm_raw * a.tm_min_share)
            .sum();
        bpm_adj.insert(t.id.clone(), (1.2 * net[i] - sum) / 5.0);
    }

    // merge stints per player
    let mut by_player: HashMap<String, Vec<(String, Stint)>> = HashMap::new();
    for ((pid, tid), s) in &stints {
        by_player.entry(pid.clone()).or_default().push((tid.clone(), s.clone()));
    }
    let mut box_prior: HashMap<String, (f64, f64)> = HashMap::new();
    let mut player_out = vec![];
    for (pid, mut sts) in by_player {
        sts.sort_by(|a, b| a.1.last_day.cmp(&b.1.last_day));
        let mut tot = Stint::default();
        let w = |f: &dyn Fn(&Adv) -> f64, by: &dyn Fn(&Adv, &Stint) -> f64| -> f64 {
            let mut num = 0.0;
            let mut den = 0.0;
            for (tid, s) in &sts {
                if let Some(a) = adv.get(&(pid.clone(), tid.clone())) {
                    num += f(a) * by(a, s);
                    den += by(a, s);
                }
            }
            d(num, den)
        };
        let by_min = |_: &Adv, s: &Stint| s.b.min;
        let per = w(&|a| a.per_u * per_scale, &by_min);
        let usg = w(&|a| a.usg, &by_min);
        let astp = w(&|a| a.astp, &by_min);
        let tovp_ = w(&|a| a.tovp, &by_min);
        let orbp = w(&|a| a.orbp, &by_min);
        let drbp = w(&|a| a.drbp, &by_min);
        let trbp = w(&|a| a.trbp, &by_min);
        let stlp = w(&|a| a.stlp, &by_min);
        let blkp = w(&|a| a.blkp, &by_min);
        let ortg = w(&|a| a.ortg, &|a, _| a.totposs);
        let drtg = w(&|a| a.drtg, &by_min);
        let mut ows = 0.0;
        let mut dws = 0.0;
        let mut bpm_num = 0.0;
        let mut vorp = 0.0;
        for (tid, s) in &sts {
            if let Some(a) = adv.get(&(pid.clone(), tid.clone())) {
                ows += a.ows;
                dws += a.dws;
                let bpm = a.bpm_raw + bpm_adj.get(tid).copied().unwrap_or(0.0);
                bpm_num += bpm * s.b.min;
                let ti = tix[tid];
                vorp += (bpm + 2.0) * a.tm_min_share * (ta[ti].gp / 82.0);
            }
            tot.b.add(&s.b);
            tot.gp += s.gp;
            tot.gs += s.gs;
            tot.pm += s.pm;
            tot.gmsc += s.gmsc;
            tot.dd += s.dd;
            tot.td += s.td;
            tot.hi_pts = tot.hi_pts.max(s.hi_pts);
            tot.hi_reb = tot.hi_reb.max(s.hi_reb);
            tot.hi_ast = tot.hi_ast.max(s.hi_ast);
            tot.oo.add(&s.oo);
        }
        let b = &tot.b;
        let bpm = d(bpm_num, b.min);
        box_prior.insert(pid.clone(), (bpm, b.min));
        let ws = ows + dws;
        let info = pinfo.get(&pid);
        let cur_team = sts.last().map(|x| x.0.clone()).unwrap_or_default();
        let gp = tot.gp.max(1) as f64;
        let mut m = Map::new();
        m.insert("id".into(), json!(pid));
        m.insert("name".into(), json!(info.map(|i| i.name.clone()).unwrap_or_default()));
        m.insert("short".into(), json!(info.map(|i| i.short.clone()).unwrap_or_default()));
        m.insert("pos".into(), json!(info.map(|i| i.pos.clone()).unwrap_or_default()));
        m.insert("jersey".into(), json!(info.map(|i| i.jersey.clone()).unwrap_or_default()));
        m.insert(
            "img".into(),
            json!(info
                .map(|i| i.headshot.clone())
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| format!("https://a.espncdn.com/i/headshots/nba/players/full/{pid}.png"))),
        );
        m.insert("age".into(), json!(info.and_then(|i| i.age)));
        m.insert("ht".into(), json!(info.map(|i| i.height.clone()).unwrap_or_default()));
        m.insert("wt".into(), json!(info.and_then(|i| i.weight)));
        m.insert("exp".into(), json!(info.and_then(|i| i.exp)));
        m.insert("college".into(), json!(info.map(|i| i.college.clone()).unwrap_or_default()));
        m.insert("team".into(), json!(cur_team));
        m.insert("roster_team".into(), json!(info.and_then(|i| i.roster_team.clone())));
        m.insert("teams".into(), json!(sts.iter().map(|x| x.0.clone()).collect::<Vec<_>>()));
        m.insert("gp".into(), json!(tot.gp));
        m.insert("gs".into(), json!(tot.gs));
        m.insert("min".into(), json!(b.min));
        for (k, v) in [
            ("pts", b.pts),
            ("fgm", b.fgm),
            ("fga", b.fga),
            ("tpm", b.tpm),
            ("tpa", b.tpa),
            ("ftm", b.ftm),
            ("fta", b.fta),
            ("oreb", b.oreb),
            ("dreb", b.dreb),
            ("reb", b.reb),
            ("ast", b.ast),
            ("stl", b.stl),
            ("blk", b.blk),
            ("tov", b.tov),
            ("pf", b.pf),
        ] {
            m.insert(format!("t_{k}"), json!(v));
        }
        m.insert("pm".into(), json!(tot.pm));
        m.insert("pm_g".into(), r(tot.pm / gp, 1));
        m.insert("gmsc".into(), r(tot.gmsc / gp, 1));
        m.insert("dd".into(), json!(tot.dd));
        m.insert("td".into(), json!(tot.td));
        m.insert("hi_pts".into(), json!(tot.hi_pts));
        m.insert("hi_reb".into(), json!(tot.hi_reb));
        m.insert("hi_ast".into(), json!(tot.hi_ast));
        m.insert("fgp".into(), r(d(b.fgm, b.fga), 3));
        m.insert("tpp".into(), r(d(b.tpm, b.tpa), 3));
        m.insert("ftp".into(), r(d(b.ftm, b.fta), 3));
        m.insert("ts".into(), r(d(b.pts, 2.0 * (b.fga + 0.44 * b.fta)), 3));
        m.insert("efg".into(), r(d(b.fgm + 0.5 * b.tpm, b.fga), 3));
        m.insert("tpar".into(), r(d(b.tpa, b.fga), 3));
        m.insert("ftr".into(), r(d(b.fta, b.fga), 3));
        m.insert("ast_to".into(), r(d(b.ast, b.tov), 2));
        m.insert("per".into(), r(per, 1));
        m.insert("usg".into(), r(usg, 1));
        m.insert("astp".into(), r(astp, 1));
        m.insert("tovp".into(), r(tovp_, 1));
        m.insert("orbp".into(), r(orbp, 1));
        m.insert("drbp".into(), r(drbp, 1));
        m.insert("trbp".into(), r(trbp, 1));
        m.insert("stlp".into(), r(stlp, 1));
        m.insert("blkp".into(), r(blkp, 1));
        m.insert("ortg".into(), r(ortg, 1));
        m.insert("drtg".into(), r(drtg, 1));
        m.insert("netrtg".into(), r(ortg - drtg, 1));
        m.insert("ows".into(), r(ows, 2));
        m.insert("dws".into(), r(dws, 2));
        m.insert("ws".into(), r(ws, 2));
        m.insert("ws48".into(), r(d(ws * 48.0, b.min), 3));
        m.insert("bpm".into(), r(bpm, 1));
        m.insert("vorp".into(), r(vorp, 2));
        // on/off court (per 100 possessions; possessions apportioned by time within each game)
        let oo = &tot.oo;
        if oo.on_sec > 0.0 {
            let (on_o, on_d) = (100.0 * d(oo.on_pf, oo.on_poss), 100.0 * d(oo.on_pa, oo.on_poss));
            m.insert("on_min".into(), r(oo.on_sec / 60.0, 0));
            m.insert("off_min".into(), r(oo.off_sec / 60.0, 0));
            m.insert("on_ortg".into(), r(on_o, 1));
            m.insert("on_drtg".into(), r(on_d, 1));
            m.insert("on_net".into(), r(on_o - on_d, 1));
            m.insert("on_pm".into(), r(oo.on_pf - oo.on_pa, 0));
            if oo.off_poss > 0.0 {
                let (off_o, off_d) = (100.0 * d(oo.off_pf, oo.off_poss), 100.0 * d(oo.off_pa, oo.off_poss));
                m.insert("off_ortg".into(), r(off_o, 1));
                m.insert("off_drtg".into(), r(off_d, 1));
                m.insert("off_net".into(), r(off_o - off_d, 1));
                m.insert("oo_ortg".into(), r(on_o - off_o, 1));
                m.insert("oo_drtg".into(), r(on_d - off_d, 1));
                m.insert("oo_net".into(), r((on_o - on_d) - (off_o - off_d), 1));
            }
        }
        player_out.push(Value::Object(m));
    }
    player_out.sort_by(|a, b| {
        b["t_pts"].as_f64().unwrap_or(0.0).partial_cmp(&a["t_pts"].as_f64().unwrap_or(0.0)).unwrap()
    });

    // ---------- RAPM ----------
    let poss_rate: HashMap<String, f64> = finals
        .iter()
        .filter_map(|g| game_poss.get(&g.id).map(|p| (g.id.clone(), p / (game_min(g) / 5.0 * 60.0))))
        .collect();
    let rows = load_stints(conn, &poss_rate)?;
    // Box-score prior: BPM, trusted in proportion to minutes, split evenly between offense and defense.
    let priors: HashMap<String, (f64, f64)> = box_prior
        .iter()
        .map(|(p, (bpm, min))| {
            let v = (bpm * min / (min + 250.0)).clamp(-8.0, 10.0);
            (p.clone(), (v * 0.5, v * 0.5))
        })
        .collect();
    let none = HashMap::new();
    if std::env::var("RAPM_CV").is_ok() {
        for lam in [250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0] {
            eprintln!(
                "lambda {lam:>6}: cv rmse  prior={:.3}  no-prior={:.3}",
                crate::rapm::cross_validate(&rows, &priors, lam, 5),
                crate::rapm::cross_validate(&rows, &none, lam, 5)
            );
        }
        eprintln!("baseline (everyone 0): {:.3}", crate::rapm::cross_validate(&rows, &none, 1e9, 5));
    }
    let lam = std::env::var("RAPM_LAMBDA").ok().and_then(|x| x.parse().ok()).unwrap_or(RAPM_LAMBDA);
    if !rows.is_empty() {
        let fp = crate::rapm::fit(&rows, &priors, lam);
        let f0 = crate::rapm::fit(&rows, &none, RAPM0_LAMBDA);
        for v in player_out.iter_mut() {
            let pid = v["id"].as_str().unwrap_or_default().to_string();
            let m = v.as_object_mut().unwrap();
            if let Some(x) = fp.players.get(&pid) {
                m.insert("rapm".into(), r(x.off + x.def, 1));
                m.insert("rapm_o".into(), r(x.off, 1));
                m.insert("rapm_d".into(), r(x.def, 1));
                m.insert("rapm_poss".into(), r(x.poss, 0));
            }
            if let Some(x) = f0.players.get(&pid) {
                m.insert("rapm0".into(), r(x.off + x.def, 1));
            }
        }
    }

    // ---------- games ----------
    let mut games_out = vec![];
    for g in &games {
        let mut m = Map::new();
        m.insert("id".into(), json!(g.id));
        m.insert("date".into(), json!(g.date));
        m.insert("day".into(), json!(g.day));
        m.insert("status".into(), json!(g.status));
        m.insert("home".into(), json!(g.home));
        m.insert("away".into(), json!(g.away));
        m.insert("hs".into(), json!(g.hs));
        m.insert("as".into(), json!(g.aws));
        m.insert("ot".into(), json!((g.periods - 4).max(0)));
        m.insert("venue".into(), json!(g.venue));
        m.insert("city".into(), json!(g.city));
        m.insert("att".into(), json!(g.attendance));
        if g.status == "STATUS_FINAL" {
            for (side, tid) in [("ht", &g.home), ("at", &g.away)] {
                if let Some((pid, pts, reb, ast)) = top.get(&(g.id.clone(), tid.clone())) {
                    let nm = pinfo.get(pid).map(|i| i.short.clone()).unwrap_or_default();
                    m.insert(side.into(), json!([pid, nm, pts, reb, ast]));
                }
            }
        } else if let (Some(&hi), Some(&ai)) = (tix.get(&g.home), tix.get(&g.away)) {
            // predictions for upcoming games
            let hca = if g.neutral { 0.0 } else { 100.0 };
            let ph = 1.0 / (1.0 + 10f64.powf(-(elo[hi] + hca - elo[ai]) / 400.0));
            m.insert("p_home".into(), r(ph, 3));
            m.insert(
                "spread".into(),
                r(massey[hi] - massey[ai] + if g.neutral { 0.0 } else { massey_hca }, 1),
            );
        }
        games_out.push(Value::Object(m));
    }

    let elo_out: Map<String, Value> = teams
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.clone(), json!(elo_hist[i])))
        .collect();

    let league = json!({
        "updated": updated,
        "games_played": finals.len(),
        "pace": r(lg_pace, 1),
        "ortg": r(lg_ortg, 1),
        "ppg": r(lg_ppg, 1),
        "ts": r(lg_ts, 3),
        "efg": r(d(lg.fgm + 0.5 * lg.tpm, lg.fga), 3),
        "tpar": r(lg_3par, 3),
        "hca_massey": r(massey_hca, 2),
        "season": "2026-27",
    });

    Ok(Model {
        updated,
        teams_json: serde_json::to_string(&team_out)?,
        players_json: serde_json::to_string(&player_out)?,
        games_json: serde_json::to_string(&games_out)?,
        league_json: serde_json::to_string(&league)?,
        elo_json: serde_json::to_string(&elo_out)?,
        poss_rate,
    })
}
