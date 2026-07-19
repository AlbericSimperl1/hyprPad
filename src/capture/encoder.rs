use std::io::Write;
use std::process::{Child, Command, Stdio};

/// Owns a running `ffmpeg -i - … out.mp4` process.
pub struct Encoder {
    child: Child,
    width: u32,
    height: u32,
}

impl Encoder {
    /// Spawn ffmpeg reading raw BGR0 frames from stdin and writing H.264 MP4.
    pub fn start(width: u32, height: u32, fps: u32, output_path: &str) -> Result<Self, String> {
        let size = format!("{width}x{height}");
        let rate = format!("{fps}");

        let mut child = Command::new("ffmpeg")
            // raw video input
            .args([
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
            ])
            // libx264 encode — fast preset, yuv420p for broad compatibility
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-tune",
                "zerolatency",
                "-pix_fmt",
                "yuv420p",
                "-g",
                &format!("{}", fps * 2),
                "-bf",
                "0",
                "-movflags",
                "+faststart",
            ])
            .arg(output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn ffmpeg: {e}"))?;

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
    pub fn finish(&mut self) -> Result<(), String> {
        // Drop stdin by replacing with None → EOF → ffmpeg finalizes.
        self.child.stdin.take();

        let output = self
            .child
            .wait()
            .map_err(|e| format!("ffmpeg wait failed: {e}"))?;

        if output.success() {
            Ok(())
        } else {
            Err(format!("ffmpeg exited with status {:?}", output.code()))
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
