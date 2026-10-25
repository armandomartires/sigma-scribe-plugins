//! Sigma Scribe's Graph screen as a WASM screen plugin.
//!
//! The app sends events (`sigma_plugin_api::Event`) and gets a frame back:
//! a draw list, hit regions and actions. State (map or list, zoom, pan,
//! selection, list page) lives here; the app owns the title bar, the close
//! button and opening notes.

#[macro_use]
mod host;
pub mod layout;
mod progress;
pub mod view;

use sigma_plugin_api::{Action, Event, Frame, Phase, Redraw, Scene};
use std::cell::RefCell;
use view::{Btn, Graph, Target, ZOOMS};

/// The layout is built in about this many steps, so the bar moves that often.
const STEPS: usize = 12;

/// The screen: building the layout (a progress picture) or ready.
enum Screen {
    Building(Box<Pending>),
    Ready(Box<Graph>),
}

/// A layout being built, with what the screen needs once it is done.
struct Pending {
    scene: Scene,
    links: Vec<(usize, usize)>,
    build: layout::Build,
    /// Iterations to run per step.
    chunk: usize,
    settings: Settings,
    /// Steps taken so far (for the log).
    steps: usize,
}

/// The reader's settings for this plugin (see `plugin.conf`).
#[derive(Clone, Copy)]
struct Settings {
    /// Open on the map (else the list).
    map: bool,
    /// Label every node.
    all_labels: bool,
    /// Fewer layout iterations.
    quick: bool,
}

