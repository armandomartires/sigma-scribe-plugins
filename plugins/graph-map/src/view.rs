//! The Graph screen: geometry, drawing and hit-testing, ported from the
//! app's built-in renderer so the two draw the same pixels. Drawing makes
//! a list of ops for the host to paint; the host draws the title bar and
//! close button itself.

use crate::host::{ascent_of, translate, width};
use crate::layout::MapLayout;
use sigma_plugin_api::{Face, Hit, Note, Op, Rect, Scene, Tone};

/// Zoom factors over the fitted scale.
pub const ZOOMS: [f64; 4] = [1.0, 1.6, 2.6, 4.2];

/// Pixels for a size in the brief's units, exactly as the host rounds them.
#[derive(Clone, Copy)]
pub struct Px(pub f32);

impl Px {
    pub fn px(self, brief: f32) -> i32 {
        (brief * self.0).round() as i32
    }
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x, y, w, h)
}

fn inset(r: Rect, d: i32) -> Rect {
    rect(r.x + d, r.y + d, r.w - 2 * d, r.h - 2 * d)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Map,
    List,
    ZoomOut,
    ZoomIn,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Btn(Btn),
    Row(usize),
    Node(usize),
    Nothing,
}

/// Where things are for the current state.
pub struct Geo {
    pub toolbar: Vec<(Btn, Rect)>,
    pub foot: Rect,
    pub area: Rect,
    /// Visible list rows (index, band).
    pub rows: Vec<(usize, Rect)>,
    pub visible: usize,
    /// Map: pixels per layout unit, the layout point at the area's centre,
    /// and the visible nodes with their boxes in drawing order.
    pub scale: f64,
    pub centre: (f64, f64),
    pub nodes: Vec<(usize, Rect)>,
}

pub struct Graph {
    pub scene: Scene,
    pub layout: MapLayout,
    /// Links between notes, as indexes (deduplicated, no self links).
    pub links: Vec<(usize, usize)>,
    pub map: bool,
    pub zoom: usize,
    pub pan: (f64, f64),
    pub selected: Option<usize>,
    /// First list row shown.
    pub first: usize,
    /// Where the last drag event was.
    pub drag: Option<(i32, i32)>,
}

/// Short label for a note path: file stem.
fn label(name: &str) -> &str {
    let base = name.rsplit('/').next().unwrap_or(name);
    base.strip_suffix(".md").unwrap_or(base)
}

fn tone_fill(ops: &mut Vec<Op>, r: Rect, c: Tone) {
    ops.push(Op::Fill { r, c });
}

impl Graph {
    pub fn new(scene: Scene, layout: MapLayout, links: Vec<(usize, usize)>) -> Self {
        Self {
            scene,
            layout,
            links,
            map: true,
            zoom: 0,
            pan: (0.0, 0.0),
            selected: None,
            first: 0,
            drag: None,
        }
    }

    fn s(&self) -> Px {
        Px(self.scene.scale)
    }

    fn ui(&self) -> f32 {
        self.scene.ui_px
    }

    fn n(&self) -> usize {
        self.layout.pos.len().min(self.scene.notes.len())
    }

    fn note(&self, i: usize) -> &Note {
        &self.scene.notes[i]
    }

    fn cur(&self) -> Option<usize> {
        (0..self.n()).find(|&i| self.note(i).current)
    }

    fn neighbour(&self, a: usize, b: usize) -> bool {
        self.links
            .iter()
            .any(|&(x, y)| (x == a && y == b) || (x == b && y == a))
    }

    fn emph(&self, i: usize) -> bool {
        self.selected
            .is_some_and(|s| s == i || self.neighbour(s, i))
    }

    fn label_size(&self) -> f32 {
        self.ui() * 0.9
    }

    fn dot(&self, i: usize, cur: Option<usize>) -> i32 {
        let s = self.s();
        if Some(i) == cur || self.emph(i) {
            s.px(30.0)
        } else {
            s.px(22.0)
        }
    }

    fn label_beside(&self, i: usize, cur: Option<usize>, all: bool) -> bool {
        self.selected != Some(i) && (all || Some(i) == cur || self.emph(i))
    }

    fn labels_all(&self, n: usize, scale: f64) -> bool {
        let spacing = scale * 1.8 / (n.max(1) as f64).sqrt();
        spacing >= f64::from(self.s().px(150.0))
    }

