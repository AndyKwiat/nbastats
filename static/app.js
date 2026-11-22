'use strict';
// ---------- utilities ----------
const $ = (s, el = document) => el.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const api = p => fetch('/api/' + p).then(r => { if (!r.ok) throw new Error(r.status); return r.json(); });
const app = $('#app');
const tip = $('#tip');
let D = {}, T = {}, P = {}, ELO = null;
const HOVER = matchMedia('(hover: hover)').matches;
const NOHS = 'https://a.espncdn.com/combiner/i?img=/i/headshots/nophoto.png&w=96&h=70';

function cdn(url, w, h) {
  if (!url) return '';
  const m = url.match(/^https:\/\/a\.espncdn\.com(\/i\/.*)$/);
  return m ? `https://a.espncdn.com/combiner/i?img=${m[1]}&w=${w}&h=${h}` : url;
}
const hsUrl = (p, w = 96) => cdn(p?.img || `https://a.espncdn.com/i/headshots/nba/players/full/${p?.id}.png`, w, Math.round(w * 0.727));
const hsImg = (p, w = 96, cls = 'hs') => `<img class="${cls}" loading="lazy" src="${hsUrl(p, w)}" alt="" onerror="this.onerror=null;this.src='${NOHS}'">`;
const logoUrl = (t, s = 64) => t ? cdn(t.logo, s, s) : '';
const logoImg = (t, s = 64, cls = 'logo') => t ? `<img class="${cls}" loading="lazy" src="${logoUrl(t, s)}" alt="${esc(t.abbr)}">` : '';
const teamLink = (t, full) => t ? `<a class="tlink" href="#/team/${t.id}">${logoImg(t, 44)}<span>${esc(full ? t.display : t.abbr)}</span></a>` : '';

function fmt(v, f) {
  if (v == null || (typeof v === 'number' && !isFinite(v))) return '–';
  if (typeof v === 'string') return esc(v);
  if (f === 'p') return (v * 100).toFixed(1);
  if (f === 'p3') return v.toFixed(3).replace(/^0/, '');
  if (f === 's1') return (v > 0 ? '+' : '') + v.toFixed(1);
  if (f === 's2') return (v > 0 ? '+' : '') + v.toFixed(2);
  if (f === 's0') return (v > 0 ? '+' : '') + v.toFixed(0);
  if (typeof f === 'number') return v.toFixed(f);
  return String(v);
}
const sgnCls = v => v > 0 ? 'pos' : v < 0 ? 'neg' : '';
const dayLabel = (d, opts = { weekday: 'short', month: 'short', day: 'numeric' }) => new Date(d + 'T12:00:00').toLocaleDateString('en-US', opts);
function lum(hex) {
  const n = parseInt(hex || '777777', 16), r = (n >> 16) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
  const f = c => c <= .03928 ? c / 12.92 : Math.pow((c + .055) / 1.055, 2.4);
  return .2126 * f(r) + .7152 * f(g) + .0722 * f(b);
}
const isDark = () => matchMedia('(prefers-color-scheme: dark)').matches;
function tcolor(t) {
  if (!t) return '#888';
  let c = t.color || '777777', a = t.alt || c;
  if (isDark()) { if (lum(c) < 0.06 && lum(a) > lum(c)) c = a; if (lum(c) < 0.04) c = '9aa4b5'; }
  else { if (lum(c) > 0.7 && lum(a) < lum(c)) c = a; }
  return '#' + c;
}
const debounce = (f, ms = 120) => { let h; return (...a) => { clearTimeout(h); h = setTimeout(() => f(...a), ms); }; };

// ---------- tooltip ----------
function showTip(html, x, y) {
  tip.innerHTML = html; tip.hidden = false;
  const r = tip.getBoundingClientRect();
  let left = x + 14, top = y + 14;
  if (left + r.width > innerWidth - 8) left = x - r.width - 14;
  if (left < 8) left = 8;
  if (top + r.height > innerHeight - 8) top = y - r.height - 14;
  tip.style.left = left + 'px'; tip.style.top = Math.max(8, top) + 'px';
}
const hideTip = () => { tip.hidden = true; };
addEventListener('scroll', hideTip, { passive: true });
const tipRows = rows => rows.map(([k, v]) => `<div class="r"><span>${k}</span><span>${v}</span></div>`).join('');

// ---------- sortable table ----------
function table(el, o) {
  const st = { key: o.key, desc: o.desc !== false };
  const wrap = document.createElement('div');
  wrap.className = 'tw';
  el.innerHTML = ''; el.appendChild(wrap);
  let rows = o.rows;
  const colv = c => c.v || (r => r[c.k]);
  function draw() {
    const col = o.cols.find(c => c.k === st.key);
    let sorted = rows;
    if (col) {
      const v = colv(col);
      sorted = rows.slice().sort((a, b) => {
        const x = v(a), y = v(b);
        const xn = x == null || (typeof x === 'number' && !isFinite(x)), yn = y == null || (typeof y === 'number' && !isFinite(y));
        if (xn && yn) return 0; if (xn) return 1; if (yn) return -1;
        if (typeof x === 'string') return st.desc ? String(y).localeCompare(x) : x.localeCompare(String(y));
        return st.desc ? y - x : x - y;
      });
    }
    if (o.limit) sorted = sorted.slice(0, o.limit);
    const th = o.cols.map(c => `<th data-k="${c.k}" class="${c.cls || 'n'} ${c.k === st.key ? 'sorted' + (st.desc ? '' : ' asc') : ''}" title="${esc(c.t || '')}">${c.l}</th>`).join('');
    const body = sorted.map((r, i) => `<tr class="${o.rowCls ? o.rowCls(r, i) : ''}">${o.rank === false ? '' : `<td class="rk">${i + 1}</td>`}${o.cols.map(c => {
      const v = colv(c)(r);
      return `<td class="${c.cls || 'n'} ${c.k === st.key ? 'sorted' : ''} ${c.sc ? sgnCls(v) : ''}">${c.h ? c.h(r) : fmt(v, c.f)}</td>`;
    }).join('')}</tr>`).join('');
    const foot = o.foot ? `<tfoot>${o.foot}</tfoot>` : '';
    wrap.innerHTML = `<table class="t"><thead><tr>${o.rank === false ? '' : '<th class="n">#</th>'}${th}</tr></thead><tbody>${body || `<tr><td colspan="${o.cols.length + 1}" class="l muted">No rows</td></tr>`}</tbody>${foot}</table>`;
  }
  wrap.addEventListener('click', e => {
    const h = e.target.closest('th[data-k]'); if (!h) return;
    const k = h.dataset.k, c = o.cols.find(c => c.k === k);
    if (st.key === k) st.desc = !st.desc; else { st.key = k; st.desc = !c.asc; }
    draw(); o.onSort && o.onSort(st);
  });
  draw();
  return { set(r) { rows = r; draw(); }, st };
}

// ---------- data ----------
const COUNT = ['pts', 'fgm', 'fga', 'tpm', 'tpa', 'ftm', 'fta', 'oreb', 'dreb', 'reb', 'ast', 'stl', 'blk', 'tov', 'pf'];
async function load() {
  const [league, teams, players, games] = await Promise.all([api('league'), api('teams'), api('players'), api('games')]);
  D = { league, teams, players, games };
  T = {}; P = {};
  teams.forEach(t => { T[t.id] = t; t.tc = tcolor(t); });
  players.forEach(p => {
    P[p.id] = p;
    p.mpg = p.min / p.gp;
    for (const k of COUNT) { p[k] = p['t_' + k] / p.gp; p['p36_' + k] = p.min ? p['t_' + k] * 36 / p.min : null; }
    p.t = T[p.team];
    p.tgp = p.t ? p.t.gp : p.gp;
    p.qual = p.gp >= Math.max(1, 0.5 * p.tgp) && p.mpg >= 12;
    // on/off needs real samples both ways: 250+ on-court and 100+ off-court minutes
    p.qoo = p.qual && p.oo_net != null && p.on_min >= 250 && p.off_min >= 100;
  });
  const up = league.updated ? new Date(league.updated) : null;
  $('#foot').innerHTML = `Data: ESPN public API · ${league.games_played} games · updated ${up ? up.toLocaleString() : '—'} · Advanced stats computed from box scores (Basketball-Reference formulas).
    <button class="btn" id="refresh" style="margin-left:8px;padding:3px 9px;font-size:12px">Refresh data</button>`;
  $('#refresh').onclick = async () => {
    const r = await fetch('/api/refresh', { method: 'POST' }).then(r => r.json());
    $('#refresh').textContent = r.started ? 'Refreshing… reload in ~30s' : 'Already refreshing';
  };
}
const teamGames = tid => D.games.filter(g => g.home === tid || g.away === tid);
const finals = () => D.games.filter(g => g.status === 'STATUS_FINAL');

// ---------- common columns ----------
const cPlayer = { k: 'name', l: 'Player', cls: 'l stick', asc: true, v: r => r.name,
  h: r => `<a class="pl" href="#/player/${r.id}">${hsImg(r, 68)}<span><span class="nm">${esc(r.name)}</span> <span class="tm">${esc(r.t?.abbr || '')}</span></span></a>` };
const cTeam = { k: 'display', l: 'Team', cls: 'l stick', asc: true, v: r => r.display, h: r => teamLink(r, true) };

