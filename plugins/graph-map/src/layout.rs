//! Graph map layout: positions for the notes of the link graph.
//!
//! The cold layout is the app's own algorithm, unchanged: a seeded
//! Fruchterman-Reingold on the linked notes, a fixed number of iterations,
//! scaled into the unit disc (radius <= 0.8); notes without links
//! ("orphans") sit evenly on a ring of radius 1.0 around it. Same names and
//! edges give the same picture.
//!
//! On top of that, layouts are cached through the host: the exact layout
//! under a key over the names and edges (a hit costs nothing), and the
//! latest layout under a fixed key, used as a *warm start* when the
//! library changed: notes that were there keep their place and only a few
//! iterations settle the change. A warm picture depends on history, not
//! only on names and edges; once cached it is stable.

use crate::host;
use std::collections::HashMap;

/// Iterations of the force simulation (fixed, so time is predictable).
pub const ITERATIONS: usize = 160;
/// Iterations and starting temperature of a warm start.
pub const WARM_ITERATIONS: usize = 30;
/// Fewest iterations of a cold approximate layout.
const MIN_ITERATIONS: usize = 60;
/// Fewest iterations of a "Quick" layout, which halves the usual number.
const QUICK_MIN_ITERATIONS: usize = 20;
/// Mixed into the cache key of a Quick layout, so the two never share one.
const QUICK_KEY: u64 = 0x9e37_79b9_7f4a_7c15;
const WARM_TEMP: f64 = 0.03;
/// Radius of the orphan ring; linked notes stay inside `INNER`.
pub const RING: f64 = 1.0;
pub const INNER: f64 = 0.8;
/// Saved positions are scaled into the disc; the force simulation works
/// in a square of side 1, whose notes spread about 0.5 from the centre.
const UNSCALE: f64 = 0.5 / INNER;
/// Up to this many linked notes the app's exact all-pairs repulsion runs
/// (so small libraries draw exactly what the built-in Graph draws). Above
/// it, repulsion is approximated with a quadtree (Barnes-Hut): the exact
/// loop is quadratic, and the WASM interpreter is about 45 times slower
/// than native code.
pub const EXACT_MAX_LINKED: usize = 150;
/// Barnes-Hut opening angle: a group of notes counts as one when its size
/// over its distance is below this.
const THETA: f64 = 1.2;
/// A warm start needs this share of the linked notes to be known.
const WARM_MIN_KNOWN: f64 = 0.5;
/// Seed of the map layout: fixed, so the same notes give the same picture.
pub const MAP_SEED: u64 = 0x5349_474d_414d_4150;

const LATEST: u64 = 0;
const MAGIC: u32 = 0x5041_4d47; // "GMAP"

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapLayout {
    /// Unit-space position of every note (x right, y down).
    pub pos: Vec<(f64, f64)>,
    pub orphan: Vec<bool>,
}

fn fnv(h: &mut u64, b: &[u8]) {
    for &x in b {
        *h ^= u64::from(x);
        *h = h.wrapping_mul(0x0100_0000_01b3);
    }
}

/// Cache key over the note names and edges (FNV-1a, 64 bit).
#[must_use]
pub fn layout_key(names: &[String], edges: &[(usize, usize)]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for n in names {
        fnv(&mut h, n.as_bytes());
        fnv(&mut h, &[0]);
    }
    fnv(&mut h, &[0xff]);
    for &(a, b) in edges {
        fnv(&mut h, &(a as u64).to_le_bytes());
        fnv(&mut h, &(b as u64).to_le_bytes());
    }
    h
}

fn name_hash(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fnv(&mut h, name.as_bytes());
    h
}

/// xorshift64*: tiny deterministic generator for the starting positions.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Where a note was last time, by note index.
pub type Known<'a> = &'a dyn Fn(usize) -> Option<(f64, f64)>;

/// Lays out `n` notes joined by directed `edges` (direction is ignored for
/// the forces). `seed` fixes the starting positions.
#[must_use]
pub fn layout(n: usize, edges: &[(usize, usize)], seed: u64) -> MapLayout {
    layout_from(n, edges, seed, None)
}

