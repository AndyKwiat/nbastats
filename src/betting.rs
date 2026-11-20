//! Betting lines + spread / total prediction models, evaluated with a strict walk-forward backtest.
//!
//! Every prediction for a game on day D is made from a model state that has only seen games on
//! days < D (games on the same day never see each other). Feature models are trained only on
//! (features, result) pairs of earlier games, and feature scaling is fit on that training set.
//! Hyper-parameters were tuned on the 2024-25 season (no lines used) and are fixed constants here;
//! the line-based results below are on 2025-26 and 2026-27, which played no part in tuning.
use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const CUR_SEASON: i64 = 2027;

#[derive(Clone, Default, Debug)]
pub struct Line {
    pub provider: String,
    pub spread: Option<f64>, // home team's line (negative = home favoured)
    pub total: Option<f64>,
    pub mlh: Option<f64>,
    pub mla: Option<f64>,
    pub spread_open: Option<f64>,
    pub total_open: Option<f64>,
    pub mlh_open: Option<f64>,
    pub mla_open: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct BG {
    pub id: String,
    pub season: i64,
    pub date: String,
    pub day: String,
    pub dayn: i64, // days since 1970 (US Eastern calendar)
    pub h: usize,
    pub a: usize,
    pub hs: f64,
    pub as_: f64,
    pub fin: bool,
    pub neutral: bool,
    pub poss: f64, // 0 = unknown
    pub status: String,
    pub line: Option<Line>,
}

fn eastern_day(iso: &str) -> String {
    let fmt = if iso.len() == 17 { "%Y-%m-%dT%H:%MZ" } else { "%Y-%m-%dT%H:%M:%SZ" };
    match chrono::NaiveDateTime::parse_from_str(iso, fmt) {
        Ok(t) => (t - chrono::Duration::hours(5)).format("%Y-%m-%d").to_string(),
        Err(_) => iso.chars().take(10).collect(),
    }
}
fn dayn(day: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| (d - chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days())
        .unwrap_or(0)
}

fn poss_est(fgm: f64, fga: f64, fta: f64, oreb: f64, tov: f64, ofgm: f64, ofga: f64, ofta: f64, ooreb: f64, otov: f64, dreb: f64, odreb: f64) -> f64 {
    let side = |fgm: f64, fga: f64, fta: f64, or: f64, tov: f64, opp_dr: f64| {
        let orp = if or + opp_dr > 0.0 { or / (or + opp_dr) } else { 0.0 };
        fga + 0.4 * fta - 1.07 * orp * (fga - fgm) + tov
    };
    0.5 * (side(fgm, fga, fta, oreb, tov, odreb) + side(ofgm, ofga, ofta, ooreb, otov, dreb))
}

pub struct Data {
    pub teams: Vec<(String, String)>, // (id, abbr)
    pub games: Vec<BG>,
}

pub fn load(conn: &Connection) -> Result<Data> {
    let mut teams: Vec<(String, String)> = conn
        .prepare("SELECT id, abbr FROM teams WHERE conf != '?' ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    teams.sort();
    let tix: HashMap<String, usize> = teams.iter().enumerate().map(|(i, t)| (t.0.clone(), i)).collect();

    let mut odds: HashMap<String, Line> = HashMap::new();
    {
        let mut st = conn.prepare(
            "SELECT game_id,provider,spread,total,ml_home,ml_away,spread_open,total_open,ml_home_open,ml_away_open FROM odds",
        )?;
        let rows = st.query_map([], |r| {
            let f = |i: usize| -> rusqlite::Result<Option<f64>> { r.get::<_, Option<f64>>(i) };
            Ok((
                r.get::<_, String>(0)?,
                Line {
                    provider: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    spread: f(2)?,
                    total: f(3)?,
                    mlh: f(4)?,
                    mla: f(5)?,
                    spread_open: f(6)?,
                    total_open: f(7)?,
                    mlh_open: f(8)?,
                    mla_open: f(9)?,
                },
            ))
        })?;
        for x in rows {
            let (k, v) = x?;
            if v.spread.is_some() || v.total.is_some() || v.mlh.is_some() {
                odds.insert(k, v);
            }
        }
    }

    let mut games = vec![];
    {
        let mut st = conn.prepare(
            "SELECT id,season,date,home_id,away_id,home_score,away_score,neutral,
                    h_fgm,h_fga,h_fta,h_oreb,h_dreb,h_tov,a_fgm,a_fga,a_fta,a_oreb,a_dreb,a_tov
             FROM hist_games WHERE fetched=1",
        )?;
        let rows = st.query_map([], |r| {
            let f = |i: usize| -> rusqlite::Result<f64> { Ok(r.get::<_, Option<i64>>(i)?.unwrap_or(0) as f64) };
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                f(5)?,
                f(6)?,
                f(7)? != 0.0,
                [f(8)?, f(9)?, f(10)?, f(11)?, f(12)?, f(13)?, f(14)?, f(15)?, f(16)?, f(17)?, f(18)?, f(19)?],
            ))
        })?;
        for x in rows {
            let (id, season, date, h, a, hs, as_, neutral, b) = x?;
            let (Some(&hi), Some(&ai)) = (tix.get(&h), tix.get(&a)) else { continue };
            if hs + as_ <= 0.0 {
                continue;
            }
            let poss = if b[1] > 0.0 && b[7] > 0.0 {
                poss_est(b[0], b[1], b[2], b[3], b[5], b[6], b[7], b[8], b[9], b[11], b[4], b[10])
            } else {
                0.0
            };
            let day = eastern_day(&date);
            games.push(BG {
                line: odds.get(&id).cloned(),
                id,
                season,
                dayn: dayn(&day),
                day,
                date,
                h: hi,
                a: ai,
                hs,
                as_,
                fin: true,
                neutral,
                poss,
                status: "STATUS_FINAL".into(),
            });
        }
    }
    {
        // current season: box-score possessions from team_games
        let mut tg: HashMap<(String, String), [f64; 6]> = HashMap::new();
        let mut st = conn.prepare("SELECT game_id,team_id,fgm,fga,fta,oreb,dreb,total_tov FROM team_games")?;
        let rows = st.query_map([], |r| {
            let f = |i: usize| -> rusqlite::Result<f64> { Ok(r.get::<_, Option<i64>>(i)?.unwrap_or(0) as f64) };
            Ok(((r.get::<_, String>(0)?, r.get::<_, String>(1)?), [f(2)?, f(3)?, f(4)?, f(5)?, f(6)?, f(7)?]))
        })?;
        for x in rows {
            let (k, v) = x?;
            tg.insert(k, v);
        }
        let mut st = conn.prepare(
            "SELECT id,date,status,home_id,away_id,home_score,away_score,neutral FROM games WHERE season_type=2",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<i64>>(5)?.unwrap_or(0) as f64,
                r.get::<_, Option<i64>>(6)?.unwrap_or(0) as f64,
                r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
            ))
        })?;
        for x in rows {
            let (id, date, status, h, a, hs, as_, neutral) = x?;
            let (Some(&hi), Some(&ai)) = (tix.get(&h), tix.get(&a)) else { continue };
            let fin = status == "STATUS_FINAL";
            if status == "STATUS_POSTPONED" || status == "STATUS_CANCELED" {
                continue;
            }
            let poss = match (tg.get(&(id.clone(), h.clone())), tg.get(&(id.clone(), a.clone()))) {
                (Some(x), Some(y)) if fin => poss_est(x[0], x[1], x[2], x[3], x[5], y[0], y[1], y[2], y[3], y[5], x[4], y[4]),
                _ => 0.0,
            };
            let day = eastern_day(&date);
            games.push(BG {
                line: odds.get(&id).cloned(),
                id,
                season: CUR_SEASON,
                dayn: dayn(&day),
                day,
                date,
                h: hi,
                a: ai,
                hs,
                as_,
                fin,
                neutral,
                poss,
                status,
            });
        }
    }
    games.sort_by(|x, y| x.date.cmp(&y.date).then(x.id.cmp(&y.id)));
    Ok(Data { teams, games })
}

