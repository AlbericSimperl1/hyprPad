#### Fase 1: De Hyprland Integratie (Virtueel Scherm)
In plaats van op kernel-niveau een scherm te faken, vertellen we Hyprland gewoon dat er een monitor is.
1. **Hyprland CLI aansturen:** In Rust roep je `hyprctl keyword monitor NAME,1920x1080@60,0x0,1` aan (of via Hyprland's IPC socket met de `hyprland` Rust crate).
2. **Capture locatie bepalen:** Omdat Hyprland een **wlroots** compositor is, is de ultieme, meest performante manier om beeld op te vangen via het **`wlr-screencopy` protocol**. 
   * *Let op:* Dit betekent dat je Rust-applicatie een Wayland-client wordt. Gebruik de crate `wayland-client` in combinatie met de `wlr-screencopy-unstable-v1` protocol extensies.

#### Fase 2: De Rust Applicatie (Capture -> Encode -> Send)
1. **Wayland Capture:** Gebruik de `wlr-screencopy` protocol om een DMA-BUF file descriptor (FD) op te halen van het virtuele Hyprland-scherm. **Geen pixel-kopieën naar de CPU!**
2. **Encode:** We gebruiken opnieuw FFmpeg (als C-library via `ffmpeg-next`), maar nu specifiek met **VAAPI** (Intel/AMD) of **NVENC** (Nvidia). We geven de DMA-BUF FD direct aan de hardware encoder.
3. **Transport:** We sturen de ruwe H.264 NAL units over UDP via de USB-C tethering verbinding, verpakt in RTP (`rtp-rs` crate).

#### Fase 3: De GUI in Rust
Aangezien je toch in de Wayland ecosystem zit, is er maar één logische keuze voor een snelle, native GUI in Rust: **EGUI met de `winit`/`smithay-egui` backend**. 
* EGUI rendert direct via een GPU context (WGPU of OpenGL). Het is razendsnel en perfect voor een klein controlepaneel (bitrate aanpassen, verbinding status, FPS weergave).

#### Fase 4: De iPad Swift Applicatie
Onveranderd ten opzichte van het vorige plan, maar ter herhaling: `Network.framework` (UDP) -> `VideoToolbox` (Hardware Decode) -> `Metal` (Direct Render).

---

### De Prestatie-Blokken (Rust Zijde)

Hier is de exacte stack die je in je `Cargo.toml` moet hebben voor maximale frames per seconde:

1. **GUI & App Loop:**
   * `eframe` (De EGUI framework crate). Dit geeft je een venster met een GUI, maar runt ook je main loop.
   * `hyprland` (Crate om IPC te praten met Hyprland, voor het aanmaken van het scherm).
2. **Wayland Intercept (De absolute sleutel tot prestatie):**
   * `wayland-client`
   * `wayland-protocols` (Zorg dat je de `wlr-screencopy-unstable-v1` protocol files importeert).
3. **Video Encoding (FFmpeg Hardware):**
   * `ffmpeg-sys-next` (Build FFmpeg met `--enable-vaapi` of `--enable-nvenc`).
   * `ffmpeg-next` (De veilige Rust wrapper).
4. **Netwerk:**
   * `tokio` (Voor een aparte, non-blocking network thread).
   * `rtp-rs` (Om H.264 pockets in RTP te gieten, de iPad verwacht dit voor soepele weergave).

---

### Architectuur & Datenstroom (Technisch)

Je moet multithreaden in Rust, anders blokkeert je GUI de video-stream, of vice versa.

**Thread 1: De GUI (Egui)**
* Knop: "Start Monitor" -> Voert `hyprctl` uit om virtueel scherm te maken.
* Slider: "Bitrate" -> Stuurt een waarde via een `Arc<Mutex<Config>>` naar de encoder thread.
* Toont FPS counter uit de netwerk-thread.

**Thread 2: Wayland Capture & Encode Loop**
* Registreert zich als `wlr-screencopy` client bij Hyprland voor het virtuele scherm.
* Hyprland roept "Frame Ready" aan.
* Haalt de `DMA-BUF` op.
* Stopt de DMA-BUF in FFmpeg's `AVCodecContext` (VAAPI/NVENC).
* Krijgt gecomprimeerde H.264 data terug.

**Thread 3: Tokio Network Sender**
* Krijgt de H.264 data via een `tokio::sync::mpsc` channel vanuit Thread 2.
* Verpakt het in RTP pakketsjes.
* Stuurt het woest en ongefilterd naar `192.168.5.2:5004` (het IP van de USB-tethered iPad).
