//! Stats tracker — atomisch bijgehouden, polled door Swift via `hyprpad_stats()`.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum State {
    Idle = 0,
    Listening = 1,
    Receiving = 2,
    Error = 3,
}

pub struct Stats {
    fps: AtomicU32,
    bytes_total: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
    state: AtomicU8,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            fps: AtomicU32::new(0),
            bytes_total: AtomicU64::new(0),
            width: AtomicU32::new(0),
            height: AtomicU32::new(0),
            state: AtomicU8::new(State::Idle as u8),
        }
    }

    pub fn reset(&self) {
        self.fps.store(0, Ordering::Relaxed);
        self.bytes_total.store(0, Ordering::Relaxed);
        self.width.store(0, Ordering::Relaxed);
        self.height.store(0, Ordering::Relaxed);
        self.state.store(State::Idle as u8, Ordering::Relaxed);
    }

    #[inline]
    pub fn add_bytes(&self, n: u64) {
        self.bytes_total.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_fps(&self, fps: u32) {
        self.fps.store(fps, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_dimensions(&self, w: u32, h: u32) {
        self.width.store(w, Ordering::Relaxed);
        self.height.store(h, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_state(&self, s: State) {
        self.state.store(s as u8, Ordering::Relaxed);
    }

    #[inline]
    pub fn state(&self) -> u8 {
        self.state.load(Ordering::Relaxed)
    }

    /// Snapshot voor de C-ABI. Kopieert alle velden — Swift kan deze thread-veilig
    /// tonen op de main thread.
    pub fn snapshot(&self) -> super::HyprpadStats {
        super::HyprpadStats {
            fps: self.fps.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
            state: self.state.load(Ordering::Relaxed),
        }
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}
