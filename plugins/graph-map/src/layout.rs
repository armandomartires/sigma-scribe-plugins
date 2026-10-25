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
const WARM_TEMP: f64 = 0.03;
/// Radius of the orphan ring; linked notes stay inside `INNER`.
pub const RING: f64 = 1.0;
pub const INNER: f64 = 0.8;
/// Saved positions are scaled into the disc; the force simulation works
/// in a square of side 1, whose notes spread about 0.5 from the centre.
const UNSCALE: f64 = 0.5 / INNER;
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
    let mut pos = vec![(0.0, 0.0); n];
    let mut orphan = vec![false; n];

    // Force layout over the linked notes, in a square of side 1.
    let m = linked.len();
    if m > 0 {
        let mut slot = vec![usize::MAX; n];
        for (k, &i) in linked.iter().enumerate() {
            slot[i] = k;
        }
        let mut rng = Rng(seed | 1);
        let (mut p, iterations, mut temp) = match known {
            Some(f) => match warm_start(&linked, &pairs, &slot, f, &mut rng) {
                Some(p) => (p, WARM_ITERATIONS, WARM_TEMP),
                None => (cold_start(m, &mut rng), ITERATIONS, 0.1),
            },
            None => (cold_start(m, &mut rng), ITERATIONS, 0.1),
        };
        let k = (1.0 / m as f64).sqrt();
        let cool = temp / (iterations as f64 + 1.0);
        let mut disp = vec![(0.0f64, 0.0f64); m];
        for _ in 0..iterations {
            disp.iter_mut().for_each(|d| *d = (0.0, 0.0));
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
            for &(a, b) in &pairs {
                let (a, b) = (slot[a], slot[b]);
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
            for (q, d) in p.iter_mut().zip(&disp) {
                let len = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-9);
                let step = len.min(temp);
                q.0 += d.0 / len * step;
                q.1 += d.1 / len * step;
            }
            temp -= cool;
        }
        // Centre and scale into the inner disc.
        let (cx, cy) = p.iter().fold((0.0, 0.0), |a, q| {
            (a.0 + q.0 / m as f64, a.1 + q.1 / m as f64)
        });
        let r = p
            .iter()
            .map(|q| ((q.0 - cx).powi(2) + (q.1 - cy).powi(2)).sqrt())
            .fold(0.0f64, f64::max)
            .max(1e-6);
        let s = if m == 1 { 0.0 } else { INNER / r };
        for (kk, &i) in linked.iter().enumerate() {
            pos[i] = ((p[kk].0 - cx) * s, (p[kk].1 - cy) * s);
        }
    }

    // Orphans: evenly on the ring, in library order, starting at the top.
    let lone: Vec<usize> = (0..n).filter(|&i| deg[i] == 0).collect();
    let ring = if m == 0 { INNER } else { RING };
    for (k, &i) in lone.iter().enumerate() {
        let a = -std::f64::consts::FRAC_PI_2
            + std::f64::consts::TAU * k as f64 / lone.len().max(1) as f64;
        pos[i] = (ring * a.cos(), ring * a.sin());
        orphan[i] = true;
    }
    MapLayout { pos, orphan }
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

/// The layout for these notes: from the cache when the library is
/// unchanged, a warm start when it changed a little, else computed fresh.
#[must_use]
pub fn layout_cached(names: &[String], edges: &[(usize, usize)]) -> MapLayout {
    let key = layout_key(names, edges);
    let hashes: Vec<u64> = names.iter().map(|n| name_hash(n)).collect();
    if let Some(saved) = host::load(key).as_deref().and_then(decode) {
        if saved.len() == names.len() && saved.iter().zip(&hashes).all(|(s, h)| s.0 == *h) {
            return MapLayout {
                pos: saved.iter().map(|s| s.1).collect(),
                orphan: saved.iter().map(|s| s.2).collect(),
            };
        }
    }
    let prev: Option<HashMap<u64, (f64, f64)>> = host::load(LATEST)
        .as_deref()
        .and_then(decode)
        .map(|v| v.into_iter().filter(|s| !s.2).map(|s| (s.0, s.1)).collect());
    let l = match &prev {
        Some(map) => {
            let find = |i: usize| map.get(&hashes[i]).copied();
            layout_from(names.len(), edges, MAP_SEED, Some(&find))
        }
        None => layout(names.len(), edges, MAP_SEED),
    };
    let blob = encode(names, &l);
    host::store(key, &blob);
    host::store(LATEST, &blob);
    l
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