fn layout_from(n: usize, edges: &[(usize, usize)], seed: u64, known: Option<Known>) -> MapLayout {
    layout_with(n, edges, seed, known, EXACT_MAX_LINKED)
}

/// [`layout_from`] with the number of linked notes up to which repulsion is
/// exact (tests force either algorithm).
fn layout_with(
    n: usize,
    edges: &[(usize, usize)],
    seed: u64,
    known: Option<Known>,
    exact_max: usize,
) -> MapLayout {
    let mut job = Job::new(n, edges, seed, known, exact_max, false);
    job.step(usize::MAX);
    job.finish()
}

/// A layout in progress. The force simulation runs a fixed number of
/// iterations; `step` runs some of them and can be called again later, so a
/// long layout can show progress and be left. Running every step gives
/// exactly the layout `layout` returns.
pub struct Job {
    n: usize,
    pairs: Vec<(usize, usize)>,
    slot: Vec<usize>,
    linked: Vec<usize>,
    lone: Vec<usize>,
    p: Vec<(f64, f64)>,
    disp: Vec<(f64, f64)>,
    k: f64,
    iterations: usize,
    done: usize,
    temp: f64,
    cool: f64,
    approx: bool,
    /// Started from the last picture, not fresh.
    pub warm: bool,
}

impl Job {
    fn new(
        n: usize,
        edges: &[(usize, usize)],
        seed: u64,
        known: Option<Known>,
        exact_max: usize,
        quick: bool,
    ) -> Self {
        let mut deg = vec![0usize; n];
        let mut pairs: Vec<(usize, usize)> = edges
            .iter()
            .filter(|&&(a, b)| a != b && a < n && b < n)
            .map(|&(a, b)| (a.min(b), a.max(b)))
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        for &(a, b) in &pairs {
            deg[a] += 1;
            deg[b] += 1;
        }
        let linked: Vec<usize> = (0..n).filter(|&i| deg[i] > 0).collect();
        let lone: Vec<usize> = (0..n).filter(|&i| deg[i] == 0).collect();
        let m = linked.len();
        let mut slot = vec![usize::MAX; n];
        for (k, &i) in linked.iter().enumerate() {
            slot[i] = k;
        }
        let mut job = Self {
            n,
            pairs,
            slot,
            linked,
            lone,
            p: Vec::new(),
            disp: Vec::new(),
            k: 0.0,
            iterations: 0,
            done: 0,
            temp: 0.0,
            cool: 0.0,
            approx: m > exact_max,
            warm: false,
        };
        if m == 0 {
            return job;
        }
        // Force layout over the linked notes, in a square of side 1.
        let mut rng = Rng(seed | 1);
        let (p, mut iterations, temp) = match known {
            Some(f) => match warm_start(&job.linked, &job.pairs, &job.slot, f, &mut rng) {
                Some(p) => {
                    job.warm = true;
                    (p, WARM_ITERATIONS, WARM_TEMP)
                }
                None => (cold_start(m, &mut rng), ITERATIONS, 0.1),
            },
            None => (cold_start(m, &mut rng), ITERATIONS, 0.1),
        };
        // The approximate regime also settles in fewer iterations: they
        // cost more the more notes there are. (A warm start has few already.)
        if m > exact_max && iterations == ITERATIONS {
            let scale = (exact_max as f64 / m as f64).sqrt();
            iterations = ((ITERATIONS as f64 * scale) as usize).clamp(MIN_ITERATIONS, ITERATIONS);
        }
        if quick {
            iterations = (iterations / 2).max(QUICK_MIN_ITERATIONS);
        }
        job.p = p;
        job.disp = vec![(0.0f64, 0.0f64); m];
        job.k = (1.0 / m as f64).sqrt();
        job.iterations = iterations;
        job.temp = temp;
        job.cool = temp / (iterations as f64 + 1.0);
        job
    }

