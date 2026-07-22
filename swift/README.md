# hyprPadClient — iPadOS 26+ stream viewer

Ontvangt de H.264 Annex-B stream van de hyprPad server (Rust/FFmpeg/UDP) en
rendert deze met VideoToolbox op een `AVSampleBufferDisplayLayer`. Doel: ≥50fps
met <100ms latency.

## Architectuur

```
UDP poort 5000 ─→ Rust core (staticlib) ─→ NALU callback ─→ Swift
                     │                                            │
                     ├─ udp.rs      (recv → ring)                 ├─ H264Decoder (VideoToolbox)
                     ├─ ring.rs     (lock-free SPSC 8 MiB)        └─ VideoView   (display layer)
                     ├─ parser.rs   (Annex-B split + EPB strip)
                     └─ stats.rs    (fps/bytes/resolutie)
```

## Bouwen

### Vooraf
- Rust 1.97+ met target `aarch64-apple-ios` geïnstalleerd
  (`rustup target add aarch64-apple-ios`)
- Swift 6.3+ met de iPhoneOS 26.x Swift SDK artifact bundle
- `cbindgen` (`cargo install cbindgen`)
- `xtool` voor device-deploy

### 1. Rust staticlib + C header

```bash
cd rust_core
cargo build --release --target aarch64-apple-ios
cbindgen --crate rust_core --output ../Sources/CRustCore/include/rust_core.h
cp target/aarch64-apple-ios/release/librust_core.a ../Sources/CRustCore/lib/
```

### 2. Swift package + iPad deploy

```bash
cd ..
xtool build       # compileert de Swift app, linkt librust_core.a
xtool install     # installeert op de verbonden iPad
```

## Poort en IP

De client luistert op **UDP poort 5000** op alle interfaces (`0.0.0.0`).
De server (hyprPad `encoder.rs`) moet streamen naar het IP van de iPad.

Op iPadOS verschijnt bij de eerste UDP-pakketten een **Local Network permission**
prompt — accepteer deze.
