# QR Frame Scanner

A lightweight always-on-top screen QR code reader. Written in Rust.

Position the frame over any QR code on your screen → decoded text appears instantly → copy it.

---

## Download

Prebuilt binaries for Windows, Linux and macOS (Apple Silicon) are on the
[Releases](https://github.com/tongelberkay/qrReader_rust/releases) page.

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

Linux needs the X11/Wayland/OpenGL dev packages — `build.sh` installs them
on apt/dnf systems.

**Manual:**
```
cargo build --release
```

---

## Releasing

CI builds every push to `main` and every pull request on Windows, Linux and macOS.

To publish a release, push a version tag. GitHub Actions builds all platforms
and attaches the binaries to a new GitHub Release:

```bash
git tag v1.0.0
git push origin v1.0.0
```

---

## Troubleshooting

- **Windows: `An Application Control policy has blocked this file (os error 4551)` while building** —
  Smart App Control is blocking Cargo's build scripts. Turn it off in
  Windows Security → App & browser control → Smart App Control.
- **NVIDIA: frame is not transparent, app is detected as a game, system lags** —
  in NVIDIA Control Panel → Manage 3D settings, set
  **Vulkan/OpenGL present method** to **Auto** or **Prefer native**
  (not *Prefer layered on DXGI Swapchain*).

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
