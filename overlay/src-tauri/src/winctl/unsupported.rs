//! Desktops without a window-control implementation: she still roams the screen,
//! she just cannot see or move other windows.

use super::{ActiveWindow, WindowControlError, WindowFrame};

fn unsupported() -> WindowControlError {
    WindowControlError::other("window control is implemented for macOS, Windows and Linux (X11)")
}

pub(super) fn frontmost_window() -> Result<Option<ActiveWindow>, WindowControlError> {
    Err(unsupported())
}

pub(super) fn window_for_pid(_pid: u32) -> Result<Option<ActiveWindow>, WindowControlError> {
    Err(unsupported())
}

pub(super) fn current_frame(_target: &ActiveWindow) -> Result<Option<ActiveWindow>, WindowControlError> {
    Err(unsupported())
}

pub(super) fn move_window(_target: &ActiveWindow, _x: i32, _y: i32, _overlay_pid: u32) -> Result<(), WindowControlError> {
    Err(unsupported())
}

pub(super) fn user_idle_seconds() -> f64 {
    0.0
}

pub(super) fn capture_window(_target: &ActiveWindow) -> Result<String, WindowControlError> {
    Err(unsupported())
}

pub(super) fn capture_window_id(_window_id: u32) -> Result<String, WindowControlError> {
    Err(unsupported())
}

pub(super) fn visible_windows(_own_pid: u32) -> Vec<WindowFrame> {
    Vec::new()
}
