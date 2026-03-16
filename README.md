# QR Frame Scanner

A lightweight always-on-top screen QR code reader. Written in Rust.

Position the frame over any QR code on your screen → decoded text appears instantly → copy it.

---

## Requirements

| Tool | Notes |
|------|-------|
| **Rust + Cargo** | https://rustup.rs — one-line installer |

No Python, no runtime dependencies, no DLLs. Everything is statically compiled in.

---

## Build

**Windows:**
```
build_windows.bat
```
Output: `QR Frame Scanner.exe` (~5–15MB)

**macOS / Linux:**
```bash
cargo build --release
# output: target/release/qr-frame
```

On KDE/GNOME Wayland, install or launch the app through the included
`com.berkay.qr-frame.desktop` entry so the compositor can associate screen
capture permissions with a stable app id.

**Manual:**
```
cargo build --release
```

---

## Usage

- **Drag** the title bar to move the window
- **Resize** from any edge (egui native resize)
- Position the green frame over a QR code on your screen
- Decoded text appears instantly
- Click **⎘ COPY TO CLIPBOARD** to copy
- The app appears in the **taskbar** normally

---

## QR Code Support

Uses `rxing` (pure Rust ZXing port) with a multi-variant decode pipeline:

- Raw grayscale
- Contrast stretched
- Inverted (light-on-dark QR codes)
- Inverted + contrast stretch
- Hard threshold binarised
- Inverted threshold
- 2× upscaled (for small QR codes)
- 2× upscaled + contrast

---

## Stack

| Crate | Purpose |
|-------|---------|
| `eframe` + `egui` | Immediate mode GUI |
| `screenshots` | Screen region capture (DXGI on Windows) |
| `rxing` | Pure Rust QR decoder |
| `image` | Image manipulation |
| `arboard` | Clipboard access |

---

## Why Rust?

- No Python runtime to bundle
- No DLL dependencies
- ~5–15MB executable vs ~70MB PyInstaller bundle
- Zero memory leaks — ownership model handles it at compile time
- Instant startup