    /// Iterations in all, and how many have run.
    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        (self.done, self.iterations)
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done >= self.iterations
    }

    /// Runs up to `iters` more iterations.
    pub fn step(&mut self, iters: usize) {
        let m = self.p.len();
        let k = self.k;
        for _ in 0..iters.min(self.iterations - self.done) {
            let (p, disp) = (&mut self.p, &mut self.disp);
            disp.iter_mut().for_each(|d| *d = (0.0, 0.0));
            if self.approx {
                repel_tree(p, k * k, disp);
            } else {
                for a in 0..m {
                    for b in (a + 1)..m {
                        let (dx, dy) = (p[a].0 - p[b].0, p[a].1 - p[b].1);
                        let d2 = (dx * dx + dy * dy).max(1e-6);
                        let f = k * k / d2; // repulsion k²/d, along the unit vector
                        disp[a].0 += dx * f;
                        disp[a].1 += dy * f;
                        disp[b].0 -= dx * f;
                        disp[b].1 -= dy * f;
                    }
                }
            }
            for &(a, b) in &self.pairs {
                let (a, b) = (self.slot[a], self.slot[b]);
                let (dx, dy) = (p[a].0 - p[b].0, p[a].1 - p[b].1);
                let d = (dx * dx + dy * dy).sqrt().max(1e-3);
                let f = d / k; // attraction d²/k
                disp[a].0 -= dx * f;
                disp[a].1 -= dy * f;
                disp[b].0 += dx * f;
                disp[b].1 += dy * f;
            }
            // Mild gravity keeps separate clusters on screen.
            for (q, d) in p.iter().zip(disp.iter_mut()) {
                d.0 -= q.0 * 0.6 * k * m as f64;
                d.1 -= q.1 * 0.6 * k * m as f64;
            }
            for (q, d) in p.iter_mut().zip(disp.iter()) {
                let len = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-9);
                let step = len.min(self.temp);
                q.0 += d.0 / len * step;
                q.1 += d.1 / len * step;
            }
            self.temp -= self.cool;
            self.done += 1;
        }
    }

    /// Runs what is left and returns the finished layout.
    #[must_use]
    pub fn finish(mut self) -> MapLayout {
        self.step(usize::MAX);
        let (n, m) = (self.n, self.p.len());
        let mut pos = vec![(0.0, 0.0); n];
        let mut orphan = vec![false; n];
        if m > 0 {
            // Centre and scale into the inner disc.
            let p = &self.p;
            let (cx, cy) = p.iter().fold((0.0, 0.0), |a, q| {
                (a.0 + q.0 / m as f64, a.1 + q.1 / m as f64)
            });
            let r = p
                .iter()
                .map(|q| ((q.0 - cx).powi(2) + (q.1 - cy).powi(2)).sqrt())
                .fold(0.0f64, f64::max)
                .max(1e-6);
            let s = if m == 1 { 0.0 } else { INNER / r };
            for (kk, &i) in self.linked.iter().enumerate() {
                pos[i] = ((p[kk].0 - cx) * s, (p[kk].1 - cy) * s);
            }
        }
        // Orphans: evenly on the ring, in library order, starting at the top.
        let ring = if m == 0 { INNER } else { RING };
        for (k, &i) in self.lone.iter().enumerate() {
            let a = -std::f64::consts::FRAC_PI_2
                + std::f64::consts::TAU * k as f64 / self.lone.len().max(1) as f64;
            pos[i] = (ring * a.cos(), ring * a.sin());
            orphan[i] = true;
        }
        MapLayout { pos, orphan }
    }
}

const MAX_DEPTH: usize = 24;
const NO_KID: usize = usize::MAX;

/// A quadtree square: how many notes it holds, their centre of mass, its
/// four quarters, and (for a leaf) its notes as a range of the tree's order.
struct Quad {
    cx: f64,
    cy: f64,
    half: f64,
    mass: f64,
    com: (f64, f64),
    kids: [usize; 4],
    start: usize,
    len: usize,
}