    fn point(area: Rect, scale: f64, centre: (f64, f64), p: (f64, f64)) -> (i32, i32) {
        (
            area.x + area.w / 2 + ((p.0 - centre.0) * scale).round() as i32,
            area.y + area.h / 2 + ((p.1 - centre.1) * scale).round() as i32,
        )
    }

    fn node_label(&self, name: &str, max: f32) -> String {
        let size = self.label_size();
        let full = label(name);
        if width(Face::Regular, size, full) <= max {
            return full.to_owned();
        }
        let mut s: String = full.to_owned();
        while !s.is_empty() && width(Face::Regular, size, &format!("{s}\u{2026}")) > max {
            s.pop();
        }
        format!("{s}\u{2026}")
    }

    /// Touch target of a node: its shape, grown to at least 44 brief px.
    fn target(&self, r: Rect) -> Rect {
        let min = self.s().px(44.0);
        let (w, h) = (r.w.max(min), r.h.max(min));
        rect(r.x + r.w / 2 - w / 2, r.y + r.h / 2 - h / 2, w, h)
    }

    fn toolbar(&self, top: i32, h: i32) -> Vec<(Btn, Rect)> {
        let s = self.s();
        let (bw, bh, gap) = (s.px(120.0), s.px(56.0), s.px(12.0));
        let y = top + (h - bh) / 2;
        let x0 = s.px(28.0);
        let mut out = vec![
            (Btn::Map, rect(x0, y, bw, bh)),
            (Btn::List, rect(x0 + bw, y, bw, bh)),
        ];
        if self.map {
            let zw = s.px(72.0);
            let right = self.scene.width - x0;
            out.push((Btn::ZoomIn, rect(right - zw, y, zw, bh)));
            out.push((Btn::ZoomOut, rect(right - 2 * zw - gap, y, zw, bh)));
            if self.selected.is_some() {
                out.push((Btn::Open, rect(x0 + 2 * bw + 2 * gap, y, bw, bh)));
            }
        }
        out
    }

    pub fn geo(&self) -> Geo {
        let s = self.s();
        let (w, hgt) = (self.scene.width, self.scene.height);
        let title_h = self.scene.top;
        let row_h = s.px(80.0);
        let bar_h = s.px(72.0);
        let toolbar = self.toolbar(title_h, bar_h);
        let top = title_h + bar_h + s.px(8.0);
        let foot_h = s.px(56.0);
        let avail = hgt - top - foot_h;
        let visible = (avail / row_h).max(1) as usize;
        let foot = rect(0, hgt - foot_h, w, foot_h);
        if self.map {
            let area = rect(0, title_h + bar_h, w, foot.y - title_h - bar_h);
            let (scale, centre, nodes) = self.map_nodes(area);
            return Geo {
                toolbar,
                foot,
                area,
                rows: Vec::new(),
                visible,
                scale,
                centre,
                nodes,
            };
        }
        let rows = (self.first..self.scene.notes.len().min(self.first + visible))
            .enumerate()
            .map(|(k, i)| (i, rect(0, top + k as i32 * row_h, w, row_h)))
            .collect();
        Geo {
            toolbar,
            foot,
            area: Rect::default(),
            rows,
            visible,
            scale: 0.0,
            centre: (0.0, 0.0),
            nodes: Vec::new(),
        }
    }

