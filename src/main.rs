// src/main.rs
// QR Frame Scanner — Rust / egui edition

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, ResizeDirection, Stroke, Vec2,
};
use image::{DynamicImage, GrayImage, ImageBuffer, Luma};
use rxing::{
    common::HybridBinarizer, BinaryBitmap, BufferedImageLuminanceSource, DecodeHintType,
    DecodeHintValue, MultiFormatReader, Reader,
};
use screenshots::Screen;

// ── Shared state ──────────────────────────────────────────────────
#[derive(Default, Clone)]
struct ScanState {
    decoded: String,
    flash_until: Option<Instant>,
    copied_until: Option<Instant>,
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
}

type CaptureRect = Arc<Mutex<Option<CaptureArea>>>;

struct QrFrameApp {
    state: Arc<Mutex<ScanState>>,
    capture_rect: CaptureRect,
    clipboard: Option<Clipboard>,
}

impl QrFrameApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.panel_fill = Color32::TRANSPARENT;
        style.visuals.window_fill = Color32::TRANSPARENT;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        cc.egui_ctx.set_style(style);

        let state: Arc<Mutex<ScanState>> = Arc::new(Mutex::new(ScanState::default()));
        let capture_rect: CaptureRect = Arc::new(Mutex::new(None));

        {
            let state = Arc::clone(&state);
            let capture_rect = Arc::clone(&capture_rect);
            let ctx = cc.egui_ctx.clone();
            thread::spawn(move || scan_loop(state, capture_rect, ctx));
        }

        Self {
            state,
            capture_rect,
            clipboard: Clipboard::new().ok(),
        }
    }
}

// ── Resize border thickness (logical pixels) ──────────────────────
const RESIZE_BORDER: f32 = 6.0;

