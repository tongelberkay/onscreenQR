// src/main.rs
// QR Frame Scanner — Rust / egui edition

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod wayland_capture;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Vec2};
use image::{GrayImage, ImageBuffer, Luma};
use rxing::{
    common::HybridBinarizer, BinaryBitmap, DecodeHintType, DecodeHintValue, Luma8LuminanceSource,
    MultiFormatReader, Reader,
};
use screenshots::Screen;

// ── Shared state ──────────────────────────────────────────────────
#[derive(Default, Clone)]
struct ScanState {
    decoded: String,
    flash_until: Option<Instant>,
    copied_until: Option<Instant>,
    last_error: Option<String>,
    idle: bool,
    wake_until: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaptureArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl CaptureArea {
    fn from_viewfinder(window_min: Pos2, viewfinder: Rect) -> Self {
        Self {
            x: (window_min.x + viewfinder.min.x).floor() as i32,
            y: (window_min.y + viewfinder.min.y).floor() as i32,
            width: viewfinder.width().max(0.0).ceil() as u32,
            height: viewfinder.height().max(0.0).ceil() as u32,
        }
    }

    fn anchor_point(self) -> (i32, i32) {
        (
            self.x + self.width.saturating_div(2) as i32,
            self.y + self.height.saturating_div(2) as i32,
        )
    }

    fn relative_to_origin(self, screen_x: i32, screen_y: i32) -> Self {
        Self {
            x: self.x - screen_x,
            y: self.y - screen_y,
            width: self.width,
            height: self.height,
        }
    }

    fn scale(self, scale_factor: f32) -> Self {
        Self {
            x: ((self.x as f32) * scale_factor).floor() as i32,
            y: ((self.y as f32) * scale_factor).floor() as i32,
            width: ((self.width as f32) * scale_factor).ceil() as u32,
            height: ((self.height as f32) * scale_factor).ceil() as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CaptureRequest {
    area: CaptureArea,
    window_origin_known: bool,
}

type CaptureRect = Arc<Mutex<Option<CaptureRequest>>>;

struct QrFrameApp {
    state: Arc<Mutex<ScanState>>,
    capture_rect: CaptureRect,
    clipboard: Option<Clipboard>,
    update: Arc<Mutex<UpdateStatus>>,
    show_about: bool,
}

impl QrFrameApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Follow the OS theme so the UI matches the native title bar
        cc.egui_ctx.all_styles_mut(|style| {
            style.visuals.panel_fill = Color32::TRANSPARENT;
            style.visuals.window_fill = Color32::TRANSPARENT;
            style.visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        });

        let state: Arc<Mutex<ScanState>> = Arc::new(Mutex::new(ScanState::default()));
        let capture_rect: CaptureRect = Arc::new(Mutex::new(None));

        {
            let state = Arc::clone(&state);
            let capture_rect = Arc::clone(&capture_rect);
            let ctx = cc.egui_ctx.clone();
            thread::spawn(move || scan_loop(state, capture_rect, ctx));
        }

        let update = Arc::new(Mutex::new(UpdateStatus::Unknown));
        spawn_update_check(Arc::clone(&update), cc.egui_ctx.clone());

        Self {
            state,
            capture_rect,
            clipboard: Clipboard::new().ok(),
            update,
            show_about: false,
        }
    }
}

// ── Colors for the light and dark themes ───────────────────────────
struct Palette {
    bar_bg: Color32,
    panel_bg: Color32,
    box_bg: Color32,
    viewfinder_tint: Color32,
    text_main: Color32,
    text_muted: Color32,
    text_label: Color32,
    text_faint: Color32,
    accent: Color32,
    accent_dim: Color32,
    copied_fg: Color32,
    copied_bg: Color32,
    error: Color32,
    flash: Color32,
}

impl Palette {
    fn for_theme(theme: egui::Theme) -> Self {
        match theme {
            egui::Theme::Dark => Self {
                bar_bg: Color32::from_rgba_unmultiplied(17, 17, 17, 240),
                panel_bg: Color32::from_rgba_unmultiplied(10, 10, 10, 240),
                box_bg: Color32::from_rgb(20, 20, 20),
                viewfinder_tint: Color32::from_rgba_premultiplied(13, 26, 18, 60),
                text_main: Color32::from_rgb(224, 224, 224),
                text_muted: Color32::from_rgb(130, 130, 130),
                text_label: Color32::from_rgb(110, 110, 110),
                text_faint: Color32::from_rgb(90, 90, 90),
                accent: Color32::from_rgb(0, 232, 122),
                accent_dim: Color32::from_rgb(0, 90, 48),
                copied_fg: Color32::WHITE,
                copied_bg: Color32::from_rgb(0, 42, 24),
                error: Color32::from_rgb(255, 68, 85),
                flash: Color32::WHITE,
            },
            egui::Theme::Light => Self {
                bar_bg: Color32::from_rgba_unmultiplied(246, 246, 246, 245),
                panel_bg: Color32::from_rgba_unmultiplied(250, 250, 250, 245),
                box_bg: Color32::from_rgb(236, 236, 236),
                viewfinder_tint: Color32::from_rgba_unmultiplied(0, 140, 70, 18),
                text_main: Color32::from_rgb(30, 30, 30),
                text_muted: Color32::from_rgb(105, 105, 105),
                text_label: Color32::from_rgb(120, 120, 120),
                text_faint: Color32::from_rgb(150, 150, 150),
                accent: Color32::from_rgb(0, 140, 72),
                accent_dim: Color32::from_rgb(130, 190, 155),
                copied_fg: Color32::from_rgb(0, 100, 50),
                copied_bg: Color32::from_rgb(210, 240, 222),
                error: Color32::from_rgb(200, 30, 50),
                flash: Color32::from_rgb(40, 220, 120),
            },
        }
    }
}

const APP_ID: &str = "com.berkay.qr-frame";
const APP_TITLE: &str = "QR Frame";
const WAYLAND_FALLBACK_SCAN_INTERVAL: Duration = Duration::from_millis(1200);
const ACTIVE_SCAN_INTERVAL: Duration = Duration::from_millis(150);
// Scanning slows down once the window has stayed put this long. It never
// stops fully, since the content under a still window can still change.
const IDLE_AFTER: Duration = Duration::from_secs(5);
const IDLE_SCAN_INTERVAL: Duration = Duration::from_secs(4);
// With a QR already found, a still window rarely needs rechecking
const DETECTED_SCAN_INTERVAL: Duration = Duration::from_secs(10);
const MOVE_SETTLE: Duration = Duration::from_millis(250);

impl eframe::App for QrFrameApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Hovering the window counts as activity and wakes the scanner
        if ctx.input(|i| i.pointer.has_pointer()) {
            self.state.lock().unwrap().wake_until = Some(Instant::now() + IDLE_AFTER);
        }
        let state = self.state.lock().unwrap().clone();
        let p = Palette::for_theme(ctx.theme());
        ctx.request_repaint_after(if state.idle {
            Duration::from_secs(1)
        } else {
            ACTIVE_SCAN_INTERVAL
        });

        // ── Hint bar ───────────────────────────────────────────────
        egui::TopBottomPanel::top("hint_bar")
            .frame(
                egui::Frame::new()
                    .fill(p.bar_bg)
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("position over a QR code")
                            .font(FontId::monospace(9.0))
                            .color(p.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let update_available =
                            matches!(*self.update.lock().unwrap(), UpdateStatus::Available(_));
                        if info_button(ui, &p, update_available).clicked() {
                            self.show_about = !self.show_about;
                        }
                    });
                });
            });

        // ── Result + copy panel ────────────────────────────────────
        egui::TopBottomPanel::bottom("result_panel")
            .frame(egui::Frame::new().fill(p.panel_bg).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("RESULT")
                        .font(FontId::monospace(9.0))
                        .color(p.text_label)
                        .strong(),
                );
                ui.add_space(4.0);

                let result_text = if state.decoded.is_empty() {
                    egui::RichText::new("—")
                        .font(FontId::monospace(10.0))
                        .color(p.text_faint)
                } else {
                    egui::RichText::new(&state.decoded)
                        .font(FontId::monospace(10.0))
                        .color(p.accent)
                };

                egui::Frame::new()
                    .fill(p.box_bg)
                    .inner_margin(10.0)
                    .corner_radius(CornerRadius::same(4))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.set_min_height(72.0);
                        ui.label(result_text);
                    });

                ui.add_space(6.0);

                let copied = state
                    .copied_until
                    .map(|t| t > Instant::now())
                    .unwrap_or(false);
                let (btn_text, btn_color, btn_bg) = if copied {
                    ("COPIED!", p.copied_fg, p.copied_bg)
                } else if state.decoded.is_empty() {
                    ("COPY TO CLIPBOARD", p.text_faint, p.box_bg)
                } else {
                    ("COPY TO CLIPBOARD", p.accent, p.box_bg)
                };

                let btn = egui::Button::new(
                    egui::RichText::new(btn_text)
                        .font(FontId::monospace(9.0))
                        .color(btn_color)
                        .strong(),
                )
                .fill(btn_bg)
                .min_size(Vec2::new(ui.available_width(), 32.0));

                if ui.add_enabled(!state.decoded.is_empty(), btn).clicked() {
                    if let Some(cb) = &mut self.clipboard {
                        let _ = cb.set_text(state.decoded.clone());
                        let mut s = self.state.lock().unwrap();
                        s.copied_until = Some(Instant::now() + Duration::from_millis(2000));
                    }
                }
            });

        // ── About panel (covers the viewfinder while open) ─────────
        if self.show_about {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(p.bar_bg).inner_margin(16.0))
                .show(ctx, |ui| self.about_ui(ui, &p));
            return;
        }

        // ── Viewfinder (transparent scan area) ─────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let available = ui.available_rect_before_wrap();

                ui.painter()
                    .rect_filled(available, CornerRadius::ZERO, p.viewfinder_tint);

                let flashing = state
                    .flash_until
                    .map(|t| t > Instant::now())
                    .unwrap_or(false);
                let corner_color = if flashing {
                    p.flash
                } else if state.idle {
                    p.accent_dim
                } else {
                    p.accent
                };
                draw_corners(
                    ui.painter(),
                    available,
                    corner_color,
                    24.0,
                    if flashing { 4.0 } else { 2.0 },
                );

                let error_str;
                let (status_text, status_color) = if let Some(err) = state.last_error.as_deref() {
                    error_str = format!("Err: {}", err);
                    (error_str.as_str(), p.error)
                } else if !state.decoded.is_empty() {
                    ("QR detected", p.accent)
                } else if state.idle {
                    ("Idle · move window to resume", p.text_muted)
                } else {
                    ("Scanning screen…", p.text_muted)
                };
                ui.painter().text(
                    Pos2::new(available.center().x, available.max.y - 10.0),
                    egui::Align2::CENTER_BOTTOM,
                    status_text,
                    FontId::monospace(9.0),
                    status_color,
                );

                // inner_rect / outer_rect are Some on X11, Windows and macOS, giving us the
                // true desktop-space origin of the window. On Wayland compositors the window
                // position is intentionally hidden, so both fields can come back as None.
                // We still record the local viewfinder size, then let the scan loop switch to
                // a full-screen fallback instead of sampling the wrong desktop coordinates.
                let window_origin =
                    ctx.input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect));
                *self.capture_rect.lock().unwrap() = Some(CaptureRequest {
                    area: CaptureArea::from_viewfinder(
                        window_origin.map(|r| r.min).unwrap_or(Pos2::ZERO),
                        available,
                    ),
                    window_origin_known: window_origin.is_some(),
                });
            });
    }
}