impl Settings {
    fn of(scene: &Scene) -> Self {
        let get = |k: &str| {
            scene
                .settings
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        Self {
            map: get("view") != Some("List"),
            all_labels: get("labels") == Some("All"),
            quick: get("layout") == Some("Quick"),
        }
    }
}

thread_local! {
    static APP: RefCell<Option<Screen>> = const { RefCell::new(None) };
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn act(a: Action) -> Frame {
    Frame {
        actions: vec![a],
        redraw: Redraw::Full,
        ..Frame::default()
    }
}

fn drawn(g: &Graph, redraw: Redraw) -> Frame {
    let (ops, hits) = g.draw();
    Frame {
        ops,
        hits,
        actions: Vec::new(),
        redraw,
        pending: false,
        field: None,
    }
}

/// The progress picture, asking the host to send `Step` when it is idle.
fn progress_frame(p: &Pending, redraw: Redraw) -> Frame {
    let (done, total) = p.build.job.progress();
    Frame {
        ops: progress::ops(&p.scene, done, total),
        redraw,
        pending: true,
        ..Frame::default()
    }
}

fn ready(
    scene: Scene,
    layout: layout::MapLayout,
    links: Vec<(usize, usize)>,
    st: Settings,
) -> Graph {
    Graph::new(scene, layout, links).with_settings(st.map, st.all_labels)
}

/// Opens the screen: at once when the layout is cached, else as a build.
fn open(scene: Scene) -> (Screen, Frame) {
    let names: Vec<String> = scene.notes.iter().map(|n| n.name.clone()).collect();
    let links: Vec<(usize, usize)> = scene
        .edges
        .iter()
        .map(|&(a, b)| (a as usize, b as usize))
        .collect();
    let settings = Settings::of(&scene);
    log!(
        host::DEBUG,
        "open: {} notes, {} links; start on {}, labels {}, layout {}",
        names.len(),
        links.len(),
        if settings.map { "Map" } else { "List" },
        if settings.all_labels { "All" } else { "Auto" },
        if settings.quick { "Quick" } else { "Best" }
    );
    match layout::start(&names, &links, settings.quick) {
        layout::Start::Ready(l) => {
            let g = ready(scene, l, links, settings);
            let f = drawn(&g, Redraw::Full);
            (Screen::Ready(Box::new(g)), f)
        }
        layout::Start::Build(b) if b.job.is_done() => {
            let g = ready(scene, b.finish(), links, settings);
            let f = drawn(&g, Redraw::Full);
            (Screen::Ready(Box::new(g)), f)
        }
        layout::Start::Build(b) => {
            let chunk = b.job.progress().1.div_ceil(STEPS).max(1);
            let p = Pending {
                scene,
                links,
                build: *b,
                chunk,
                settings,
                steps: 0,
            };
            let f = progress_frame(&p, Redraw::Full);
            (Screen::Building(Box::new(p)), f)
        }
    }
}

/// An event while the layout is being built: `Step` does the next piece,
/// `Back` leaves, anything else just shows the progress again.
fn building(mut p: Box<Pending>, ev: Event) -> (Screen, Frame) {
    match ev {
        Event::Step => {
            p.build.job.step(p.chunk);
            p.steps += 1;
            let (done, total) = p.build.job.progress();
            log!(
                host::DEBUG,
                "layout: step {}, {done}/{total} iterations",
                p.steps
            );
            if p.build.job.is_done() {
                log!(host::INFO, "layout: built in {} steps", p.steps);
                let Pending {
                    scene,
                    links,
                    build,
                    settings,
                    ..
                } = *p;
                let g = ready(scene, build.finish(), links, settings);
                let f = drawn(&g, Redraw::Full);
                (Screen::Ready(Box::new(g)), f)
            } else {
                let f = progress_frame(&p, Redraw::Partial);
                (Screen::Building(p), f)
            }
        }
        Event::Back => (Screen::Building(p), act(Action::Close)),
        _ => {
            let f = progress_frame(&p, Redraw::None);
            (Screen::Building(p), f)
        }
    }
}

impl Graph {
    /// One zoom step; back to x1 recentres.
    fn zoom_step(&mut self, zoom_in: bool) -> Option<Redraw> {
        if !self.map {
            return None;
        }
        let z = if zoom_in {
            (self.zoom + 1).min(ZOOMS.len() - 1)
        } else {
            self.zoom.saturating_sub(1)
        };
        if z == self.zoom {
            return None;
        }
        self.zoom = z;
        if z == 0 {
            self.pan = (0.0, 0.0);
        }
        Some(Redraw::Full)
    }

    /// Pans the map by a screen distance (the picture follows the finger).
    fn pan_by(&mut self, dx: i32, dy: i32) -> Option<Redraw> {
        if !self.map || (dx == 0 && dy == 0) {
            return None;
        }
        let scale = self.geo().scale.max(1e-6);
        let lim = 2.0;
        self.pan.0 = (self.pan.0 - f64::from(dx) / scale).clamp(-lim, lim);
        self.pan.1 = (self.pan.1 - f64::from(dy) / scale).clamp(-lim, lim);
        Some(Redraw::Partial)
    }

    fn page(&mut self, forward: bool) -> Option<Redraw> {
        let g = self.geo();
        if self.map {
            // Page buttons pan half a view down (forward) or up.
            let h = g.area.h / 2;
            return self.pan_by(0, if forward { -h } else { h });
        }
        let visible = g.visible.max(1);
        let n = self.scene.notes.len();
        let next = if forward {
            if self.first + visible < n {
                self.first + visible
            } else {
                self.first
            }
        } else {
            self.first.saturating_sub(visible)
        };
        (next != self.first).then(|| {
            self.first = next;
            Redraw::Partial
        })
    }

    fn tap(&mut self, x: i32, y: i32) -> Result<Option<Redraw>, Action> {
        let g = self.geo();
        Ok(match self.hit(&g, x, y) {
            Target::Btn(b @ (Btn::Map | Btn::List)) => {
                let map = b == Btn::Map;
                if self.map == map {
                    None
                } else {
                    self.map = map;
                    self.selected = None;
                    Some(Redraw::Full)
                }
            }
            Target::Btn(b @ (Btn::ZoomIn | Btn::ZoomOut)) => self.zoom_step(b == Btn::ZoomIn),
            Target::Btn(Btn::Open) => match self.selected {
                Some(s) => return Err(Action::PreviewNote(s as u32)),
                None => None,
            },
            Target::Row(i) => return Err(Action::OpenNote(i as u32)),
            Target::Node(i) => {
                if self.selected == Some(i) {
                    return Err(Action::PreviewNote(i as u32));
                }
                self.selected = Some(i);
                // The pill grows in one non-flashing region.
                Some(Redraw::Clean)
            }
            Target::Nothing => {
                if self.map && self.selected.take().is_some() {
                    Some(Redraw::Clean)
                } else {
                    None
                }
            }
        })
    }
}

/// Handles one event and answers with the screen as it is now.
pub fn handle(ev: Event) -> Frame {
    APP.with(|a| {
        let mut slot = a.borrow_mut();
        if let Event::Open(scene) = ev {
            let (screen, frame) = open(scene);
            *slot = Some(screen);
            return frame;
        }
        match slot.take() {
            None => Frame::default(),
            Some(Screen::Building(p)) => {
                let (screen, frame) = building(p, ev);
                *slot = Some(screen);
                frame
            }
            Some(Screen::Ready(mut g)) => {
                let frame = on_ready(&mut g, ev);
                *slot = Some(Screen::Ready(g));
                frame
            }
        }
    })
}

/// An event on the finished screen.
fn on_ready(g: &mut Graph, ev: Event) -> Frame {
    let r = match ev {
        Event::Open(_) | Event::Step => None,
        Event::Back => return act(Action::Close),
        Event::Tap { x, y } => match g.tap(x, y) {
            Ok(r) => r,
            Err(action) => return act(action),
        },
        Event::PageTurn { forward } => g.page(forward),
        Event::Wheel { dy, .. } => {
            if g.map {
                g.zoom_step(dy > 0)
            } else {
                g.page(dy < 0)
            }
        }
        Event::Drag { phase, x, y } => match phase {
            Phase::Start => {
                let area = g.geo().area;
                g.drag = (g.map && area.contains(x, y)).then_some((x, y));
                None
            }
            Phase::Move | Phase::End => match g.drag {
                Some((x0, y0)) => {
                    g.drag = (phase == Phase::Move).then_some((x, y));
                    g.pan_by(x - x0, y - y0)
                }
                None => None,
            },
        },
    };
    drawn(g, r.unwrap_or(Redraw::None))
}

#[no_mangle]
pub extern "C" fn sigma_abi() -> u32 {
    sigma_plugin_api::ABI
}

/// A buffer in plugin memory for the next event.
#[no_mangle]
pub extern "C" fn sigma_alloc(len: u32) -> u32 {
    INPUT.with(|b| {
        let mut b = b.borrow_mut();
        *b = vec![0u8; len as usize];
        b.as_ptr() as usize as u32
    })
}

/// Handles the event in the buffer `sigma_alloc` returned; the answer is
/// `(ptr << 32) | len`, valid until the next call.
#[no_mangle]
pub extern "C" fn sigma_call(ptr: u32, len: u32) -> u64 {
    // SAFETY: the host wrote `len` bytes at `ptr`, in the buffer sigma_alloc
    // returned, which stays alive until the next call.
    let bytes = unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
    let frame = match Event::decode(bytes) {
        Ok(ev) => handle(ev),
        Err(_) => Frame::default(),
    };
    OUTPUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = frame.encode();
        (u64::from(o.as_ptr() as usize as u32) << 32) | o.len() as u64
    })
}

#[cfg(test)]
mod tests;