impl eframe::App for QrFrameApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(150));
        let state = self.state.lock().unwrap().clone();

        let win_rect = ctx.screen_rect();

        // ── Resize hit zones on all edges + corners ────────────────
        // We paint invisible interaction zones along every edge/corner
        // and issue BeginResize when the user drags them.
        let dirs: &[(ResizeDirection, Rect)] = &[
            (
                ResizeDirection::NorthWest,
                Rect::from_min_size(win_rect.min, Vec2::splat(RESIZE_BORDER)),
            ),
            (
                ResizeDirection::North,
                Rect::from_min_size(
                    Pos2::new(win_rect.min.x + RESIZE_BORDER, win_rect.min.y),
                    Vec2::new(win_rect.width() - RESIZE_BORDER * 2.0, RESIZE_BORDER),
                ),
            ),
            (
                ResizeDirection::NorthEast,
                Rect::from_min_size(
                    Pos2::new(win_rect.max.x - RESIZE_BORDER, win_rect.min.y),
                    Vec2::splat(RESIZE_BORDER),
                ),
            ),
            (
                ResizeDirection::East,
                Rect::from_min_size(
                    Pos2::new(
                        win_rect.max.x - RESIZE_BORDER,
                        win_rect.min.y + RESIZE_BORDER,
                    ),
                    Vec2::new(RESIZE_BORDER, win_rect.height() - RESIZE_BORDER * 2.0),
                ),
            ),
            (
                ResizeDirection::SouthEast,
                Rect::from_min_size(
                    win_rect.max - Vec2::splat(RESIZE_BORDER),
                    Vec2::splat(RESIZE_BORDER),
                ),
            ),
            (
                ResizeDirection::South,
                Rect::from_min_size(
                    Pos2::new(
                        win_rect.min.x + RESIZE_BORDER,
                        win_rect.max.y - RESIZE_BORDER,
                    ),
                    Vec2::new(win_rect.width() - RESIZE_BORDER * 2.0, RESIZE_BORDER),
                ),
            ),
            (
                ResizeDirection::SouthWest,
                Rect::from_min_size(
                    Pos2::new(win_rect.min.x, win_rect.max.y - RESIZE_BORDER),
                    Vec2::splat(RESIZE_BORDER),
                ),
            ),
            (
                ResizeDirection::West,
                Rect::from_min_size(
                    Pos2::new(win_rect.min.x, win_rect.min.y + RESIZE_BORDER),
                    Vec2::new(RESIZE_BORDER, win_rect.height() - RESIZE_BORDER * 2.0),
                ),
            ),
        ];

        // Use an Area that covers the full window for resize zones
        egui::Area::new(egui::Id::new("resize_zones"))
            .fixed_pos(win_rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for (dir, rect) in dirs {
                    let cursor = resize_cursor(*dir);
                    let resp = ui.interact(
                        *rect,
                        egui::Id::new(format!("resize_{:?}", dir)),
                        egui::Sense::drag(),
                    );
                    if resp.hovered() {
                        ctx.set_cursor_icon(cursor);
                    }
                    if resp.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(*dir));
                    }
                }
            });

        // ── Title bar ──────────────────────────────────────────────
        let title_id = egui::Id::new("titlebar_drag");
        egui::TopBottomPanel::top("titlebar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(17, 17, 17, 240))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                let mut buttons_min_x = f32::MAX;
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("QR FRAME")
                            .font(FontId::monospace(11.0))
                            .color(Color32::from_rgb(224, 224, 224))
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("  position over a QR code")
                            .font(FontId::monospace(9.0))
                            .color(Color32::from_rgb(56, 56, 56)),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_size = Vec2::new(28.0, 28.0);

                        // Close button — turns red on hover
                        let (close_rect, close) =
                            ui.allocate_exact_size(btn_size, egui::Sense::click());
                        let close_color = if close.hovered() {
                            ctx.set_cursor_icon(CursorIcon::PointingHand);
                            Color32::from_rgb(255, 68, 85)
                        } else {
                            Color32::from_rgb(80, 80, 80)
                        };
                        ui.painter().text(
                            close_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "✕",
                            FontId::monospace(13.0),
                            close_color,
                        );
                        if close.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        ui.add_space(4.0);

                        // Minimize button — turns white on hover
                        let (mini_rect, mini) =
                            ui.allocate_exact_size(btn_size, egui::Sense::click());
                        let mini_color = if mini.hovered() {
                            ctx.set_cursor_icon(CursorIcon::PointingHand);
                            Color32::from_rgb(224, 224, 224)
                        } else {
                            Color32::from_rgb(80, 80, 80)
                        };
                        ui.painter().text(
                            mini_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "—",
                            FontId::monospace(13.0),
                            mini_color,
                        );
                        if mini.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }

                        // Track leftmost button edge so drag zone won't overlap
                        buttons_min_x = mini_rect.min.x.min(close_rect.min.x);
                    });
                });

                // Drag title bar to move window — only the area LEFT of the buttons
                let titlebar_rect = ui.min_rect();
                let drag_rect = Rect::from_min_max(
                    titlebar_rect.min,
                    Pos2::new(buttons_min_x, titlebar_rect.max.y),
                );
                let drag = ui.interact(drag_rect, title_id, egui::Sense::drag());
                if drag.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });

        // ── Result + copy panel ────────────────────────────────────
        egui::TopBottomPanel::bottom("result_panel")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(10, 10, 10, 240))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("RESULT")
                        .font(FontId::monospace(9.0))
                        .color(Color32::from_rgb(42, 42, 42))
                        .strong(),
                );
                ui.add_space(4.0);

                let result_text = if state.decoded.is_empty() {
                    egui::RichText::new("—")
                        .font(FontId::monospace(10.0))
                        .color(Color32::from_rgb(50, 50, 50))
                } else {
                    egui::RichText::new(&state.decoded)
                        .font(FontId::monospace(10.0))
                        .color(Color32::from_rgb(0, 232, 122))
                };

                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 20, 20))
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
                    ("✓  COPIED!", Color32::WHITE, Color32::from_rgb(0, 42, 24))
                } else if state.decoded.is_empty() {
                    (
                        "⎘  COPY TO CLIPBOARD",
                        Color32::from_rgb(42, 42, 42),
                        Color32::from_rgb(20, 20, 20),
                    )
                } else {
                    (
                        "⎘  COPY TO CLIPBOARD",
                        Color32::from_rgb(0, 232, 122),
                        Color32::from_rgb(20, 20, 20),
                    )
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

        // ── Viewfinder (transparent scan area) ─────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let available = ui.available_rect_before_wrap();

                ui.painter().rect_filled(
                    available,
                    CornerRadius::ZERO,
                    Color32::from_rgba_premultiplied(13, 26, 18, 60),
                );

                let flashing = state
                    .flash_until
                    .map(|t| t > Instant::now())
                    .unwrap_or(false);
                let corner_color = if flashing {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(0, 232, 122)
                };
                draw_corners(ui.painter(), available, corner_color, 24.0, 2.0);

                let (status_text, status_color) = if state.decoded.is_empty() {
                    ("Scanning screen…", Color32::from_rgb(56, 56, 56))
                } else {
                    ("✓  QR detected", Color32::from_rgb(0, 232, 122))
                };
                ui.painter().text(
                    Pos2::new(available.center().x, available.max.y - 10.0),
                    egui::Align2::CENTER_BOTTOM,
                    status_text,
                    FontId::monospace(9.0),
                    status_color,
                );

                if let Some(window_rect) =
                    ctx.input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect))
                {
                    // Keep the capture area in logical desktop points. The screenshots crate
                    // applies each platform's DPI rules internally.
                    *self.capture_rect.lock().unwrap() =
                        Some(CaptureArea::from_viewfinder(window_rect.min, available));
                }
            });
    }
}

