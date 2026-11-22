//! On/off-court tracking from ESPN play-by-play.
//! Rebuilds who is on the floor from starters + substitution events, then credits each player
//! with seconds played and points scored/allowed while on the court.
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Default, Debug, Clone)]
pub struct OnOff {
    pub team: String,
    pub sec: f64,
    pub pf: f64, // team points while on court
    pub pa: f64, // opponent points while on court
}

/// A stretch of play with the same ten players on the floor.
#[derive(Default, Debug, Clone)]
pub struct Stint {
    pub home: Vec<String>,
    pub away: Vec<String>,
    pub sec: f64,
    pub hpts: f64,
    pub apts: f64,
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        o => o.to_string(),
    }
}

pub fn clock_secs(c: &str) -> f64 {
    match c.split_once(':') {
        Some((m, sec)) => m.parse::<f64>().unwrap_or(0.0) * 60.0 + sec.parse::<f64>().unwrap_or(0.0),
        None => c.parse::<f64>().unwrap_or(0.0),
    }
}
pub fn period_len(p: i64) -> f64 {
    if p <= 4 { 720.0 } else { 300.0 }
}
pub fn period_start(p: i64) -> f64 {
    if p <= 4 { (p - 1) as f64 * 720.0 } else { 2880.0 + (p - 5) as f64 * 300.0 }
}

struct Ev {
    period: i64,
    t: f64, // seconds elapsed in game
    sub: bool,
    team: String,
    parts: Vec<String>,
    pts: f64, // points scored on this play (by `team`)
}

