//! Her view of the desktop: the active window, the on-screen windows she stands on,
//! moving the active window for a prank, how long you have been idle, and window
//! snapshots. One implementation per OS; every coordinate here is in logical points,
//! the same space the webview and `set_position(Logical)` use.

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
pub(crate) mod windows;
#[cfg(target_os = "windows")]
use self::windows as imp;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use unsupported as imp;

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveWindow {
    pub pid: u32,
    pub app: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// The OS window handle when the platform reports one (an HWND or X11 window), else 0.
    pub handle: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Permission,
    Other,
}

#[derive(Clone, Debug)]
pub struct WindowControlError {
    pub kind: ErrorKind,
    pub message: String,
}

impl WindowControlError {
    #[allow(dead_code)]
    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Other, message: message.into() }
    }

    #[allow(dead_code)]
    pub(crate) fn permission(message: impl Into<String>) -> Self {
        Self { kind: ErrorKind::Permission, message: message.into() }
    }
}

impl fmt::Display for WindowControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// One on-screen app window, for Hermes-chan to stand on, climb or jump to.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowFrame {
    pub id: u32,
    pub pid: u32,
    pub app: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

// 1.0_f64
static SCALE_BITS: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000);

/// Her window's display scale. Windows and X11 report device pixels; dividing by this
/// gives the logical points the webview works in. macOS already reports points.
pub fn set_scale_factor(scale: f64) {
    if scale.is_finite() && scale > 0.0 {
        SCALE_BITS.store(scale.to_bits(), Ordering::Relaxed);
    }
}

#[allow(dead_code)]
pub(crate) fn scale_factor() -> f64 {
    f64::from_bits(SCALE_BITS.load(Ordering::Relaxed))
}

/// The frontmost application window.
pub fn frontmost_window() -> Result<Option<ActiveWindow>, WindowControlError> {
    imp::frontmost_window()
}

/// Current front window of a process previously observed as active.
pub fn window_for_pid(pid: u32) -> Result<Option<ActiveWindow>, WindowControlError> {
    imp::window_for_pid(pid)
}

/// Normal app windows on screen, front to back.
pub fn visible_windows(own_pid: u32) -> Vec<WindowFrame> {
    imp::visible_windows(own_pid)
}

/// Seconds since the last keyboard, mouse or trackpad input; 0 when the OS cannot say,
/// which keeps her from moving windows on her own.
pub fn user_idle_seconds() -> f64 {
    imp::user_idle_seconds()
}

/// PNG base64 of one validated window rectangle, for an opt-in sample or computer use.
pub fn capture_window(target: &ActiveWindow) -> Result<String, WindowControlError> {
    imp::capture_window(target)
}

/// PNG base64 of one window by the id `visible_windows` reported, for the collapse prank.
pub fn capture_window_id(window_id: u32) -> Result<String, WindowControlError> {
    imp::capture_window_id(window_id)
}

/// Nudge the validated active window by a relative logical offset.
pub fn nudge_window(
    target: &ActiveWindow,
    dx: i32,
    dy: i32,
    overlay_pid: u32,
) -> Result<(), WindowControlError> {
    imp::move_window(
        target,
        target.x.saturating_add(dx),
        target.y.saturating_add(dy),
        overlay_pid,
    )
}

/// Shake the validated active window and restore its original position.
pub fn shake_window(target: &ActiveWindow, overlay_pid: u32) -> Result<(), WindowControlError> {
    for dx in [-18, 18, -14, 14, -8, 8, 0] {
        if let Err(error) = imp::move_window(target, target.x.saturating_add(dx), target.y, overlay_pid) {
            let _ = imp::move_window(target, target.x, target.y, overlay_pid);
            return Err(error);
        }
        thread::sleep(Duration::from_millis(45));
    }
    Ok(())
}

/// Pull the validated active window down by `dy` in a few steps, as if she drags it.
pub fn pull_window(target: &ActiveWindow, dy: i32, overlay_pid: u32) -> Result<(), WindowControlError> {
    for step in 1..=4 {
        imp::move_window(target, target.x, target.y.saturating_add(dy * step / 4), overlay_pid)?;
        thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

/// Put the window back where it was, unless someone has moved it since.
pub fn restore_window(
    target: &ActiveWindow,
    moved_to: (i32, i32),
    overlay_pid: u32,
) -> Result<(), WindowControlError> {
    match imp::current_frame(target)? {
        Some(now) if (now.x - moved_to.0).abs() <= 2 && (now.y - moved_to.1).abs() <= 2 => {
            imp::move_window(target, target.x, target.y, overlay_pid)
        }
        _ => Ok(()),
    }
}

/// Encode tightly packed RGBA pixels as PNG base64, refusing results over `max_bytes`.
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) fn png_base64(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_bytes: usize,
) -> Result<String, WindowControlError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let failed = |error: png::EncodingError| WindowControlError::other(format!("could not encode the window snapshot: {error}"));
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(rgba).map_err(failed)?;
        writer.finish().map_err(failed)?;
    }
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(WindowControlError::other(format!(
            "window snapshot was empty or exceeded {} MiB",
            max_bytes / (1024 * 1024)
        )));
    }
    Ok(STANDARD.encode(bytes))
}