// ---------------------------------------------------------------- linear algebra

/// Solve the symmetric positive-definite system A x = b (Cholesky; A is n×n row-major).
fn chol_solve(a: &mut [f64], b: &[f64], n: usize) -> Vec<f64> {
    for j in 0..n {
        let mut s = a[j * n + j];
        for k in 0..j {
            s -= a[j * n + k] * a[j * n + k];
        }
        let d = s.max(1e-9).sqrt();
        a[j * n + j] = d;
        for i in (j + 1)..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = s / d;
        }
    }
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * y[k];
        }
        y[i] = s / a[i * n + i];
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = y[i];
        for k in (i + 1)..n {
            s -= a[k * n + i] * x[k];
        }
        x[i] = s / a[i * n + i];
    }
    x
}

/// Weighted ridge regression over sparse rows. `pen[i]` is the L2 penalty on parameter i.
struct Ridge {
    n: usize,
    a: Vec<f64>,
    b: Vec<f64>,
}
impl Ridge {
    fn new(pen: &[f64]) -> Self {
        let n = pen.len();
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            a[i * n + i] = pen[i] + 1e-6;
        }
        Ridge { n, a, b: vec![0.0; n] }
    }
    fn add(&mut self, x: &[(usize, f64)], y: f64, w: f64) {
        for &(i, xi) in x {
            for &(j, xj) in x {
                self.a[i * self.n + j] += w * xi * xj;
            }
            self.b[i] += w * xi * y;
        }
    }
    fn solve(mut self) -> Vec<f64> {
        chol_solve(&mut self.a, &self.b, self.n)
    }
}

// ---------------------------------------------------------------- parameters

#[derive(Clone, Copy, Debug)]
pub struct P {
    pub elo_k: f64,
    pub elo_hca: f64,
    pub elo_carry: f64,
    pub hl: f64,        // half-life (days) for the decayed rating models
    pub offseason: f64, // the summer counts as this many days of decay
    pub lam: f64,       // ridge penalty for decayed margin ratings
    pub lam_eff: f64,   // ridge penalty for efficiency/points ratings
    pub ewma: f64,      // per-game smoothing for recent-form averages
    pub sigma: f64,     // margin st.dev. used for win probabilities
}
pub const DEFAULT: P = P {
    elo_k: 25.0,
    elo_hca: 70.0,
    elo_carry: 0.75,
    hl: 30.0,
    offseason: 60.0,
    lam: 4.0,
    lam_eff: 3.0,
    ewma: 0.12,
    sigma: 14.0,
};

// ---------------------------------------------------------------- base models

struct Elo {
    r: Vec<f64>,
    season: i64,
}
impl Elo {
    fn margin(&self, p: &P, g: &BG) -> f64 {
        let hca = if g.neutral { 0.0 } else { p.elo_hca };
        (self.r[g.h] + hca - self.r[g.a]) / 28.0
    }
    fn season_check(&mut self, p: &P, season: i64) {
        if season != self.season {
            if self.season != 0 {
                for x in self.r.iter_mut() {
                    *x = p.elo_carry * *x + (1.0 - p.elo_carry) * 1505.0;
                }
            }
            self.season = season;
        }
    }
    fn update(&mut self, p: &P, g: &BG) {
        let hca = if g.neutral { 0.0 } else { p.elo_hca };
        let d = self.r[g.h] + hca - self.r[g.a];
        let exp = 1.0 / (1.0 + 10f64.powf(-d / 400.0));
        let mov = g.hs - g.as_;
        let won = if mov > 0.0 { 1.0 } else { 0.0 };
        let wd = if mov > 0.0 { d } else { -d };
        let mult = (mov.abs() + 3.0).powf(0.8) / (7.5 + 0.006 * wd);
        let delta = p.elo_k * mult * (won - exp);
        self.r[g.h] += delta;
        self.r[g.a] -= delta;
    }
}

/// Fitted rating snapshot for one day.
#[derive(Clone, Default)]
struct Fit {
    // decayed margin ratings
    m_hca: f64,
    m_r: Vec<f64>,
    // season-to-date margin ratings (no decay)
    s_hca: f64,
    s_r: Vec<f64>,
    // decayed points model: pts = mu ± hh/2 + o_t + d_opp
    p_mu: f64,
    p_hh: f64,
    p_o: Vec<f64>,
    p_d: Vec<f64>,
    // decayed efficiency (per 100) + pace
    e_mu: f64,
    e_hh: f64,
    e_o: Vec<f64>,
    e_d: Vec<f64>,
    q0: f64,
    q: Vec<f64>,
}

