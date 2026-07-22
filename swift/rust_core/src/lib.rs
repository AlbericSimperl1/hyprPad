//! rust_core — C-ABI tussen Rust en Swift voor hyprPadClient (iPadOS 26+).
//!
//! De Swift app roept `hyprpad_start()` aan met een `HyprpadCallbacks` struct
//! (één NALU-callback) en een optionele log-callback. Stats gaan via polling
//! (`hyprpad_stats()`) zodat er géén callbacks op Rust-threads binnenkomen die
//! SwiftUI-state muteren — dat was de voornaamste crash-oorzaak in de vorige app.
//!
//! Enige globale state wordt beschermd door een `OnceLock<Mutex<Engine>>`.

mod parser;
mod ring;
mod stats;
mod udp;

use std::os::raw::{c_char, c_void};
use std::sync::{Mutex, OnceLock};

use ring::Ring;
use stats::Stats;

// ─── C-ABI types ───────────────────────────────────────────────────────────

/// NALU-callback. Wordt aangeroepen op de `nalu-parser` thread (background).
/// De pointer is enkel geldig tijdens de call — kopieer de bytes in Swift.
#[unsafe(no_mangle)]
pub type OnNalu = extern "C" fn(data: *const u8, len: u32, nal_type: u8, ctx: *mut c_void);

/// Log-callback. `level`: 0=info, 1=warn, 2=error. Swift moet altijd een geldige
/// functie meegeven (geen `Option`) — cbindgen vertaalt `Option<fn>` namelijk
/// niet naar een C-function-pointer.
#[unsafe(no_mangle)]
pub type OnLog = extern "C" fn(level: u8, msg: *const c_char, ctx: *mut c_void);

/// Callbacks die Swift meegeeft. Eén struct = geen volgorderisico's.
/// Beide velden zijn verplicht (geen NULL).
#[repr(C)]
pub struct HyprpadCallbacks {
    pub on_nalu: OnNalu,
    pub on_log: OnLog,
}

/// State van de engine, opgevraagd via `hyprpad_stats()`.
/// `state`: 0=idle, 1=listening (socket open), 2=decoding (frames ontvangen),
/// 3=error.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct HyprpadStats {
    pub fps: u32,
    pub bytes_total: u64,
    pub width: u32,
    pub height: u32,
    pub state: u8,
}

// ─── Engine ────────────────────────────────────────────────────────────────

struct Engine {
    udp: Option<udp::UdpReceiver>,
    parser: Option<parser::NaluParser>,
    ring: std::sync::Arc<Ring>,
    stats: std::sync::Arc<Stats>,
}

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();

/// `usize`-encoding van de context-pointer: `usize` is altijd `Send + Copy`,
/// dus worker-threads kunnen de closure veilig capturen. Bij de callback-cast
/// zetten we terug naar `*mut c_void`.
#[inline]
fn ctx_to_usize(p: *mut c_void) -> usize {
    p as usize
}
#[inline]
fn usize_to_ctx(u: usize) -> *mut c_void {
    u as *mut c_void
}

/// Start de UDP-listener + NALU-parser op `port`.
///
/// # Safety
/// - Niet twee keer aanroepen zonder `hyprpad_stop` ertussen.
/// - `ctx` wordt ongewijzigd teruggegeven aan elke callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hyprpad_start(
    port: u16,
    callbacks: HyprpadCallbacks,
    ctx: *mut c_void,
) -> bool {
    let mutex = ENGINE.get_or_init(|| {
        Mutex::new(Engine {
            udp: None,
            parser: None,
            ring: std::sync::Arc::new(Ring::new()),
            stats: std::sync::Arc::new(Stats::new()),
        })
    });

    let mut engine = match mutex.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };

    // Indien al gestart: eerst netjes stoppen.
    engine.udp.take();
    engine.parser.take();
    engine.stats.reset();

    let ring = engine.ring.clone();
    let stats = engine.stats.clone();
    let stats_for_udp = stats.clone();

    let ctx_u = ctx_to_usize(ctx);

    // Log-wrapper: formateer &str naar C-string.
    let log_cb = callbacks.on_log;
    let log_ctx_u = ctx_u;
    let logger: Box<dyn FnMut(u8, &str) + Send> = Box::new(move |level: u8, msg: &str| {
        let c = std::ffi::CString::new(msg).unwrap_or_default();
        log_cb(level, c.as_ptr(), usize_to_ctx(log_ctx_u));
    });

    // UDP receiver.
    let udp = match udp::UdpReceiver::start(port, ring.clone(), stats_for_udp, logger) {
        Ok(u) => u,
        Err(e) => {
            let msg = std::ffi::CString::new(format!("UDP bind fout: {e}")).unwrap_or_default();
            log_cb(2, msg.as_ptr(), usize_to_ctx(ctx_u));
            return false;
        }
    };

    // NALU-parser.
    let nalu_cb = callbacks.on_nalu;
    let nalu_ctx_u = ctx_u;
    let stats_for_parser = stats.clone();
    let parser = parser::NaluParser::start(
        ring,
        stats_for_parser,
        move |data, len, nal_type| {
            nalu_cb(data, len, nal_type, usize_to_ctx(nalu_ctx_u));
        },
    );

    engine.udp = Some(udp);
    engine.parser = Some(parser);
    true
}

/// Stop de actieve stream en join de worker-threads.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hyprpad_stop() {
    if let Some(mutex) = ENGINE.get() {
        if let Ok(mut engine) = mutex.lock() {
            engine.udp.take();
            engine.parser.take();
            engine.stats.set_state(stats::State::Idle);
        }
    }
}

/// Poll de stats. Veilig om vanaf elke thread (incl. main) aangeroepen te worden.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hyprpad_stats() -> HyprpadStats {
    if let Some(mutex) = ENGINE.get() {
        if let Ok(engine) = mutex.lock() {
            return engine.stats.snapshot();
        }
    }
    HyprpadStats::default()
}