/// Builds the square holding `order` (the notes in that square, which sit at
/// `base..` of the full order) and, in place, sorts them into quarters.
fn build_tree(
    nodes: &mut Vec<Quad>,
    p: &[(f64, f64)],
    order: &mut [usize],
    scratch: &mut [usize],
    base: usize,
    (cx, cy, half): (f64, f64, f64),
    depth: usize,
) -> usize {
    let len = order.len();
    let (sx, sy) = order
        .iter()
        .fold((0.0, 0.0), |a, &i| (a.0 + p[i].0, a.1 + p[i].1));
    let id = nodes.len();
    nodes.push(Quad {
        cx,
        cy,
        half,
        mass: len as f64,
        com: (sx / len as f64, sy / len as f64),
        kids: [NO_KID; 4],
        start: base,
        len,
    });
    if len <= 1 || depth >= MAX_DEPTH {
        return id;
    }
    let quarter = |i: usize| usize::from(p[i].0 >= cx) + 2 * usize::from(p[i].1 >= cy);
    let mut counts = [0usize; 4];
    for &i in order.iter() {
        counts[quarter(i)] += 1;
    }
    let mut next = [
        0,
        counts[0],
        counts[0] + counts[1],
        counts[0] + counts[1] + counts[2],
    ];
    for &i in order.iter() {
        let q = quarter(i);
        scratch[next[q]] = i;
        next[q] += 1;
    }
    order.copy_from_slice(&scratch[..len]);
    let h = half / 2.0;
    let mut rest = order;
    let mut at = base;
    for (q, &n) in counts.iter().enumerate() {
        let (mine, tail) = rest.split_at_mut(n);
        rest = tail;
        if n > 0 {
            let c = (
                cx + if q & 1 == 1 { h } else { -h },
                cy + if q & 2 == 2 { h } else { -h },
                h,
            );
            let kid = build_tree(nodes, p, mine, &mut scratch[..n], at, c, depth + 1);
            nodes[id].kids[q] = kid;
        }
        at += n;
    }
    id
}

/// Repulsion k²/d on every note, with distant groups counted as one note at
/// their centre of mass (Barnes-Hut). Near notes and leaves are exact.
fn repel_tree(p: &[(f64, f64)], k2: f64, disp: &mut [(f64, f64)]) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for q in p {
        x0 = x0.min(q.0);
        y0 = y0.min(q.1);
        x1 = x1.max(q.0);
        y1 = y1.max(q.1);
    }
    let half = ((x1 - x0).max(y1 - y0) / 2.0).max(1e-9) * 1.0001;
    let mut nodes = Vec::with_capacity(p.len() * 2);
    let mut order: Vec<usize> = (0..p.len()).collect();
    let mut scratch = vec![0usize; p.len()];
    build_tree(
        &mut nodes,
        p,
        &mut order,
        &mut scratch,
        0,
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0, half),
        0,
    );
    let mut stack = Vec::with_capacity(64);
    for (a, d) in disp.iter_mut().enumerate() {
        stack.push(0usize);
        while let Some(n) = stack.pop() {
            let q = &nodes[n];
            if q.kids == [NO_KID; 4] {
                for &b in order[q.start..q.start + q.len].iter().filter(|&&b| b != a) {
                    let (dx, dy) = (p[a].0 - p[b].0, p[a].1 - p[b].1);
                    let f = k2 / (dx * dx + dy * dy).max(1e-6);
                    d.0 += dx * f;
                    d.1 += dy * f;
                }
                continue;
            }
            let (dx, dy) = (p[a].0 - q.com.0, p[a].1 - q.com.1);
            let d2 = dx * dx + dy * dy;
            let inside = (p[a].0 - q.cx).abs() <= q.half && (p[a].1 - q.cy).abs() <= q.half;
            if !inside && 4.0 * q.half * q.half < THETA * THETA * d2 {
                let f = q.mass * k2 / d2.max(1e-6);
                d.0 += dx * f;
                d.1 += dy * f;
            } else {
                stack.extend(q.kids.iter().copied().filter(|&c| c != NO_KID));
            }
        }
    }
}