// ── Map resize direction to cursor icon ───────────────────────────
fn resize_cursor(dir: ResizeDirection) -> CursorIcon {
    match dir {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
    }
}

// ── Corner bracket painter ─────────────────────────────────────────
fn draw_corners(painter: &egui::Painter, rect: Rect, color: Color32, arm: f32, width: f32) {
    let stroke = Stroke::new(width, color);
    let min = rect.min;
    let max = rect.max;
    let pad = 4.0;
    painter.line_segment(
        [
            Pos2::new(min.x + pad, min.y + pad + arm),
            Pos2::new(min.x + pad, min.y + pad),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(min.x + pad, min.y + pad),
            Pos2::new(min.x + pad + arm, min.y + pad),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(max.x - pad - arm, min.y + pad),
            Pos2::new(max.x - pad, min.y + pad),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(max.x - pad, min.y + pad),
            Pos2::new(max.x - pad, min.y + pad + arm),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(min.x + pad, max.y - pad - arm),
            Pos2::new(min.x + pad, max.y - pad),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(min.x + pad, max.y - pad),
            Pos2::new(min.x + pad + arm, max.y - pad),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(max.x - pad, max.y - pad - arm),
            Pos2::new(max.x - pad, max.y - pad),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(max.x - pad, max.y - pad),
            Pos2::new(max.x - pad - arm, max.y - pad),
        ],
        stroke,
    );
}

// ── Scan thread ────────────────────────────────────────────────────
fn scan_loop(state: Arc<Mutex<ScanState>>, capture_rect: CaptureRect, ctx: egui::Context) {
    loop {
        thread::sleep(Duration::from_millis(150));

        let capture_area = match *capture_rect.lock().unwrap() {
            Some(r) => r,
            None => continue,
        };

        if capture_area.width < 10 || capture_area.height < 10 {
            continue;
        }

        let (anchor_x, anchor_y) = capture_area.anchor_point();
        let screen = match Screen::from_point(anchor_x, anchor_y).ok().or_else(|| {
            Screen::all()
                .ok()
                .and_then(|screens| screens.first().cloned())
        }) {
            Some(screen) => screen,
            None => continue,
        };

        let capture_area =
            capture_area.relative_to_origin(screen.display_info.x, screen.display_info.y);
        let screenshot = match screen.capture_area(
            capture_area.x,
            capture_area.y,
            capture_area.width,
            capture_area.height,
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let capture_width = screenshot.width();
        let capture_height = screenshot.height();
        if capture_width < 10 || capture_height < 10 {
            continue;
        }

        let sw = capture_width as usize;
        let raw = screenshot.as_raw();

        let luma: GrayImage = ImageBuffer::from_fn(capture_width, capture_height, |px, py| {
            let idx = (py as usize * sw + px as usize) * 4;
            if idx + 2 < raw.len() {
                let r = raw[idx] as u32;
                let g = raw[idx + 1] as u32;
                let b = raw[idx + 2] as u32;
                Luma([((r * 77 + g * 150 + b * 29) >> 8) as u8])
            } else {
                Luma([0u8])
            }
        });

        let decoded = try_decode_variants(&luma);

        let mut s = state.lock().unwrap();
        if let Some(text) = decoded {
            if text != s.decoded {
                s.decoded = text;
                s.flash_until = Some(Instant::now() + Duration::from_millis(200));
                ctx.request_repaint();
            }
        } else if !s.decoded.is_empty() {
            s.decoded.clear();
            ctx.request_repaint();
        }
    }
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
    let dyn_img = DynamicImage::ImageLuma8(img.clone());
    let source = BufferedImageLuminanceSource::new(dyn_img);
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
}

// ── Entry point ────────────────────────────────────────────────────
fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("QR Frame")
            .with_inner_size([380.0, 480.0])
            .with_min_inner_size([300.0, 400.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(true),
        ..Default::default()
    };

    eframe::run_native(
        "QR Frame",
        options,
        Box::new(|cc| Ok(Box::new(QrFrameApp::new(cc)))),
    )
    .unwrap();
}