    fn map_nodes(&self, area: Rect) -> (f64, (f64, f64), Vec<(usize, Rect)>) {
        let s = self.s();
        let n = self.n();
        let cur = self.cur();
        let anchor = cur.map_or((0.0, 0.0), |i| self.layout.pos[i]);
        let (mut dx, mut dy) = (1e-3f64, 1e-3f64);
        for p in &self.layout.pos[..n] {
            dx = dx.max((p.0 - anchor.0).abs());
            dy = dy.max((p.1 - anchor.1).abs());
        }
        let (mx, my) = (f64::from(s.px(110.0)), f64::from(s.px(40.0)));
        let fit = ((f64::from(area.w) / 2.0 - mx) / dx)
            .min((f64::from(area.h) / 2.0 - my) / dy)
            .max(1.0);
        let scale = fit * ZOOMS[self.zoom.min(ZOOMS.len() - 1)];
        let centre = (anchor.0 + self.pan.0, anchor.1 + self.pan.1);
        let bh = s.px(46.0);
        let mut out = Vec::new();
        for i in 0..n {
            let (x, y) = Self::point(area, scale, centre, self.layout.pos[i]);
            let r = if self.selected == Some(i) {
                let w = (width(Face::Bold, self.label_size(), label(&self.note(i).name)) as i32
                    + bh)
                    .max(bh);
                rect(x - w / 2, y - bh / 2, w, bh)
            } else {
                let d = self.dot(i, cur);
                rect(x - d / 2, y - d / 2, d, d)
            };
            if r.x + r.w >= area.x
                && r.x <= area.x + area.w
                && r.y + r.h >= area.y
                && r.y <= area.y + area.h
            {
                out.push((i, r));
            }
        }
        // Emphasised nodes last, so they draw on top and win taps.
        out.sort_by_key(|&(i, _)| {
            u8::from(self.emph(i))
                + 2 * u8::from(Some(i) == cur)
                + 4 * u8::from(self.selected == Some(i))
        });
        (scale, centre, out)
    }

    /// What a tap at `(x, y)` lands on.
    pub fn hit(&self, g: &Geo, x: i32, y: i32) -> Target {
        if let Some((b, _)) = g.toolbar.iter().find(|(_, r)| r.contains(x, y)) {
            return Target::Btn(*b);
        }
        if self.map {
            return if g.area.contains(x, y) {
                g.nodes
                    .iter()
                    .rev()
                    .find(|(_, r)| self.target(*r).contains(x, y))
                    .map_or(Target::Nothing, |&(i, _)| Target::Node(i))
            } else {
                Target::Nothing
            };
        }
        g.rows
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map_or(Target::Nothing, |(i, _)| Target::Row(*i))
    }

    fn footer(&self) -> String {
        let n = self.scene.notes.len();
        let links = self.scene.links;
        let mut f = translate(&format!(
            "{n} note{} \u{b7} {} link{}",
            if n == 1 { "" } else { "s" },
            links,
            if links == 1 { "" } else { "s" }
        ));
        if let Some(s) = self.selected.and_then(|s| self.scene.notes.get(s)) {
            f.push_str(&format!(
                " \u{b7} {} \u{2014} {}",
                label(&s.name),
                translate("tap again to open")
            ));
        } else if links == 0 {
            f.push_str(" \u{b7} ");
            f.push_str(&translate("write [[a note name]] to join two notes"));
        }
        f
    }

    /// Text with its baseline set the way the host's controls do: the
    /// middle of `r` plus a third of the ascent.
    fn centred_y(r: Rect, face: Face, size: f32) -> f32 {
        r.y as f32 + r.h as f32 / 2.0 + ascent_of(face, size) / 2.6
    }

    fn draw_toolbar(&self, g: &Geo, ops: &mut Vec<Op>) {
        let ui = self.ui();
        let th = self.s().px(2.0).max(2);
        for &(b, r) in &g.toolbar {
            let on = matches!((b, self.map), (Btn::Map, true) | (Btn::List, false));
            let zoom = if self.map { self.zoom } else { 0 };
            let dim = matches!((b, zoom), (Btn::ZoomOut, 0) | (Btn::ZoomIn, 3));
            ops.push(Op::Fill {
                r,
                c: if on { Tone::Ink } else { Tone::Paper },
            });
            ops.push(Op::Stroke {
                r,
                t: th,
                c: if dim { Tone::Rule } else { Tone::Ink },
            });
            let text = match b {
                Btn::Map => "Map",
                Btn::List => "List",
                Btn::ZoomOut => "\u{2212}",
                Btn::ZoomIn => "+",
                Btn::Open => "Open",
            };
            let size = if matches!(b, Btn::ZoomIn | Btn::ZoomOut) {
                ui * 1.4
            } else {
                ui
            };
            let face = if on { Face::Bold } else { Face::Regular };
            let w = width(face, size, text);
            ops.push(Op::Text {
                face,
                size,
                x: r.x as f32 + (r.w as f32 - w) / 2.0,
                y: Self::centred_y(r, face, size),
                s: text.to_owned(),
                c: if on {
                    Tone::Paper
                } else if dim {
                    Tone::Rule
                } else {
                    Tone::Ink
                },
            });
        }
    }