// ── About panel ────────────────────────────────────────────────────
const REPO_URL: &str = "https://github.com/tongelberkay/onscreenQR";

impl QrFrameApp {
    fn about_ui(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let text = |s: &str, size: f32, color: Color32| {
            egui::RichText::new(s)
                .font(FontId::monospace(size))
                .color(color)
        };
        let button = |s: &str| {
            egui::Button::new(text(s, 9.0, p.text_main).strong())
                .fill(p.box_bg)
                .min_size(Vec2::new(0.0, 26.0))
        };

        ui.label(text("QR FRAME", 13.0, p.text_main).strong());
        ui.label(text(
            concat!("version ", env!("CARGO_PKG_VERSION")),
            9.0,
            p.text_muted,
        ));
        ui.add_space(8.0);
        ui.label(text(env!("CARGO_PKG_DESCRIPTION"), 9.0, p.text_muted));
        ui.add_space(4.0);
        ui.hyperlink_to(
            text("github.com/tongelberkay/onscreenQR", 9.0, p.accent),
            REPO_URL,
        );

        ui.add_space(16.0);
        ui.label(text("UPDATES", 9.0, p.text_label).strong());
        ui.add_space(4.0);

        let status = self.update.lock().unwrap().clone();
        let (status_text, status_color) = match &status {
            UpdateStatus::Unknown => ("Not checked yet".to_string(), p.text_muted),
            UpdateStatus::Checking => ("Checking…".to_string(), p.text_muted),
            UpdateStatus::UpToDate => ("You're on the latest version".to_string(), p.text_muted),
            UpdateStatus::Available(tag) => (format!("Update available: {tag}"), p.accent),
            UpdateStatus::Failed => ("Couldn't check for updates".to_string(), p.error),
        };
        ui.label(text(&status_text, 9.0, status_color));
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if let UpdateStatus::Available(_) = status {
                if ui.add(button("DOWNLOAD")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                        "{REPO_URL}/releases/latest"
                    )));
                }
            }
            let checking = status == UpdateStatus::Checking;
            if ui.add_enabled(!checking, button("CHECK NOW")).clicked() {
                spawn_update_check(Arc::clone(&self.update), ui.ctx().clone());
            }
        });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if ui
                .add(button("CLOSE").min_size(Vec2::new(ui.available_width(), 28.0)))
                .clicked()
            {
                self.show_about = false;
            }
        });
    }
}