/// Effective time: the summer gap between seasons is compressed to `p.offseason` days.
fn eff_time(games: &[BG], p: &P) -> Vec<f64> {
    let mut out = vec![0.0; games.len()];
    let mut shift = 0.0;
    let mut prev: Option<(i64, i64)> = None; // (season, last day)
    for (i, g) in games.iter().enumerate() {
        if let Some((s, last)) = prev {
            if g.season != s {
                let gap = (g.dayn - last) as f64;
                shift += (gap - p.offseason).max(0.0);
            }
        }
        out[i] = g.dayn as f64 - shift;
        prev = Some((g.season, g.dayn));
    }
    out
}

fn fit_day(games: &[BG], te: &[f64], upto: usize, now_te: f64, season: i64, nt: usize, p: &P, hl: f64) -> Fit {
    let decay = |i: usize| if hl.is_finite() { 0.5f64.powf((now_te - te[i]) / hl) } else { 1.0 };
    let mut f = Fit::default();
    // margin ratings: params [hca, r0..r29]
    let mut pen = vec![p.lam; nt + 1];
    pen[0] = 0.01;
    let mut rm = Ridge::new(&pen);
    let mut pen_s = vec![2.0; nt + 1];
    pen_s[0] = 0.01;
    let mut rs = Ridge::new(&pen_s);
    // points: [mu, hh, o.., d..]
    let mut pen_p = vec![p.lam_eff; 2 + 2 * nt];
    pen_p[0] = 1e-4;
    pen_p[1] = 0.01;
    let mut rp = Ridge::new(&pen_p);
    let mut re = Ridge::new(&pen_p);
    let mut pen_q = vec![p.lam_eff; 1 + nt];
    pen_q[0] = 1e-4;
    let mut rq = Ridge::new(&pen_q);
    let mut sw = 0.0;
    for i in 0..upto {
        let g = &games[i];
        if !g.fin {
            continue;
        }
        let w = decay(i);
        if w < 1e-4 {
            continue;
        }
        sw += w;
        let ho = if g.neutral { 0.0 } else { 1.0 };
        rm.add(&[(0, ho), (1 + g.h, 1.0), (1 + g.a, -1.0)], g.hs - g.as_, w);
        if g.season == season {
            rs.add(&[(0, ho), (1 + g.h, 1.0), (1 + g.a, -1.0)], g.hs - g.as_, 1.0);
        }
        let hh = 0.5 * ho;
        rp.add(&[(0, 1.0), (1, hh), (2 + g.h, 1.0), (2 + nt + g.a, 1.0)], g.hs, w);
        rp.add(&[(0, 1.0), (1, -hh), (2 + g.a, 1.0), (2 + nt + g.h, 1.0)], g.as_, w);
        if g.poss > 50.0 {
            re.add(&[(0, 1.0), (1, hh), (2 + g.h, 1.0), (2 + nt + g.a, 1.0)], 100.0 * g.hs / g.poss, w);
            re.add(&[(0, 1.0), (1, -hh), (2 + g.a, 1.0), (2 + nt + g.h, 1.0)], 100.0 * g.as_ / g.poss, w);
            rq.add(&[(0, 1.0), (1 + g.h, 1.0), (1 + g.a, 1.0)], g.poss, w);
        }
    }
    if sw == 0.0 {
        f.m_r = vec![0.0; nt];
        f.s_r = vec![0.0; nt];
        f.p_mu = 113.0;
        f.p_o = vec![0.0; nt];
        f.p_d = vec![0.0; nt];
        f.e_mu = 114.0;
        f.e_o = vec![0.0; nt];
        f.e_d = vec![0.0; nt];
        f.q0 = 99.0;
        f.q = vec![0.0; nt];
        return f;
    }
    let x = rm.solve();
    f.m_hca = x[0];
    f.m_r = x[1..].to_vec();
    let x = rs.solve();
    f.s_hca = x[0];
    f.s_r = x[1..].to_vec();
    let x = rp.solve();
    f.p_mu = x[0];
    f.p_hh = x[1];
    f.p_o = x[2..2 + nt].to_vec();
    f.p_d = x[2 + nt..].to_vec();
    let x = re.solve();
    f.e_mu = x[0];
    f.e_hh = x[1];
    f.e_o = x[2..2 + nt].to_vec();
    f.e_d = x[2 + nt..].to_vec();
    let x = rq.solve();
    f.q0 = x[0] / 1.0;
    f.q = x[1..].to_vec();
    f
}

impl Fit {
    fn srs_margin(&self, g: &BG) -> f64 {
        (if g.neutral { 0.0 } else { self.m_hca }) + self.m_r[g.h] - self.m_r[g.a]
    }
    fn season_margin(&self, g: &BG) -> f64 {
        (if g.neutral { 0.0 } else { self.s_hca }) + self.s_r[g.h] - self.s_r[g.a]
    }
    fn pts(&self, g: &BG) -> (f64, f64) {
        let hh = if g.neutral { 0.0 } else { 0.5 * self.p_hh };
        (self.p_mu + hh + self.p_o[g.h] + self.p_d[g.a], self.p_mu - hh + self.p_o[g.a] + self.p_d[g.h])
    }
    fn eff(&self, g: &BG) -> (f64, f64, f64) {
        let hh = if g.neutral { 0.0 } else { 0.5 * self.e_hh };
        let pace = self.q0 + self.q[g.h] + self.q[g.a];
        let oh = self.e_mu + hh + self.e_o[g.h] + self.e_d[g.a];
        let oa = self.e_mu - hh + self.e_o[g.a] + self.e_d[g.h];
        (pace * oh / 100.0, pace * oa / 100.0, pace)
    }
}