fn cold_start(m: usize, rng: &mut Rng) -> Vec<(f64, f64)> {
    (0..m)
        .map(|_| (rng.next() - 0.5, rng.next() - 0.5))
        .collect()
}

/// Starting positions from a previous picture: known notes where they were,
/// new notes beside the notes they link to. `None` when too few are known.
fn warm_start(
    linked: &[usize],
    pairs: &[(usize, usize)],
    slot: &[usize],
    known: Known,
    rng: &mut Rng,
) -> Option<Vec<(f64, f64)>> {
    let m = linked.len();
    let prev: Vec<Option<(f64, f64)>> = linked
        .iter()
        .map(|&i| known(i).map(|(x, y)| (x * UNSCALE, y * UNSCALE)))
        .collect();
    let have = prev.iter().filter(|p| p.is_some()).count();
    if (have as f64) < WARM_MIN_KNOWN * m as f64 {
        return None;
    }
    let mut near: Vec<Vec<usize>> = vec![Vec::new(); m];
    for &(a, b) in pairs {
        near[slot[a]].push(slot[b]);
        near[slot[b]].push(slot[a]);
    }
    let mut p: Vec<(f64, f64)> = prev.iter().map(|q| q.unwrap_or((0.0, 0.0))).collect();
    for k in 0..m {
        if prev[k].is_some() {
            continue;
        }
        let around: Vec<(f64, f64)> = near[k].iter().filter_map(|&j| prev[j]).collect();
        let (jx, jy) = ((rng.next() - 0.5) * 0.06, (rng.next() - 0.5) * 0.06);
        p[k] = if around.is_empty() {
            (rng.next() - 0.5, rng.next() - 0.5)
        } else {
            let c = around.len() as f64;
            (
                around.iter().map(|q| q.0).sum::<f64>() / c + jx,
                around.iter().map(|q| q.1).sum::<f64>() / c + jy,
            )
        };
    }
    Some(p)
}

fn encode(names: &[String], l: &MapLayout) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + names.len() * 25);
    b.extend_from_slice(&MAGIC.to_le_bytes());
    b.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for (i, n) in names.iter().enumerate() {
        b.extend_from_slice(&name_hash(n).to_le_bytes());
        b.extend_from_slice(&l.pos[i].0.to_le_bytes());
        b.extend_from_slice(&l.pos[i].1.to_le_bytes());
        b.push(u8::from(l.orphan[i]));
    }
    b
}

/// One saved note: its name hash, place and orphan flag.
type Saved = (u64, (f64, f64), bool);

/// A saved picture.
fn decode(b: &[u8]) -> Option<Vec<Saved>> {
    let n = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
    if b.get(..4)? != MAGIC.to_le_bytes() || b.len() != 8 + n.checked_mul(25)? {
        return None;
    }
    let f = |at: usize| f64::from_le_bytes(b[at..at + 8].try_into().unwrap_or([0; 8]));
    Some(
        (0..n)
            .map(|i| {
                let o = 8 + i * 25;
                let h = u64::from_le_bytes(b[o..o + 8].try_into().unwrap_or([0; 8]));
                (h, (f(o + 8), f(o + 16)), b[o + 24] != 0)
            })
            .collect(),
    )
}

/// A layout being built for these notes, to be finished and cached.
pub struct Build {
    pub job: Job,
    key: u64,
    names: Vec<String>,
}

impl Build {
    /// Runs what is left, caches the picture and returns it.
    #[must_use]
    pub fn finish(self) -> MapLayout {
        let l = self.job.finish();
        let blob = encode(&self.names, &l);
        host::store(self.key, &blob);
        host::store(LATEST, &blob);
        l
    }
}

/// How a layout request starts.
pub enum Start {
    /// The library is unchanged: the saved picture, at no cost.
    Ready(MapLayout),
    /// A layout to run (warm-started when the library changed a little).
    Build(Box<Build>),
}