// ── Info button (drawn, so it never depends on font glyphs) ────────
fn info_button(ui: &mut egui::Ui, p: &Palette, badge: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::click());
    let color = if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        p.text_main
    } else {
        p.text_muted
    };
    let c = rect.center();
    let painter = ui.painter();
    painter.circle_stroke(c, 6.0, Stroke::new(1.2_f32, color));
    painter.circle_filled(c + Vec2::new(0.0, -2.8), 0.9, color);
    painter.line_segment(
        [c + Vec2::new(0.0, -0.8), c + Vec2::new(0.0, 3.2)],
        Stroke::new(1.3_f32, color),
    );
    if badge {
        painter.circle_filled(rect.right_top() + Vec2::new(-2.0, 2.0), 2.5, p.accent);
    }
    resp.on_hover_text("About")
}

// ── Update check (GitHub Releases, notify only) ────────────────────
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/tongelberkay/onscreenQR/releases/latest";

#[derive(Debug, Clone, Default, PartialEq)]
enum UpdateStatus {
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available(String),
    Failed,
}

fn spawn_update_check(status: Arc<Mutex<UpdateStatus>>, ctx: egui::Context) {
    *status.lock().unwrap() = UpdateStatus::Checking;
    thread::spawn(move || {
        let result = match fetch_latest_tag() {
            Some(tag) if is_newer(&tag, env!("CARGO_PKG_VERSION")) => UpdateStatus::Available(tag),
            Some(_) => UpdateStatus::UpToDate,
            None => UpdateStatus::Failed,
        };
        *status.lock().unwrap() = result;
        ctx.request_repaint();
    });
}

