//! Lock-free SPSC ring buffer — vangt UDP-bursts op zonder heap-allocaties.
//!
//! Single producer (UDP-thread) / single consumer (parser-thread). Synchronisatie
//! loopt uitsluitend via de atomic `committed` teller met Release/Acquire.
//!
//! Backing buffer is 8 MiB (groter dan de vorige 4 MiB) zodat bitrate-pieken op
//! 1080p@60 niet tot overwrites leiden voordat de parser uitgelezen heeft.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const CAPACITY: usize = 8 * 1024 * 1024; // 8 MiB

pub struct Ring {
    buf: UnsafeCell<[u8; CAPACITY]>,
    /// Volgende schrijfpositie. Monotoon; modulo CAPACITY voor de index.
    write_pos: AtomicUsize,
    /// Totale bytes ooit weggeschreven (high-water mark voor de reader).
    committed: AtomicUsize,
    /// Totaal aantal bytes dat ooit in de ring geschreven is (stats).
    pub bytes_in: AtomicU64,
}

// Synchronisatie loopt via de atomics — UnsafeCell op zich is niet Sync.
unsafe impl Sync for Ring {}
unsafe impl Send for Ring {}

impl Ring {
    pub fn new() -> Self {
        Self {
            buf: UnsafeCell::new([0u8; CAPACITY]),
            write_pos: AtomicUsize::new(0),
            committed: AtomicUsize::new(0),
            bytes_in: AtomicU64::new(0),
        }
    }

    /// Schrijf een UDP-pakket in de ring. Retourneert `false` als het pakket
    /// groter is dan CAPACITY (wordt geskipped).
    pub fn write(&self, data: &[u8]) -> bool {
        let len = data.len();
        if len == 0 || len >= CAPACITY {
            return len < CAPACITY;
        }

        let start = self.write_pos.load(Ordering::Relaxed);

        // Safety: enkel de UDP-thread (enige producer) raakt [start, start+len)
        // aan. De reader ziet deze bytes pas nadat we `committed` verhogen.
        unsafe {
            let buf = &mut *self.buf.get();
            let start_idx = start % CAPACITY;
            if start_idx + len <= CAPACITY {
                buf[start_idx..start_idx + len].copy_from_slice(data);
            } else {
                // Wrap-around: schrijf in twee stukken.
                let first = CAPACITY - start_idx;
                buf[start_idx..CAPACITY].copy_from_slice(&data[..first]);
                buf[0..len - first].copy_from_slice(&data[first..]);
            }
        }

        let end = start + len;
        self.write_pos.store(end, Ordering::Relaxed);
        self.committed.store(end, Ordering::Release);
        self.bytes_in
            .fetch_add(len as u64, Ordering::Relaxed);
        true
    }

    /// Lees alle sinds `last_read` binnengekomen bytes in `out`. Retourneert de
    /// nieuwe high-water mark (opslaan voor de volgende call).
    pub fn read_since(&self, last_read: usize, out: &mut Vec<u8>) -> usize {
        let committed = self.committed.load(Ordering::Acquire);
        if committed <= last_read {
            return last_read;
        }

        let available = committed - last_read;
        // Begrens tot CAPACITY: alles wat ouder is is overschreven.
        let to_read = available.min(CAPACITY);
        let read_start = committed - to_read;

        // Safety: enkel de parser-thread (enige consumer) leest uit
        // [read_start, committed). De producer raakt deze bytes pas weer aan
        // als write_pos voorbij committed + CAPACITY komt — niet zolang we lezen.
        unsafe {
            let buf = &*self.buf.get();
            let start_idx = read_start % CAPACITY;
            let prev_len = out.len();
            out.resize(prev_len + to_read, 0);

            if start_idx + to_read <= CAPACITY {
                out[prev_len..].copy_from_slice(&buf[start_idx..start_idx + to_read]);
            } else {
                let first = CAPACITY - start_idx;
                out[prev_len..prev_len + first].copy_from_slice(&buf[start_idx..CAPACITY]);
                out[prev_len + first..].copy_from_slice(&buf[0..to_read - first]);
            }
        }

        committed
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}
