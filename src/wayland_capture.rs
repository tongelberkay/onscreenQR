// src/wayland_capture.rs
// Wayland screen capture through the xdg-desktop-portal ScreenCast API + PipeWire.
//
// Wayland hides window positions and blocks direct screen reads, and the one-shot
// Screenshot portal prompts (or fails) on every call. ScreenCast asks once, hands us a
// continuous PipeWire stream of the chosen monitor, and returns a restore token so later
// launches can skip the dialog. Works on GNOME, KDE, and wlroots/Hyprland portals.

use std::fs;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::PersistMode;
use image::GrayImage;
use pipewire as pw;
use pw::spa;
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::Pod;

// The scan loop decodes at most this often, so converting more frames is wasted work.
const FRAME_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Default)]
struct Shared {
    latest: Option<GrayImage>,
    error: Option<String>,
    failed: bool,
}

#[derive(Clone)]
pub struct WaylandCapture {
    shared: Arc<Mutex<Shared>>,
}

impl WaylandCapture {
    /// Starts the portal handshake and PipeWire stream on a background thread.
    pub fn start(on_update: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let thread_shared = Arc::clone(&shared);
        thread::spawn(move || {
            if let Err(e) = run(&thread_shared, &on_update) {
                let mut s = thread_shared.lock().unwrap();
                s.error = Some(e);
                s.failed = true;
            }
            on_update();
        });
        Self { shared }
    }

    /// Takes the newest frame, if one arrived since the last call.
    pub fn take_frame(&self) -> Option<GrayImage> {
        self.shared.lock().unwrap().latest.take()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.lock().unwrap().error.clone()
    }

    /// True once the portal or PipeWire gave up; callers should fall back.
    pub fn failed(&self) -> bool {
        self.shared.lock().unwrap().failed
    }
}

pub fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn run(shared: &Arc<Mutex<Shared>>, on_update: &impl Fn()) -> Result<(), String> {
    let (fd, node_id) = pollster::block_on(open_portal_stream())?;
    run_pipewire(fd, node_id, shared, on_update)
}

async fn open_portal_stream() -> Result<(OwnedFd, u32), String> {
    let err = |e: ashpd::Error| format!("Screen cast portal: {e}");

    let proxy = Screencast::new().await.map_err(err)?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(err)?;

    let token = load_restore_token();
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Hidden)
                .set_sources(Some(SourceType::Monitor.into()))
                .set_multiple(false)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(token.as_deref()),
        )
        .await
        .map_err(err)?;

    let streams = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(err)?
        .response()
        .map_err(|_| "Screen sharing was cancelled".to_string())?;

    if let Some(token) = streams.restore_token() {
        save_restore_token(token);
    }

    let node_id = streams
        .streams()
        .first()
        .map(|s| s.pipe_wire_node_id())
        .ok_or("Screen cast portal returned no streams")?;

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(err)?;

    // The portal ends the cast when the session is closed; keep it for the app's lifetime.
    std::mem::forget(session);
    Ok((fd, node_id))
}

struct StreamData {
    format: VideoInfoRaw,
    last_frame: Option<Instant>,
}

fn run_pipewire(
    fd: OwnedFd,
    node_id: u32,
    shared: &Arc<Mutex<Shared>>,
    on_update: &impl Fn(),
) -> Result<(), String> {
    let err = |e: pw::Error| format!("PipeWire: {e}");

    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(err)?;
    let core = context.connect_fd_rc(fd, None).map_err(err)?;

    let stream = pw::stream::StreamBox::new(
        &core,
        "qr-frame-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(err)?;

    let frame_shared = Arc::clone(shared);
    let _listener = stream
        .add_local_listener_with_user_data(StreamData {
            format: Default::default(),
            last_frame: None,
        })
        .param_changed(|_, data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let _ = data.format.parse(param);
        })
        .process(move |stream, data| {
            // Always dequeue so PipeWire can recycle the buffer.
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            if data
                .last_frame
                .is_some_and(|t| t.elapsed() < FRAME_INTERVAL)
            {
                return;
            }
            let size = data.format.size();
            let format = data.format.format();
            let datas = buffer.datas_mut();
            let Some(plane) = datas.first_mut() else {
                return;
            };
            let chunk = plane.chunk();
            let (offset, stride) = (chunk.offset() as usize, chunk.stride());
            let Some(bytes) = plane.data() else { return };
            let stride = if stride > 0 {
                stride as usize
            } else {
                size.width as usize * 4
            };
            if let Some(img) = to_luma(&bytes[offset.min(bytes.len())..], size.width, size.height, stride, format) {
                data.last_frame = Some(Instant::now());
                let mut s = frame_shared.lock().unwrap();
                s.latest = Some(img);
                s.error = None;
            }
        })
        .register()
        .map_err(err)?;

    let format_pod = build_format_pod();
    let mut params = [Pod::from_bytes(&format_pod).ok_or("PipeWire: bad format pod")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(err)?;

    on_update();
    mainloop.run();
    Err("Screen cast stream ended".into())
}

fn to_luma(bytes: &[u8], width: u32, height: u32, stride: usize, format: VideoFormat) -> Option<GrayImage> {
    // Channel order of the first three bytes; the fourth is alpha/padding.
    let (r, g, b) = match format {
        VideoFormat::BGRx | VideoFormat::BGRA => (2, 1, 0),
        VideoFormat::RGBx | VideoFormat::RGBA => (0, 1, 2),
        _ => return None,
    };
    if width < 10 || height < 10 || bytes.len() < stride * (height as usize - 1) + width as usize * 4 {
        return None;
    }
    let mut luma = Vec::with_capacity((width * height) as usize);
    for row in bytes.chunks(stride).take(height as usize) {
        for px in row[..width as usize * 4].chunks_exact(4) {
            let (r, g, b) = (px[r] as u32, px[g] as u32, px[b] as u32);
            luma.push(((r * 77 + g * 150 + b * 29) >> 8) as u8);
        }
    }
    GrayImage::from_raw(width, height, luma)
}

fn build_format_pod() -> Vec<u8> {
    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    use spa::pod::{property, Value};
    use spa::utils::{Fraction, Rectangle, SpaTypes};

    let obj = spa::pod::object!(
        SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        property!(FormatProperties::MediaType, Id, MediaType::Video),
        property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle { width: 1920, height: 1080 },
            Rectangle { width: 1, height: 1 },
            Rectangle { width: 8192, height: 8192 }
        ),
        property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 5, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: 1000, denom: 1 }
        ),
    );
    spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(obj))
        .expect("serialize format pod")
        .0
        .into_inner()
}

// ── Restore token (skips the share dialog on later launches) ──────
fn token_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("qr-frame").join("screencast-token"))
}

fn load_restore_token() -> Option<String> {
    let token = fs::read_to_string(token_path()?).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn save_restore_token(token: &str) {
    if let Some(path) = token_path() {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(path, token);
    }
}