/// The layout for these notes: from the cache when the library is
/// unchanged, otherwise a job (a warm start when it changed a little, else
/// a fresh one) for the caller to run in steps. `quick` halves the
/// iterations; a quick layout has its own cache entry.
#[must_use]
pub fn start(names: &[String], edges: &[(usize, usize)], quick: bool) -> Start {
    let key = layout_key(names, edges) ^ if quick { QUICK_KEY } else { 0 };
    let hashes: Vec<u64> = names.iter().map(|n| name_hash(n)).collect();
    let blob = host::load(key);
    match blob.as_deref().map(decode) {
        Some(Some(saved)) => {
            if saved.len() == names.len() && saved.iter().zip(&hashes).all(|(s, h)| s.0 == *h) {
                log!(
                    host::DEBUG,
                    "layout: saved picture found ({} notes)",
                    names.len()
                );
                return Start::Ready(MapLayout {
                    pos: saved.iter().map(|s| s.1).collect(),
                    orphan: saved.iter().map(|s| s.2).collect(),
                });
            }
            log!(
                host::DEBUG,
                "layout: the library changed since the saved picture"
            );
        }
        Some(None) => log!(
            host::WARNING,
            "layout: the saved picture is damaged; ignoring it"
        ),
        None => log!(host::DEBUG, "layout: no saved picture for this library"),
    }
    let last = host::load(LATEST);
    let last = last.as_deref().map(decode);
    if last == Some(None) {
        log!(
            host::WARNING,
            "layout: the last picture is damaged; starting fresh"
        );
    }
    let prev: Option<HashMap<u64, (f64, f64)>> = last
        .flatten()
        .map(|v| v.into_iter().filter(|s| !s.2).map(|s| (s.0, s.1)).collect());
    let job = match &prev {
        Some(map) => {
            let find = |i: usize| map.get(&hashes[i]).copied();
            Job::new(
                names.len(),
                edges,
                MAP_SEED,
                Some(&find),
                EXACT_MAX_LINKED,
                quick,
            )
        }
        None => Job::new(names.len(), edges, MAP_SEED, None, EXACT_MAX_LINKED, quick),
    };
    log!(
        host::INFO,
        "layout: building {} notes in {} iterations ({}{})",
        names.len(),
        job.progress().1,
        if job.warm { "warm start" } else { "fresh" },
        if quick { ", quick" } else { "" }
    );
    Start::Build(Box::new(Build {
        job,
        key,
        names: names.to_vec(),
    }))
}

