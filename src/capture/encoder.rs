use std::io::Write;
use std::process::{Child, Command, Stdio};

/// Owns a running `ffmpeg -i - … out.mp4` process
pub struct Encoder {
    child: Child,
    width: u32,
    height: u32,
}

impl Encoder {
    /// Spawn ffmpeg reading raw BGR0 frames from stdin and streaming them as
    /// H.264 / MPEG-TS over UDP — mirrors the standalone command:
    ///
    /// ```text
    /// ffmpeg -re -f lavfi -i testsrc=size=1920x1080:rate=60 \
    ///   -c:v libx264 -preset ultrafast -tune zerolatency \
    ///   -g 30 -keyint_min 30 \
    ///   -f mpegts udp://192.168.0.119:5000?pkt_size=1316
    /// ```
    ///
    /// `output_path` is only kept for API compatibility with the caller; the
    /// stream is the single UDP output (writing an extra `.mp4` alongside the
    /// UDP url breaks the keyframe structure VLC needs to actually decode).
    pub fn start(width: u32, height: u32, fps: u32, _output_path: &str) -> Result<Self, String> {
        let size = format!("{width}x{height}");
        let rate = format!("{fps}");
        let gop = format!("{fps}");
        let ipad_udp_url = "udp://192.168.0.119:5000?pkt_size=1316";

        let mut child = Command::new("ffmpeg")
            .args([
                // --- INPUT: raw BGR0 frames from stdin (Rust) ---
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "bgr0",
                "-video_size",
                &size,
                "-framerate",
                &rate,
                "-i",
                "-",
                // --- ENCODER: match the working standalone command ---
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                // bgr0 must be converted to yuv420p for H.264 compatibility.
                // (testsrc in the standalone command already outputs yuv420p,
                //  so it doesn't need this flag — we do.)
                "-pix_fmt",
                "yuv420p",
                "-g",
                &gop,
                "-keyint_min",
                &gop,
                // --- LOW-LATENCY MUXER FLAGS ---
                // Prevent ffmpeg from buffering TS packets before flushing.
                "-fflags",
                "nobuffer",
                "-flags",
                "low_delay",
                "-flush_packets",
                "1",
                "-max_delay",
                "0",
                "-muxdelay",
                "0",
                "-muxpreload",
                "0",
                // --- OUTPUT: single MPEG-TS over UDP, must be last arg ---
                "-f",
                "mpegts",
                ipad_udp_url,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())?;

        // Sanity: ffmpeg may exit instantly if the path is bad. Read stderr
        // lazily; we only inspect it on finalize failure.
        let _ = child.stderr.take();

        Ok(Self {
            child,
            width,
            height,
        })
    }

    /// Push one tightly-packed BGR0 frame (`width * height * 4` bytes).
    pub fn push_frame(&mut self, bytes: &[u8]) -> Result<(), String> {
        let expected = (self.width as usize) * (self.height as usize) * 4;
        if bytes.len() < expected {
            return Err(format!(
                "Short frame: got {} bytes, expected {expected}",
                bytes.len()
            ));
        }
        if let Some(stdin) = self.child.stdin.as_mut() {
            stdin
                .write_all(&bytes[..expected])
                .map_err(|e| format!("ffmpeg stdin write failed: {e}"))
        } else {
            Err("ffmpeg stdin closed".into())
        }
    }

    /// Close stdin so ffmpeg flushes its encoder and writes the MP4 `moov`
    /// atom (required for a playable file). Blocks until ffmpeg exits.
    // pub fn finish(&mut self) -> Result<(), String> {
    //     // Drop stdin by replacing with None → EOF → ffmpeg finalizes.
    //     self.child.stdin.take();

    //     let output = self
    //         .child
    //         .wait()
    //         .map_err(|e| format!("ffmpeg wait failed: {e}"))?;

    //     if output.success() {
    //         Ok(())
    //     } else {
    //         Err(format!("ffmpeg exited with status {:?}", output.code()))
    //     }
    // }

    pub fn finish(&mut self) -> Result<(), String> {
        // 1. Sluit de stdin pipe direct af.
        // Dit stuurt het EOF (End of File) signaal naar FFmpeg, zodat hij weet dat de opname stopt.
        if let Some(stdin) = self.child.stdin.take() {
            std::mem::drop(stdin);
        }

        // 2. Gebruik .wait() in plaats van .try_wait().
        // .wait() is blokkerend en wacht tot FFmpeg de MP4-container netjes heeft afgesloten.
        match self.child.wait() {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("FFmpeg is gestopt met een foutcode: {}", status))
                }
            }
            Err(e) => Err(format!(
                "Fout tijdens het wachten op het FFmpeg-proces: {}",
                e
            )),
        }
    }

    // pub fn is_alive(&mut self) -> bool {
    //     match self.child.try_wait() {
    //         Ok(None) => true,
    //         _ => false,
    //     }
    // }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // If the caller forgot finish(), at least close stdin so the child
        // doesn't hang forever waiting for input.
        self.child.stdin.take();
        let _ = self.child.wait();
    }
}
