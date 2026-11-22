//! Regularized Adjusted Plus-Minus (RAPM).
//!
//! Every stint (same ten players on court) gives two observations: home offense vs away defense and
//! away offense vs home defense, in points per 100 possessions, weighted by possessions. We fit
//!   pts/100 = mu + home_court + sum(offense of the 5 attackers) + sum(defense of the 5 defenders)
//! with ridge regression, shrinking each player toward a prior (0, or a box-score estimate).
//! Players who always share the floor can only be told apart by the minutes they don't share,
//! and the ridge penalty keeps those small samples from producing wild numbers.
use std::collections::HashMap;

pub struct StintRow {
    pub game: String,
    pub poss: f64,
    pub home: Vec<String>,
    pub away: Vec<String>,
    pub hpts: f64,
    pub apts: f64,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Rapm {
    pub off: f64, // points per 100 added on offense
    pub def: f64, // points per 100 prevented on defense (positive = good)
    pub poss: f64,
}

pub struct Fit {
    pub players: HashMap<String, Rapm>,
    pub mu: f64,
    pub hca: f64,
}

/// priors: player -> (offense prior, defense prior) in the same units as the output.
pub fn fit(rows: &[StintRow], priors: &HashMap<String, (f64, f64)>, lambda: f64) -> Fit {
    let mut ix: HashMap<&str, usize> = HashMap::new();
    for r in rows {
        for p in r.home.iter().chain(r.away.iter()) {
            let n = ix.len();
            ix.entry(p.as_str()).or_insert(n);
        }
    }
    let np = ix.len();
    let m = 2 + 2 * np; // [mu, hca, off_0.., def_0..] (def = points allowed, flipped at the end)
    let mut a = vec![0.0f64; m * m];
    let mut b = vec![0.0f64; m];
    let mut add = |feat: &[usize], y: f64, w: f64| {
        for &i in feat {
            b[i] += w * y;
            for &j in feat {
                a[i * m + j] += w;
            }
        }
    };
    for r in rows {
        if r.poss <= 0.0 || r.home.len() != 5 || r.away.len() != 5 {
            continue;
        }
        let w = r.poss;
        // home offense vs away defense
        let mut f: Vec<usize> = vec![0, 1];
        f.extend(r.home.iter().map(|p| 2 + ix[p.as_str()]));
        f.extend(r.away.iter().map(|p| 2 + np + ix[p.as_str()]));
        add(&f, 100.0 * r.hpts / r.poss, w);
        // away offense vs home defense
        let mut f: Vec<usize> = vec![0];
        f.extend(r.away.iter().map(|p| 2 + ix[p.as_str()]));
        f.extend(r.home.iter().map(|p| 2 + np + ix[p.as_str()]));
        add(&f, 100.0 * r.apts / r.poss, w);
    }
    for (p, &i) in &ix {
        let (po, pd) = priors.get(*p).copied().unwrap_or((0.0, 0.0));
        a[(2 + i) * m + 2 + i] += lambda;
        b[2 + i] += lambda * po;
        a[(2 + np + i) * m + 2 + np + i] += lambda;
        b[2 + np + i] += lambda * (-pd);
    }
    // tiny ridge on mu/hca for numerical safety
    a[0] += 1e-6;
    a[m + 1] += 1e-6;
    let x = cholesky_solve(a, b, m);
    let mut poss: HashMap<&str, f64> = HashMap::new();
    for r in rows {
        for p in r.home.iter().chain(r.away.iter()) {
            *poss.entry(p.as_str()).or_default() += r.poss;
        }
    }
    let players = ix
        .iter()
        .map(|(p, &i)| (p.to_string(), Rapm { off: x[2 + i], def: -x[2 + np + i], poss: poss[p] }))
        .collect();
    Fit { players, mu: x[0], hca: x[1] }
}

fn cholesky_solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Vec<f64> {
    // in-place lower-triangular Cholesky: A = L L^T
    for j in 0..n {
        let mut s = a[j * n + j];
        for k in 0..j {
            s -= a[j * n + k] * a[j * n + k];
        }
        let d = s.max(1e-12).sqrt();
        a[j * n + j] = d;
        for i in (j + 1)..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k];
            }
            a[i * n + j] = s / d;
        }
    }
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in (i + 1)..n {
            s -= a[k * n + i] * b[k];
        }
        b[i] = s / a[i * n + i];
    }
    b
}

/// Out-of-sample error (possession-weighted RMSE in pts/100) with k-fold cross-validation by game.
pub fn cross_validate(rows: &[StintRow], priors: &HashMap<String, (f64, f64)>, lambda: f64, k: usize) -> f64 {
    let fold = |g: &str| g.bytes().fold(7u64, |h, c| h.wrapping_mul(31).wrapping_add(c as u64)) as usize % k;
    let (mut se, mut w) = (0.0, 0.0);
    for f in 0..k {
        let train: Vec<&StintRow> = rows.iter().filter(|r| fold(&r.game) != f).collect();
        let train: Vec<StintRow> = train
            .into_iter()
            .map(|r| StintRow { game: r.game.clone(), poss: r.poss, home: r.home.clone(), away: r.away.clone(), hpts: r.hpts, apts: r.apts })
            .collect();
        let fit = fit(&train, priors, lambda);
        let val = |p: &String| {
            fit.players.get(p).copied().unwrap_or_else(|| {
                let (o, d) = priors.get(p).copied().unwrap_or((0.0, 0.0));
                Rapm { off: o, def: d, poss: 0.0 }
            })
        };
        for r in rows.iter().filter(|r| fold(&r.game) == f && r.poss > 0.0) {
            let ho: f64 = r.home.iter().map(|p| val(p).off).sum();
            let hd: f64 = r.home.iter().map(|p| val(p).def).sum();
            let ao: f64 = r.away.iter().map(|p| val(p).off).sum();
            let ad: f64 = r.away.iter().map(|p| val(p).def).sum();
            let ph = fit.mu + fit.hca + ho - ad;
            let pa = fit.mu + ao - hd;
            se += r.poss * ((100.0 * r.hpts / r.poss - ph).powi(2) + (100.0 * r.apts / r.poss - pa).powi(2));
            w += 2.0 * r.poss;
        }
    }
    (se / w).sqrt()
}