    fn draw_map(&self, g: &Geo, ops: &mut Vec<Op>) {
        let s = self.s();
        let area = g.area;
        let n = self.n();
        let boxes: std::collections::HashMap<usize, Rect> = g.nodes.iter().copied().collect();
        let line_w = s.px(2.0).max(2);
        // Links: incident to the selection in ink, others mute (rule when a
        // selection exists, so the neighbourhood stands out).
        let mut edges: Vec<(usize, usize)> = self
            .links
            .iter()
            .copied()
            .filter(|&(a, b)| a < n && b < n && a != b)
            .collect();
        edges.sort_by_key(|&(a, b)| self.selected.is_some_and(|s| s == a || s == b));
        for (a, b) in edges {
            let pa = Self::point(area, g.scale, g.centre, self.layout.pos[a]);
            let pb = Self::point(area, g.scale, g.centre, self.layout.pos[b]);
            let hot = self.selected.is_some_and(|s| s == a || s == b);
            let col = if hot {
                Tone::Ink
            } else if self.selected.is_some() {
                Tone::Rule
            } else {
                Tone::Mute
            };
            let w = if hot { line_w + line_w / 2 } else { line_w };
            // Plain lines, clipped to the node shapes.
            let start = boxes.get(&a).map_or(pa, |r| exit(*r, pb, pa));
            let end = boxes.get(&b).map_or(pb, |r| exit(*r, pa, pb));
            ops.push(Op::Line {
                a: start,
                b: end,
                w,
                c: col,
            });
        }
        // Nodes.
        let cur = self.cur();
        let border = s.px(2.0).max(2);
        let size = self.label_size();
        let all = self.labels_all(n, g.scale);
        let maxw = s.px(200.0) as f32;
        let gap = s.px(8.0);
        for &(i, r) in &g.nodes {
            let emph = self.emph(i);
            let is_cur = Some(i) == cur;
            let sel = self.selected == Some(i);
            let quiet = self.selected.is_some() && !emph && !is_cur;
            let (bg, fg, edge) = if is_cur {
                (Tone::Ink, Tone::Paper, Tone::Ink)
            } else if quiet {
                (Tone::Paper, Tone::Mute, Tone::Rule)
            } else {
                (
                    Tone::Paper,
                    Tone::Ink,
                    if self.layout.orphan.get(i) == Some(&true) {
                        Tone::Mute
                    } else {
                        Tone::Ink
                    },
                )
            };
            ops.push(Op::Pill { r, c: edge });
            let bw = if sel || emph { border * 2 } else { border };
            ops.push(Op::Pill {
                r: inset(r, bw),
                c: bg,
            });
            if sel && is_cur {
                ops.push(Op::Pill {
                    r: inset(r, bw * 2),
                    c: Tone::Ink,
                });
            }
            let (text, face, tx, ink) = if sel {
                let face = Face::Bold;
                let text = label(&self.note(i).name).to_owned();
                let w = width(face, size, &text);
                (text, face, r.x as f32 + (r.w as f32 - w) / 2.0, fg)
            } else if self.label_beside(i, cur, all) {
                let face = if is_cur { Face::Bold } else { Face::Regular };
                let ink = if quiet { Tone::Mute } else { Tone::Ink };
                (
                    self.node_label(&self.note(i).name, maxw),
                    face,
                    (r.x + r.w + gap) as f32,
                    ink,
                )
            } else {
                continue;
            };
            ops.push(Op::Text {
                face,
                size,
                x: tx,
                y: Self::centred_y(r, face, size),
                s: text,
                c: ink,
            });
        }
    }

