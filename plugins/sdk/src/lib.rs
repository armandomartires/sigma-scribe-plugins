//! Plumbing shared by Sigma Scribe's pane plugins: the host's text metrics
//! and translation, the three exports every module needs, and the small
//! pieces of drawing the drawer's panes have in common (header, hint,
//! "N more", fitting text).

use sigma_core::wrap::{wrap, TextMeasure};
use sigma_plugin_api::{Face, Hit, Op, Rect, Scene, Tone};

pub use sigma_plugin_api as api;

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Face;

    #[link(wasm_import_module = "sigma")]
    extern "C" {
        fn text_width(face: i32, size: f32, ptr: *const u8, len: i32) -> f32;
        fn ascent(face: i32, size: f32) -> f32;
        fn tr(ptr: *const u8, len: i32, out: *mut u8, cap: i32) -> i32;
    }

    pub fn width(face: Face, size: f32, s: &str) -> f32 {
        unsafe { text_width(face as i32, size, s.as_ptr(), s.len() as i32) }
    }

    pub fn ascent_of(face: Face, size: f32) -> f32 {
        unsafe { ascent(face as i32, size) }
    }

    pub fn translate(s: &str) -> String {
        let mut out = vec![0u8; s.len() * 4 + 256];
        let n = unsafe {
            tr(
                s.as_ptr(),
                s.len() as i32,
                out.as_mut_ptr(),
                out.len() as i32,
            )
        };
        if n < 0 {
            return s.to_owned();
        }
        out.truncate(n as usize);
        String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
    }
}

/// Natively (for tests) the host is a pretend with simple metrics.
#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::Face;

    pub fn width(_: Face, size: f32, s: &str) -> f32 {
        s.chars().count() as f32 * size * 0.5
    }

    pub fn ascent_of(_: Face, size: f32) -> f32 {
        size * 0.8
    }

    pub fn translate(s: &str) -> String {
        s.to_owned()
    }
}

pub use imp::{ascent_of, translate, width};

/// The host's text metrics as `sigma_core`'s wrapping wants them.
pub struct Measure {
    pub face: Face,
    pub size: f32,
}

impl TextMeasure for Measure {
    fn width(&self, text: &str) -> u32 {
        width(self.face, self.size, text).ceil() as u32
    }
}

/// `text` cut into rows no wider than `max` px, as the app's own wrapping
/// does: the byte range of each row.
#[must_use]
pub fn wrap_rows(text: &str, max: i32, face: Face, size: f32) -> Vec<(usize, usize)> {
    wrap(text, max.max(1) as u32, &Measure { face, size })
        .into_iter()
        .map(|r| (r.start, r.end))
        .collect()
}

/// `s` shortened with an ellipsis until it fits `max` px.
#[must_use]
pub fn fit_text(s: &str, face: Face, size: f32, max: f32) -> String {
    if width(face, size, s) <= max {
        return s.to_owned();
    }
    let mut o = s.to_owned();
    while !o.is_empty() && width(face, size, &format!("{o}\u{2026}")) > max {
        o.pop();
    }
    format!("{o}\u{2026}")
}

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x, y, w, h)
}

/// The inset of `r` by `d` on every side (negative grows it).
#[must_use]
pub fn inset(r: Rect, d: i32) -> Rect {
    Rect::new(r.x + d, r.y + d, r.w - 2 * d, r.h - 2 * d)
}

/// Where a pane sits and the host's sizes, from the scene.
#[derive(Clone, Copy, Debug)]
pub struct Pane {
    /// The whole pane, header included.
    pub area: Rect,
    /// The header band.
    pub header: Rect,
    /// Below the header, inside the side padding.
    pub body: Rect,
    pub ui: f32,
    pub small: f32,
    scale: f32,
}

impl Pane {
    #[must_use]
    pub fn of(scene: &Scene) -> Self {
        let scale = scene.scale;
        let d = |v: f32| (v * scale).round() as i32;
        let area = Rect::new(scene.left, scene.top, scene.width, scene.bottom - scene.top);
        let (pad, head_h) = (d(12.0), d(44.0));
        Self {
            area,
            header: Rect::new(area.x, area.y, area.w, head_h),
            body: Rect::new(
                area.x + pad,
                area.y + head_h,
                area.w - 2 * pad,
                area.h - head_h,
            ),
            ui: scene.ui_px,
            small: scene.ui_px * 0.82,
            scale,
        }
    }