fn fetch_latest_tag() -> Option<String> {
    let body = ureq::get(LATEST_RELEASE_API)
        .set(
            "User-Agent",
            concat!("qr-frame/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .ok()?
        .into_string()
        .ok()?;
    // Only "tag_name" is needed, so pull it out without a JSON parser
    let rest = body.split("\"tag_name\"").nth(1)?;
    let start = rest.find('"')? + 1;
    let end = start + rest[start..].find('"')?;
    Some(rest[start..end].to_string())
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u64>().ok());
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

fn is_newer(tag: &str, current: &str) -> bool {
    matches!((parse_version(tag), parse_version(current)), (Some(a), Some(b)) if a > b)
}

// ── Corner bracket painter ─────────────────────────────────────────
fn draw_corners(painter: &egui::Painter, rect: Rect, color: Color32, arm: f32, width: f32) {
    let pad = 4.0;
    let radius = 6.0;
    let inner = rect.shrink(pad);
    // (corner point, horizontal direction, vertical direction) into the frame
    let corners = [
        (inner.left_top(), 1.0, 1.0),
        (inner.right_top(), -1.0, 1.0),
        (inner.left_bottom(), 1.0, -1.0),
        (inner.right_bottom(), -1.0, -1.0),
    ];
    for (c, dx, dy) in corners {
        // Vertical arm, rounded bend, horizontal arm as one continuous path
        let center = c + Vec2::new(dx * radius, dy * radius);
        let points: Vec<Pos2> = (0..=8)
            .map(|i| {
                let t = std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
                center + Vec2::new(-dx * radius * t.cos(), -dy * radius * t.sin())
            })
            .collect();
        painter.add(egui::epaint::PathShape::line(
            points,
            Stroke::new(width, color),
        ));

        // Arms as capsules: one anti-aliased shape each, so the tips come out
        // round and as smooth as the rest of the bracket
        let half = width / 2.0;
        let vertical = Rect::from_two_pos(
            c + Vec2::new(-half, dy * (arm - half)),
            c + Vec2::new(half, dy * radius),
        );
        let horizontal = Rect::from_two_pos(
            c + Vec2::new(dx * (arm - half), -half),
            c + Vec2::new(dx * radius, half),
        );
        let capsule = CornerRadius::same(half.round() as u8);
        painter.rect_filled(vertical.expand2(Vec2::new(0.0, half)), capsule, color);
        painter.rect_filled(horizontal.expand2(Vec2::new(half, 0.0)), capsule, color);
    }
}

// ── Scan thread ────────────────────────────────────────────────────
fn scan_loop(state: Arc<Mutex<ScanState>>, capture_rect: CaptureRect, ctx: egui::Context) {
    let mut last_wayland_fallback_scan = Instant::now() - WAYLAND_FALLBACK_SCAN_INTERVAL;
    let mut screen_stream: ScreenStream = Default::default();
    let mut last_area: Option<CaptureArea> = None;
    let mut last_moved = Instant::now();
    let mut last_scan = Instant::now() - IDLE_SCAN_INTERVAL;

    loop {
        thread::sleep(ACTIVE_SCAN_INTERVAL);

        let capture_request = match *capture_rect.lock().unwrap() {
            Some(r) => r,
            None => continue,
        };
        let capture_area = capture_request.area;

        if capture_area.width < 10 || capture_area.height < 10 {
            continue;
        }

        // Moving or resizing the window changes the capture area
        if last_area != Some(capture_area) {
            last_area = Some(capture_area);
            last_moved = Instant::now();
        }
        let (idle, idle_interval) = {
            let mut s = state.lock().unwrap();
            let woken = s.wake_until.is_some_and(|t| t > Instant::now());
            let idle = !woken && last_moved.elapsed() >= IDLE_AFTER;
            if s.idle != idle {
                s.idle = idle;
                ctx.request_repaint();
            }
            let interval = if s.decoded.is_empty() {
                IDLE_SCAN_INTERVAL
            } else {
                DETECTED_SCAN_INTERVAL
            };
            (idle, interval)
        };
        // Skip captures mid-drag: grabbing the screen under a moving window
        // stalls the compositor and makes dragging stutter
        if last_moved.elapsed() < MOVE_SETTLE || (idle && last_scan.elapsed() < idle_interval) {
            continue;
        }
        last_scan = Instant::now();

        let decoded = if capture_request.window_origin_known {
            let (anchor_x, anchor_y) = capture_area.anchor_point();
            let screen = match Screen::from_point(anchor_x, anchor_y).ok().or_else(|| {
                Screen::all()
                    .ok()
                    .and_then(|screens| screens.first().cloned())
            }) {
                Some(screen) => screen,
                None => {
                    let mut s = state.lock().unwrap();
                    s.last_error = Some("No screen found at window".into());
                    ctx.request_repaint();
                    continue;
                }
            };

            let crop = capture_area
                .relative_to_origin(screen.display_info.x, screen.display_info.y)
                .scale(screen.display_info.scale_factor);

            match capture_luma(&screen, Some(crop)) {
                Ok(Some(luma)) => {
                    clear_capture_error(&state, &ctx);
                    try_decode_variants(&luma)
                }
                Ok(None) => continue,
                Err(e) => {
                    let mut s = state.lock().unwrap();
                    s.last_error = Some(format!("Capture err: {}", e));
                    ctx.request_repaint();
                    continue;
                }
            }
        } else {
            match poll_screen_stream(&mut screen_stream, &ctx) {
                StreamPoll::Frame(luma) => {
                    clear_capture_error(&state, &ctx);
                    let decoded = try_decode_variants(&luma);
                    publish_decoded(&state, &ctx, decoded);
                    continue;
                }
                StreamPoll::Pending => continue,
                StreamPoll::Unavailable => {}
            }

            if last_wayland_fallback_scan.elapsed() < WAYLAND_FALLBACK_SCAN_INTERVAL {
                continue;
            }
            last_wayland_fallback_scan = Instant::now();

            match scan_all_screens() {
                Ok(Some((decoded, had_capture))) => {
                    if had_capture {
                        clear_capture_error(&state, &ctx);
                    }
                    decoded
                }
                Ok(None) => continue,
                Err(e) => {
                    let mut s = state.lock().unwrap();
                    s.last_error = Some(format!("Capture err: {}", e));
                    ctx.request_repaint();
                    continue;
                }
            }
        };

        publish_decoded(&state, &ctx, decoded);
    }
}

fn publish_decoded(state: &Arc<Mutex<ScanState>>, ctx: &egui::Context, decoded: Option<String>) {
    let mut s = state.lock().unwrap();
    if let Some(text) = decoded {
        if text != s.decoded {
            s.decoded = text;
            s.flash_until = Some(Instant::now() + Duration::from_millis(350));
            ctx.request_repaint();
        }
    } else if !s.decoded.is_empty() {
        s.decoded.clear();
        ctx.request_repaint();
    }
}

// ── Wayland screen cast stream ─────────────────────────────────────
// Only Linux/Wayland needs it: elsewhere the window origin is known and
// `screenshots` can grab exactly the viewfinder area.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum StreamPoll {
    Frame(GrayImage),
    Pending,
    Unavailable,
}

#[cfg(target_os = "linux")]
type ScreenStream = Option<wayland_capture::WaylandCapture>;
#[cfg(not(target_os = "linux"))]
type ScreenStream = ();

#[cfg(target_os = "linux")]
fn poll_screen_stream(stream: &mut ScreenStream, ctx: &egui::Context) -> StreamPoll {
    if !wayland_capture::is_wayland_session() {
        return StreamPoll::Unavailable;
    }
    let capture = stream.get_or_insert_with(|| {
        let ctx = ctx.clone();
        wayland_capture::WaylandCapture::start(move || ctx.request_repaint())
    });
    if capture.failed() {
        return StreamPoll::Unavailable;
    }
    match capture.take_frame() {
        Some(luma) => StreamPoll::Frame(luma),
        None => StreamPoll::Pending,
    }
}

#[cfg(not(target_os = "linux"))]
fn poll_screen_stream(_: &mut ScreenStream, _: &egui::Context) -> StreamPoll {
    StreamPoll::Unavailable
}

fn clear_capture_error(state: &Arc<Mutex<ScanState>>, ctx: &egui::Context) {
    let mut s = state.lock().unwrap();
    if s.last_error.is_some() {
        s.last_error = None;
        ctx.request_repaint();
    }
}

fn scan_all_screens() -> Result<Option<(Option<String>, bool)>, String> {
    let screens = Screen::all().map_err(|e| e.to_string())?;
    if screens.is_empty() {
        return Ok(None);
    }

    let mut had_capture = false;
    for screen in screens {
        if let Some(luma) = capture_luma(&screen, None)? {
            had_capture = true;
            if let Some(decoded) = try_decode_variants(&luma) {
                return Ok(Some((Some(decoded), true)));
            }
        }
    }

    Ok(Some((None, had_capture)))
}

fn capture_luma(screen: &Screen, crop: Option<CaptureArea>) -> Result<Option<GrayImage>, String> {
    let full_shot = screen.capture().map_err(|e| e.to_string())?;
    let full_width = full_shot.width();
    let full_height = full_shot.height();

    let crop = crop.unwrap_or(CaptureArea {
        x: 0,
        y: 0,
        width: full_width,
        height: full_height,
    });

    let crop_x = crop.x.max(0) as u32;
    let crop_y = crop.y.max(0) as u32;
    let crop_w = crop.width.min(full_width.saturating_sub(crop_x));
    let crop_h = crop.height.min(full_height.saturating_sub(crop_y));
    if crop_w < 10 || crop_h < 10 {
        return Ok(None);
    }

    let full_raw = full_shot.as_raw();
    let full_stride = full_width as usize * 4;
    let mut cropped: Vec<u8> = Vec::with_capacity((crop_w * crop_h * 4) as usize);
    for row in 0..crop_h {
        let src_y = crop_y + row;
        let src_start = src_y as usize * full_stride + crop_x as usize * 4;
        let src_end = src_start + crop_w as usize * 4;
        if src_end <= full_raw.len() {
            cropped.extend_from_slice(&full_raw[src_start..src_end]);
        }
    }

    let raw = cropped.as_slice();
    let sw = crop_w as usize;
    Ok(Some(ImageBuffer::from_fn(crop_w, crop_h, |px, py| {
        let idx = (py as usize * sw + px as usize) * 4;
        if idx + 2 < raw.len() {
            let r = raw[idx] as u32;
            let g = raw[idx + 1] as u32;
            let b = raw[idx + 2] as u32;
            Luma([((r * 77 + g * 150 + b * 29) >> 8) as u8])
        } else {
            Luma([0u8])
        }
    })))
}

// ── Decode pipeline ────────────────────────────────────────────────
fn try_decode_variants(gray: &GrayImage) -> Option<String> {
    if let Some(r) = decode_luma(gray) {
        return Some(r);
    }
    let stretched = contrast_stretch(gray);
    if let Some(r) = decode_luma(&stretched) {
        return Some(r);
    }
    let inverted = invert_luma(gray);
    if let Some(r) = decode_luma(&inverted) {
        return Some(r);
    }
    if let Some(r) = decode_luma(&contrast_stretch(&inverted)) {
        return Some(r);
    }
    let thresh = threshold_luma(gray, 128);
    if let Some(r) = decode_luma(&thresh) {
        return Some(r);
    }
    if let Some(r) = decode_luma(&invert_luma(&thresh)) {
        return Some(r);
    }
    let (w, h) = gray.dimensions();
    if w < 600 && h < 600 {
        let up = upscale_2x(gray);
        if let Some(r) = decode_luma(&up) {
            return Some(r);
        }
        if let Some(r) = decode_luma(&contrast_stretch(&up)) {
            return Some(r);
        }
    }
    None
}

fn decode_luma(img: &GrayImage) -> Option<String> {
    let source = Luma8LuminanceSource::new(img.as_raw().clone(), img.width(), img.height());
    let bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
    let mut hints = rxing::DecodingHintDictionary::default();
    hints.insert(
        DecodeHintType::POSSIBLE_FORMATS,
        DecodeHintValue::PossibleFormats([rxing::BarcodeFormat::QR_CODE].iter().cloned().collect()),
    );
    hints.insert(DecodeHintType::TRY_HARDER, DecodeHintValue::TryHarder(true));
    let mut reader = MultiFormatReader::default();
    reader
        .decode_with_hints(&mut bitmap.into(), &hints)
        .ok()
        .map(|r| r.getText().to_string())
        .filter(|s| !s.is_empty())
}

fn contrast_stretch(img: &GrayImage) -> GrayImage {
    let (min_v, max_v) = img
        .pixels()
        .fold((255u8, 0u8), |(mn, mx), p| (mn.min(p[0]), mx.max(p[0])));
    if max_v == min_v {
        return img.clone();
    }
    let range = (max_v - min_v) as f32;
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let v = img.get_pixel(x, y)[0];
        Luma([((v - min_v) as f32 / range * 255.0) as u8])
    })
}