/// Returns per-player on-court totals and the list of stints for one game
/// (both empty if there is no usable play-by-play).
pub fn compute(v: &Value) -> (HashMap<String, OnOff>, Vec<Stint>) {
    let mut out: HashMap<String, OnOff> = HashMap::new();
    let mut stints: Vec<Stint> = vec![];
    let comp = &v["header"]["competitions"][0];
    let mut home_id = String::new();
    for t in comp["competitors"].as_array().cloned().unwrap_or_default() {
        if s(&t["homeAway"]) == "home" {
            home_id = s(&t["team"]["id"]);
        }
    }
    // player -> team, starters, minutes
    let mut team_of: HashMap<String, String> = HashMap::new();
    let mut starters: HashMap<String, Vec<String>> = HashMap::new();
    let mut minutes: HashMap<String, f64> = HashMap::new();
    let mut teams: Vec<String> = vec![];
    for pt in v["boxscore"]["players"].as_array().cloned().unwrap_or_default() {
        let tid = s(&pt["team"]["id"]);
        teams.push(tid.clone());
        let block = &pt["statistics"][0];
        let keys: Vec<String> = block["keys"].as_array().map(|a| a.iter().map(s).collect()).unwrap_or_default();
        let mi = keys.iter().position(|k| k == "minutes");
        for a in block["athletes"].as_array().cloned().unwrap_or_default() {
            let pid = s(&a["athlete"]["id"]);
            team_of.insert(pid.clone(), tid.clone());
            let m = mi.and_then(|i| a["stats"][i].as_str()).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
            minutes.insert(pid.clone(), m);
            if a["starter"].as_bool().unwrap_or(false) {
                starters.entry(tid.clone()).or_default().push(pid);
            }
        }
    }
    if teams.len() != 2 || home_id.is_empty() {
        return (out, stints);
    }
    let mut evs: Vec<(i64, Ev)> = vec![];
    for p in v["plays"].as_array().cloned().unwrap_or_default() {
        let period = p["period"]["number"].as_i64().unwrap_or(0);
        if period < 1 {
            continue;
        }
        let seq = s(&p["sequenceNumber"]).parse::<i64>().unwrap_or(0);
        let clock = clock_secs(&s(&p["clock"]["displayValue"]));
        let t = period_start(period) + (period_len(period) - clock).clamp(0.0, period_len(period));
        evs.push((
            seq,
            Ev {
                period,
                t,
                sub: s(&p["type"]["id"]) == "584" || s(&p["type"]["text"]) == "Substitution",
                team: s(&p["team"]["id"]),
                parts: p["participants"]
                    .as_array()
                    .map(|a| a.iter().map(|x| s(&x["athlete"]["id"])).collect())
                    .unwrap_or_default(),
                // ESPN's running score is occasionally wrong; the per-play scoreValue is reliable.
                pts: if p["scoringPlay"].as_bool().unwrap_or(false) { p["scoreValue"].as_f64().unwrap_or(0.0) } else { 0.0 },
            },
        ));
    }
    if evs.len() < 50 {
        return (out, stints);
    }
    // ESPN sometimes logs substitutions late (higher sequence number than plays after them),
    // so order by game clock first and use the sequence number only to break ties.
    evs.sort_by(|a, b| a.1.t.partial_cmp(&b.1.t).unwrap().then(a.0.cmp(&b.0)));
    let evs: Vec<Ev> = evs.into_iter().map(|e| e.1).collect();
    let nper = evs.iter().map(|e| e.period).max().unwrap_or(4).max(4);

    let mut prev_end: HashMap<String, Vec<String>> = HashMap::new();
    for per in 1..=nper {
        let pe: Vec<&Ev> = evs.iter().filter(|e| e.period == per).collect();
        // ---- infer who started the period ----
        let mut on: HashMap<String, Vec<String>> = HashMap::new();
        for tid in &teams {
            let st = starters.get(tid).cloned().unwrap_or_default();
            if per == 1 && st.len() == 5 {
                on.insert(tid.clone(), st);
                continue;
            }
            let mut entered: HashSet<String> = HashSet::new();
            let mut strong: Vec<String> = vec![]; // subbed out before entering = surely started
            let mut weak: Vec<String> = vec![]; // appeared in a play before entering
            for e in &pe {
                if e.sub && e.parts.len() >= 2 {
                    let (inn, outp) = (&e.parts[0], &e.parts[1]);
                    if team_of.get(outp) == Some(tid) && !entered.contains(outp) && !strong.contains(outp) {
                        strong.push(outp.clone());
                    }
                    if team_of.get(inn) == Some(tid) {
                        entered.insert(inn.clone());
                    }
                } else {
                    for a in &e.parts {
                        if team_of.get(a) == Some(tid) && !entered.contains(a) && !strong.contains(a) && !weak.contains(a) {
                            weak.push(a.clone());
                        }
                    }
                }
            }
            let mut lineup: Vec<String> = strong;
            for w in weak {
                if lineup.len() < 5 && !lineup.contains(&w) {
                    lineup.push(w);
                }
            }
            // fill gaps: previous period's finishers who weren't subbed in this period, then by minutes
            if lineup.len() < 5 {
                for p in prev_end.get(tid).cloned().unwrap_or_default() {
                    if lineup.len() < 5 && !lineup.contains(&p) && !entered.contains(&p) {
                        lineup.push(p);
                    }
                }
            }
            if lineup.len() < 5 {
                let mut cands: Vec<(&String, &f64)> =
                    minutes.iter().filter(|(p, _)| team_of.get(*p) == Some(tid)).collect();
                cands.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
                for (p, _) in cands {
                    if lineup.len() < 5 && !lineup.contains(p) && !entered.contains(p) {
                        lineup.push(p.clone());
                    }
                }
            }
            lineup.truncate(5);
            on.insert(tid.clone(), lineup);
        }
        // ---- walk the period ----
        let mut t_prev = period_start(per);
        let credit_time = |on: &HashMap<String, Vec<String>>, out: &mut HashMap<String, OnOff>, dt: f64| {
            for (tid, ps) in on {
                for p in ps {
                    let o = out.entry(p.clone()).or_default();
                    o.team = tid.clone();
                    o.sec += dt;
                }
            }
        };
        let away_id = teams.iter().find(|t| **t != home_id).cloned().unwrap_or_default();
        let mut cur = Stint::default();
        let flush = |on: &HashMap<String, Vec<String>>, cur: &mut Stint, stints: &mut Vec<Stint>| {
            if cur.sec > 0.0 || cur.hpts != 0.0 || cur.apts != 0.0 {
                let mut h = on.get(&home_id).cloned().unwrap_or_default();
                let mut a = on.get(&away_id).cloned().unwrap_or_default();
                h.sort();
                a.sort();
                stints.push(Stint { home: h, away: a, ..cur.clone() });
            }
            *cur = Stint::default();
        };
        for e in pe {
            if e.t > t_prev {
                credit_time(&on, &mut out, e.t - t_prev);
                cur.sec += e.t - t_prev;
                t_prev = e.t;
            }
            if e.pts != 0.0 {
                if e.team == home_id { cur.hpts += e.pts } else { cur.apts += e.pts }
                for (tid, ps) in &on {
                    let (f, a) = if *tid == e.team { (e.pts, 0.0) } else { (0.0, e.pts) };
                    for p in ps {
                        let o = out.entry(p.clone()).or_default();
                        o.team = tid.clone();
                        o.pf += f;
                        o.pa += a;
                    }
                }
            }
            if e.sub && e.parts.len() >= 2 {
                let (inn, outp) = (&e.parts[0], &e.parts[1]);
                let tid = team_of.get(outp).or_else(|| team_of.get(inn)).cloned().unwrap_or_else(|| e.team.clone());
                flush(&on, &mut cur, &mut stints);
                if let Some(l) = on.get_mut(&tid) {
                    l.retain(|x| x != outp);
                    if !l.contains(inn) {
                        l.push(inn.clone());
                    }
                }
            }
        }
        let end = period_start(per) + period_len(per);
        if end > t_prev {
            credit_time(&on, &mut out, end - t_prev);
            cur.sec += end - t_prev;
        }
        flush(&on, &mut cur, &mut stints);
        prev_end = on;
    }
    (out, stints)
}

#[cfg(test)]
mod tests {
    #[test]
    fn debug_file() {
        let Ok(p) = std::env::var("ONOFF_FILE") else { return };
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let (r, st) = super::compute(&v);
        eprintln!("stints {} sec {} pts {} {}", st.len(), st.iter().map(|x| x.sec).sum::<f64>(), st.iter().map(|x| x.hpts).sum::<f64>(), st.iter().map(|x| x.apts).sum::<f64>());
        let mut by: std::collections::HashMap<String, (f64, f64)> = Default::default();
        for o in r.values() {
            let e = by.entry(o.team.clone()).or_default();
            e.0 += o.sec / 60.0;
            e.1 += o.pf / 5.0;
        }
        eprintln!("{by:?}");
    }
}
