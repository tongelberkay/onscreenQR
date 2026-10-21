# QR Frame Scanner

A lightweight always-on-top screen QR code reader. Written in Rust.

Position the frame over any QR code on your screen → decoded text appears instantly → copy it.

## Download

Prebuilt binaries for Windows, Linux and macOS (Apple Silicon) are on the
[Releases](https://github.com/tongelberkay/qrReader_rust/releases) page.

## Usage

- Drag the title bar to move, drag any edge to resize
- Put the green frame over a QR code — the decoded text appears below
- Click **⎘ COPY TO CLIPBOARD** to copy it

### Wayland

Wayland doesn't let apps read the screen directly, so the app uses the
desktop's screen-sharing portal. On first launch pick the monitor to scan in
the "share your screen" dialog; the choice is remembered (delete
`~/.config/qr-frame/screencast-token` to reset it). Since Wayland hides window
positions, the whole shared monitor is scanned rather than just the frame.

Needs PipeWire and an xdg-desktop-portal backend (GNOME, KDE, wlroots/Sway,
Hyprland). Copy `com.berkay.qr-frame.desktop` to `~/.local/share/applications/`
to give the app a proper name in the dialog.

## Build

Requires [Rust](https://rustup.rs).

```bash
cargo build --release
```

Output: `target/release/qr-frame` (`qr-frame.exe` on Windows).
`build_windows.bat` and `build.sh` wrap this; `build.sh` also installs the
Linux dev packages (X11, Wayland, OpenGL, PipeWire, libclang) on apt/dnf.
On macOS, `make_app.sh` wraps the binary into `QR Frame.app`.

## Releasing

CI builds every push and pull request on Windows, Linux and macOS. Pushing a
version tag builds all platforms and publishes a GitHub Release:

```bash
git tag v1.0.0
git push origin v1.0.0
```

## Troubleshooting

- **Windows: `An Application Control policy has blocked this file (os error 4551)` while building** —
  Smart App Control blocks Cargo's build scripts. Turn it off in
  Windows Security → App & browser control → Smart App Control.
- **NVIDIA: frame is not transparent, app is detected as a game, system lags** —
  in NVIDIA Control Panel → Manage 3D settings, set
  **Vulkan/OpenGL present method** to **Auto** or **Prefer native**
  (not *Prefer layered on DXGI Swapchain*).