/// [`start`] run to the end (tests; the screen runs it in steps).
#[must_use]
pub fn layout_cached(names: &[String], edges: &[(usize, usize)]) -> MapLayout {
    match start(names, edges, false) {
        Start::Ready(l) => l,
        Start::Build(mut b) => {
            b.job.step(usize::MAX);
            b.finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("n{i:03}.md")).collect()
    }

    fn graph(n: usize, m: usize) -> Vec<(usize, usize)> {
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        let mut r = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        (0..m)
            .map(|_| ((r() % n as u64) as usize, (r() % n as u64) as usize))
            .collect()
    }

    fn clear() {
        host::CACHE.with(|c| c.borrow_mut().clear());
        host::STORES.with(|n| *n.borrow_mut() = 0);
    }

    #[test]
    fn cold_layout_is_the_apps_layout_bit_for_bit() {
        for (n, m) in [(0, 0), (1, 0), (2, 1), (15, 12), (60, 100), (150, 260)] {
            let e = graph(n.max(1), m);
            let e: Vec<_> = e.into_iter().filter(|&(a, b)| a < n && b < n).collect();
            let mine = layout(n, &e, MAP_SEED);
            let theirs = sigma_core::graphmap::layout(n, &e, MAP_SEED);
            assert_eq!(mine.pos, theirs.pos, "n={n}");
            assert_eq!(mine.orphan, theirs.orphan, "n={n}");
        }
        let nm = names(5);
        let e = [(0, 1), (1, 2)];
        assert_eq!(
            layout_key(&nm, &e),
            sigma_core::graphmap::layout_key(&nm, &e)
        );
    }

    #[test]
    fn an_unchanged_library_is_one_cache_read() {
        clear();
        let (nm, e) = (names(40), graph(40, 70));
        let first = layout_cached(&nm, &e);
        assert_eq!(first, layout(40, &e, MAP_SEED), "no cache: the cold layout");
        let stores = host::STORES.with(|n| *n.borrow());
        let again = layout_cached(&nm, &e);
        assert_eq!(again, first);
        assert_eq!(
            host::STORES.with(|n| *n.borrow()),
            stores,
            "a hit stores nothing"
        );
    }

    #[test]
    fn a_small_change_keeps_notes_where_they_were() {
        clear();
        let (nm, e) = (names(80), graph(80, 140));
        let before = layout_cached(&nm, &e);
        // One more note, linked to two others, and one link removed.
        let mut nm2 = nm.clone();
        nm2.push("new.md".into());
        let mut e2 = e.clone();
        e2.push((80, 3));
        e2.push((80, 40));
        e2.remove(5);
        let after = layout_cached(&nm2, &e2);
        let cold = layout(81, &e2, MAP_SEED);
        assert_ne!(after, cold, "a warm start, not a fresh layout");
        let moved = (0..80)
            .filter(|&i| !before.orphan[i] && !after.orphan[i])
            .map(|i| {
                let (a, b) = (before.pos[i], after.pos[i]);
                ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
            })
            .collect::<Vec<_>>();
        let mean = moved.iter().sum::<f64>() / moved.len() as f64;
        // A fresh layout would move the average note far more.
        let fresh = (0..80)
            .filter(|&i| !before.orphan[i] && !cold.orphan[i])
            .map(|i| {
                let (a, b) = (before.pos[i], cold.pos[i]);
                ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
            })
            .sum::<f64>()
            / moved.len() as f64;
        assert!(mean < fresh / 2.0, "warm {mean:.3} vs fresh {fresh:.3}");
        for p in &after.pos {
            assert!((p.0 * p.0 + p.1 * p.1).sqrt() <= RING + 1e-9);
        }
        // And the new picture is itself cached.
        assert_eq!(layout_cached(&nm2, &e2), after);
    }

    fn spread(l: &MapLayout, e: &[(usize, usize)]) -> (f64, f64, f64) {
        let d = |a: usize, b: usize| {
            ((l.pos[a].0 - l.pos[b].0).powi(2) + (l.pos[a].1 - l.pos[b].1).powi(2)).sqrt()
        };
        let linked: Vec<usize> = (0..l.pos.len()).filter(|&i| !l.orphan[i]).collect();
        let (mut sum, mut cnt, mut min) = (0.0, 0, f64::MAX);
        for (x, &a) in linked.iter().enumerate() {
            for &b in &linked[x + 1..] {
                sum += d(a, b);
                cnt += 1;
                min = min.min(d(a, b));
            }
        }
        let edge = e
            .iter()
            .filter(|&&(a, b)| a != b && !l.orphan[a] && !l.orphan[b])
            .map(|&(a, b)| d(a, b))
            .sum::<f64>()
            / e.len() as f64;
        (sum / f64::from(cnt), edge, min)
    }

    #[test]
    fn the_tree_matches_all_pairs_repulsion() {
        let mut s = 7u64;
        let mut r = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let p: Vec<(f64, f64)> = (0..400).map(|_| (r(), r())).collect();
        let k2 = 1.0 / 400.0;
        let mut exact = vec![(0.0, 0.0); p.len()];
        for a in 0..p.len() {
            for b in 0..p.len() {
                if a != b {
                    let (dx, dy) = (p[a].0 - p[b].0, p[a].1 - p[b].1);
                    let f = k2 / (dx * dx + dy * dy).max(1e-6);
                    exact[a].0 += dx * f;
                    exact[a].1 += dy * f;
                }
            }
        }
        let mut tree = vec![(0.0, 0.0); p.len()];
        repel_tree(&p, k2, &mut tree);
        let err: f64 = exact
            .iter()
            .zip(&tree)
            .map(|(a, b)| ((a.0 - b.0).hypot(a.1 - b.1)) / a.0.hypot(a.1).max(1e-9))
            .sum::<f64>()
            / p.len() as f64;
        assert!(err < 0.05, "mean relative force error {err:.4}");
    }

    #[test]
    fn small_libraries_stay_exact_and_large_ones_use_the_tree() {
        let e = graph(150, 260);
        let m = (0..150)
            .filter(|&i| e.iter().any(|&(a, b)| a != b && (a == i || b == i)))
            .count();
        assert!(m <= EXACT_MAX_LINKED);
        assert_eq!(
            layout(150, &e, MAP_SEED),
            layout_with(150, &e, MAP_SEED, None, usize::MAX)
        );
        // Above the threshold the tree runs and is deterministic.
        let big = graph(400, 700);
        let a = layout(400, &big, MAP_SEED);
        assert_eq!(a, layout(400, &big, MAP_SEED));
        assert_ne!(
            a,
            layout_with(400, &big, MAP_SEED, None, usize::MAX),
            "approximate"
        );
    }

    #[test]
    fn the_approximate_picture_has_the_same_character() {
        let e = graph(320, 560);
        let exact = layout_with(320, &e, MAP_SEED, None, usize::MAX);
        let tree = layout(320, &e, MAP_SEED);
        let (all_x, edge_x, min_x) = spread(&exact, &e);
        let (all_t, edge_t, min_t) = spread(&tree, &e);
        // Linked notes are closer than strangers by about the same factor,
        // spread over the disc by about the same amount, and not stacked.
        let (rx, rt) = (edge_x / all_x, edge_t / all_t);
        assert!(
            (rt / rx - 1.0).abs() < 0.3,
            "edge/all ratio {rx:.3} vs {rt:.3}"
        );
        assert!(
            (all_t / all_x - 1.0).abs() < 0.3,
            "spread {all_x:.3} vs {all_t:.3}"
        );
        assert!(
            min_t > min_x / 10.0 && min_t > 1e-4,
            "min distance {min_x:.5} vs {min_t:.5}"
        );
        for p in &tree.pos {
            assert!((p.0 * p.0 + p.1 * p.1).sqrt() <= RING + 1e-9);
        }
    }

    #[test]
    fn a_large_library_warm_starts_too() {
        clear();
        let (nm, e) = (names(360), graph(360, 620));
        let before = layout_cached(&nm, &e);
        let mut nm2 = nm.clone();
        nm2.push("new.md".into());
        let mut e2 = e.clone();
        e2.push((360, 3));
        e2.push((360, 40));
        let after = layout_cached(&nm2, &e2);
        let moved: f64 = (0..360)
            .filter(|&i| !before.orphan[i] && !after.orphan[i])
            .map(|i| (before.pos[i].0 - after.pos[i].0).hypot(before.pos[i].1 - after.pos[i].1))
            .sum::<f64>()
            / 360.0;
        assert!(moved < 0.2, "mean move {moved:.3}");
        assert_eq!(layout_cached(&nm2, &e2), after, "and it is cached");
    }

    #[test]
    fn a_very_different_library_starts_fresh() {
        clear();
        let (nm, e) = (names(30), graph(30, 50));
        let _ = layout_cached(&nm, &e);
        let other: Vec<String> = (0..30).map(|i| format!("other{i}.md")).collect();
        let l = layout_cached(&other, &e);
        assert_eq!(l, layout(30, &e, MAP_SEED));
    }

    #[test]
    fn damaged_cache_entries_are_ignored() {
        clear();
        let (nm, e) = (names(10), graph(10, 14));
        host::store(layout_key(&nm, &e), b"junk");
        host::store(0, &[1, 2, 3]);
        assert_eq!(layout_cached(&nm, &e), layout(10, &e, MAP_SEED));
    }
}