// ---------- router ----------
const routes = [
  [/^#\/?$/, home], [/^#\/standings/, standings], [/^#\/leaders/, leaders], [/^#\/players(?:\?(.*))?$/, players],
  [/^#\/player\/(\w+)/, player], [/^#\/teams(?:\?(.*))?$/, teams], [/^#\/team\/(\w+)/, team],
  [/^#\/games(?:\/([\d-]+))?/, games], [/^#\/game\/(\w+)/, game], [/^#\/rankings/, rankings], [/^#\/charts/, charts], [/^#\/betting/, betting],
];
let cleanup = [];
async function route() {
  hideTip();
  cleanup.forEach(f => f()); cleanup = [];
  const h = location.hash || '#/';
  document.querySelectorAll('#nav a').forEach(a => {
    const href = a.getAttribute('href');
    a.classList.toggle('on', href === '#/' ? (h === '#/' || h === '#') : h.startsWith(href.replace(/s$/, '')));
  });
  for (const [re, fn] of routes) {
    const m = h.match(re);
    if (m) {
      app.innerHTML = '<div class="loading">Loading…</div>';
      try { await fn(...m.slice(1)); } catch (e) { console.error(e); app.innerHTML = `<div class="loading">Error: ${esc(e.message)}</div>`; }
      if (!h.includes('?')) scrollTo(0, 0);
      return;
    }
  }
  app.innerHTML = '<div class="loading">Not found</div>';
}
const qs = s => Object.fromEntries(new URLSearchParams(s || ''));

// ---------- HOME ----------
function gameCard(g) {
  const h = T[g.home], a = T[g.away];
  const fin = g.status === 'STATUS_FINAL';
  const row = (t, s, win, top) => `<div class="tr ${fin && !win ? 'lose' : ''}">${logoImg(t, 60)}<div class="nm">${esc(t?.location || '')} <small>${esc(t?.name || '')}</small>${top ? `<div class="sub">${esc(top[1])} ${top[2]} pts · ${top[3]} reb · ${top[4]} ast</div>` : ''}</div><div class="s">${fin ? s : ''}</div></div>`;
  let ft;
  if (fin) ft = `<span>Final${g.ot ? ' / ' + (g.ot > 1 ? g.ot : '') + 'OT' : ''}</span><span>${esc(g.venue)}</span>`;
  else {
    const t = new Date(g.date).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    const fav = g.p_home >= .5 ? h : a, p = g.p_home >= .5 ? g.p_home : 1 - g.p_home;
    ft = `<span>${t}</span><span>Elo: ${esc(fav?.abbr)} ${(p * 100).toFixed(0)}% · line ${esc(g.spread > 0 ? h?.abbr : a?.abbr)} -${Math.abs(g.spread ?? 0).toFixed(1)}</span>`;
  }
  return `<a class="card gc" href="#/game/${g.id}">${row(a, g.as, g.as > g.hs, g.at)}${row(h, g.hs, g.hs > g.as, g.ht)}<div class="ft">${ft}</div></a>`;
}

async function home() {
  const L = D.league;
  const f = finals();
  const lastDay = f.length ? f[f.length - 1].day : null;
  const recent = f.filter(g => g.day === lastDay).reverse();
  const tiles = [['Games', L.games_played], ['Pace', fmt(L.pace, 1)], ['Off Rtg', fmt(L.ortg, 1)], ['PPG', fmt(L.ppg, 1)], ['TS%', fmt(L.ts, 'p')], ['3PA rate', fmt(L.tpar, 'p')]];
  app.innerHTML = `
    <div class="pagehead"><div><h1>2026-27 Regular Season</h1><p>Every game, every box score, advanced stats and power ratings.</p></div></div>
    <div class="tiles" style="margin:0 0 16px">${tiles.map(([k, v]) => `<div class="tile"><div class="k">${k}</div><div class="v">${v}</div><div class="r">league avg</div></div>`).join('')}</div>
    <div class="grid leaders-grid" id="hl"></div>
    <div class="grid g2 mt">
      <div class="card"><div class="hd"><h2>Latest · ${lastDay ? dayLabel(lastDay) : ''}</h2><a class="sub" href="#/games">All games →</a></div><div class="grid g2" id="recent"></div></div>
      <div class="card flush"><div class="hd"><h2>Power Rankings</h2><a class="sub" href="#/rankings">Full table →</a></div><div id="pr"></div></div>
    </div>
    <div class="card mt"><div class="hd"><h2>Offense vs Defense</h2><a class="sub" href="#/charts">More charts →</a></div><div id="c1"></div></div>
    <div class="grid g2 mt"><div class="card flush"><div class="hd"><h2>East</h2></div><div id="se"></div></div><div class="card flush"><div class="hd"><h2>West</h2></div><div id="sw"></div></div></div>`;
  $('#hl').innerHTML = ['pts', 'reb', 'ast', 'per'].map(k => leaderCard(CATS.find(c => c.k === k), 'pg', 5)).join('');
  $('#recent').innerHTML = recent.slice(0, 8).map(gameCard).join('');
  table($('#pr'), {
    rows: D.teams, key: 'power_rank', desc: false, limit: 10, rank: false, cols: [
      { k: 'power_rank', l: 'Rk', h: r => `<span class="pr">${r.power_rank}</span>` }, cTeam,
      { k: 'w', l: 'W-L', v: r => r.pct, h: r => `${r.w}-${r.l}` }, { k: 'net', l: 'Net', f: 's1', sc: 1 }, { k: 'elo', l: 'Elo', f: 0 },
      { k: 'form', l: 'L10', cls: 'n', v: r => r.form.reduce((a, b) => a + b, 0), h: r => formDots(r.form) }]
  });
  standingsTable($('#se'), 'East', true); standingsTable($('#sw'), 'West', true);
  chartOD($('#c1'));
}

// ---------- STANDINGS ----------
const formDots = f => `<span class="form">${f.map(x => `<i class="${x ? 'w' : ''}"></i>`).join('')}</span>`;
function standingsTable(el, conf, compact) {
  const rows = D.teams.filter(t => !conf || t.conf === conf).sort((a, b) => b.pct - a.pct || (b.w - b.l) - (a.w - a.l) || b.net - a.net);
  const lead = rows[0];
  rows.forEach((t, i) => { t.gb = ((lead.w - t.w) + (t.l - lead.l)) / 2; t.seed = i + 1; });
  const cols = compact ? [cTeam, { k: 'w', l: 'W' }, { k: 'l', l: 'L' }, { k: 'pct', l: 'Pct', f: 'p3' }, { k: 'gb', l: 'GB', h: r => r.gb ? r.gb.toFixed(1) : '–' }, { k: 'diff', l: 'Diff', f: 's1', sc: 1 }, { k: 'streak', l: 'Strk', cls: 'n' }]
    : [cTeam, { k: 'w', l: 'W' }, { k: 'l', l: 'L' }, { k: 'pct', l: 'Pct', f: 'p3' }, { k: 'gb', l: 'GB', h: r => r.gb ? r.gb.toFixed(1) : '–' },
      { k: 'home', l: 'Home', cls: 'n', v: r => +r.home.split('-')[0] }, { k: 'away', l: 'Away', cls: 'n', v: r => +r.away.split('-')[0] },
      { k: 'confrec', l: 'Conf', cls: 'n', v: r => +r.confrec.split('-')[0] }, { k: 'divrec', l: 'Div', cls: 'n', v: r => +r.divrec.split('-')[0] },
      { k: 'close', l: '≤5 pts', t: 'Record in games decided by 5 or fewer', v: r => +r.close.split('-')[0] },
      { k: 'ppg', l: 'PPG', f: 1 }, { k: 'oppg', l: 'Opp', f: 1 }, { k: 'diff', l: 'Diff', f: 's1', sc: 1 },
      { k: 'pyth_w', l: 'xW', f: 1, t: 'Pythagorean expected wins' }, { k: 'streak', l: 'Strk', v: r => (r.streak[0] === 'W' ? 1 : -1) * +r.streak.slice(1) , h: r => r.streak },
      { k: 'l10', l: 'L10', v: r => r.form.reduce((a, b) => a + b, 0), h: r => `${r.l10} ${formDots(r.form)}` }];
  table(el, { rows, key: 'pct', cols, rowCls: (r, i) => !conf ? '' : (i === 5 || i === 9) ? 'sep' : '' });
}
async function standings() {
  app.innerHTML = `<div class="pagehead"><div><h1>Standings</h1><p>Dashed lines: top 6 (playoffs) and 7–10 (play-in).</p></div>
    <div class="seg" id="mode"><button data-m="conf" class="on">Conference</button><button data-m="div">Division</button><button data-m="league">League</button></div></div><div id="st"></div>`;
  const draw = m => {
    $('#mode').querySelectorAll('button').forEach(b => b.classList.toggle('on', b.dataset.m === m));
    const st = $('#st');
    if (m === 'conf') { st.innerHTML = `<div class="card flush"><div class="hd"><h2>Eastern Conference</h2></div><div id="e"></div></div><div class="card flush mt"><div class="hd"><h2>Western Conference</h2></div><div id="w"></div></div>`; standingsTable($('#e'), 'East'); standingsTable($('#w'), 'West'); }
    else if (m === 'league') { st.innerHTML = `<div class="card flush"><div id="a"></div></div>`; standingsTable($('#a'), null); }
    else {
      const divs = ['Atlantic', 'Central', 'Southeast', 'Northwest', 'Pacific', 'Southwest'];
      st.innerHTML = `<div class="grid g2">${divs.map(d => `<div class="card flush"><div class="hd"><h2>${d}</h2></div><div id="d-${d}"></div></div>`).join('')}</div>`;
      divs.forEach(d => {
        const rows = D.teams.filter(t => t.div === d).sort((a, b) => b.pct - a.pct);
        rows.forEach(t => t.gb = ((rows[0].w - t.w) + (t.l - rows[0].l)) / 2);
        table($('#d-' + d), { rows, key: 'pct', cols: [cTeam, { k: 'w', l: 'W' }, { k: 'l', l: 'L' }, { k: 'pct', l: 'Pct', f: 'p3' }, { k: 'gb', l: 'GB', h: r => r.gb ? r.gb.toFixed(1) : '–' }, { k: 'divrec', l: 'Div', v: r => +r.divrec.split('-')[0] }, { k: 'net', l: 'Net', f: 's1', sc: 1 }] });
      });
    }
  };
  $('#mode').onclick = e => { const b = e.target.closest('button'); if (b) draw(b.dataset.m); };
  draw('conf');
}

// ---------- LEADERS ----------
const qFG = p => p.qual && p.t_fgm >= 3 * p.tgp;
const q3P = p => p.qual && p.t_tpm >= 1 * p.tgp;
const qFT = p => p.qual && p.t_ftm >= 1.2 * p.tgp;
const CATS = [
  { k: 'pts', l: 'Points', cnt: 1 }, { k: 'reb', l: 'Rebounds', cnt: 1 }, { k: 'ast', l: 'Assists', cnt: 1 },
  { k: 'stl', l: 'Steals', cnt: 1 }, { k: 'blk', l: 'Blocks', cnt: 1 }, { k: 'tpm', l: '3-Pointers Made', cnt: 1 },
  { k: 'per', l: 'PER', f: 1, q: p => p.qual, d: 'Player Efficiency Rating (league avg = 15)' },
  { k: 'ws', l: 'Win Shares', f: 1, d: 'Estimated wins contributed' },
  { k: 'bpm', l: 'Box Plus/Minus', f: 's1', q: p => p.qual, d: 'Points per 100 poss. above average (BPM 1.0 estimate)' },
  { k: 'vorp', l: 'VORP', f: 2, d: 'Value over replacement player' },
  { k: 'ws48', l: 'WS / 48', f: 'p3', q: p => p.qual },
  { k: 'rapm', l: 'RAPM', f: 's1', q: p => p.qual && p.rapm != null, d: 'Adjusted plus-minus: on-court impact controlling for teammates and opponents' },
  { k: 'rapm_o', l: 'Offensive RAPM', f: 's1', q: p => p.qual && p.rapm != null },
  { k: 'rapm_d', l: 'Defensive RAPM', f: 's1', q: p => p.qual && p.rapm != null },
  { k: 'oo_net', l: 'On-Off Net Rating', f: 's1', q: p => p.qoo, d: 'Team net rating with him on court minus with him off' },
  { k: 'oo_ortg', l: 'On-Off Off. Rating', f: 's1', q: p => p.qoo },
  { k: 'oo_drtg', l: 'On-Off Def. Rating', f: 's1', q: p => p.qoo, asc: 1 },
  { k: 'on_net', l: 'On-Court Net Rating', f: 's1', q: p => p.qoo },
  { k: 'on_ortg', l: 'On-Court Off. Rating', f: 1, q: p => p.qoo },
  { k: 'on_drtg', l: 'On-Court Def. Rating', f: 1, q: p => p.qoo, asc: 1 },
  { k: 'ts', l: 'True Shooting %', f: 'p', q: qFG },
  { k: 'fgp', l: 'Field Goal %', f: 'p', q: qFG }, { k: 'tpp', l: '3-Point %', f: 'p', q: q3P }, { k: 'ftp', l: 'Free Throw %', f: 'p', q: qFT },
  { k: 'efg', l: 'Effective FG %', f: 'p', q: qFG },
  { k: 'usg', l: 'Usage %', f: 1, q: p => p.qual }, { k: 'ortg', l: 'Offensive Rating', f: 1, q: p => p.qual && p.usg >= 15 },
  { k: 'drtg', l: 'Defensive Rating', f: 1, q: p => p.qual, asc: 1 },
  { k: 'gmsc', l: 'Game Score', f: 1, q: p => p.qual }, { k: 'pm_g', l: 'Plus/Minus per game', f: 's1', q: p => p.qual },
  { k: 'astp', l: 'Assist %', f: 1, q: p => p.qual }, { k: 'trbp', l: 'Rebound %', f: 1, q: p => p.qual },
  { k: 'stlp', l: 'Steal %', f: 1, q: p => p.qual }, { k: 'blkp', l: 'Block %', f: 1, q: p => p.qual },
  { k: 'mpg', l: 'Minutes', f: 1, q: p => p.qual }, { k: 'dd', l: 'Double-Doubles', f: 0 }, { k: 'td', l: 'Triple-Doubles', f: 0 },
  { k: 'hi_pts', l: 'Season High · Points', f: 0 },
];
function catKey(c, mode) { return !c.cnt ? c.k : mode === 'tot' ? 't_' + c.k : mode === 'p36' ? 'p36_' + c.k : c.k; }
function catRows(c, mode) {
  const k = catKey(c, mode);
  let rows = D.players.filter(p => p[k] != null && isFinite(p[k]));
  if (c.cnt && mode === 'pg') rows = rows.filter(p => p.qual);
  else if (c.cnt && mode === 'p36') rows = rows.filter(p => p.min >= 150 && p.qual);
  else if (c.q) rows = rows.filter(c.q);
  return rows.sort((a, b) => c.asc ? a[k] - b[k] : b[k] - a[k]);
}
function leaderCard(c, mode, n = 10) {
  const k = catKey(c, mode);
  const rows = catRows(c, mode).slice(0, n);
  if (!rows.length) return '';
  const f = c.cnt ? (mode === 'tot' ? 0 : 1) : c.f;
  const top = rows[0];
  return `<div class="card lc" style="--tc:${top.t?.tc || '#445'}">
    <a class="lead" href="#/player/${top.id}">${hsImg(top, 260, '')}<div class="info"><div class="cat">${c.l}${c.cnt && mode === 'p36' ? ' / 36' : ''}</div>
      <div class="big">${fmt(top[k], f)}</div><div class="who">${esc(top.name)} <small>${esc(top.t?.abbr || '')}</small></div></div></a>
    <ol>${rows.slice(1).map((p, i) => `<li><a href="#/player/${p.id}"><span class="i">${i + 2}</span>${hsImg(p, 60)}<span class="nm">${esc(p.name)}<small>${esc(p.t?.abbr || '')}</small></span><span class="v">${fmt(p[k], f)}</span></a></li>`).join('')}</ol>
    <a class="more" href="#/players?sort=${k}${c.asc ? '&asc=1' : ''}&mode=${['pts', 'reb', 'ast', 'stl', 'blk', 'tpm'].includes(c.k) ? mode : /^(oo_|on_|rapm)/.test(c.k) ? 'oo' : 'adv'}">Full list →</a></div>`;
}
async function leaders() {
  app.innerHTML = `<div class="pagehead"><div><h1>League Leaders</h1><p>Qualified: played ≥ 50% of team games and ≥ 12 min per game. Shooting % require minimum makes. On/off ratings need 250+ minutes on court and 100+ off. RAPM adjusts for teammates and opponents.</p></div>
    <div class="seg" id="mode"><button data-m="pg" class="on">Per Game</button><button data-m="tot">Totals</button><button data-m="p36">Per 36</button></div></div>
    <div class="grid leaders-grid" id="lg"></div>`;
  const draw = m => {
    $('#mode').querySelectorAll('button').forEach(b => b.classList.toggle('on', b.dataset.m === m));
    $('#lg').innerHTML = CATS.map(c => leaderCard(c, m)).join('');
  };
  $('#mode').onclick = e => { const b = e.target.closest('button'); if (b) draw(b.dataset.m); };
  draw('pg');
}

// ---------- PLAYERS ----------
const P_MODES = {
  pg: { l: 'Per Game', cols: () => [cGP, cMPG, ...['pts', 'reb', 'ast', 'stl', 'blk', 'tov'].map(k => ({ k, l: k.toUpperCase().replace('TOV', 'TO'), f: 1 })),
    { k: 'tpm', l: '3PM', f: 1 }, { k: 'fgp', l: 'FG%', f: 'p' }, { k: 'tpp', l: '3P%', f: 'p' }, { k: 'ftp', l: 'FT%', f: 'p' }, { k: 'ts', l: 'TS%', f: 'p' }, { k: 'pm_g', l: '+/-', f: 's1', sc: 1 }], key: 'pts' },
  tot: { l: 'Totals', cols: () => [cGP, { k: 'gs', l: 'GS' }, { k: 'min', l: 'MIN', f: 0 }, ...COUNT.map(k => ({ k: 't_' + k, l: LBL[k], f: 0 })), { k: 'pm', l: '+/-', f: 's0', sc: 1 }, { k: 'dd', l: 'DD' }, { k: 'td', l: 'TD' }], key: 't_pts' },
  p36: { l: 'Per 36', cols: () => [cGP, cMPG, ...['pts', 'reb', 'ast', 'stl', 'blk', 'tov', 'tpm', 'fga', 'fta', 'oreb'].map(k => ({ k: 'p36_' + k, l: LBL[k], f: 1 }))], key: 'p36_pts' },
  adv: { l: 'Advanced', cols: () => [cGP, cMPG, { k: 'per', l: 'PER', f: 1, t: 'Player Efficiency Rating' }, { k: 'ts', l: 'TS%', f: 'p' }, { k: 'efg', l: 'eFG%', f: 'p' }, { k: 'usg', l: 'USG%', f: 1 },
    { k: 'astp', l: 'AST%', f: 1 }, { k: 'tovp', l: 'TOV%', f: 1 }, { k: 'orbp', l: 'ORB%', f: 1 }, { k: 'drbp', l: 'DRB%', f: 1 }, { k: 'trbp', l: 'TRB%', f: 1 },
    { k: 'stlp', l: 'STL%', f: 1 }, { k: 'blkp', l: 'BLK%', f: 1 }, { k: 'ortg', l: 'ORtg', f: 1 }, { k: 'drtg', l: 'DRtg', f: 1 }, { k: 'netrtg', l: 'Net', f: 's1', sc: 1 },
    { k: 'ows', l: 'OWS', f: 1 }, { k: 'dws', l: 'DWS', f: 1 }, { k: 'ws', l: 'WS', f: 1 }, { k: 'ws48', l: 'WS/48', f: 'p3' }, { k: 'bpm', l: 'BPM', f: 's1', sc: 1 }, { k: 'vorp', l: 'VORP', f: 1 }, { k: 'gmsc', l: 'GmSc', f: 1 }], key: 'per' },
  shoot: { l: 'Shooting', cols: () => [cGP, { k: 'fgm', l: 'FGM', f: 1 }, { k: 'fga', l: 'FGA', f: 1 }, { k: 'fgp', l: 'FG%', f: 'p' }, { k: 'tpm', l: '3PM', f: 1 }, { k: 'tpa', l: '3PA', f: 1 }, { k: 'tpp', l: '3P%', f: 'p' },
    { k: 'ftm', l: 'FTM', f: 1 }, { k: 'fta', l: 'FTA', f: 1 }, { k: 'ftp', l: 'FT%', f: 'p' }, { k: 'efg', l: 'eFG%', f: 'p' }, { k: 'ts', l: 'TS%', f: 'p' }, { k: 'tpar', l: '3PAr', f: 'p3' }, { k: 'ftr', l: 'FTr', f: 'p3' }, { k: 'hi_pts', l: 'High', f: 0 }], key: 'ts' },
};
P_MODES.oo = { l: 'Impact', key: 'rapm', cols: () => [cGP, { k: 'on_min', l: 'On MIN', f: 0 }, { k: 'off_min', l: 'Off MIN', f: 0 },
  { k: 'rapm', l: 'RAPM', f: 's1', sc: 1, t: 'Regularized adjusted plus-minus (per 100 poss.), controls for teammates & opponents' }, { k: 'rapm_o', l: 'O-RAPM', f: 's1', sc: 1 }, { k: 'rapm_d', l: 'D-RAPM', f: 's1', sc: 1 },
  { k: 'rapm0', l: 'Pure RAPM', f: 's1', sc: 1, t: 'RAPM without the box-score prior' },
  { k: 'on_ortg', l: 'On ORtg', f: 1 }, { k: 'off_ortg', l: 'Off ORtg', f: 1 }, { k: 'oo_ortg', l: '± ORtg', f: 's1', sc: 1, t: 'On minus off (higher = better offense with him)' },
  { k: 'on_drtg', l: 'On DRtg', f: 1, asc: 1 }, { k: 'off_drtg', l: 'Off DRtg', f: 1, asc: 1 }, { k: 'oo_drtg', l: '± DRtg', f: 's1', asc: 1, t: 'On minus off (negative = better defense with him)', h: r => `<span class="${sgnCls(-(r.oo_drtg ?? 0))}">${fmt(r.oo_drtg, 's1')}</span>` },
  { k: 'on_net', l: 'On Net', f: 's1', sc: 1 }, { k: 'off_net', l: 'Off Net', f: 's1', sc: 1 }, { k: 'oo_net', l: 'On-Off', f: 's1', sc: 1, t: 'On-court net minus off-court net' }, { k: 'on_pm', l: 'On +/-', f: 's0', sc: 1 }] };
const LBL = { pts: 'PTS', fgm: 'FGM', fga: 'FGA', tpm: '3PM', tpa: '3PA', ftm: 'FTM', fta: 'FTA', oreb: 'OREB', dreb: 'DREB', reb: 'REB', ast: 'AST', stl: 'STL', blk: 'BLK', tov: 'TO', pf: 'PF' };
const cGP = { k: 'gp', l: 'GP' };
const cMPG = { k: 'mpg', l: 'MIN', f: 1 };
async function players(q) {
  const o = qs(q);
  let mode = P_MODES[o.mode] ? o.mode : 'pg';
  app.innerHTML = `<div class="pagehead"><div><h1>Players</h1><p>${D.players.length} players have appeared this season. Tap any column to sort.</p></div></div>
    <div class="card flush"><div class="hd"><div class="controls">
      <input type="search" id="q" placeholder="Search players…" value="${esc(o.q || '')}">
      <select id="tm"><option value="">All teams</option>${D.teams.map(t => `<option value="${t.id}">${esc(t.display)}</option>`).join('')}</select>
      <select id="pos"><option value="">All pos.</option><option>G</option><option>F</option><option>C</option></select>
      <label class="chk"><input type="checkbox" id="qual" ${o.all ? '' : 'checked'}> Qualified only</label></div>
      <div class="seg" id="mode">${Object.entries(P_MODES).map(([k, m]) => `<button data-m="${k}">${m.l}</button>`).join('')}</div></div><div id="tb"></div></div>`;
  let sortKey = o.sort, sortAsc = !!o.asc;
  const rows = () => {
    const s = $('#q').value.trim().toLowerCase(), tm = $('#tm').value, pos = $('#pos').value, qual = $('#qual').checked;
    return D.players.filter(p => (!s || p.name.toLowerCase().includes(s)) && (!tm || p.teams.includes(tm)) && (!pos || (p.pos || '').includes(pos)) && (!qual || (mode === 'oo' ? p.qoo : p.qual)));
  };
  let tb;
  const draw = () => {
    $('#mode').querySelectorAll('button').forEach(b => b.classList.toggle('on', b.dataset.m === mode));
    const cols = [cPlayer, ...P_MODES[mode].cols()];
    const key = cols.some(c => c.k === sortKey) ? sortKey : P_MODES[mode].key;
    tb = table($('#tb'), { rows: rows(), cols, key, desc: !(key === sortKey && sortAsc), onSort: st => { sortKey = st.key; sortAsc = !st.desc; } });
  };
  $('#mode').onclick = e => { const b = e.target.closest('button'); if (b) { mode = b.dataset.m; draw(); } };
  const re = () => tb.set(rows());
  $('#q').oninput = debounce(re, 150); $('#tm').onchange = re; $('#pos').onchange = re; $('#qual').onchange = re;
  draw();
}

// ---------- PLAYER ----------
function rankAmong(p, k, asc) {
  const pool = D.players.filter(x => x.qual && x[k] != null);
  if (!p.qual) return '';
  const r = 1 + pool.filter(x => asc ? x[k] < p[k] : x[k] > p[k]).length;
  return `#${r} of ${pool.length}`;
}
async function player(id) {
  const p = P[id];
  if (!p) { app.innerHTML = '<div class="loading">Player not found</div>'; return; }
  const [{ log }, { splits }] = await Promise.all([api(`player/${id}/log`), api(`player/${id}/splits`).catch(() => ({ splits: [] }))]);
  const t = p.t;
  const tiles = [['PTS', p.pts, 1, 'pts'], ['REB', p.reb, 1, 'reb'], ['AST', p.ast, 1, 'ast'], ['TS%', p.ts, 'p', 'ts'], ['PER', p.per, 1, 'per'], ['WS', p.ws, 1, 'ws'], ['BPM', p.bpm, 's1', 'bpm'], ['+/-', p.pm_g, 's1', 'pm_g']];
  const bio = [['Age', p.age], ['Height', p.ht], ['Weight', p.wt ? p.wt + ' lb' : ''], ['Exp', p.exp != null ? (p.exp ? p.exp + ' yrs' : 'Rookie') : ''], ['College', p.college]].filter(x => x[1]);
  const traded = p.teams.length > 1 ? `<span class="badge">Also: ${p.teams.slice(0, -1).map(x => esc(T[x]?.abbr)).join(', ')}</span>` : '';
  app.innerHTML = `
    <div class="hero" style="--tc:${t?.tc || '#345'}"><div class="row">
      <img class="big" src="${hsUrl(p, 520)}" alt="" onerror="this.onerror=null;this.src='${NOHS}'">
      <div class="meta"><div class="kick">${t ? teamLink(t, true) : ''}<span>·</span><span>#${esc(p.jersey)}</span><span>·</span><span>${esc(p.pos)}</span>${traded}</div>
        <h1 style="font-size:44px;margin-top:4px">${esc(p.name)}</h1>
        <div class="bio">${bio.map(([k, v]) => `<span>${k} <b>${esc(v)}</b></span>`).join('')}<span>GP <b>${p.gp}</b> (${p.gs} GS)</span><span>MIN <b>${p.mpg.toFixed(1)}</b></span></div>
        <div class="tiles">${tiles.map(([k, v, f, rk]) => `<div class="tile"><div class="k">${k}</div><div class="v">${fmt(v, f)}</div><div class="r">${rankAmong(p, rk)}</div></div>`).join('')}</div>
      </div></div></div>
    <div class="card"><div class="hd"><h2>Points by game</h2><span class="sub">orange = win · gray = loss · hover for line</span></div><div class="spark" id="spark"></div></div>
    <div class="grid g2 mt">
      <div class="card flush"><div class="hd"><h2>Season</h2></div><div id="season"></div></div>
      <div class="card flush"><div class="hd"><h2>Advanced</h2></div><div id="adv"></div></div>
    </div>
    <div class="card flush mt"><div class="hd"><h2>On / Off Court</h2><span class="sub">Team per 100 possessions, in games he played</span></div><div id="oo"></div></div>
    <div class="card flush mt"><div class="hd"><h2>With & Without Teammates</h2><span class="sub">Team net rating per 100 possessions · minutes in grey</span></div><div id="splits"></div></div>
    <div class="card flush mt"><div class="hd"><h2>Game Log</h2></div><div id="log"></div></div>`;
  // season table
  const sRows = [
    { l: 'Per game', v: k => p[k], f: 1 }, { l: 'Per 36', v: k => p['p36_' + k], f: 1 }, { l: 'Totals', v: k => p['t_' + k], f: 0 }];
  const ks = ['pts', 'reb', 'oreb', 'ast', 'stl', 'blk', 'tov', 'fgm', 'fga', 'tpm', 'tpa', 'ftm', 'fta', 'pf'];
  $('#season').innerHTML = `<div class="tw"><table class="t"><thead><tr><th class="l"></th>${ks.map(k => `<th>${LBL[k]}</th>`).join('')}</tr></thead><tbody>${sRows.map(r => `<tr><td class="l"><b>${r.l}</b></td>${ks.map(k => `<td>${fmt(r.v(k), r.f)}</td>`).join('')}</tr>`).join('')}
    <tr><td class="l"><b>Shooting</b></td><td colspan="${ks.length}" class="l">FG ${fmt(p.fgp, 'p')}% · 3P ${fmt(p.tpp, 'p')}% · FT ${fmt(p.ftp, 'p')}% · eFG ${fmt(p.efg, 'p')}% · TS ${fmt(p.ts, 'p')}% · 3PAr ${fmt(p.tpar, 'p3')} · FTr ${fmt(p.ftr, 'p3')}</td></tr></tbody></table></div>`;
  const adv = [['PER', p.per, 1, 'per'], ['USG%', p.usg, 1, 'usg'], ['AST%', p.astp, 1, 'astp'], ['TOV%', p.tovp, 1, 'tovp', 1], ['ORB%', p.orbp, 1, 'orbp'], ['DRB%', p.drbp, 1, 'drbp'], ['TRB%', p.trbp, 1, 'trbp'], ['STL%', p.stlp, 1, 'stlp'], ['BLK%', p.blkp, 1, 'blkp'],
    ['ORtg', p.ortg, 1, 'ortg'], ['DRtg', p.drtg, 1, 'drtg', 1], ['OWS', p.ows, 2, 'ows'], ['DWS', p.dws, 2, 'dws'], ['WS', p.ws, 2, 'ws'], ['WS/48', p.ws48, 'p3', 'ws48'], ['BPM', p.bpm, 's1', 'bpm'], ['VORP', p.vorp, 2, 'vorp'], ['Game Score', p.gmsc, 1, 'gmsc'], ['DD / TD', `${p.dd} / ${p.td}`]];
  $('#adv').innerHTML = `<div class="tw"><table class="t"><tbody>${adv.map(([k, v, f, rk, asc]) => `<tr><td class="l">${k}</td><td><b>${fmt(v, f)}</b></td><td class="muted">${rk ? rankAmong(p, rk, asc) : ''}</td></tr>`).join('')}</tbody></table></div>`;
  // on/off
  if (p.on_ortg == null) $('#oo').innerHTML = '<div class="sub" style="padding:0 16px 14px">No play-by-play data.</div>';
  else {
    const cell = (v, f, good) => `<td class="${good == null ? '' : good ? 'pos' : 'neg'}"><b>${fmt(v, f)}</b></td>`;
    const rk = (k, asc) => p.qoo ? (() => { const pool = D.players.filter(x => x.qoo); return `#${1 + pool.filter(x => asc ? x[k] < p[k] : x[k] > p[k]).length} of ${pool.length}`; })() : '';
    $('#oo').innerHTML = `<div class="tw"><table class="t"><thead><tr><th class="l"></th><th>MIN</th><th>Off Rtg</th><th>Def Rtg</th><th>Net Rtg</th></tr></thead><tbody>
      <tr><td class="l"><b>On court</b></td><td>${fmt(p.on_min, 0)}</td>${cell(p.on_ortg, 1)}${cell(p.on_drtg, 1)}${cell(p.on_net, 's1')}</tr>
      <tr><td class="l"><b>Off court</b></td><td>${fmt(p.off_min, 0)}</td>${cell(p.off_ortg, 1)}${cell(p.off_drtg, 1)}${cell(p.off_net, 's1')}</tr>
      <tr><td class="l"><b>On − Off</b></td><td></td>${cell(p.oo_ortg, 's1', p.oo_ortg > 0)}${cell(p.oo_drtg, 's1', p.oo_drtg < 0)}${cell(p.oo_net, 's1', p.oo_net > 0)}</tr>
      <tr><td class="l muted">League rank</td><td></td><td class="muted">${rk('oo_ortg')}</td><td class="muted">${rk('oo_drtg', 1)}</td><td class="muted">${rk('oo_net')}</td></tr>
    </tbody></table></div>${p.qoo ? '' : '<div class="sub" style="padding:8px 16px 12px">Not ranked: needs 250+ minutes on and 100+ off court.</div>'}`;
  }
  // RAPM + teammate splits
  if (p.rapm != null) {
    const pool = D.players.filter(x => x.qual && x.rapm != null);
    const rk = k => p.qual ? `#${1 + pool.filter(x => x[k] > p[k]).length} of ${pool.length}` : '';
    $('#oo').insertAdjacentHTML('beforeend', `<div class="tiles" style="padding:0 16px 14px">${[['RAPM', 'rapm'], ['Off RAPM', 'rapm_o'], ['Def RAPM', 'rapm_d'], ['Pure RAPM', 'rapm0']].map(([l, k]) => `<div class="tile"><div class="k">${l}</div><div class="v ${sgnCls(p[k])}">${fmt(p[k], 's1')}</div><div class="r">${rk(k)}</div></div>`).join('')}</div>
      <div class="sub" style="padding:0 16px 14px">Raw on/off credits a player for whoever he shares the floor with. RAPM fits every stint of the season at once (all ten players on court), so shared minutes are split by what happens when each player plays without the other. It is shrunk toward his box score (BPM) until the minutes say otherwise; "Pure RAPM" uses no box score.</div>`);
  }
  const net = c => c && c.net != null ? `<b class="${sgnCls(c.net)}">${fmt(c.net, 's1')}</b> <span class="muted">${c.min}</span>` : `<span class="muted">${c ? c.min + ' min' : '–'}</span>`;
  const sp = splits.filter(x => P[x.id]);
  $('#splits').innerHTML = sp.length ? `<div class="tw"><table class="t"><thead><tr><th class="l">Teammate</th><th>Both on</th><th>${esc(p.short || p.name)} only</th><th>Teammate only</th><th>Neither</th></tr></thead><tbody>${sp.map(x => `<tr><td class="l stick"><a class="pl" href="#/player/${x.id}">${hsImg(P[x.id], 68)}<span class="nm">${esc(P[x.id].name)}</span></a></td><td>${net(x.both)}</td><td>${net(x.me)}</td><td>${net(x.mate)}</td><td>${net(x.neither)}</td></tr>`).join('')}</tbody></table></div>`
    : '<div class="sub" style="padding:0 16px 14px">No teammates with 30+ shared minutes yet.</div>';
  // log
  log.forEach(g => {
    g.day = D.games.find(x => x.id === g.game_id)?.day || g.date.slice(0, 10);
    g.isHome = g.home_id === g.team_id;
    g.opp = T[g.isHome ? g.away_id : g.home_id];
    const my = g.isHome ? g.home_score : g.away_score, op = g.isHome ? g.away_score : g.home_score;
    g.win = my > op; g.res = `${g.win ? 'W' : 'L'} ${my}-${op}`; g.margin = my - op;
    g.gmsc = g.pts + 0.4 * g.fgm - 0.7 * g.fga - 0.4 * (g.fta - g.ftm) + 0.7 * g.oreb + 0.3 * g.dreb + g.stl + 0.7 * g.ast + 0.7 * g.blk - 0.4 * g.pf - g.tov;
  });
  const played = log.filter(g => !g.dnp && g.min > 0);
  const mx = Math.max(10, ...played.map(g => g.pts));
  $('#spark').innerHTML = played.map(g => `<a class="${g.win ? '' : 'l'}" href="#/game/${g.game_id}" style="height:${Math.max(2, g.pts / mx * 100)}%" data-t="${esc(`<b>${dayLabel(g.day)} ${g.isHome ? 'vs' : '@'} ${g.opp?.abbr}</b>${tipRows([['Result', g.res], ['PTS', g.pts], ['REB / AST', g.reb + ' / ' + g.ast], ['FG', g.fgm + '-' + g.fga], ['MIN', g.min]])}`)}"></a>`).join('');
  $('#spark').onmousemove = e => { const a = e.target.closest('a'); if (a) showTip(a.dataset.t, e.clientX, e.clientY); else hideTip(); };
  $('#spark').onmouseleave = hideTip;
  table($('#log'), {
    rows: log, key: 'day', rank: false, cols: [
      { k: 'day', l: 'Date', cls: 'l', h: r => `<a href="#/game/${r.game_id}">${dayLabel(r.day, { month: 'short', day: 'numeric' })}</a>` },
      { k: 'opp', l: 'Opp', cls: 'l', v: r => r.opp?.abbr, h: r => `${r.isHome ? 'vs' : '@'} ${teamLink(r.opp)}` },
      { k: 'margin', l: 'Result', cls: 'l', h: r => `<span class="${r.win ? 'pos' : 'neg'}">${r.res}</span>` },
      { k: 'min', l: 'MIN', h: r => r.dnp ? `<span class="muted">DNP</span>` : r.min },
      ...['pts', 'reb', 'ast', 'stl', 'blk', 'tov'].map(k => ({ k, l: LBL[k] })),
      { k: 'fgm', l: 'FG', h: r => `${r.fgm}-${r.fga}` }, { k: 'tpm', l: '3P', h: r => `${r.tpm}-${r.tpa}` }, { k: 'ftm', l: 'FT', h: r => `${r.ftm}-${r.fta}` },
      { k: 'pm', l: '+/-', f: 's0', sc: 1 }, { k: 'gmsc', l: 'GmSc', f: 1 }]
  });
}

// ---------- TEAMS ----------
const T_MODES = {
  trad: { l: 'Traditional', key: 'ppg', cols: [{ k: 'gp', l: 'GP' }, { k: 'w', l: 'W' }, { k: 'l', l: 'L' }, { k: 'ppg', l: 'PTS', f: 1 }, { k: 'fgm', l: 'FGM', f: 1 }, { k: 'fga', l: 'FGA', f: 1 }, { k: 'fgp', l: 'FG%', f: 'p' },
    { k: 'tpm', l: '3PM', f: 1 }, { k: 'tpa', l: '3PA', f: 1 }, { k: 'tpp', l: '3P%', f: 'p' }, { k: 'ftm', l: 'FTM', f: 1 }, { k: 'fta', l: 'FTA', f: 1 }, { k: 'ftp', l: 'FT%', f: 'p' },
    { k: 'oreb', l: 'OREB', f: 1 }, { k: 'dreb', l: 'DREB', f: 1 }, { k: 'reb', l: 'REB', f: 1 }, { k: 'ast', l: 'AST', f: 1 }, { k: 'tov', l: 'TO', f: 1, asc: 1 }, { k: 'stl', l: 'STL', f: 1 }, { k: 'blk', l: 'BLK', f: 1 }, { k: 'pf', l: 'PF', f: 1 }, { k: 'diff', l: '+/-', f: 's1', sc: 1 }] },
  opp: { l: 'Opponent', key: 'oppg', asc: 1, cols: [{ k: 'oppg', l: 'PTS', f: 1, asc: 1 }, { k: 'o_fgm', l: 'FGM', f: 1, asc: 1 }, { k: 'o_fga', l: 'FGA', f: 1, asc: 1 }, { k: 'o_fgp', l: 'FG%', f: 'p', asc: 1 },
    { k: 'o_tpm', l: '3PM', f: 1, asc: 1 }, { k: 'o_tpa', l: '3PA', f: 1, asc: 1 }, { k: 'o_tpp', l: '3P%', f: 'p', asc: 1 }, { k: 'o_fta', l: 'FTA', f: 1, asc: 1 }, { k: 'o_oreb', l: 'OREB', f: 1, asc: 1 },
    { k: 'o_reb', l: 'REB', f: 1, asc: 1 }, { k: 'o_ast', l: 'AST', f: 1, asc: 1 }, { k: 'o_tov', l: 'TO', f: 1 }, { k: 'o_stl', l: 'STL', f: 1, asc: 1 }, { k: 'o_blk', l: 'BLK', f: 1, asc: 1 }, { k: 'o_paint', l: 'Paint', f: 1, asc: 1 }, { k: 'o_fb', l: 'Fast Br.', f: 1, asc: 1 }] },
  adv: { l: 'Advanced', key: 'net', cols: [{ k: 'w', l: 'W' }, { k: 'l', l: 'L' }, { k: 'ortg', l: 'ORtg', f: 1 }, { k: 'drtg', l: 'DRtg', f: 1, asc: 1 }, { k: 'net', l: 'Net', f: 's1', sc: 1 }, { k: 'pace', l: 'Pace', f: 1 },
    { k: 'ts', l: 'TS%', f: 'p' }, { k: 'o_ts', l: 'Opp TS%', f: 'p', asc: 1 }, { k: 'tpar', l: '3PAr', f: 'p3' }, { k: 'astp', l: 'AST/FGM', f: 'p' }, { k: 'ast_to', l: 'AST/TO', f: 2 },
    { k: 'stlp', l: 'STL/100', f: 1 }, { k: 'blkp', l: 'BLK%', f: 1 }, { k: 'pyth_w', l: 'Pyth W', f: 1 }, { k: 'luck', l: 'Luck', f: 's1', sc: 1 }, { k: 'srs', l: 'SRS', f: 's2', sc: 1 }, { k: 'sos', l: 'SOS', f: 's2', sc: 1 }] },
  ff: { l: 'Four Factors', key: 'net', cols: [{ k: 'net', l: 'Net', f: 's1', sc: 1 }, { k: 'efg', l: 'eFG%', f: 'p' }, { k: 'tovp', l: 'TOV%', f: 'p', asc: 1 }, { k: 'orbp', l: 'ORB%', f: 'p' }, { k: 'ftr', l: 'FT/FGA', f: 'p3' },
    { k: 'o_efg', l: 'Opp eFG%', f: 'p', asc: 1 }, { k: 'o_tovp', l: 'Opp TOV%', f: 'p' }, { k: 'drbp', l: 'DRB%', f: 'p' }, { k: 'o_ftr', l: 'Opp FT/FGA', f: 'p3', asc: 1 }] },
  misc: { l: 'Misc', key: 'paint', cols: [{ k: 'paint', l: 'Paint', f: 1 }, { k: 'o_paint', l: 'Opp Paint', f: 1, asc: 1 }, { k: 'fb', l: 'Fast Break', f: 1 }, { k: 'o_fb', l: 'Opp FB', f: 1, asc: 1 }, { k: 'pot', l: 'Pts off TO', f: 1 },
    { k: 'home', l: 'Home', cls: 'n', v: r => +r.home.split('-')[0] }, { k: 'away', l: 'Away', cls: 'n', v: r => +r.away.split('-')[0] }, { k: 'close', l: '≤5 pts', v: r => +r.close.split('-')[0] }] },
};
async function teams(q) {
  const o = qs(q);
  let mode = T_MODES[o.mode] ? o.mode : 'adv';
  app.innerHTML = `<div class="pagehead"><div><h1>Team Stats</h1><p>Per game. Ratings are per 100 possessions.</p></div>
    <div class="seg" id="mode">${Object.entries(T_MODES).map(([k, m]) => `<button data-m="${k}">${m.l}</button>`).join('')}</div></div>
    <div class="card flush"><div id="tb"></div></div>`;
  const draw = () => {
    $('#mode').querySelectorAll('button').forEach(b => b.classList.toggle('on', b.dataset.m === mode));
    const m = T_MODES[mode];
    table($('#tb'), { rows: D.teams, key: m.key, desc: !m.asc, cols: [cTeam, ...m.cols] });
  };
  $('#mode').onclick = e => { const b = e.target.closest('button'); if (b) { mode = b.dataset.m; draw(); } };
  draw();
}

// ---------- TEAM ----------
function teamRank(t, k, asc) { return 1 + D.teams.filter(x => asc ? x[k] < t[k] : x[k] > t[k]).length; }
const ord = n => n + (['th', 'st', 'nd', 'rd'][(n % 100 - 20) % 10] || ['th', 'st', 'nd', 'rd'][n % 100] || 'th');
async function team(id) {
  const t = T[id];
  if (!t) { app.innerHTML = '<div class="loading">Team not found</div>'; return; }
  const confRows = D.teams.filter(x => x.conf === t.conf).sort((a, b) => b.pct - a.pct || b.net - a.net);
  const seed = confRows.indexOf(t) + 1;
  const tiles = [['Off Rtg', t.ortg, 1, 'ortg'], ['Def Rtg', t.drtg, 1, 'drtg', 1], ['Net Rtg', t.net, 's1', 'net'], ['Pace', t.pace, 1, 'pace'], ['Elo', t.elo, 0, 'elo'], ['SRS', t.srs, 's2', 'srs'], ['SOS', t.sos, 's2', 'sos'], ['Power', t.power_rank, 0]];
  app.innerHTML = `
    <div class="hero" style="--tc:${t.tc}"><div class="row"><img class="tlogo" src="${logoUrl(t, 240)}" alt="">
      <div class="meta"><div class="kick">${esc(t.conf)}ern Conference · ${esc(t.div)}</div><h1 style="font-size:44px">${esc(t.display)}</h1>
      <div class="bio"><span><b>${t.w}-${t.l}</b></span><span><b>${ord(seed)}</b> in ${t.conf}</span><span>Home <b>${t.home}</b></span><span>Away <b>${t.away}</b></span><span>L10 <b>${t.l10}</b></span><span>Streak <b>${t.streak}</b></span><span>Pyth <b>${fmt(t.pyth_w, 1)}</b> W</span></div>
      <div class="tiles">${tiles.map(([k, v, f, rk, asc]) => `<div class="tile"><div class="k">${k}</div><div class="v">${fmt(v, f)}</div><div class="r">${rk ? ord(teamRank(t, rk, asc)) + ' in NBA' : 'composite rank'}</div></div>`).join('')}</div></div></div></div>
    <div class="grid g2">
      <div class="card"><div class="hd"><h2>Four Factors</h2><span class="sub">vs league average</span></div><div id="ff"></div></div>
      <div class="card"><div class="hd"><h2>Elo Rating</h2></div><div id="elo"></div></div>
    </div>
    <div class="card flush mt"><div class="hd"><h2>Roster</h2><div class="seg" id="rm"><button data-m="pg" class="on">Per Game</button><button data-m="adv">Advanced</button><button data-m="shoot">Shooting</button><button data-m="oo">On/Off</button></div></div><div id="roster"></div></div>
    <div class="card flush mt"><div class="hd"><h2>Schedule & Results</h2></div><div id="sched"></div></div>`;
  // four factors
  const lg = k => D.teams.reduce((a, x) => a + x[k], 0) / D.teams.length;
  const ffs = [['eFG%', 'efg', 0], ['TOV%', 'tovp', 1], ['ORB%', 'orbp', 0], ['FT/FGA', 'ftr', 0], ['Opp eFG%', 'o_efg', 1], ['Opp TOV%', 'o_tovp', 0], ['DRB%', 'drbp', 0], ['Opp FT/FGA', 'o_ftr', 1]];
  $('#ff').innerHTML = ffs.map(([l, k, low]) => {
    const v = t[k], a = lg(k), good = low ? v < a : v > a, rk = teamRank(t, k, low);
    return `<div class="cmp" style="grid-template-columns:96px 1fr 60px 54px"><div class="lbl" style="text-align:left">${l}</div>
      <div class="bar"><i style="width:${Math.min(100, v / (a * 1.6) * 100)}%;background:${good ? 'var(--good)' : 'var(--bad)'}"></i></div>
      <div class="v r">${fmt(v, k.includes('ftr') ? 'p3' : 'p')}</div><div class="sub" style="text-align:right">${ord(rk)}</div></div>`;
  }).join('') + `<div class="sub" style="margin-top:6px">League avg: eFG ${fmt(lg('efg'), 'p')}% · TOV ${fmt(lg('tovp'), 'p')}% · ORB ${fmt(lg('orbp'), 'p')}% · FT/FGA ${fmt(lg('ftr'), 'p3')}</div>`;
  ELO = ELO || await api('elo');
  lineChart($('#elo'), { series: [{ t, pts: ELO[t.id] }], hot: new Set([t.id]), height: 260, yRef: 1500 });
  // roster
  const roster = D.players.filter(p => p.teams.includes(id));
  let rm = 'pg';
  const drawRoster = () => {
    $('#rm').querySelectorAll('button').forEach(b => b.classList.toggle('on', b.dataset.m === rm));
    const m = P_MODES[rm];
    table($('#roster'), { rows: roster, key: rm === 'pg' ? 'mpg' : m.key, cols: [cPlayer, ...m.cols()] });
  };
  $('#rm').onclick = e => { const b = e.target.closest('button'); if (b) { rm = b.dataset.m; drawRoster(); } };
  drawRoster();
  // schedule
  let w = 0, l = 0;
  const sched = teamGames(id).map(g => {
    const home = g.home === id, opp = T[home ? g.away : g.home], fin = g.status === 'STATUS_FINAL';
    const my = home ? g.hs : g.as, op = home ? g.as : g.hs;
    if (fin) { if (my > op) w++; else l++; }
    return { g, home, opp, fin, my, op, rec: fin ? `${w}-${l}` : '', margin: fin ? my - op : null, day: g.day };
  });
  table($('#sched'), {
    rows: sched, key: 'day', desc: false, rank: false, cols: [
      { k: 'day', l: 'Date', cls: 'l', h: r => `<a href="#/game/${r.g.id}">${dayLabel(r.day)}</a>` },
      { k: 'opp', l: 'Opponent', cls: 'l', v: r => r.opp?.abbr, h: r => `<span class="muted">${r.home ? 'vs' : '@'}</span> ${teamLink(r.opp, true)}` },
      { k: 'margin', l: 'Result', cls: 'l', h: r => r.fin ? `<a href="#/game/${r.g.id}" class="${r.my > r.op ? 'pos' : 'neg'}"><b>${r.my > r.op ? 'W' : 'L'}</b> ${r.my}-${r.op}${r.g.ot ? ' (OT)' : ''}</a>` : `<span class="muted">${new Date(r.g.date).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })} · win prob ${fmt((r.home ? r.g.p_home : 1 - r.g.p_home) * 100, 0)}%</span>` },
      { k: 'rec', l: 'Record', v: r => r.fin ? +r.rec.split('-')[0] : null, h: r => r.rec },
      { k: 'top', l: 'Top scorer', cls: 'l', v: r => r.fin ? (r.home ? r.g.ht : r.g.at)?.[2] : null, h: r => { const x = r.fin && (r.home ? r.g.ht : r.g.at); return x ? `<a href="#/player/${x[0]}">${esc(x[1])}</a> ${x[2]}` : ''; } }]
  });
}

// ---------- GAMES ----------
async function games(day) {
  const days = [...new Set(D.games.map(g => g.day))].sort();
  const f = finals();
  const def = f.length ? f[f.length - 1].day : days[0];
  day = day && days.includes(day) ? day : def;
  app.innerHTML = `<div class="pagehead"><div><h1>Games</h1><p>${f.length} games played · upcoming games show Elo win probability and a Massey-rating point spread.</p></div></div>
    <div class="days" id="days">${days.map(d => `<button data-d="${d}" class="${d === day ? 'on' : ''}">${dayLabel(d, { weekday: 'short' })}<b>${dayLabel(d, { day: 'numeric' })}</b>${dayLabel(d, { month: 'short' })}</button>`).join('')}</div>
    <h2 class="mt">${dayLabel(day, { weekday: 'long', month: 'long', day: 'numeric' })}</h2><div class="grid g3" id="gl"></div>`;
  $('#gl').innerHTML = D.games.filter(g => g.day === day).map(gameCard).join('');
  $('#days').onclick = e => { const b = e.target.closest('button'); if (b) location.hash = '#/games/' + b.dataset.d; };
  $('#days .on')?.scrollIntoView({ inline: 'center', block: 'nearest' });
}

// ---------- GAME ----------
async function game(id) {
  const g = D.games.find(x => x.id === id);
  if (!g) { app.innerHTML = '<div class="loading">Game not found</div>'; return; }
  const h = T[g.home], a = T[g.away];
  const fin = g.status === 'STATUS_FINAL';
  const box = fin ? await api('game/' + id) : null;
  const side = (t, s, win, cls) => `<a class="side ${cls} ${fin && !win ? 'lose' : ''}" href="#/team/${t.id}">${logoImg(t, 144)}<div><div class="tn cond" style="font-size:22px;font-weight:700">${esc(t.display)}</div><div class="sub">${t.w}-${t.l}</div></div><div class="s">${fin ? s : ''}</div></a>`;
  app.innerHTML = `<div class="card"><div class="sb">${side(a, g.as, g.as > g.hs, '')}<div class="mid">${fin ? 'FINAL' + (g.ot ? (g.ot > 1 ? ` ${g.ot}OT` : ' OT') : '') : new Date(g.date).toLocaleString([], { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })}<div class="sub">${dayLabel(g.day)}</div></div>${side(h, g.hs, g.hs > g.as, 'r')}</div>
    <div class="sub" style="text-align:center;margin-top:8px">${esc(g.venue)}${g.city ? ' · ' + esc(g.city) : ''}${g.att ? ' · Att. ' + g.att.toLocaleString() : ''}</div><div id="ls" class="mt"></div></div><div id="gb"></div>`;
  if (!fin) {
    const ph = g.p_home;
    $('#gb').innerHTML = `<div class="card mt"><h2>Preview</h2><p>Elo win probability: <b>${esc(h.abbr)} ${(ph * 100).toFixed(0)}%</b> · <b>${esc(a.abbr)} ${((1 - ph) * 100).toFixed(0)}%</b>. Projected margin (Massey + home court): <b>${esc(g.spread > 0 ? h.abbr : a.abbr)} by ${Math.abs(g.spread).toFixed(1)}</b>.</p><div id="cmp"></div></div>`;
    $('#cmp').innerHTML = compareBars(a, h, [['Off Rtg', 'ortg', 1], ['Def Rtg', 'drtg', 1, 1], ['Net Rtg', 'net', 1], ['Pace', 'pace', 1], ['Elo', 'elo', 0], ['SRS', 'srs', 1], ['eFG%', 'efg', 'p'], ['3P%', 'tpp', 'p']]);
    return;
  }
  const tg = Object.fromEntries(box.teams.map(x => [x.team_id, x]));
  const ta = tg[a.id], th = tg[h.id];
  // linescore
  const la = JSON.parse(ta.linescore || '[]'), lh = JSON.parse(th.linescore || '[]');
  const per = Math.max(la.length, lh.length);
  const pl = i => i < 4 ? 'Q' + (i + 1) : (i === 4 ? 'OT' : 'OT' + (i - 3));
  $('#ls').innerHTML = `<div class="tw"><table class="t" style="max-width:560px;margin:0 auto"><thead><tr><th class="l"></th>${Array.from({ length: per }, (_, i) => `<th>${pl(i)}</th>`).join('')}<th>T</th></tr></thead>
    <tbody>${[[a, la, g.as], [h, lh, g.hs]].map(([t, l, s]) => `<tr><td class="l">${teamLink(t)}</td>${l.map(x => `<td>${x}</td>`).join('')}<td><b>${s}</b></td></tr>`).join('')}</tbody></table></div>`;
  // per-game advanced
  const poss = (t, o) => t.fga + 0.4 * t.fta - 1.07 * (t.oreb / (t.oreb + o.dreb)) * (t.fga - t.fgm) + t.total_tov;
  const ps = 0.5 * (poss(ta, th) + poss(th, ta));
  const mins = 48 + 5 * Math.max(0, per - 4);
  for (const [t, o] of [[ta, th], [th, ta]]) {
    t.fgp = t.fgm / t.fga; t.tpp = t.tpm / t.tpa; t.ftp = t.fta ? t.ftm / t.fta : null;
    t.efg = (t.fgm + 0.5 * t.tpm) / t.fga; t.tovp = t.total_tov / (t.fga + 0.44 * t.fta + t.total_tov);
    t.orbp = t.oreb / (t.oreb + o.dreb); t.ftr = t.ftm / t.fga; t.ortg = 100 * t.pts / ps; t.ts = t.pts / (2 * (t.fga + 0.44 * t.fta));
    t.pace = 48 * ps / mins;
  }
  const lead = box.players.filter(p => !p.dnp && p.min > 0).map(p => ({ ...p, gmsc: p.pts + 0.4 * p.fgm - 0.7 * p.fga - 0.4 * (p.fta - p.ftm) + 0.7 * p.oreb + 0.3 * p.dreb + p.stl + 0.7 * p.ast + 0.7 * p.blk - 0.4 * p.pf - p.tov }));
  const stars = lead.slice().sort((x, y) => y.gmsc - x.gmsc).slice(0, 4);
  $('#gb').innerHTML = `
    <div class="grid g4 mt">${stars.map(p => `<a class="card star" href="#/player/${p.player_id}" style="--tc:${T[p.team_id]?.tc}">${hsImg({ id: p.player_id, img: p.headshot }, 200, '')}<div><div class="cat">Game Score <b>${p.gmsc.toFixed(1)}</b></div><div class="who">${esc(p.name)}</div><div class="sub">${p.pts} pts · ${p.reb} reb · ${p.ast} ast</div></div></a>`).join('')}</div>
    <div class="grid g2 mt"><div class="card"><div class="hd"><h2>Team Stats</h2><span class="sub">${esc(a.abbr)} vs ${esc(h.abbr)}</span></div>${compareBars(a, h, [['FG%', 'fgp', 'p'], ['3P%', 'tpp', 'p'], ['FT%', 'ftp', 'p'], ['Rebounds', 'reb', 0], ['Off Reb', 'oreb', 0], ['Assists', 'ast', 0], ['Turnovers', 'total_tov', 0, 1], ['Steals', 'stl', 0], ['Blocks', 'blk', 0], ['Paint Pts', 'paint', 0], ['Fast Break', 'fast_break', 0], ['Pts off TO', 'pts_off_to', 0], ['Largest Lead', 'largest_lead', 0]], ta, th)}</div>
    <div class="card"><div class="hd"><h2>Four Factors</h2><span class="sub">${ps.toFixed(1)} possessions</span></div>${compareBars(a, h, [['eFG%', 'efg', 'p'], ['TOV%', 'tovp', 'p', 1], ['ORB%', 'orbp', 'p'], ['FT/FGA', 'ftr', 'p3'], ['TS%', 'ts', 'p'], ['Off Rtg', 'ortg', 1]], ta, th)}</div></div>
    <div class="card flush mt"><div class="hd"><h2>${logoImg(a, 44)} ${esc(a.display)}</h2></div><div id="ba"></div></div>
    <div class="card flush mt"><div class="hd"><h2>${logoImg(h, 44)} ${esc(h.display)}</h2></div><div id="bh"></div></div>`;
  for (const [t, el, tt] of [[a, '#ba', ta], [h, '#bh', th]]) {
    const rows = lead.filter(p => p.team_id === t.id);
    const dnp = box.players.filter(p => p.team_id === t.id && (p.dnp || !p.min));
    const foot = `<tr><td class="rk"></td><td class="l stick">Team</td><td>${mins * 5}</td><td>${tt.pts}</td><td>${tt.reb}</td><td>${tt.ast}</td><td>${tt.stl}</td><td>${tt.blk}</td><td>${tt.tov}</td><td>${tt.fgm}-${tt.fga}</td><td>${tt.tpm}-${tt.tpa}</td><td>${tt.ftm}-${tt.fta}</td><td>${tt.oreb}</td><td>${tt.pf}</td><td></td><td></td></tr>`;
    table($(el), {
      rows, key: 'min', foot, rowCls: r => '', cols: [
        { k: 'name', l: 'Player', cls: 'l stick', asc: 1, h: r => `<a class="pl" href="#/player/${r.player_id}">${hsImg({ id: r.player_id, img: r.headshot }, 68)}<span><span class="nm">${esc(r.short_name || r.name)}</span> <span class="tm">${esc(r.position || '')}${r.starter ? ' ·S' : ''}</span></span></a>` },
        { k: 'min', l: 'MIN' }, ...['pts', 'reb', 'ast', 'stl', 'blk', 'tov'].map(k => ({ k, l: LBL[k] })),
        { k: 'fgm', l: 'FG', h: r => `${r.fgm}-${r.fga}` }, { k: 'tpm', l: '3P', h: r => `${r.tpm}-${r.tpa}` }, { k: 'ftm', l: 'FT', h: r => `${r.ftm}-${r.fta}` },
        { k: 'oreb', l: 'OREB' }, { k: 'pf', l: 'PF' }, { k: 'pm', l: '+/-', f: 's0', sc: 1 }, { k: 'gmsc', l: 'GmSc', f: 1 }]
    });
    if (dnp.length) $(el).insertAdjacentHTML('beforeend', `<div class="sub" style="padding:10px 16px">DNP: ${dnp.map(p => `${esc(p.short_name || p.name)}${p.reason ? ' (' + esc(p.reason.toLowerCase()) + ')' : ''}`).join(', ')}</div>`);
  }
}
function compareBars(a, h, items, sa = a, sh = h) {
  return `<div class="cmp" style="margin-bottom:4px"><div>${logoImg(a, 44)}</div><div></div><div></div><div></div><div style="text-align:right">${logoImg(h, 44)}</div></div>` + items.map(([l, k, f, low]) => {
    const va = sa[k] ?? 0, vh = sh[k] ?? 0, mx = Math.max(Math.abs(va), Math.abs(vh)) || 1;
    const aw = low ? va < vh : va > vh, hw = low ? vh < va : vh > va;
    return `<div class="cmp"><div class="v">${fmt(sa[k], f)}</div><div class="bar l"><i style="width:${Math.abs(va) / mx * 100}%;background:${aw ? a.tc : 'var(--line2)'}"></i></div><div class="lbl">${l}</div><div class="bar"><i style="width:${Math.abs(vh) / mx * 100}%;background:${hw ? h.tc : 'var(--line2)'}"></i></div><div class="v r">${fmt(sh[k], f)}</div></div>`;
  }).join('');
}

// ---------- RANKINGS ----------
const RK = [
  ['elo', 'Elo', 0, 'FiveThirtyEight-style Elo: starts at 1500, K=20, +100 home court, margin-of-victory multiplier. Updates after every game, so it is the most "recent form" weighted.'],
  ['srs', 'SRS', 's2', 'Simple Rating System: average margin of victory adjusted for opponent strength (points better than an average team on a neutral floor).'],
  ['sagarin', 'Sagarin-style', 's2', 'Approximation of Sagarin\'s blend: a margin-based "predictor" (blowouts capped at 20, recent games weighted more) averaged with a win/loss-only Bradley-Terry rating, in points.'],
  ['massey', 'Massey', 's2', 'Least-squares fit of every game margin with a home-court term. Purely points-based; also drives the spreads on upcoming games.'],
  ['colley', 'Colley', 3, 'Colley Matrix: wins and losses only (margin ignored), adjusted for schedule. Used in the old BCS formula. 0.5 = average.'],
  ['rpi', 'RPI', 'p3', 'Rating Percentage Index: 25% own win %, 50% opponents\' win %, 25% opponents\' opponents\' win %. Rewards schedule strength heavily.'],
  ['bt', 'Bradley-Terry', 'p3', 'Logistic win/loss model (like Sagarin\'s ELO-CHESS). Shown as expected win % against an average team.'],
  ['net', 'Net Rtg', 's1', 'Points scored minus allowed per 100 possessions. Not schedule-adjusted.'],
];
function rkBadge(r) {
  const x = (r - 1) / 29; // 0 best .. 1 worst
  const c = x < .5 ? `color-mix(in srgb, var(--good) ${Math.round((0.5 - x) * 2 * 45)}%, transparent)` : `color-mix(in srgb, var(--bad) ${Math.round((x - 0.5) * 2 * 45)}%, transparent)`;
  return `<span class="rkb" style="background:${c}">${r}</span>`;
}
async function rankings() {
  app.innerHTML = `<div class="pagehead"><div><h1>Power Rankings</h1><p>Eight rating systems computed from every game so far. "Power" is the average rank across all of them.</p></div></div>
    <div class="card flush"><div id="tb"></div></div>
    <div class="card mt"><h2>The systems</h2><div class="explain">${RK.map(([k, l, , d]) => `<div><b>${l}</b>${d}</div>`).join('')}
      <div><b>Pythagorean W</b>Expected wins from points scored/allowed (exponent 14). "Luck" = actual wins − expected.</div>
      <div><b>SOS</b>Strength of schedule from SRS: average opponent rating (points vs average).</div></div></div>`;
  table($('#tb'), {
    rows: D.teams, key: 'power_rank', desc: false, rank: false, cols: [
      { k: 'power_rank', l: 'Power', h: r => `<span class="pr">${r.power_rank}</span>` }, cTeam,
      { k: 'pct', l: 'W-L', h: r => `${r.w}-${r.l}` }, { k: 'composite', l: 'Avg Rk', f: 1, asc: 1 },
      ...RK.map(([k, l, f]) => ({ k, l, t: l, h: r => `${fmt(r[k], f)} ${rkBadge(r['rk_' + k])}` })),
      { k: 'pyth_w', l: 'Pyth W', f: 1 }, { k: 'luck', l: 'Luck', f: 's1', sc: 1 }, { k: 'sos', l: 'SOS', f: 's2', sc: 1 },
      { k: 'l10', l: 'L10', v: r => r.form.reduce((a, b) => a + b, 0), h: r => formDots(r.form) }]
  });
}

// ---------- CHARTS ----------
function niceTicks(lo, hi, n = 6) {
  const span = hi - lo, step0 = span / n, mag = Math.pow(10, Math.floor(Math.log10(step0)));
  const step = [1, 2, 2.5, 5, 10].map(x => x * mag).find(x => span / x <= n) || mag * 10;
  const out = []; for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) out.push(+v.toFixed(10));
  return out;
}
function scatter(el, o) {
  const draw = () => {
    const W = Math.max(300, el.clientWidth || 600), small = W < 560;
    const H = Math.round(Math.min(640, Math.max(360, W * (small ? 1.05 : 0.6))));
    const m = { l: small ? 40 : 52, r: 14, t: 14, b: 44 };
    const xs = o.pts.map(p => p.x), ys = o.pts.map(p => p.y);
    const pad = (a, b) => (b - a) * 0.07 || 1;
    let x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.min(...ys), y1 = Math.max(...ys);
    if (o.square) { const lo = Math.min(x0, y0), hi = Math.max(x1, y1); x0 = y0 = lo; x1 = y1 = hi; }
    const px = pad(x0, x1), py = pad(y0, y1); x0 -= px; x1 += px; y0 -= py; y1 += py;
    const sx = v => m.l + (v - x0) / (x1 - x0) * (W - m.l - m.r);
    const sy = v => o.invY ? m.t + (v - y0) / (y1 - y0) * (H - m.t - m.b) : H - m.b - (v - y0) / (y1 - y0) * (H - m.t - m.b);
    const xf = o.xf || (v => v), yf = o.yf || (v => v);
    let s = `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="${esc(o.title || '')}"><defs><clipPath id="cc-${o.id}" clipPathUnits="objectBoundingBox"><circle cx=".5" cy=".5" r=".5"/></clipPath>
      <clipPath id="plot-${o.id}"><rect x="${m.l}" y="${m.t}" width="${W - m.l - m.r}" height="${H - m.t - m.b}"/></clipPath></defs><g class="grid">`;
    niceTicks(x0, x1, small ? 5 : 8).forEach(v => { s += `<line x1="${sx(v)}" x2="${sx(v)}" y1="${m.t}" y2="${H - m.b}"/><text x="${sx(v)}" y="${H - m.b + 16}" text-anchor="middle">${xf(v)}</text>`; });
    niceTicks(y0, y1, 6).forEach(v => { s += `<line x1="${m.l}" x2="${W - m.r}" y1="${sy(v)}" y2="${sy(v)}"/><text x="${m.l - 6}" y="${sy(v) + 4}" text-anchor="end">${yf(v)}</text>`; });
    s += `</g><g clip-path="url(#plot-${o.id})">`;
    // diagonal reference lines
    (o.diags || []).forEach(dg => {
      // y = x * slope + c  (in data units)
      const ya = x0 * (dg.slope ?? 1) + dg.c, yb = x1 * (dg.slope ?? 1) + dg.c;
      s += `<line class="ref" x1="${sx(x0)}" y1="${sy(ya)}" x2="${sx(x1)}" y2="${sy(yb)}"/>`;
      if (dg.label) { const lx = x0 + (x1 - x0) * (dg.at ?? .9), ly = lx * (dg.slope ?? 1) + dg.c; if (ly > Math.min(y0, y1) && ly < Math.max(y0, y1)) s += `<text x="${sx(lx)}" y="${sy(ly) - 5}" text-anchor="middle" style="font-size:10px">${dg.label}</text>`; }
    });
    if (o.avgX != null) s += `<line class="avg" x1="${sx(o.avgX)}" x2="${sx(o.avgX)}" y1="${m.t}" y2="${H - m.b}"/>`;
    if (o.avgY != null) s += `<line class="avg" x1="${m.l}" x2="${W - m.r}" y1="${sy(o.avgY)}" y2="${sy(o.avgY)}"/>`;
    s += '</g>';
    if (o.quads) {
      const [tl, tr, bl, br] = o.quads, xl = m.l + 8, xr = W - m.r - 8, yt = m.t + 16, yb = H - m.b - 8;
      s += `<text class="q" x="${xl}" y="${yt}">${tl}</text><text class="q" x="${xr}" y="${yt}" text-anchor="end">${tr}</text><text class="q" x="${xl}" y="${yb}">${bl}</text><text class="q" x="${xr}" y="${yb}" text-anchor="end">${br}</text>`;
    }
    s += `<text class="axl" x="${(m.l + W - m.r) / 2}" y="${H - 6}" text-anchor="middle">${o.xl}</text><text class="axl" transform="translate(12 ${(m.t + H - m.b) / 2}) rotate(-90)" text-anchor="middle">${o.yl}</text>`;
    const base = o.size || (small ? 26 : 34);
    // draw biggest first so small marks stay on top
    const order = o.pts.map((p, i) => i).sort((i, j) => (o.pts[j].r || 1) - (o.pts[i].r || 1));
    order.forEach(i => {
      const p = o.pts[i], r = base * (p.r || 1), cx = sx(p.x), cy = sy(p.y);
      s += `<g class="pt" data-i="${i}" style="transform-origin:${cx}px ${cy}px">`;
      if (p.kind === 'hs') s += `<circle cx="${cx}" cy="${cy}" r="${r / 2 + 1.5}" fill="${p.color}" stroke="var(--card)" stroke-width="2"/><image href="${p.img}" x="${cx - r / 2}" y="${cy - r / 2}" width="${r}" height="${r}" preserveAspectRatio="xMidYMin slice" clip-path="url(#cc-${o.id})"/>`;
      else if (p.img) s += `<circle cx="${cx}" cy="${cy}" r="${r / 2}" fill="var(--card)" opacity=".85"/><image href="${p.img}" x="${cx - r * 0.42}" y="${cy - r * 0.42}" width="${r * 0.84}" height="${r * 0.84}"/>`;
      else s += `<circle cx="${cx}" cy="${cy}" r="${Math.max(4, r / 4)}" fill="${p.color}" stroke="var(--card)" stroke-width="2" opacity=".85"/>`;
      s += `<circle cx="${cx}" cy="${cy}" r="${Math.max(10, r / 2 + 2)}" fill="transparent"/></g>`;
    });
    s += '</svg>';
    el.innerHTML = `<div class="chart">${s}</div>`;
  };
  draw();
  let tapped = null;
  el.onmousemove = e => { const g = e.target.closest('.pt'); if (g) { const p = o.pts[g.dataset.i]; showTip(p.tip, e.clientX, e.clientY); } else hideTip(); };
  el.onmouseleave = hideTip;
  el.onclick = e => {
    const g = e.target.closest('.pt'); if (!g) { tapped = null; hideTip(); return; }
    const p = o.pts[g.dataset.i];
    if (HOVER || tapped === p) { location.hash = p.href; return; }
    tapped = p; showTip(p.tip + '<div class="sub" style="margin-top:4px">Tap again to open</div>', e.clientX, e.clientY);
  };
  const ro = new ResizeObserver(debounce(draw, 150)); ro.observe(el); cleanup.push(() => ro.disconnect());
}
const teamPts = (fx, fy, tipf) => D.teams.map(t => ({ x: fx(t), y: fy(t), img: logoUrl(t, 80), color: t.tc, href: '#/team/' + t.id, tip: `<b>${esc(t.display)}</b> <span class="muted">${t.w}-${t.l}</span>${tipRows(tipf(t))}` }));
const lgAvg = k => D.teams.reduce((a, t) => a + t[k] * t.gp, 0) / D.teams.reduce((a, t) => a + t.gp, 0);

function chartOD(el) {
  scatter(el, {
    id: 'od', title: 'Offensive vs defensive rating', xl: 'Offensive rating →', yl: '← Defensive rating (better ↑)', invY: true,
    pts: teamPts(t => t.ortg, t => t.drtg, t => [['Off Rtg', fmt(t.ortg, 1) + ` (${ord(teamRank(t, 'ortg'))})`], ['Def Rtg', fmt(t.drtg, 1) + ` (${ord(teamRank(t, 'drtg', 1))})`], ['Net', fmt(t.net, 's1')]]),
    avgX: D.league.ortg, avgY: D.league.ortg, quads: ['Defense first', 'Contenders', 'Rebuilding', 'Offense first'],
    diags: [-10, -5, 0, 5, 10].map(c => ({ slope: 1, c: -c, label: (c > 0 ? '+' : '') + c + ' net', at: 0.92 })),
  });
}
async function charts() {
  ELO = ELO || await api('elo');
  const L = D.league;
  const defs = [
    ['od', 'Offense vs Defense', 'Points scored and allowed per 100 possessions. Up and to the right is good; dashed diagonals are net-rating lines.', chartOD],
    ['pace', 'Pace vs Net Rating', 'Do fast teams win? Possessions per 48 minutes against point differential per 100 possessions.', el => scatter(el, {
      id: 'pace', xl: 'Pace (poss / 48) →', yl: 'Net rating →', pts: teamPts(t => t.pace, t => t.net, t => [['Pace', fmt(t.pace, 1)], ['Net', fmt(t.net, 's1')], ['Off / Def', fmt(t.ortg, 1) + ' / ' + fmt(t.drtg, 1)]]),
      avgX: L.pace, avgY: 0, quads: ['Slow & good', 'Fast & good', 'Slow & bad', 'Fast & bad'] })],
    ['luck', 'Luck: Actual vs Expected Wins', 'Pythagorean expected wins (from points scored/allowed) against actual wins. Above the line = winning more close games than the margins suggest.', el => scatter(el, {
      id: 'luck', xl: 'Pythagorean expected wins →', yl: 'Actual wins →', square: true, pts: teamPts(t => t.pyth_w, t => t.w, t => [['Actual W', t.w], ['Expected W', fmt(t.pyth_w, 1)], ['Luck', fmt(t.luck, 's1')], ['≤5 pt games', t.close]]),
      diags: [{ slope: 1, c: 0, label: 'expected', at: .85 }], quads: ['Lucky', '', '', 'Unlucky'] })],
    ['three', 'Three-Point Profile', 'Share of shots from three against how well they make them. Size of the swing: how much of the offense lives behind the arc.', el => scatter(el, {
      id: 'three', xl: '3PA rate (share of FGA) →', yl: '3P% →', xf: v => (v * 100).toFixed(0) + '%', yf: v => (v * 100).toFixed(0) + '%',
      pts: teamPts(t => t.tpar, t => t.tpp, t => [['3PA rate', fmt(t.tpar, 'p') + '%'], ['3P%', fmt(t.tpp, 'p') + '%'], ['3PM / game', fmt(t.tpm, 1)]]),
      avgX: L.tpar, avgY: lgAvg('tpp'), quads: ['Selective & accurate', 'Volume & accurate', 'Avoid the 3', 'Volume, misfiring'] })],
    ['ffd', 'Shooting Battle', 'Effective FG% for vs against — the most important of the four factors on both ends.', el => scatter(el, {
      id: 'ffd', xl: 'eFG% →', yl: '← Opponent eFG% (better ↑)', invY: true, xf: v => (v * 100).toFixed(0) + '%', yf: v => (v * 100).toFixed(0) + '%',
      pts: teamPts(t => t.efg, t => t.o_efg, t => [['eFG%', fmt(t.efg, 'p')], ['Opp eFG%', fmt(t.o_efg, 'p')]]), avgX: L.efg, avgY: L.efg, quads: ['Stingy', 'Elite shot-making & D', 'Struggling', 'Shoot-outs'] })],
    ['sos', 'Schedule Strength vs Rating', 'SRS strength of schedule (avg opponent rating) vs schedule-adjusted SRS. Teams on the right have earned their record the hard way.', el => scatter(el, {
      id: 'sos', xl: 'Strength of schedule →', yl: 'SRS →', pts: teamPts(t => t.sos, t => t.srs, t => [['SRS', fmt(t.srs, 's2')], ['SOS', fmt(t.sos, 's2')], ['MOV', fmt(t.mov, 's2')]]), avgX: 0, avgY: 0, quads: ['Soft schedule, good', 'Tough schedule, good', 'Soft schedule, bad', 'Tough schedule, bad'] })],
    ['usg', 'Usage vs Efficiency (players)', 'Who carries the load efficiently? Usage % vs True Shooting %, players with 250+ minutes. Bigger = more minutes.', el => {
      const ps = D.players.filter(p => p.min >= 250 && p.ts != null);
      const mx = Math.max(...ps.map(p => p.min));
      scatter(el, {
        id: 'usg', xl: 'Usage % →', yl: 'True shooting % →', yf: v => (v * 100).toFixed(0) + '%', size: innerWidth < 560 ? 30 : 40, avgY: L.ts, avgX: 20,
        pts: ps.map(p => ({ x: p.usg, y: p.ts, kind: 'hs', r: 0.45 + 0.55 * Math.sqrt(p.min / mx), img: hsUrl(p, 96), color: p.t?.tc || '#888', href: '#/player/' + p.id,
          tip: `<b>${esc(p.name)}</b> <span class="muted">${esc(p.t?.abbr || '')}</span>${tipRows([['USG%', fmt(p.usg, 1)], ['TS%', fmt(p.ts, 'p')], ['PPG', fmt(p.pts, 1)], ['MIN', p.min]])}` })),
        quads: ['Efficient role player', 'Efficient star', 'Struggling', 'Volume chucker'] });
    }],
    ['pvp', 'Impact Map (players)', 'Box Plus/Minus vs Win Shares per 48 for rotation players (300+ minutes). Top-right = the league\'s most productive players.', el => {
      const ps = D.players.filter(p => p.min >= 300);
      const mx = Math.max(...ps.map(p => p.min));
      scatter(el, {
        id: 'pvp', xl: 'Box Plus/Minus →', yl: 'Win Shares / 48 →', yf: v => v.toFixed(2), size: innerWidth < 560 ? 30 : 40, avgX: 0, avgY: .1,
        pts: ps.map(p => ({ x: p.bpm, y: p.ws48, kind: 'hs', r: 0.45 + 0.55 * Math.sqrt(p.min / mx), img: hsUrl(p, 96), color: p.t?.tc || '#888', href: '#/player/' + p.id,
          tip: `<b>${esc(p.name)}</b> <span class="muted">${esc(p.t?.abbr || '')}</span>${tipRows([['BPM', fmt(p.bpm, 's1')], ['WS/48', fmt(p.ws48, 'p3')], ['WS', fmt(p.ws, 1)], ['PER', fmt(p.per, 1)]])}` })) });
    }],
    ['onoff', 'On Court vs Off Court', 'Team net rating with each player on the floor vs on the bench (same games). Above the diagonal = the team is better with him playing. 250+ on-court and 100+ off-court minutes only.', el => {
      const ps = D.players.filter(p => p.qoo);
      const mx = Math.max(...ps.map(p => p.on_min));
      scatter(el, {
        id: 'onoff', xf: v => (v > 0 ? '+' : '') + v, yf: v => (v > 0 ? '+' : '') + v, xl: 'Off-court net rating →', yl: 'On-court net rating →', square: true, size: innerWidth < 560 ? 30 : 40,
        diags: [{ slope: 1, c: 0, label: 'no difference', at: .12 }, { slope: 1, c: 10, label: '+10 swing', at: .12 }, { slope: 1, c: -10, label: '−10 swing', at: .88 }],
        quads: ['Carries a bad team', 'Good team, better with him', 'Bad team, worse with him', 'Team better without him'],
        pts: ps.map(p => ({ x: p.off_net, y: p.on_net, kind: 'hs', r: 0.45 + 0.55 * Math.sqrt(p.on_min / mx), img: hsUrl(p, 96), color: p.t?.tc || '#888', href: '#/player/' + p.id,
          tip: `<b>${esc(p.name)}</b> <span class="muted">${esc(p.t?.abbr || '')}</span>${tipRows([['On-court net', fmt(p.on_net, 's1')], ['Off-court net', fmt(p.off_net, 's1')], ['On − Off', fmt(p.oo_net, 's1')], ['Min on / off', `${p.on_min} / ${p.off_min}`]])}` })) });
    }],
    ['swing', 'Offensive vs Defensive Swing', 'How the team changes when each player sits down: on-minus-off offensive rating vs on-minus-off defensive rating. Up = the defense allows fewer points with him; right = the offense scores more. Dashed lines = total net swing.', el => {
      const ps = D.players.filter(p => p.qoo);
      const mx = Math.max(...ps.map(p => p.on_min));
      scatter(el, {
        id: 'swing', xf: v => (v > 0 ? '+' : '') + v, yf: v => (v > 0 ? '+' : '') + v, xl: 'Offensive swing (on − off ORtg) →', yl: 'Defensive swing (on − off DRtg, better ↑)', invY: true, size: innerWidth < 560 ? 30 : 40, avgX: 0, avgY: 0,
        diags: [-10, 0, 10, 20].map(c => ({ slope: 1, c: -c, label: (c > 0 ? '+' : c < 0 ? '−' : '') + Math.abs(c) + ' net', at: .93 })),
        quads: ['Defensive anchor', 'Two-way impact', 'Hurts both ends', 'Offensive engine'],
        pts: ps.map(p => ({ x: p.oo_ortg, y: p.oo_drtg, kind: 'hs', r: 0.45 + 0.55 * Math.sqrt(p.on_min / mx), img: hsUrl(p, 96), color: p.t?.tc || '#888', href: '#/player/' + p.id,
          tip: `<b>${esc(p.name)}</b> <span class="muted">${esc(p.t?.abbr || '')}</span>${tipRows([['ORtg on / off', `${fmt(p.on_ortg, 1)} / ${fmt(p.off_ortg, 1)}`], ['DRtg on / off', `${fmt(p.on_drtg, 1)} / ${fmt(p.off_drtg, 1)}`], ['Net swing', fmt(p.oo_net, 's1')]])}` })) });
    }],
    ['rapm', 'Two-Way Impact (RAPM)', 'Offensive vs defensive RAPM: each player\'s estimated points per 100 possessions added on each end, after controlling for teammates and opponents. Rotation players with 300+ minutes.', el => {
      const ps = D.players.filter(p => p.rapm != null && p.min >= 300);
      const mx = Math.max(...ps.map(p => p.min));
      scatter(el, {
        id: 'rapm', xf: v => (v > 0 ? '+' : '') + v, yf: v => (v > 0 ? '+' : '') + v, xl: 'Offensive RAPM →', yl: 'Defensive RAPM →', avgX: 0, avgY: 0, size: innerWidth < 560 ? 30 : 40,
        diags: [-4, 0, 4, 8].map(c => ({ slope: -1, c, label: (c > 0 ? '+' : c < 0 ? '−' : '') + Math.abs(c) + ' total', at: .94 })),
        quads: ['Defensive specialist', 'Two-way star', 'Negative both ends', 'Offensive specialist'],
        pts: ps.map(p => ({ x: p.rapm_o, y: p.rapm_d, kind: 'hs', r: 0.45 + 0.55 * Math.sqrt(p.min / mx), img: hsUrl(p, 96), color: p.t?.tc || '#888', href: '#/player/' + p.id,
          tip: `<b>${esc(p.name)}</b> <span class="muted">${esc(p.t?.abbr || '')}</span>${tipRows([['RAPM', fmt(p.rapm, 's1')], ['Offense', fmt(p.rapm_o, 's1')], ['Defense', fmt(p.rapm_d, 's1')], ['Raw on-off', fmt(p.oo_net, 's1')]])}` })) });
    }],
    ['shrink', 'Raw On/Off vs RAPM', 'Raw on-off net rating against teammate-adjusted RAPM. Players far right but low have been riding a great teammate (or a weak bench behind them); RAPM gives the credit back to whoever earned it.', el => {
      const ps = D.players.filter(p => p.qoo && p.rapm != null);
      const mx = Math.max(...ps.map(p => p.on_min));
      scatter(el, {
        id: 'shrink', xf: v => (v > 0 ? '+' : '') + v, yf: v => (v > 0 ? '+' : '') + v, xl: 'Raw on-off net rating →', yl: 'RAPM →', avgX: 0, avgY: 0, size: innerWidth < 560 ? 30 : 40,
        quads: ['Underrated by raw on/off', 'Truly elite', 'Negative impact', 'Helped by teammates'],
        pts: ps.map(p => ({ x: p.oo_net, y: p.rapm, kind: 'hs', r: 0.45 + 0.55 * Math.sqrt(p.on_min / mx), img: hsUrl(p, 96), color: p.t?.tc || '#888', href: '#/player/' + p.id,
          tip: `<b>${esc(p.name)}</b> <span class="muted">${esc(p.t?.abbr || '')}</span>${tipRows([['Raw on-off', fmt(p.oo_net, 's1')], ['RAPM', fmt(p.rapm, 's1')], ['Min on / off', `${p.on_min} / ${p.off_min}`]])}` })) });
    }],
    ['elo', 'Elo Through the Season', 'Every team\'s Elo after each game. Tap logos to highlight teams.', el => {
      const sorted = D.teams.slice().sort((a, b) => b.elo - a.elo);
      const hot = new Set([...sorted.slice(0, 4), sorted[sorted.length - 1]].map(t => t.id));
      el.innerHTML = '<div id="eloc"></div><div class="legend" id="eleg"></div>';
      const redraw = () => {
        lineChart($('#eloc'), { series: D.teams.map(t => ({ t, pts: ELO[t.id] })), hot, yRef: 1500, height: 420 });
        $('#eleg').innerHTML = sorted.map(t => `<button data-t="${t.id}" class="${hot.has(t.id) ? 'on' : ''}">${logoImg(t, 36)}${esc(t.abbr)} ${fmt(t.elo, 0)}</button>`).join('');
      };
      el.onclick = e => { const b = e.target.closest('button[data-t]'); if (!b) return; hot.has(b.dataset.t) ? hot.delete(b.dataset.t) : hot.add(b.dataset.t); redraw(); };
      redraw();
    }],
  ];
  app.innerHTML = `<div class="pagehead"><div><h1>Charts</h1><p>Tap or hover any logo/headshot for details; click to open.</p></div>
    <div class="controls"><select id="jump">${defs.map(d => `<option value="${d[0]}">${d[1]}</option>`).join('')}</select></div></div>
    ${defs.map(([id, t, desc]) => `<div class="card mt" id="ch-${id}"><h2>${t}</h2><p class="chart-desc">${desc}</p><div id="c-${id}"></div></div>`).join('')}`;
  $('#jump').onchange = e => $('#ch-' + e.target.value).scrollIntoView({ behavior: 'smooth' });
  defs.forEach(([id, , , fn]) => fn($('#c-' + id)));
}

function lineChart(el, o) {
  const draw = () => {
    const W = Math.max(300, el.clientWidth || 600), H = o.height || 380, small = W < 560;
    const m = { l: 44, r: small ? 12 : 44, t: 10, b: 28 };
    const all = o.series.flatMap(s => s.pts);
    const t0 = new Date(all.reduce((a, p) => p[0] < a ? p[0] : a, all[0][0]) + 'T12:00:00').getTime();
    const t1 = new Date(all.reduce((a, p) => p[0] > a ? p[0] : a, all[0][0]) + 'T12:00:00').getTime();
    let y0 = Math.min(...all.map(p => p[1])), y1 = Math.max(...all.map(p => p[1]));
    y0 -= 15; y1 += 15;
    const sx = d => m.l + (new Date(d + 'T12:00:00').getTime() - t0) / Math.max(1, t1 - t0) * (W - m.l - m.r);
    const sy = v => H - m.b - (v - y0) / (y1 - y0) * (H - m.t - m.b);
    let s = `<svg viewBox="0 0 ${W} ${H}"><g class="grid">`;
    niceTicks(y0, y1, 5).forEach(v => { s += `<line x1="${m.l}" x2="${W - m.r}" y1="${sy(v)}" y2="${sy(v)}"/><text x="${m.l - 6}" y="${sy(v) + 4}" text-anchor="end">${v}</text>`; });
    const days = [...new Set(all.map(p => p[0]))].sort();
    const step = Math.ceil(days.length / (small ? 4 : 8));
    days.forEach((d, i) => { if (i % step === 0) s += `<text x="${sx(d)}" y="${H - 8}" text-anchor="middle">${dayLabel(d, { month: 'short', day: 'numeric' })}</text>`; });
    s += '</g>';
    if (o.yRef) s += `<line class="avg" x1="${m.l}" x2="${W - m.r}" y1="${sy(o.yRef)}" y2="${sy(o.yRef)}"/>`;
    const anyHot = o.hot && o.hot.size;
    s += `<g class="${anyHot ? 'dim' : ''}">`;
    const ordered = o.series.slice().sort((a, b) => (o.hot?.has(a.t.id) ? 1 : 0) - (o.hot?.has(b.t.id) ? 1 : 0));
    for (const se of ordered) {
      const hot = o.hot?.has(se.t.id);
      const d = se.pts.map((p, i) => `${i ? 'L' : 'M'}${sx(p[0]).toFixed(1)},${sy(p[1]).toFixed(1)}`).join('');
      s += `<path class="line ${hot ? 'hot' : ''}" data-t="${se.t.id}" d="${d}" stroke="${hot ? se.t.tc : 'var(--ink3)'}"/>`;
      if (hot && !small) { const lp = se.pts[se.pts.length - 1]; s += `<image href="${logoUrl(se.t, 48)}" x="${sx(lp[0]) + 4}" y="${sy(lp[1]) - 11}" width="22" height="22"/>`; }
    }
    s += `</g><line id="xh" class="ref" y1="${m.t}" y2="${H - m.b}" x1="-10" x2="-10"/></svg>`;
    el.innerHTML = `<div class="chart">${s}</div>`;
    const svg = $('svg', el);
    svg.onmousemove = svg.ontouchstart = e => {
      const pt = e.touches ? e.touches[0] : e;
      const r = svg.getBoundingClientRect(), x = (pt.clientX - r.left) * W / r.width;
      let best = days[0]; for (const d of days) if (Math.abs(sx(d) - x) < Math.abs(sx(best) - x)) best = d;
      $('#xh', el).setAttribute('x1', sx(best)); $('#xh', el).setAttribute('x2', sx(best));
      const show = o.series.filter(se => !anyHot || o.hot.has(se.t.id)).map(se => {
        let v = null; for (const p of se.pts) if (p[0] <= best) v = p[1]; return [se.t, v];
      }).filter(x => x[1] != null).sort((a, b) => b[1] - a[1]).slice(0, 10);
      showTip(`<b>${dayLabel(best)}</b>${tipRows(show.map(([t, v]) => [`<span style="color:${t.tc}">●</span> ${esc(t.abbr)}`, v]))}`, pt.clientX, pt.clientY);
    };
    svg.onmouseleave = () => { hideTip(); $('#xh', el).setAttribute('x1', -10); $('#xh', el).setAttribute('x2', -10); };
  };
  draw();
  const ro = new ResizeObserver(debounce(draw, 150)); ro.observe(el); cleanup.push(() => ro.disconnect());
}


// ---------- BETTING ----------
const MCOL = ['#8a94a6', '#4c9bff', '#b07cff', '#2fd27a', '#ff6b2c', '#f5c542', '#ff5ea8', '#14b8a6'];
let BET = null, betModel = 7, betSeason = null, betOpen = false;
const abbr = id => T[id]?.abbr || '?';
function lineTxt(g, s) {
  if (s == null) return '–';
  if (s === 0) return 'PK';
  return s < 0 ? `${abbr(g.home)} ${s.toFixed(1)}` : `${abbr(g.away)} -${s.toFixed(1)}`;
}
const mlTxt = v => v == null ? '–' : (v > 0 ? '+' : '') + v;
const pct = v => (v * 100).toFixed(1) + '%';
function atsPick(g, m) {
  const s = g.line?.spread; if (s == null) return null;
  const e = m + s; if (e === 0) return null;
  return e > 0 ? { side: g.home, txt: `${abbr(g.home)} ${s > 0 ? '+' : ''}${s === 0 ? 'PK' : s.toFixed(1)}`, edge: Math.abs(e) }
    : { side: g.away, txt: `${abbr(g.away)} ${-s > 0 ? '+' : ''}${s === 0 ? 'PK' : (-s).toFixed(1)}`, edge: Math.abs(e) };
}
function ouPick(g, t) {
  const L = g.line?.total; if (L == null) return null;
  const e = t - L; if (e === 0) return null;
  return { over: e > 0, txt: `${e > 0 ? 'Over' : 'Under'} ${L}`, edge: Math.abs(e) };
}
const recTxt = r => r.w + r.l === 0 ? '–' : `${r.w}-${r.l}${r.p ? '-' + r.p : ''}`;
function recCell(r) {
  if (!r || r.w + r.l === 0) return '<span class="muted">–</span>';
  const p = r.w / (r.w + r.l);
  return `<span class="${p > 0.524 ? 'pos' : p < 0.476 ? 'neg' : ''}"><b>${(p * 100).toFixed(1)}%</b></span> <span class="muted">${recTxt(r)} · ${r.units > 0 ? '+' : ''}${r.units.toFixed(1)}u</span>`;
}

function betCard(g) {
  const h = T[g.home], a = T[g.away], L = g.line, P = g.p[betModel];
  const t = new Date(g.date);
  const when = t.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' }) + ' · ' + t.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const fav = P[0] >= 0 ? h : a;
  const ap = atsPick(g, P[0]), op = ouPick(g, P[1]);
  const mv = (o, c, f) => o != null && c != null && o !== c ? `<span class="muted"> (open ${f(o)})</span>` : '';
  const lineRow = L ? `<div class="bl">
      <div><span class="k">Spread</span><b>${lineTxt(g, L.spread)}</b>${mv(L.spread_open, L.spread, x => lineTxt(g, x))}</div>
      <div><span class="k">Total</span><b>${L.total ?? '–'}</b>${mv(L.total_open, L.total, x => x)}</div>
      <div><span class="k">Moneyline</span><b>${abbr(g.away)} ${mlTxt(L.mla)} / ${abbr(g.home)} ${mlTxt(L.mlh)}</b></div>
    </div>` : `<div class="bl muted">No line posted yet (ESPN/DraftKings lines usually appear the day before).</div>`;
  let impl = '';
  if (L?.mlh != null && L?.mla != null) {
    const ih = L.mlh < 0 ? -L.mlh / (-L.mlh + 100) : 100 / (L.mlh + 100), ia = L.mla < 0 ? -L.mla / (-L.mla + 100) : 100 / (L.mla + 100);
    impl = ` · market ${abbr(g.home)} ${pct(ih / (ih + ia))}`;
  }
  const models = BET.models.map((m, i) => {
    const q = g.p[i], ap2 = atsPick(g, q[0]), op2 = ouPick(g, q[1]);
    return `<tr class="${i === betModel ? 'sel' : ''}"><td class="l"><span class="dot" style="background:${MCOL[i]}"></span>${esc(m.name)}</td><td>${q[0] >= 0 ? abbr(g.home) : abbr(g.away)} -${Math.abs(q[0]).toFixed(1)}</td><td>${q[1].toFixed(1)}</td><td>${pct(q[2])}</td><td class="l">${ap2 ? `${ap2.txt} <span class="muted">${ap2.edge.toFixed(1)}</span>` : '–'}</td><td class="l">${op2 ? `${op2.txt} <span class="muted">${op2.edge.toFixed(1)}</span>` : '–'}</td></tr>`;
  }).join('');
  return `<div class="card bc">
    <div class="bh"><a href="#/team/${a.id}">${logoImg(a, 60)}</a><div class="mid"><div class="cond m">${esc(a.abbr)} <span class="muted">@</span> ${esc(h.abbr)}</div><div class="sub">${when}</div></div><a href="#/team/${h.id}">${logoImg(h, 60)}</a></div>
    ${lineRow}
    <div class="bp">
      <div><span class="k">${esc(BET.models[betModel].name)}</span><b>${esc(fav.abbr)} by ${Math.abs(P[0]).toFixed(1)}</b> · total ${P[1].toFixed(1)} · ${esc(h.abbr)} win ${pct(P[2])}${impl}</div>
      <div class="picks">${ap ? `<span class="pick ${ap.edge >= 3 ? 'strong' : ''}">ATS: ${ap.txt} <small>edge ${ap.edge.toFixed(1)}</small></span>` : ''}${op ? `<span class="pick ${op.edge >= 3 ? 'strong' : ''}">${op.txt} <small>edge ${op.edge.toFixed(1)}</small></span>` : ''}</div>
    </div>
    <details><summary>All models</summary><div class="tw"><table class="t sm"><thead><tr><th class="l">Model</th><th>Spread</th><th>Total</th><th>${esc(h.abbr)} win</th><th class="l">ATS pick</th><th class="l">O/U pick</th></tr></thead><tbody>${models}</tbody></table></div></details>
  </div>`;
}

function multiLine(el, o) {
  const draw = () => {
    const W = Math.max(300, el.clientWidth || 600), H = o.height || 320, small = W < 560;
    const m = { l: 46, r: 14, t: 10, b: 30 };
    const all = o.series.flatMap(s => s.pts);
    if (!all.length) { el.innerHTML = '<div class="muted">No data yet</div>'; return; }
    const xs = o.xs; // array of x keys (ordered)
    const xi = new Map(xs.map((x, i) => [x, i]));
    let y0 = Math.min(o.yRef ?? Infinity, ...all.map(p => p[1])), y1 = Math.max(o.yRef ?? -Infinity, ...all.map(p => p[1]));
    const pad = (y1 - y0) * 0.08 || 1; y0 -= pad; y1 += pad;
    const sx = x => m.l + xi.get(x) / Math.max(1, xs.length - 1) * (W - m.l - m.r);
    const sy = v => H - m.b - (v - y0) / (y1 - y0) * (H - m.t - m.b);
    let s = `<svg viewBox="0 0 ${W} ${H}"><g class="grid">`;
    niceTicks(y0, y1, 5).forEach(v => { s += `<line x1="${m.l}" x2="${W - m.r}" y1="${sy(v)}" y2="${sy(v)}"/><text x="${m.l - 6}" y="${sy(v) + 4}" text-anchor="end">${o.yf ? o.yf(v) : v}</text>`; });
    const step = Math.ceil(xs.length / (small ? 4 : 8));
    xs.forEach((x, i) => { if (i % step === 0 || o.allTicks) s += `<text x="${sx(x)}" y="${H - 8}" text-anchor="middle">${o.xf ? o.xf(x) : x}</text>`; });
    s += '</g>';
    if (o.yRef != null) s += `<line class="ref" x1="${m.l}" x2="${W - m.r}" y1="${sy(o.yRef)}" y2="${sy(o.yRef)}"/>`;
    const anyHot = o.hot && o.hot.size;
    for (const se of o.series) {
      const hot = !anyHot || o.hot.has(se.key);
      const d = se.pts.map((p, i) => `${i ? 'L' : 'M'}${sx(p[0]).toFixed(1)},${sy(p[1]).toFixed(1)}`).join('');
      s += `<path class="line" d="${d}" stroke="${se.color}" style="opacity:${hot ? 1 : .12};stroke-width:${hot ? 2.2 : 1.2}"/>`;
      if (o.dots) se.pts.forEach(p => { s += `<circle cx="${sx(p[0])}" cy="${sy(p[1])}" r="3" fill="${se.color}" style="opacity:${hot ? 1 : .15}"/>`; });
    }
    s += `<line class="xh ref" y1="${m.t}" y2="${H - m.b}" x1="-10" x2="-10"/></svg>`;
    el.innerHTML = `<div class="chart">${s}</div>`;
    const svg = $('svg', el), xh = $('.xh', el);
    svg.onmousemove = svg.ontouchstart = e => {
      const pt = e.touches ? e.touches[0] : e;
      const r = svg.getBoundingClientRect(), x = (pt.clientX - r.left) * W / r.width;
      let best = xs[0]; for (const k of xs) if (Math.abs(sx(k) - x) < Math.abs(sx(best) - x)) best = k;
      xh.setAttribute('x1', sx(best)); xh.setAttribute('x2', sx(best));
      const rows = o.series.filter(se => !anyHot || o.hot.has(se.key)).map(se => { let v = null; for (const p of se.pts) if (xi.get(p[0]) <= xi.get(best)) v = p[1]; return [se, v]; })
        .filter(x => x[1] != null).sort((a, b) => b[1] - a[1]);
      showTip(`<b>${o.xf ? o.xf(best) : best}</b>${tipRows(rows.map(([se, v]) => [`<span style="color:${se.color}">●</span> ${esc(se.name)}`, o.yf ? o.yf(v) : v]))}`, pt.clientX, pt.clientY);
    };
    svg.onmouseleave = () => { hideTip(); xh.setAttribute('x1', -10); xh.setAttribute('x2', -10); };
  };
  draw();
  const ro = new ResizeObserver(debounce(draw, 150)); ro.observe(el); cleanup.push(() => ro.disconnect());
}

async function betting() {
  BET = await api('betting');
  if (!BET.models) { app.innerHTML = '<div class="loading">Betting models are still being computed — try again in a minute.</div>'; return; }
  const B = BET;
  const seasons = B.backtest.map(b => b.season);
  if (betSeason == null || !seasons.includes(betSeason)) betSeason = seasons[seasons.length - 1];
  const cnt = B.counts.by_season.map(s => `${s.season - 1}-${String(s.season % 100).padStart(2, '0')}: ${s.games} games${s.lines ? `, ${s.lines} with lines` : ''}`).join(' · ');
  app.innerHTML = `<div class="pagehead"><div><h1>Betting Lab</h1><p>DraftKings lines (via ESPN) vs. eight prediction models, plus an honest walk-forward backtest against closing lines.</p></div>
      <div class="controls"><select id="bm">${B.models.map((m, i) => `<option value="${i}" ${i === betModel ? 'selected' : ''}>${esc(m.name)}</option>`).join('')}</select></div></div>
    <h2>Upcoming games with lines</h2><div class="grid g2" id="up"></div><div id="up2"></div>
    <div class="card mt"><div class="hd"><h2>Backtest vs real lines</h2><div class="controls"><div class="seg" id="bo"><button data-o="0" class="${betOpen ? '' : 'on'}">vs closing</button><button data-o="1" class="${betOpen ? 'on' : ''}">vs opening</button></div><div class="seg" id="bs">${B.backtest.map(b => `<button data-s="${b.season}" class="${b.season === betSeason ? 'on' : ''}">${b.label}</button>`).join('')}</div></div></div>
      <p class="chart-desc">Every prediction made only from games on <i>earlier days</i>. ATS/O-U columns: picking the side the model prefers vs the closing (or opening) line (all games, then only when the model disagrees by 3+ / 6+ points), units at -110. Break-even is 52.4%. ML: betting the model's side when its win probability beats the no-vig market by 3+ points, at the actual moneyline.</p>
      <div id="bt"></div></div>
    <div class="grid g2 mt">
      <div class="card"><h2>Cumulative ATS profit</h2><p class="chart-desc">Units won betting every game against the spread at -110, by model (${esc(B.backtest.map(b => b.label).join(' + '))}). Tap a model to highlight.</p><div id="cu"></div><div class="legend" id="cul"></div></div>
      <div class="card"><h2>Does recency matter?</h2><p class="chart-desc">Margin error of the rating models vs how fast old games fade (half-life in days; "∞" = every game counts equally). Lower is better. Games with lines only.</p><div id="rc"></div><div id="rct"></div></div>
    </div>
    <div class="card mt"><div class="hd"><h2>Recent results</h2><span class="sub">selected model vs closing line</span></div><div id="rr"></div></div>
    <div class="card mt"><h2>Models</h2><div class="tw"><table class="t"><tbody>${B.models.map((m, i) => `<tr><td class="l"><span class="dot" style="background:${MCOL[i]}"></span><b>${esc(m.name)}</b></td><td class="l wrap">${esc(m.desc)}</td></tr>`).join('')}</tbody></table></div>
      <p class="chart-desc mt"><b>No peeking:</b> games are processed one calendar day at a time; all predictions for a day are made before any of that day's results are added. Ratings are re-fit every day from earlier games only; ridge / neural-net models are trained only on earlier games' features and outcomes (feature scaling also from that training set), retrained weekly. Hyper-parameters (Elo K, half-life, ridge penalties) were tuned on 2024-25 only, which has no lines and isn't in this backtest. Training data: ${cnt}.</p>
      <p class="chart-desc">For fun / research only. Even good models rarely beat closing lines after the vig.</p></div>`;
  $('#bm').onchange = e => { betModel = +e.target.value; drawUp(); drawRecent(); };
  const drawUp = () => {
    const lined = B.upcoming.filter(g => g.line), rest = B.upcoming.filter(g => !g.line);
    $('#up').innerHTML = lined.length ? lined.map(betCard).join('') : '<div class="card muted">No lines posted right now — ESPN/DraftKings lines usually appear the day before tip-off. Model predictions for the week are below.</div>';
    const q = i => g => g.p[betModel][i];
    $('#up2').innerHTML = rest.length ? `<div class="card flush mt"><div class="hd"><h3>Later this week · model only (no line yet)</h3></div><div class="tw"><table class="t sm"><thead><tr><th class="l">Date</th><th class="l">Game</th><th>Model spread</th><th>Model total</th><th>Home win</th></tr></thead><tbody>${rest.map(g => `<tr><td class="l">${dayLabel(g.day)}</td><td class="l">${esc(abbr(g.away))} @ ${esc(abbr(g.home))}</td><td>${q(0)(g) >= 0 ? abbr(g.home) : abbr(g.away)} -${Math.abs(q(0)(g)).toFixed(1)}</td><td>${q(1)(g).toFixed(1)}</td><td>${pct(q(2)(g))}</td></tr>`).join('')}</tbody></table></div></div>` : '';
  };
  const drawBt = () => {
    const bt = B.backtest.find(b => b.season === betSeason);
    const row = (name, color, a, i) => `<tr class="${i === betModel ? 'sel' : ''}"><td class="l stick">${color ? `<span class="dot" style="background:${color}"></span>` : ''}<b>${esc(name)}</b></td>
      <td>${a.n}</td><td>${a.mae.toFixed(2)}</td><td>${a.tmae.toFixed(2)}</td><td>${pct(a.su)}</td>
      ${i < 0 ? `<td colspan="7" class="l muted">benchmark: the ${betOpen ? 'opening' : 'closing'} line itself</td>` : `
      <td>${recCell(a.ats[0])}</td><td>${recCell(a.ats[1])}</td><td>${recCell(a.ats[2])}</td>
      <td>${recCell(a.ou[0])}</td><td>${recCell(a.ou[1])}</td><td>${recCell(a.ou[2])}</td>
      <td>${a.ml.n ? `<span class="${a.ml.units > 0 ? 'pos' : 'neg'}"><b>${a.ml.units > 0 ? '+' : ''}${a.ml.units.toFixed(1)}u</b></span> <span class="muted">${a.ml.w}/${a.ml.n}</span>` : '–'}</td>`}</tr>`;
    const mk = betOpen ? bt.market_open : bt.market, ms = betOpen ? bt.models_open : bt.models;
    $('#bt').innerHTML = `<div class="tw"><table class="t bt"><thead><tr><th class="l stick">Model</th><th title="games with a closing line">N</th><th title="mean absolute error of predicted margin">Margin MAE</th><th title="mean absolute error of predicted total">Total MAE</th><th title="straight-up winner accuracy">SU</th>
      <th>ATS all</th><th>ATS 3+</th><th>ATS 6+</th><th>O/U all</th><th>O/U 3+</th><th>O/U 6+</th><th>ML</th></tr></thead>
      <tbody>${row(betOpen ? 'Opening line' : 'Closing line', null, mk, -1)}${ms.map((a, i) => row(B.models[i].name, MCOL[i], a, i)).join('')}</tbody></table></div>`;
  };
  const hot = new Set([betModel]);
  const drawCum = () => {
    const days = [...new Set(B.series.flatMap(s => s.map(p => p[0])))].sort();
    multiLine($('#cu'), { xs: days, xf: d => dayLabel(d, { month: 'short', day: 'numeric', year: '2-digit' }), yRef: 0, hot, yf: v => (v > 0 ? '+' : '') + (+v).toFixed(0),
      series: B.series.map((s, i) => ({ key: i, name: B.models[i].name, color: MCOL[i], pts: s })) });
    $('#cul').innerHTML = B.models.map((m, i) => `<button data-i="${i}" class="${hot.has(i) ? 'on' : ''}"><span class="dot" style="background:${MCOL[i]}"></span>${esc(m.name)} ${(() => { const s = B.series[i]; const v = s.length ? s[s.length - 1][1] : 0; return `<b class="${v > 0 ? 'pos' : 'neg'}">${v > 0 ? '+' : ''}${v.toFixed(1)}</b>`; })()}</button>`).join('');
  };
  $('#cul').onclick = e => { const b = e.target.closest('button'); if (!b) return; const i = +b.dataset.i; hot.has(i) ? hot.delete(i) : hot.add(i); drawCum(); };
  const drawRec = () => {
    const xs = B.recency.map(r => r.hl == null ? '∞' : String(r.hl));
    multiLine($('#rc'), { xs, allTicks: true, dots: true, height: 260, yf: v => (+v).toFixed(2),
      series: [{ key: 'srs', name: 'SRS margin MAE', color: MCOL[3], pts: B.recency.map((r, i) => [xs[i], r.srs]) },
               { key: 'eff', name: 'Efficiency margin MAE', color: MCOL[4], pts: B.recency.map((r, i) => [xs[i], r.eff]) }] });
    const best = B.recency.reduce((a, r) => r.eff < a.eff ? r : a);
    const inf = B.recency.find(r => r.hl == null);
    $('#rct').innerHTML = `<p class="chart-desc">Best half-life here: <b>${best.hl ?? '∞'} days</b> (eff MAE ${best.eff.toFixed(2)} vs ${inf.eff.toFixed(2)} with no recency). The live models use ${B.params.hl} days, chosen on 2024-25.</p>`;
  };
  const drawRecent = () => {
    const rows = B.recent.slice(0, 80).map(g => {
      const q = g.p[betModel], am = g.hs - g.as, at = g.hs + g.as;
      const ap = atsPick(g, q[0]), op = ouPick(g, q[1]);
      const s = g.line?.spread;
      let ar = '–', or = '–';
      if (ap) { const res = am + s; ar = res === 0 ? '<span class="muted">push</span>' : ((res > 0) === (ap.side === g.home) ? '<span class="pos">✓</span>' : '<span class="neg">✗</span>'); ar = `${ap.txt} ${ar}`; }
      if (op) { const res = at - g.line.total; or = res === 0 ? '<span class="muted">push</span>' : ((res > 0) === op.over ? '<span class="pos">✓</span>' : '<span class="neg">✗</span>'); or = `${op.txt} ${or}`; }
      return `<tr><td class="l">${dayLabel(g.day, { month: 'short', day: 'numeric' })}</td><td class="l"><a href="#/game/${g.id}">${esc(abbr(g.away))} ${g.as} @ ${esc(abbr(g.home))} ${g.hs}</a></td>
        <td>${lineTxt(g, s)}</td><td>${g.line?.total ?? '–'}</td><td>${q[0] >= 0 ? abbr(g.home) : abbr(g.away)} -${Math.abs(q[0]).toFixed(1)}</td><td>${q[1].toFixed(1)}</td><td class="l">${ar}</td><td class="l">${or}</td></tr>`;
    }).join('');
    $('#rr').innerHTML = `<div class="tw"><table class="t sm"><thead><tr><th class="l">Date</th><th class="l">Final</th><th>Line</th><th>O/U</th><th>Model</th><th>Model tot.</th><th class="l">ATS pick</th><th class="l">O/U pick</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  };
  $('#bo').onclick = e => { const b = e.target.closest('button'); if (!b) return; betOpen = b.dataset.o === '1'; $('#bo').querySelectorAll('button').forEach(x => x.classList.toggle('on', x === b)); drawBt(); };
  $('#bs').onclick = e => { const b = e.target.closest('button'); if (!b) return; betSeason = +b.dataset.s; $('#bs').querySelectorAll('button').forEach(x => x.classList.toggle('on', x === b)); drawBt(); };
  const oldBm = $('#bm').onchange;
  $('#bm').onchange = e => { oldBm(e); drawBt(); hot.clear(); hot.add(betModel); drawCum(); };
  drawUp(); drawBt(); drawCum(); drawRec(); drawRecent();
}

// ---------- boot ----------
addEventListener('hashchange', route);
load().then(route).catch(e => { app.innerHTML = `<div class="loading">Failed to load data: ${esc(e.message)}</div>`; });
