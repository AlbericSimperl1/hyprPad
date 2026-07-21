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
    /// **raw H.264 Annex-B over UDP** — de iPad-client (hyprPadClient) zoekt
    /// Annex-B startcodes (`00 00 00 01`), dus we mogen géén MPEG-TS muxen.
    ///
    /// VLC accepteerde MPEG-TS maar buffert sterk; de hyprPadClient heeft
    /// geen TS-demuxer. Door raw Annex-B te sturen krijgt de Rust NALU-parser
    /// werk direct (en is `<100ms` latency haalbaar).
    ///
    /// Equivalent ffmpeg command-line:
    ///
    /// ```text
    /// ffmpeg -f rawvideo -pixel_format bgr0 -video_size 1920x1080 -framerate 60 \
    ///   -i - -c:v libx264 -preset ultrafast -tune zerolatency \
    ///   -profile:v baseline -level 4.0 -bf 0 \
    ///   -g 60 -keyint_min 60 -sc_threshold 0 \
    ///   -pix_fmt yuv420p -f h264 udp://192.168.0.119:5000?pkt_size=1316
    /// ```
    ///
    /// output_path is louter voor API-compatibiliteit met de oude MP4-pipeline.
    pub fn start(width: u32, height: u32, fps: u32, _output_path: &str) -> Result<Self, String> {
        let size = format!("{width}x{height}");
        let rate = format!("{fps}");
        // Korte GOP (1 s) → snelle recovery bij packetverlies én stijger I-frame cadans
        // voor onder 100 ms end-to-end latency.
        let gop = format!("{}", fps.max(1));
        let ipad_udp_url = "udp://192.168.0.119:5000?pkt_size=1316";

        let mut child = Command::new("ffmpeg")
            .args([
                // --- INPUT: raw BGR0 frames van stdin (PipeWire) ---
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
                // --- ENCODER: x264, ultra-low latency ---
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                // VideoToolbox op iOS verdraagt slecht B-frames; met -bf 0 zijn
                // alleen I / P actief → decoder loopt realtime zonder reorder queue.
                "-bf",
                "0",
                // Baseline / 4.0 past bij iPad hardware decoder + geen B-frames.
                "-profile:v",
                "baseline",
                "-level",
                "4.0",
                // bgr0 → yuv420p (x264 vereist yuv420p voor H.264 baseline).
                "-pix_fmt",
                "yuv420p",
                // Voorspelbare keyframes: 1x per seconde, geen scenecut-triggers.
                "-g",
                &gop,
                "-keyint_min",
                &gop,
                "-sc_threshold",
                "0",
                // Herhaal SPS/PPS voor elke keyframe → client kan op elk moment
                // aansluiten en heeft meteen de format-description nodig.
                "-x264-params",
                "repeat-headers=1:annexb=1",
                // --- LOW-LATENCY MUXER FLAGS (van toepassing op h264 raw) ---
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
                // --- OUTPUT: raw H.264 Annex-B over UDP, laatste arg ---
                "-f",
                "h264",
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