    fn draw_list(&self, g: &Geo, ops: &mut Vec<Op>) {
        let s = self.s();
        let ui = self.ui();
        let tx = s.px(28.0) as f32;
        let max = (self.scene.width - s.px(56.0)) as f32;
        for &(i, r) in &g.rows {
            let row = self.note(i);
            if row.current {
                tone_fill(ops, rect(0, r.y, s.px(6.0), r.h), Tone::Ink);
            }
            let face = if row.current {
                Face::Bold
            } else {
                Face::Regular
            };
            ops.push(Op::Text {
                face,
                size: ui * 1.1,
                x: tx,
                y: r.y as f32 + r.h as f32 * 0.45,
                s: row.name.clone(),
                c: Tone::Ink,
            });
            let mut detail = translate(&row.detail);
            let full_len = detail.len();
            while !detail.is_empty() && width(Face::Regular, ui * 0.85, &detail) > max {
                detail.pop();
            }
            if detail.len() < full_len {
                detail.pop();
                detail.push('\u{2026}');
            }
            ops.push(Op::Text {
                face: Face::Regular,
                size: ui * 0.85,
                x: tx,
                y: r.y as f32 + r.h as f32 * 0.8,
                s: detail,
                c: Tone::Mute,
            });
            tone_fill(ops, rect(0, r.y + r.h - 1, r.w, 1), Tone::Rule);
        }
        let pages = self.scene.notes.len().div_ceil(g.visible.max(1)).max(1);
        let links = self.scene.links;
        let mut foot = if links == 0 {
            "No links yet. Write [[a note name]] to join two notes.".to_owned()
        } else if links == 1 {
            "1 link".to_owned()
        } else {
            format!("{links} links")
        };
        if pages > 1 {
            foot.push_str(&format!(
                " \u{b7} page {} of {pages}",
                self.first / g.visible.max(1) + 1
            ));
        }
        self.foot(g, ops, foot);
    }

    /// The footer rule and its text.
    fn foot(&self, g: &Geo, ops: &mut Vec<Op>, text: String) {
        let ui = self.ui();
        let tx = self.s().px(28.0) as f32;
        tone_fill(ops, rect(0, g.foot.y, g.foot.w, 1), Tone::Rule);
        ops.push(Op::Text {
            face: Face::Regular,
            size: ui,
            x: tx,
            y: Self::centred_y(g.foot, Face::Regular, ui),
            s: text,
            c: Tone::Mute,
        });
    }

    /// The whole picture and its tappable regions.
    pub fn draw(&self) -> (Vec<Op>, Vec<Hit>) {
        let g = self.geo();
        let mut ops = Vec::new();
        let (w, h) = (self.scene.width, self.scene.height);
        tone_fill(&mut ops, rect(0, 0, w, h), Tone::Paper);
        let mut hits = Vec::new();
        let mut id = 0u32;
        let mut hit = |label: String, r: Rect| {
            id += 1;
            hits.push(Hit { id, rect: r, label });
        };
        if self.map {
            self.draw_map(&g, &mut ops);
            // Bars are painted over the map, which clips it.
            tone_fill(&mut ops, rect(0, 0, g.area.w, g.area.y), Tone::Paper);
            tone_fill(&mut ops, rect(0, g.foot.y, g.foot.w, g.foot.h), Tone::Paper);
            self.draw_toolbar(&g, &mut ops);
            let foot = self.footer();
            self.foot(&g, &mut ops, foot);
            for &(i, r) in &g.nodes {
                let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
                if g.area.contains(cx, cy) {
                    hit(format!("Node:{}", self.note(i).name), r);
                }
            }
        } else {
            self.draw_toolbar(&g, &mut ops);
            self.draw_list(&g, &mut ops);
            for &(i, r) in &g.rows {
                hit(format!("Graph:{}", self.note(i).name), r);
            }
        }
        for &(b, r) in &g.toolbar {
            let name = match b {
                Btn::Map => "Map",
                Btn::List => "List",
                Btn::ZoomOut => "Zoom-",
                Btn::ZoomIn => "Zoom+",
                Btn::Open => "Open",
            };
            hit(name.to_owned(), r);
        }
        (ops, hits)
    }
}

/// Where the segment from `from` to the centre `to` of box `r` crosses its
/// border (the point on the border facing `from`).
fn exit(r: Rect, from: (i32, i32), to: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (f64::from(from.0 - to.0), f64::from(from.1 - to.1));
    if dx == 0.0 && dy == 0.0 {
        return to;
    }
    let (hw, hh) = (f64::from(r.w) / 2.0 + 2.0, f64::from(r.h) / 2.0 + 2.0);
    let k = (hw / dx.abs().max(1e-9))
        .min(hh / dy.abs().max(1e-9))
        .min(1.0);
    (
        to.0 + (dx * k).round() as i32,
        to.1 + (dy * k).round() as i32,
    )
}