// ---------------------------------------------------------------- tiny MLP

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.next().max(1e-12), self.next());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// One hidden layer (tanh), two linear outputs, trained with Adam on squared error.
#[derive(Clone)]
struct Mlp {
    ni: usize,
    nh: usize,
    no: usize,
    w: Vec<f64>, // [w1 (nh*ni), b1 (nh), w2 (no*nh), b2 (no)]
    m: Vec<f64>,
    v: Vec<f64>,
    t: f64,
}
impl Mlp {
    fn new(ni: usize, nh: usize, no: usize, rng: &mut Rng) -> Self {
        let n = nh * ni + nh + no * nh + no;
        let mut w = vec![0.0; n];
        for i in 0..nh * ni {
            w[i] = rng.normal() / (ni as f64).sqrt();
        }
        let o2 = nh * ni + nh;
        for i in 0..no * nh {
            w[o2 + i] = rng.normal() / (nh as f64).sqrt() * 0.5;
        }
        Mlp { ni, nh, no, m: vec![0.0; n], v: vec![0.0; n], w, t: 0.0 }
    }
    fn forward(&self, x: &[f64], h: &mut [f64], out: &mut [f64]) {
        let (ni, nh) = (self.ni, self.nh);
        for j in 0..nh {
            let mut s = self.w[nh * ni + j];
            for i in 0..ni {
                s += self.w[j * ni + i] * x[i];
            }
            h[j] = s.tanh();
        }
        let o2 = nh * ni + nh;
        let ob = o2 + self.no * nh;
        for k in 0..self.no {
            let mut s = self.w[ob + k];
            for j in 0..nh {
                s += self.w[o2 + k * nh + j] * h[j];
            }
            out[k] = s;
        }
    }
    fn train(&mut self, xs: &[Vec<f64>], ys: &[Vec<f64>], epochs: usize, lr: f64, l2: f64, rng: &mut Rng) {
        let (ni, nh, no) = (self.ni, self.nh, self.no);
        let n = xs.len();
        let bs = 64;
        let mut idx: Vec<usize> = (0..n).collect();
        let mut g = vec![0.0; self.w.len()];
        let mut h = vec![0.0; nh];
        let mut out = vec![0.0; no];
        let mut dh = vec![0.0; nh];
        let o2 = nh * ni + nh;
        let ob = o2 + no * nh;
        for _ in 0..epochs {
            for i in (1..n).rev() {
                let j = (rng.next() * (i + 1) as f64) as usize;
                idx.swap(i, j.min(i));
            }
            for chunk in idx.chunks(bs) {
                g.iter_mut().for_each(|x| *x = 0.0);
                for &s in chunk {
                    self.forward(&xs[s], &mut h, &mut out);
                    dh.iter_mut().for_each(|x| *x = 0.0);
                    for k in 0..no {
                        let e = (out[k] - ys[s][k]) / chunk.len() as f64;
                        g[ob + k] += e;
                        for j in 0..nh {
                            g[o2 + k * nh + j] += e * h[j];
                            dh[j] += e * self.w[o2 + k * nh + j];
                        }
                    }
                    for j in 0..nh {
                        let dz = dh[j] * (1.0 - h[j] * h[j]);
                        g[nh * ni + j] += dz;
                        for i in 0..ni {
                            g[j * ni + i] += dz * xs[s][i];
                        }
                    }
                }
                self.t += 1.0;
                let (b1, b2) = (0.9f64, 0.999f64);
                let c1 = 1.0 - b1.powf(self.t);
                let c2 = 1.0 - b2.powf(self.t);
                for i in 0..self.w.len() {
                    let gi = g[i] + l2 * self.w[i];
                    self.m[i] = b1 * self.m[i] + (1.0 - b1) * gi;
                    self.v[i] = b2 * self.v[i] + (1.0 - b2) * gi * gi;
                    self.w[i] -= lr * (self.m[i] / c1) / ((self.v[i] / c2).sqrt() + 1e-8);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- engine

pub const MODELS: &[(&str, &str, &str)] = &[
    ("naive", "Naive baseline", "Home team by the league home edge; total = recent league average. Anything useful must beat this."),
    ("elo", "Elo (MOV)", "538-style Elo with margin-of-victory multiplier, 75% carry-over between seasons. Total = recent scoring form."),
    ("srs_season", "SRS – season only", "Least-squares margin ratings from this season's games, every game weighted equally (no recency)."),
    ("srs_decay", "SRS – recency weighted", "Same margin ratings, but each game's weight halves every 30 days; last season fades in as a prior."),
    ("eff", "Adj. efficiency + pace", "KenPom-style: opponent-adjusted offensive / defensive points per 100 possessions and pace, recency weighted. Predicts each team's score."),
    ("ridge", "Ridge regression", "Linear model on game-time features: all ratings above, rest days, back-to-backs, last-10 form, neutral site."),
    ("nn", "Neural net (MLP)", "3 × one-hidden-layer nets (16 tanh units) on the same features, retrained weekly; outputs margin and total."),
    ("ens", "Ensemble", "Average of Elo, recency SRS, efficiency, ridge and neural net."),
];
const NM: usize = 8;
const NF: usize = 14;

#[derive(Clone, Copy, Default)]
pub struct Pred {
    pub m: f64,
    pub t: f64,
}

pub struct Run {
    pub preds: Vec<[Pred; NM]>, // per game
    pub elo: Vec<f64>,
    pub fit: Option<Fit>,
}

pub fn run(games: &[BG], nt: usize, p: &P, with_nn: bool) -> Run {
    let te = eff_time(games, p);
    let n = games.len();
    let mut preds = vec![[Pred::default(); NM]; n];
    let mut elo = Elo { r: vec![1505.0; nt], season: 0 };
    // recent-form state
    let mut ew_for = vec![112.0; nt];
    let mut ew_ag = vec![112.0; nt];
    let mut last10: Vec<Vec<f64>> = vec![vec![]; nt];
    let mut last_day: Vec<Option<i64>> = vec![None; nt];
    let mut cur_season = 0;
    let mut lg_total = 224.0;
    let mut lg_hca = 2.5;
    // training set for feature models
    let mut tx: Vec<[f64; NF]> = vec![];
    let mut ty: Vec<[f64; 2]> = vec![];
    let mut ridge_w: Option<(Vec<f64>, Vec<f64>)> = None;
    let mut nets: Vec<Mlp> = vec![];
    let mut norm: Option<(Vec<f64>, Vec<f64>, [f64; 2], [f64; 2])> = None;
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let mut last_train_day = i64::MIN / 2;
    let mut last_fit = None;

    let mut i = 0;
    while i < n {
        let day = games[i].dayn;
        let mut j = i;
        while j < n && games[j].dayn == day {
            j += 1;
        }
        let season = games[i].season;
        if season != cur_season {
            elo.season_check(p, season);
            for l in last10.iter_mut() {
                l.clear();
            }
            cur_season = season;
        }
        let any_prior = games[..i].iter().any(|g| g.fin && g.season == season);
        let fit = fit_day(games, &te, i, te[i], season, nt, p, p.hl);

        // (re)train feature models weekly once there's enough history
        if tx.len() >= 300 && day - last_train_day >= 7 {
            last_train_day = day;
            let mut mean = vec![0.0; NF];
            let mut sd = vec![0.0; NF];
            for x in &tx {
                for k in 0..NF {
                    mean[k] += x[k] / tx.len() as f64;
                }
            }
            for x in &tx {
                for k in 0..NF {
                    sd[k] += (x[k] - mean[k]).powi(2) / tx.len() as f64;
                }
            }
            sd.iter_mut().for_each(|s| *s = s.sqrt().max(1e-6));
            let ym = [ty.iter().map(|y| y[0]).sum::<f64>() / ty.len() as f64, ty.iter().map(|y| y[1]).sum::<f64>() / ty.len() as f64];
            let ys = [
                (ty.iter().map(|y| (y[0] - ym[0]).powi(2)).sum::<f64>() / ty.len() as f64).sqrt(),
                (ty.iter().map(|y| (y[1] - ym[1]).powi(2)).sum::<f64>() / ty.len() as f64).sqrt(),
            ];
            let xs: Vec<Vec<f64>> = tx.iter().map(|x| (0..NF).map(|k| (x[k] - mean[k]) / sd[k]).collect()).collect();
            // ridge (closed form) on standardised features, one per target
            let mut ws = vec![];
            for t in 0..2 {
                let mut pen = vec![30.0; NF + 1];
                pen[NF] = 1e-6;
                let mut r = Ridge::new(&pen);
                for (x, y) in xs.iter().zip(&ty) {
                    let row: Vec<(usize, f64)> = x.iter().cloned().enumerate().chain([(NF, 1.0)]).collect();
                    r.add(&row, y[t], 1.0);
                }
                ws.push(r.solve());
            }
            ridge_w = Some((ws[0].clone(), ws[1].clone()));
            if with_nn {
                let yn: Vec<Vec<f64>> = ty.iter().map(|y| vec![(y[0] - ym[0]) / ys[0], (y[1] - ym[1]) / ys[1]]).collect();
                if nets.is_empty() {
                    for _ in 0..3 {
                        let mut m = Mlp::new(NF, 16, 2, &mut rng);
                        m.train(&xs, &yn, 80, 0.005, 1e-3, &mut rng);
                        nets.push(m);
                    }
                } else {
                    for m in nets.iter_mut() {
                        m.train(&xs, &yn, 8, 0.002, 1e-3, &mut rng);
                    }
                }
            }
            norm = Some((mean, sd, ym, ys));
        }

        for k in i..j {
            let g = &games[k];
            let em = elo.margin(p, g);
            let ew_total = 0.5 * (ew_for[g.h] + ew_ag[g.a] + ew_for[g.a] + ew_ag[g.h]);
            let sm = fit.srs_margin(g);
            let ss = fit.season_margin(g);
            let (ph, pa) = fit.pts(g);
            let (eh, ea, pace) = fit.eff(g);
            let rest = |t: usize| last_day[t].map(|d| ((day - d) as f64).min(4.0)).unwrap_or(4.0);
            let (rh, ra) = (rest(g.h), rest(g.a));
            let l10 = |t: usize| if last10[t].is_empty() { 0.0 } else { last10[t].iter().sum::<f64>() / last10[t].len() as f64 };
            let gp = ((last10[g.h].len() + last10[g.a].len()) as f64).min(20.0) / 20.0;
            let feat: [f64; NF] = [
                em,
                sm,
                eh - ea,
                eh + ea,
                ss,
                ph + pa,
                rh,
                ra,
                (rh <= 1.0) as i32 as f64,
                (ra <= 1.0) as i32 as f64,
                l10(g.h) - l10(g.a),
                gp,
                g.neutral as i32 as f64,
                pace,
            ];
            let mut pr = [Pred::default(); NM];
            let nh = if g.neutral { 0.0 } else { lg_hca };
            pr[0] = Pred { m: nh, t: lg_total };
            pr[1] = Pred { m: em, t: ew_total };
            pr[2] = Pred { m: if any_prior { ss } else { nh }, t: ph + pa };
            pr[3] = Pred { m: sm, t: ph + pa };
            pr[4] = Pred { m: eh - ea, t: eh + ea };
            match (&ridge_w, &norm) {
                (Some((wm, wt)), Some((mean, sd, _, _))) => {
                    let xs: Vec<f64> = (0..NF).map(|k| (feat[k] - mean[k]) / sd[k]).collect();
                    let dot = |w: &Vec<f64>| xs.iter().zip(w).map(|(a, b)| a * b).sum::<f64>() + w[NF];
                    pr[5] = Pred { m: dot(wm), t: dot(wt) };
                }
                _ => pr[5] = pr[4],
            }
            match (&norm, nets.is_empty()) {
                (Some((mean, sd, ym, ys)), false) => {
                    let xs: Vec<f64> = (0..NF).map(|k| (feat[k] - mean[k]) / sd[k]).collect();
                    let mut h = vec![0.0; 16];
                    let mut o = vec![0.0; 2];
                    let (mut sm_, mut st_) = (0.0, 0.0);
                    for net in &nets {
                        net.forward(&xs, &mut h, &mut o);
                        sm_ += (o[0] * ys[0] + ym[0]) / nets.len() as f64;
                        st_ += (o[1] * ys[1] + ym[1]) / nets.len() as f64;
                    }
                    pr[6] = Pred { m: sm_, t: st_ };
                }
                _ => pr[6] = pr[5],
            }
            let ens = [1usize, 3, 4, 5, 6];
            pr[7] = Pred {
                m: ens.iter().map(|&q| pr[q].m).sum::<f64>() / ens.len() as f64,
                t: ens.iter().map(|&q| if q == 1 { pr[4].t } else { pr[q].t }).sum::<f64>() / ens.len() as f64,
            };
            preds[k] = pr;
            if g.fin {
                tx.push(feat);
                ty.push([g.hs - g.as_, g.hs + g.as_]);
            }
        }
        // ---- update with this day's results
        for k in i..j {
            let g = &games[k];
            last_day[g.h] = Some(day);
            last_day[g.a] = Some(day);
            if !g.fin {
                continue;
            }
            elo.update(p, g);
            let a = p.ewma;
            ew_for[g.h] = (1.0 - a) * ew_for[g.h] + a * g.hs;
            ew_ag[g.h] = (1.0 - a) * ew_ag[g.h] + a * g.as_;
            ew_for[g.a] = (1.0 - a) * ew_for[g.a] + a * g.as_;
            ew_ag[g.a] = (1.0 - a) * ew_ag[g.a] + a * g.hs;
            lg_total = 0.97 * lg_total + 0.03 * (g.hs + g.as_);
            if !g.neutral {
                lg_hca = 0.995 * lg_hca + 0.005 * (g.hs - g.as_);
            }
            for (t, m) in [(g.h, g.hs - g.as_), (g.a, g.as_ - g.hs)] {
                last10[t].push(m);
                if last10[t].len() > 10 {
                    last10[t].remove(0);
                }
            }
        }
        last_fit = Some(fit);
        i = j;
    }
    let _ = last_fit;
    let fin_fit = fit_day(games, &te, n, te.last().copied().unwrap_or(0.0) + 1.0, cur_season, nt, p, p.hl);
    Run { preds, elo: elo.r, fit: Some(fin_fit) }
}

// ---------------------------------------------------------------- evaluation

fn phi(x: f64) -> f64 {
    // standard normal CDF (Abramowitz-Stegun 7.1.26 via erf)
    let t = 1.0 / (1.0 + 0.3275911 * (x.abs() / 2f64.sqrt()));
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t
            * (-(x * x) / 2.0).exp();
    if x >= 0.0 {
        0.5 * (1.0 + y)
    } else {
        0.5 * (1.0 - y)
    }
}
fn implied(ml: f64) -> f64 {
    if ml < 0.0 {
        -ml / (-ml + 100.0)
    } else {
        100.0 / (ml + 100.0)
    }
}
fn payout(ml: f64) -> f64 {
    if ml < 0.0 {
        100.0 / -ml
    } else {
        ml / 100.0
    }
}
pub fn win_prob(p: &P, margin: f64) -> f64 {
    phi(margin / p.sigma)
}

#[derive(Default, Clone)]
struct Acc {
    n: f64,
    m_ae: f64,
    m_se: f64,
    t_ae: f64,
    t_n: f64,
    su_w: f64,
    su_n: f64,
    ats: [[f64; 3]; 3], // [edge≥0, ≥3, ≥6] × [w, l, push]
    ou: [[f64; 3]; 3],
    ml_n: f64,
    ml_units: f64,
    ml_w: f64,
}
const EDGES: [f64; 3] = [0.0, 3.0, 6.0];
const VIG_WIN: f64 = 100.0 / 110.0;

impl Acc {
    fn units(r: &[f64; 3]) -> f64 {
        r[0] * VIG_WIN - r[1]
    }
    fn json(&self) -> Value {
        let rec = |r: &[f64; 3]| {
            let n = r[0] + r[1];
            json!({ "w": r[0], "l": r[1], "p": r[2], "pct": if n > 0.0 { r[0] / n } else { 0.0 }, "units": (Acc::units(r) * 100.0).round() / 100.0 })
        };
        json!({
            "n": self.n,
            "mae": self.m_ae / self.n.max(1.0),
            "rmse": (self.m_se / self.n.max(1.0)).sqrt(),
            "tmae": self.t_ae / self.t_n.max(1.0),
            "su": self.su_w / self.su_n.max(1.0),
            "ats": self.ats.iter().map(rec).collect::<Vec<_>>(),
            "ou": self.ou.iter().map(rec).collect::<Vec<_>>(),
            "ml": { "n": self.ml_n, "w": self.ml_w, "units": (self.ml_units * 100.0).round() / 100.0 },
        })
    }
}

/// The (spread, total, home ML, away ML) a bet would be placed at: closing or opening line.
fn line_at(g: &BG, open: bool) -> Option<(Option<f64>, Option<f64>, Option<f64>, Option<f64>)> {
    let l = g.line.as_ref()?;
    let v = if open { (l.spread_open, l.total_open, l.mlh_open, l.mla_open) } else { (l.spread, l.total, l.mlh, l.mla) };
    v.0?;
    Some(v)
}

fn score(acc: &mut Acc, g: &BG, pr: Pred, p: &P, open: bool) {
    let am = g.hs - g.as_;
    let at = g.hs + g.as_;
    let Some((spread, total, mlh, mla)) = line_at(g, open) else { return };
    acc.n += 1.0;
    acc.m_ae += (pr.m - am).abs();
    acc.m_se += (pr.m - am).powi(2);
    if am != 0.0 && pr.m != 0.0 {
        acc.su_n += 1.0;
        if (pr.m > 0.0) == (am > 0.0) {
            acc.su_w += 1.0;
        }
    }
    if let Some(s) = spread {
        let edge = pr.m + s; // >0 → model likes home to cover
        let res = am + s;
        for (e, th) in EDGES.iter().enumerate() {
            if edge.abs() >= *th && edge != 0.0 {
                if res == 0.0 {
                    acc.ats[e][2] += 1.0;
                } else if (res > 0.0) == (edge > 0.0) {
                    acc.ats[e][0] += 1.0;
                } else {
                    acc.ats[e][1] += 1.0;
                }
            }
        }
    }
    if let Some(t) = total {
        acc.t_ae += (pr.t - at).abs();
        acc.t_n += 1.0;
        let edge = pr.t - t;
        let res = at - t;
        for (e, th) in EDGES.iter().enumerate() {
            if edge.abs() >= *th && edge != 0.0 {
                if res == 0.0 {
                    acc.ou[e][2] += 1.0;
                } else if (res > 0.0) == (edge > 0.0) {
                    acc.ou[e][0] += 1.0;
                } else {
                    acc.ou[e][1] += 1.0;
                }
            }
        }
    }
    if let (Some(mh), Some(ma)) = (mlh, mla) {
        let ph = win_prob(p, pr.m);
        let (ih, ia) = (implied(mh), implied(ma));
        let (fh, fa) = (ih / (ih + ia), ia / (ih + ia));
        // bet the side whose model probability beats the no-vig market probability by 3+ points
        let pick = if ph - fh >= 0.03 { Some((true, mh)) } else if (1.0 - ph) - fa >= 0.03 { Some((false, ma)) } else { None };
        if let Some((home, ml)) = pick {
            acc.ml_n += 1.0;
            if (am > 0.0) == home {
                acc.ml_w += 1.0;
                acc.ml_units += payout(ml);
            } else {
                acc.ml_units -= 1.0;
            }
        }
    }
}

/// Market line as a "model" (benchmark): margin = -spread, total = total.
fn market_pred(g: &BG, open: bool) -> Option<Pred> {
    let (s, t, _, _) = line_at(g, open)?;
    Some(Pred { m: -s?, t: t.unwrap_or(f64::NAN) })
}

fn mae_only(games: &[BG], preds: &[[Pred; NM]], from: i64, to: i64) -> Vec<(f64, f64, f64)> {
    let mut out: Vec<(f64, f64, f64)> = vec![(0.0, 0.0, 0.0); NM];
    for (g, pr) in games.iter().zip(preds) {
        if !g.fin || g.dayn < from || g.dayn > to {
            continue;
        }
        for m in 0..NM {
            out[m].0 += (pr[m].m - (g.hs - g.as_)).abs();
            out[m].1 += (pr[m].t - (g.hs + g.as_)).abs();
            out[m].2 += 1.0;
        }
    }
    out.iter().map(|(a, b, n)| (a / n.max(1.0), b / n.max(1.0), *n)).collect()
}

/// Hyper-parameter tuning on the 2024-25 season only (Dec 1 – end), MAE vs actual results. CLI only.
pub fn tune(conn: &Connection) -> Result<()> {
    let d = load(conn)?;
    let nt = d.teams.len();
    let g: Vec<BG> = d.games.iter().filter(|g| g.season <= 2025).cloned().collect();
    eprintln!("tuning on {} games of 2024-25", g.len());
    let from = dayn("2024-12-01");
    let to = dayn("2025-04-30");
    let show = |label: String, p: &P| {
        let r = run(&g, nt, p, false);
        let m = mae_only(&g, &r.preds, from, to);
        println!(
            "{label:<34} elo {:.3} srsS {:.3} srsD {:.3} eff {:.3}/{:.3} pts {:.3} ew {:.3} ridge {:.3}/{:.3}",
            m[1].0, m[2].0, m[3].0, m[4].0, m[4].1, m[3].1, m[1].1, m[5].0, m[5].1
        );
    };
    show("default".into(), &DEFAULT);
    for k in [20.0, 25.0, 30.0, 40.0] {
        show(format!("elo_k {k}"), &P { elo_k: k, ..DEFAULT });
    }


    for lam_eff in [1.0, 2.0, 3.0, 5.0] {
        for hl in [20.0, 30.0, 45.0, 60.0, 90.0, 1e9] {
            show(format!("lam_eff {lam_eff} hl {hl}"), &P { lam_eff, hl, ..DEFAULT });
        }
    }
    for off in [0.0, 30.0, 60.0, 120.0] {
        show(format!("offseason {off}"), &P { offseason: off, ..DEFAULT });
    }
    for ewma in [0.05, 0.08, 0.12, 0.2] {
        show(format!("ewma {ewma}"), &P { ewma, ..DEFAULT });
    }
    // sigma: residual st.dev. of the eff model margin
    let r = run(&g, nt, &DEFAULT, false);
    let (mut s, mut c) = (0.0, 0.0);
    for (x, pr) in g.iter().zip(&r.preds) {
        if x.dayn >= from {
            s += (pr[4].m - (x.hs - x.as_)).powi(2);
            c += 1.0;
        }
    }
    println!("eff margin residual sd {:.2}", (s / c).sqrt());
    Ok(())
}

pub fn build(conn: &Connection) -> Result<String> {
    let t0 = std::time::Instant::now();
    let d = load(conn)?;
    let nt = d.teams.len();
    let p = DEFAULT;
    let r = run(&d.games, nt, &p, true);
    let games = &d.games;

    // ---- per-season backtest on games with a closing line
    let mut by_season: Vec<(i64, Vec<Acc>, Acc, Vec<Acc>, Acc)> = vec![];
    let mut series: Vec<Vec<(String, f64)>> = vec![vec![]; NM + 1]; // cumulative ATS units (edge ≥ 0), current + last season
    let mut cum = vec![0.0; NM + 1];
    let mut last_day = String::new();
    for (g, pr) in games.iter().zip(&r.preds) {
        if !g.fin || g.line.as_ref().and_then(|l| l.spread).is_none() {
            continue;
        }
        if !by_season.iter().any(|x| x.0 == g.season) {
            by_season.push((g.season, vec![Acc::default(); NM], Acc::default(), vec![Acc::default(); NM], Acc::default()));
        }
        let e = by_season.iter_mut().find(|x| x.0 == g.season).unwrap();
        for m in 0..NM {
            score(&mut e.1[m], g, pr[m], &p, false);
            score(&mut e.3[m], g, pr[m], &p, true);
        }
        if let Some(mp) = market_pred(g, false) {
            score(&mut e.2, g, mp, &p, false);
        }
        if let Some(mp) = market_pred(g, true) {
            score(&mut e.4, g, mp, &p, true);
        }
        // cumulative ATS units
        let s = g.line.as_ref().unwrap().spread.unwrap();
        let res = (g.hs - g.as_) + s;
        for m in 0..NM {
            let edge = pr[m].m + s;
            if edge != 0.0 && res != 0.0 {
                cum[m] += if (edge > 0.0) == (res > 0.0) { VIG_WIN } else { -1.0 };
            }
        }
        if g.day != last_day {
            for m in 0..NM {
                series[m].push((g.day.clone(), cum[m]));
            }
            last_day = g.day.clone();
        } else {
            for m in 0..NM {
                if let Some(x) = series[m].last_mut() {
                    x.1 = cum[m];
                }
            }
        }
    }
    let backtest: Vec<Value> = by_season
        .iter()
        .map(|(s, accs, mk, accs_o, mk_o)| {
            json!({
                "season": s,
                "label": format!("{}-{:02}", s - 1, s % 100),
                "models": accs.iter().map(|a| a.json()).collect::<Vec<_>>(),
                "market": mk.json(),
                "models_open": accs_o.iter().map(|a| a.json()).collect::<Vec<_>>(),
                "market_open": mk_o.json(),
            })
        })
        .collect();

    // ---- recency test: margin MAE vs half-life for the decayed SRS + efficiency models
    let lined_from = games.iter().filter(|g| g.line.is_some() && g.fin).map(|g| g.dayn).min().unwrap_or(0);
    let mut recency = vec![];
    for hl in [7.0, 14.0, 21.0, 30.0, 45.0, 60.0, 90.0, 150.0, f64::INFINITY] {
        let te = eff_time(games, &p);
        let (mut a1, mut a2, mut t2, mut c) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut i = 0;
        while i < games.len() {
            let mut j = i;
            while j < games.len() && games[j].dayn == games[i].dayn {
                j += 1;
            }
            if games[i].dayn >= lined_from && games[i..j].iter().any(|g| g.fin) {
                let f = fit_day(games, &te, i, te[i], games[i].season, nt, &p, hl);
                for g in &games[i..j] {
                    if !g.fin {
                        continue;
                    }
                    let (eh, ea, _) = f.eff(g);
                    a1 += (f.srs_margin(g) - (g.hs - g.as_)).abs();
                    a2 += (eh - ea - (g.hs - g.as_)).abs();
                    t2 += (eh + ea - (g.hs + g.as_)).abs();
                    c += 1.0;
                }
            }
            i = j;
        }
        recency.push(json!({ "hl": if hl.is_finite() { json!(hl) } else { json!(null) }, "srs": a1 / c.max(1.0), "eff": a2 / c.max(1.0), "eff_t": t2 / c.max(1.0), "n": c }));
    }

    // ---- per-game rows: upcoming + recent finals with lines
    let today = (chrono::Utc::now() - chrono::Duration::hours(5)).format("%Y-%m-%d").to_string();
    let tday = dayn(&today);
    let line_json = |l: &Option<Line>| match l {
        Some(l) => json!({
            "provider": l.provider, "spread": l.spread, "total": l.total, "mlh": l.mlh, "mla": l.mla,
            "spread_open": l.spread_open, "total_open": l.total_open, "mlh_open": l.mlh_open, "mla_open": l.mla_open,
        }),
        None => Value::Null,
    };
    let pred_json = |pr: &[Pred; NM]| -> Value {
        pr.iter().map(|x| json!([(x.m * 10.0).round() / 10.0, (x.t * 10.0).round() / 10.0, (win_prob(&p, x.m) * 1000.0).round() / 1000.0])).collect()
    };
    let mut upcoming = vec![];
    let mut recent = vec![];
    for (g, pr) in games.iter().zip(&r.preds) {
        if g.season != CUR_SEASON {
            continue;
        }
        let row = json!({
            "id": g.id, "date": g.date, "day": g.day, "home": d.teams[g.h].0, "away": d.teams[g.a].0,
            "hs": g.hs, "as": g.as_, "status": g.status, "line": line_json(&g.line), "p": pred_json(pr),
        });
        if !g.fin && g.dayn >= tday && g.dayn <= tday + 7 {
            upcoming.push(row);
        } else if g.fin && g.line.is_some() {
            recent.push(row);
        }
    }
    let recent: Vec<Value> = recent.into_iter().rev().take(150).collect();

    let elo: Vec<Value> = d.teams.iter().enumerate().map(|(i, t)| json!([t.0, (r.elo[i] * 10.0).round() / 10.0])).collect();
    let fit = r.fit.unwrap_or_default();
    let ratings: Vec<Value> = d
        .teams
        .iter()
        .enumerate()
        .map(|(i, t)| {
            json!({ "id": t.0, "elo": (r.elo[i]*10.0).round()/10.0, "srs": fit.m_r.get(i), "ortg": fit.e_mu + fit.e_o.get(i).copied().unwrap_or(0.0),
                    "drtg": fit.e_mu + fit.e_d.get(i).copied().unwrap_or(0.0), "pace": fit.q0 + 2.0 * fit.q.get(i).copied().unwrap_or(0.0) })
        })
        .collect();
    let seasons = [2025i64, 2026, 2027];
    let by_s: Vec<Value> = seasons
        .iter()
        .map(|s| {
            json!({
                "season": s,
                "games": games.iter().filter(|g| g.fin && g.season == *s).count(),
                "lines": games.iter().filter(|g| g.fin && g.season == *s && g.line.as_ref().and_then(|l| l.spread).is_some()).count(),
            })
        })
        .collect();
    let counts = json!({ "games": games.iter().filter(|g| g.fin).count(), "by_season": by_s });
    let out = json!({
        "models": MODELS.iter().map(|(k, n, d)| json!({"key": k, "name": n, "desc": d})).collect::<Vec<_>>(),
        "params": { "elo_k": p.elo_k, "elo_hca": p.elo_hca, "hl": p.hl, "offseason": p.offseason, "sigma": p.sigma },
        "hca": fit.m_hca,
        "counts": counts,
        "edges": EDGES,
        "backtest": backtest,
        "series": series[..NM].iter().map(|s| s.iter().map(|(d, u)| json!([d, (u * 100.0).round() / 100.0])).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "recency": recency,
        "upcoming": upcoming,
        "recent": recent,
        "ratings": ratings,
        "elo": elo,
        "ms": t0.elapsed().as_millis() as u64,
    });
    eprintln!("betting model built in {:?}", t0.elapsed());
    Ok(out.to_string())
}
