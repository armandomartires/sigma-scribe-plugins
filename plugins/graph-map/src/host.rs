//! What the app gives a plugin: text metrics, translation and a small
//! persistent cache. On wasm these are imports from the host module
//! `sigma`; natively (for tests) they are simple stand-ins.

use sigma_plugin_api::Face;

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Face;

    #[link(wasm_import_module = "sigma")]
    extern "C" {
        fn text_width(face: i32, size: f32, ptr: *const u8, len: i32) -> f32;
        fn ascent(face: i32, size: f32) -> f32;
        fn tr(ptr: *const u8, len: i32, out: *mut u8, cap: i32) -> i32;
        fn cache_get(key: i64, out: *mut u8, cap: i32) -> i32;
        fn cache_put(key: i64, ptr: *const u8, len: i32) -> i32;
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

    pub fn load(key: u64) -> Option<Vec<u8>> {
        let mut out = vec![0u8; 1 << 20];
        let n = unsafe { cache_get(key as i64, out.as_mut_ptr(), out.len() as i32) };
        if n < 0 {
            return None;
        }
        out.truncate(n as usize);
        Some(out)
    }

    pub fn store(key: u64, bytes: &[u8]) {
        let _ = unsafe { cache_put(key as i64, bytes.as_ptr(), bytes.len() as i32) };
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::Face;
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        pub static CACHE: RefCell<HashMap<u64, Vec<u8>>> = RefCell::new(HashMap::new());
        pub static STORES: RefCell<u32> = const { RefCell::new(0) };
    }

    pub fn width(_: Face, size: f32, s: &str) -> f32 {
        s.chars().count() as f32 * size * 0.5
    }

    pub fn ascent_of(_: Face, size: f32) -> f32 {
        size * 0.8
    }

    pub fn translate(s: &str) -> String {
        s.to_owned()
    }

    pub fn load(key: u64) -> Option<Vec<u8>> {
        CACHE.with(|c| c.borrow().get(&key).cloned())
    }

    pub fn store(key: u64, bytes: &[u8]) {
        STORES.with(|n| *n.borrow_mut() += 1);
        CACHE.with(|c| c.borrow_mut().insert(key, bytes.to_vec()));
    }
}

pub use imp::{ascent_of, load, store, translate, width};

#[cfg(all(test, not(target_arch = "wasm32")))]
pub use imp::{CACHE, STORES};