fn invert_luma(img: &GrayImage) -> GrayImage {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        Luma([255 - img.get_pixel(x, y)[0]])
    })
}

fn threshold_luma(img: &GrayImage, thresh: u8) -> GrayImage {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        Luma([if img.get_pixel(x, y)[0] > thresh {
            255
        } else {
            0
        }])
    })
}

fn upscale_2x(img: &GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    ImageBuffer::from_fn(w * 2, h * 2, |x, y| *img.get_pixel(x / 2, y / 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_area_uses_logical_window_points() {
        let window_min = Pos2::new(100.0, 200.0);
        let viewfinder = Rect::from_min_max(Pos2::new(24.2, 48.8), Pos2::new(324.1, 348.1));

        assert_eq!(
            CaptureArea::from_viewfinder(window_min, viewfinder),
            CaptureArea {
                x: 124,
                y: 248,
                width: 300,
                height: 300,
            }
        );
    }

    #[test]
    fn capture_area_converts_to_monitor_relative_coordinates() {
        let area = CaptureArea {
            x: 2200,
            y: 180,
            width: 320,
            height: 240,
        };

        assert_eq!(
            area.relative_to_origin(1920, 0),
            CaptureArea {
                x: 280,
                y: 180,
                width: 320,
                height: 240,
            }
        );
    }

    #[test]
    fn capture_area_anchor_point_uses_the_center() {
        let area = CaptureArea {
            x: 1920,
            y: 100,
            width: 321,
            height: 241,
        };

        assert_eq!(area.anchor_point(), (2080, 220));
    }

    #[test]
    fn capture_area_scale_converts_logical_to_physical_pixels() {
        let area = CaptureArea {
            x: 140,
            y: 80,
            width: 301,
            height: 201,
        };

        assert_eq!(
            area.scale(1.5),
            CaptureArea {
                x: 210,
                y: 120,
                width: 452,
                height: 302,
            }
        );
    }
}

// ── Window/taskbar icon (Windows, macOS, Linux) ────────────────────
fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../assets/icon.png");
    eframe::icon_data::from_png_bytes(bytes).expect("invalid icon PNG")
}

// ── Entry point ────────────────────────────────────────────────────
fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_app_id(APP_ID)
            .with_inner_size([380.0, 480.0])
            .with_min_inner_size([300.0, 400.0])
            .with_decorations(true)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(true)
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(|cc| Ok(Box::new(QrFrameApp::new(cc)))),
    )
    .unwrap();
}