    /// A length in brief px at the host's scale.
    #[must_use]
    pub fn d(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    /// Baseline that centres text of `size` in `r`.
    #[must_use]
    pub fn baseline(r: Rect, face: Face, size: f32) -> f32 {
        r.y as f32 + r.h as f32 / 2.0 + ascent_of(face, size) / 2.6
    }

    /// The pane's title and the rule under it.
    pub fn header(&self, ops: &mut Vec<Op>, title: &str) {
        ops.push(Op::Text {
            face: Face::Bold,
            size: self.ui,
            x: (self.area.x + self.d(12.0)) as f32,
            y: Self::baseline(self.header, Face::Bold, self.ui),
            s: translate(title),
            c: Tone::Ink,
        });
        ops.push(Op::Fill {
            r: Rect::new(
                self.area.x + self.d(12.0),
                self.header.y + self.header.h - 1,
                self.area.w - self.d(24.0),
                1,
            ),
            c: Tone::Rule,
        });
    }

    /// A wrapped muted hint at the top of the body.
    pub fn hint(&self, ops: &mut Vec<Op>, text: &str) {
        let text = translate(text);
        for (k, (a, b)) in wrap_rows(&text, self.body.w, Face::Regular, self.small)
            .into_iter()
            .enumerate()
        {
            ops.push(Op::Text {
                face: Face::Regular,
                size: self.small,
                x: self.body.x as f32,
                y: self.body.y as f32 + self.d(24.0) as f32 + k as f32 * self.small * 1.4,
                s: text[a..b].to_owned(),
                c: Tone::Mute,
            });
        }
    }

    /// "N more…" at the foot of the body.
    pub fn more(&self, ops: &mut Vec<Op>, n: usize) {
        ops.push(Op::Text {
            face: Face::Regular,
            size: self.small,
            x: self.body.x as f32,
            y: (self.body.y + self.body.h) as f32 - self.small * 0.5,
            s: translate(&format!("{n} more\u{2026}")),
            c: Tone::Mute,
        });
    }
}

/// A tappable region the host can name (emulator scripts, audits).
#[must_use]
pub fn hit(id: u32, rect: Rect, label: impl Into<String>) -> Hit {
    Hit {
        id,
        rect,
        label: label.into(),
    }
}

/// The three exports a module needs. `$handle` is `fn(Event) -> Frame`.
#[macro_export]
macro_rules! export {
    ($handle:path) => {
        thread_local! {
            static INPUT: ::std::cell::RefCell<Vec<u8>> = const { ::std::cell::RefCell::new(Vec::new()) };
            static OUTPUT: ::std::cell::RefCell<Vec<u8>> = const { ::std::cell::RefCell::new(Vec::new()) };
        }

        #[no_mangle]
        pub extern "C" fn sigma_abi() -> u32 {
            $crate::api::ABI
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

        /// Handles the event in the buffer `sigma_alloc` returned; the
        /// answer is `(ptr << 32) | len`, valid until the next call.
        #[no_mangle]
        pub extern "C" fn sigma_call(ptr: u32, len: u32) -> u64 {
            // SAFETY: the host wrote `len` bytes at `ptr`, in the buffer
            // sigma_alloc returned, which stays alive until the next call.
            let bytes =
                unsafe { ::std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
            let frame = match $crate::api::Event::decode(bytes) {
                Ok(ev) => $handle(ev),
                Err(_) => $crate::api::Frame::default(),
            };
            OUTPUT.with(|o| {
                let mut o = o.borrow_mut();
                *o = frame.encode();
                (u64::from(o.as_ptr() as usize as u32) << 32) | o.len() as u64
            })
        }
    };
}

/// A scene for plugin tests: a pane at the right of a 1440 px screen.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn test_scene(height: i32) -> Scene {
    Scene {
        width: 320,
        height: 1920,
        scale: 1.0,
        ui_px: 28.0,
        top: 100,
        bottom: 100 + height,
        notes: Vec::new(),
        edges: Vec::new(),
        links: 0,
        settings: Vec::new(),
        left: 1120,
        todos: Vec::new(),
        headings: Vec::new(),
    }
}
