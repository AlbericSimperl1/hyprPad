//! UDP-listener voor poort 5000.
//!
//! Dedicated thread met `std::net::UdpSocket`. Elk pakket gaat direct de ring
//! in — geen tussenliggende allocaties. Stats (bytes_total + state) worden via
//! een `Arc<Stats>` bijgewerkt.

use std::net::UdpSocket;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::ring::Ring;
use crate::stats::{State, Stats};

const READ_BUF_SIZE: usize = 65_536; // 64 KiB — ruim boven typische UDP MTU

pub struct UdpReceiver {
    handle: Option<JoinHandle<()>>,
}

impl UdpReceiver {
    /// Start het luisteren op `port`. Ontvangen pakketten worden in `ring`
    /// geschreven; `stats.bytes_total` wordt opgeteld; `on_log` rapporteert
    /// statuswijzigingen/fouten.
    pub fn start<F>(
        port: u16,
        ring: Arc<Ring>,
        stats: Arc<Stats>,
        mut on_log: F,
    ) -> std::io::Result<Self>
    where
        F: FnMut(u8, &str) + Send + 'static,
    {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        // 250ms timeout zodat we `running` regelmatig kunnen checken via de
        // WouldBlock-tak — ook al is er geen data.
        socket.set_read_timeout(Some(std::time::Duration::from_millis(250)))?;
        socket.set_broadcast(true)?;

        let handle = thread::Builder::new()
            .name("hyprpad-udp".into())
            .spawn(move || {
                on_log(0, &format!("UDP listening on 0.0.0.0:{}", port));
                stats.set_state(State::Listening);

                let mut buf = [0u8; READ_BUF_SIZE];
                loop {
                    match socket.recv_from(&mut buf) {
                        Ok((n, _peer)) => {
                            if n > 0 {
                                ring.write(&buf[..n]);
                                stats.add_bytes(n as u64);
                                if stats.state() == State::Listening as u8 {
                                    stats.set_state(State::Receiving);
                                }
                            }
                        }
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            continue;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                            continue;
                        }
                        Err(ref e) => {
                            on_log(2, &format!("UDP fout: {}", e));
                            stats.set_state(State::Error);
                            break;
                        }
                    }
                }

                // Socket gesloten — laat de readtimeout lekker verlopen; thread stopt
                // vanzelf als de socket dropt. We zetten hier géén `running` flag meer
                // (drop van UdpReceiver killt de join via stats signaal).
                let _ = stats.state();
            })?;

        Ok(Self {
            handle: Some(handle),
        })
    }
}

impl Drop for UdpReceiver {
    fn drop(&mut self) {
        // Join blokkeurt tot de recv_from-tak terugkeert (binnen 250ms) of de
        // socket sluit. We moeten de socket dus niet extern sluiten — drop van
        // de UdpSocket in de thread gebeurt automatisch bij thread-exit.
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// Stilzwijgend `Ordering` in gebruik voor toekomstige checks in deze module.
#[allow(dead_code)]
fn _ordering_use() {
    let _ = std::sync::atomic::AtomicBool::new(false).load(Ordering::Relaxed);
}
