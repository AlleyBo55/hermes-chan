//! Windows: user32 and DWM for the window list and the active window, SetWindowPos for
//! moves, GDI for snapshots. user32 reports device pixels (Tauri makes the process
//! per-monitor DPI aware), so everything is divided by her window's scale on the way
//! out and multiplied back on the way in.

use super::{png_base64, scale_factor, ActiveWindow, WindowControlError, WindowFrame};
use std::{collections::HashMap, ffi::c_void, mem::size_of, path::Path};
use windows::{
    core::{BOOL, PWSTR},
    Win32::{
        Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HWND, LPARAM, RECT},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::{
                BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
                GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
                DIB_RGB_COLORS, HBITMAP, HDC, ROP_CODE, SRCCOPY,
            },
        },
        Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS},
        System::{
            SystemInformation::GetTickCount,
            Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
        },
        UI::{
            Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
            WindowsAndMessaging::{
                EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
                IsWindowVisible, IsZoomed, SetWindowPos, GWL_EXSTYLE, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE,
                SWP_NOSIZE, SWP_NOZORDER, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            },
        },
    },
};

const MAX_SAMPLE_BYTES: usize = 6 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 12 * 1024 * 1024;
/// The desktop and the taskbar are top-level windows too, but not ones to visit.
const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// HWNDs are 32-bit values sign-extended to pointer size, so they survive a u32 round trip.
pub(crate) fn hwnd_from_id(id: u32) -> HWND {
    HWND(id as i32 as isize as *mut c_void)
}

fn id_from_hwnd(hwnd: HWND) -> u32 {
    hwnd.0 as isize as i32 as u32
}

fn hwnd_from_handle(handle: u64) -> HWND {
    HWND(handle as usize as *mut c_void)
}

pub(crate) fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec passed to EnumWindows below, alive for the whole call.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        BOOL(1)
    }
    let mut list: Vec<HWND> = Vec::new();
    // EnumWindows walks top-level windows in z-order, front first.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<HWND> as isize)) };
    list
}

pub(crate) fn window_title(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let copied = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

pub(crate) fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// The executable's name without extension: "Code", "chrome", "WindowsTerminal".
pub(crate) fn process_name(pid: u32) -> String {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return String::new();
    };
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length)
    }
    .is_ok();
    let _ = unsafe { CloseHandle(process) };
    if !ok {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    Path::new(&path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The visible frame, without the invisible resize borders Windows 10+ adds.
fn frame_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    let from_dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
    };
    if from_dwm.is_err() {
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    }
    (rect.right > rect.left && rect.bottom > rect.top).then_some(rect)
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let read = unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, size_of::<u32>() as u32)
    };
    read.is_ok() && cloaked != 0
}

fn ex_style(hwnd: HWND) -> u32 {
    unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 }
}

/// What a person would call an app window: shown, on this virtual desktop, not minimised,
/// titled, and neither a tool palette nor the desktop or the taskbar.
pub(crate) fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }
        if GetWindowTextLengthW(hwnd) <= 0 {
            return false;
        }
    }
    if is_cloaked(hwnd) || ex_style(hwnd) & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }
    !SHELL_CLASSES.contains(&class_name(hwnd).as_str())
}

fn logical(rect: &RECT) -> (f64, f64, f64, f64) {
    let scale = scale_factor();
    (
        rect.left as f64 / scale,
        rect.top as f64 / scale,
        (rect.right - rect.left) as f64 / scale,
        (rect.bottom - rect.top) as f64 / scale,
    )
}

fn describe(hwnd: HWND) -> Option<ActiveWindow> {
    let rect = frame_rect(hwnd)?;
    let pid = window_pid(hwnd);
    let (x, y, w, h) = logical(&rect);
    Some(ActiveWindow {
        pid,
        app: process_name(pid),
        title: window_title(hwnd),
        x: x.round() as i32,
        y: y.round() as i32,
        w: w.round().max(1.0) as i32,
        h: h.round().max(1.0) as i32,
        handle: hwnd.0 as usize as u64,
    })
}

pub(super) fn frontmost_window() -> Result<Option<ActiveWindow>, WindowControlError> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() || SHELL_CLASSES.contains(&class_name(hwnd).as_str()) {
        return Ok(None);
    }
    Ok(describe(hwnd))
}

pub(super) fn window_for_pid(pid: u32) -> Result<Option<ActiveWindow>, WindowControlError> {
    Ok(top_level_windows()
        .into_iter()
        .find(|hwnd| window_pid(*hwnd) == pid && is_app_window(*hwnd))
        .and_then(describe))
}

/// The target's own window if it still exists, else its process's front window.
fn target_hwnd(target: &ActiveWindow) -> Option<HWND> {
    if target.handle != 0 {
        let hwnd = hwnd_from_handle(target.handle);
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() && window_pid(hwnd) == target.pid {
            return Some(hwnd);
        }
        return None;
    }
    top_level_windows()
        .into_iter()
        .find(|hwnd| window_pid(*hwnd) == target.pid && is_app_window(*hwnd))
}

pub(super) fn current_frame(target: &ActiveWindow) -> Result<Option<ActiveWindow>, WindowControlError> {
    Ok(target_hwnd(target).and_then(describe))
}

pub(super) fn move_window(
    target: &ActiveWindow,
    x: i32,
    y: i32,
    overlay_pid: u32,
) -> Result<(), WindowControlError> {
    let hwnd = target_hwnd(target)
        .ok_or_else(|| WindowControlError::other("target window is no longer available"))?;
    let active_pid = window_pid(unsafe { GetForegroundWindow() });
    if active_pid != target.pid && active_pid != overlay_pid {
        return Err(WindowControlError::other("target window is no longer active"));
    }
    if unsafe { IsZoomed(hwnd) }.as_bool() {
        return Err(WindowControlError::other("a maximised window stays where it is"));
    }
    let frame = frame_rect(hwnd).ok_or_else(|| WindowControlError::other("target window has no frame"))?;
    let mut outer = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut outer) }
        .map_err(|error| WindowControlError::other(format!("could not read the window frame: {error}")))?;
    // SetWindowPos places the outer rectangle, invisible borders included.
    let scale = scale_factor();
    let left = (x as f64 * scale).round() as i32 - (frame.left - outer.left);
    let top = (y as f64 * scale).round() as i32 - (frame.top - outer.top);
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            left,
            top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        )
    }
    .map_err(|error| {
        if error.code() == ERROR_ACCESS_DENIED.to_hresult() {
            WindowControlError::permission(
                "Windows does not let her move windows of apps running as administrator",
            )
        } else {
            WindowControlError::other(format!("could not move the window: {error}"))
        }
    })
}

pub(super) fn user_idle_seconds() -> f64 {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0.0;
    }
    unsafe { GetTickCount() }.wrapping_sub(info.dwTime) as f64 / 1000.0
}

/// Copy a bitmap's pixels as RGBA, cropped to (x, y, w, h). The bitmap must not be
/// selected into a device context while GetDIBits reads it.
unsafe fn bitmap_rgba(dc: HDC, bitmap: HBITMAP, full_w: i32, full_h: i32, crop: (i32, i32, i32, i32)) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: full_w,
            biHeight: -full_h, // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; full_w as usize * full_h as usize * 4];
    let lines = unsafe {
        GetDIBits(dc, bitmap, 0, full_h as u32, Some(bgra.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS)
    };
    if lines != full_h {
        return None;
    }
    let (cx, cy, cw, ch) = crop;
    let mut rgba = Vec::with_capacity(cw as usize * ch as usize * 4);
    for row in cy..cy + ch {
        let start = (row as usize * full_w as usize + cx as usize) * 4;
        for pixel in bgra[start..start + cw as usize * 4].chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    Some(rgba)
}

/// GDI copy of a screen rectangle in device pixels.
fn capture_screen_rect(x: i32, y: i32, w: i32, h: i32, max_bytes: usize) -> Result<String, WindowControlError> {
    if w <= 0 || h <= 0 || w > 16_384 || h > 16_384 {
        return Err(WindowControlError::other("snapshot area is empty or too large"));
    }
    let rgba = unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err(WindowControlError::other("could not open the screen for a snapshot"));
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let previous = SelectObject(memory, bitmap.into());
        let copied = BitBlt(memory, 0, 0, w, h, Some(screen), x, y, ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0));
        SelectObject(memory, previous);
        let pixels = copied.ok().and_then(|_| bitmap_rgba(memory, bitmap, w, h, (0, 0, w, h)));
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        pixels
    }
    .ok_or_else(|| WindowControlError::other("could not copy the screen"))?;
    png_base64(w as u32, h as u32, &rgba, max_bytes)
}

pub(super) fn capture_window(target: &ActiveWindow) -> Result<String, WindowControlError> {
    let scale = scale_factor();
    let px = |value: i32| (value as f64 * scale).round() as i32;
    capture_screen_rect(px(target.x), px(target.y), px(target.w), px(target.h), MAX_SAMPLE_BYTES)
}

/// PrintWindow renders the window itself, so nothing in front of it (her own body
/// included) ends up in the picture. Falls back to a plain screen copy of its frame.
pub(super) fn capture_window_id(window_id: u32) -> Result<String, WindowControlError> {
    let hwnd = hwnd_from_id(window_id);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(WindowControlError::other("that window is gone"));
    }
    let frame = frame_rect(hwnd).ok_or_else(|| WindowControlError::other("that window has no frame"))?;
    let mut outer = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut outer) }
        .map_err(|error| WindowControlError::other(format!("could not read the window frame: {error}")))?;
    let (full_w, full_h) = (outer.right - outer.left, outer.bottom - outer.top);
    let crop_x = (frame.left - outer.left).clamp(0, full_w.max(0));
    let crop_y = (frame.top - outer.top).clamp(0, full_h.max(0));
    let crop = (
        crop_x,
        crop_y,
        (frame.right - frame.left).min(full_w - crop_x),
        (frame.bottom - frame.top).min(full_h - crop_y),
    );
    if full_w > 0 && full_h > 0 && full_w <= 16_384 && full_h <= 16_384 && crop.2 > 0 && crop.3 > 0 {
        let printed = unsafe {
            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, full_w, full_h);
            let previous = SelectObject(memory, bitmap.into());
            // PW_RENDERFULLCONTENT (2) also captures DirectComposition content (browsers, WebView2).
            let ok = PrintWindow(hwnd, memory, PRINT_WINDOW_FLAGS(2)).as_bool();
            SelectObject(memory, previous);
            let pixels = if ok { bitmap_rgba(memory, bitmap, full_w, full_h, crop) } else { None };
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            pixels
        };
        if let Some(rgba) = printed {
            return png_base64(crop.2 as u32, crop.3 as u32, &rgba, MAX_SNAPSHOT_BYTES);
        }
    }
    capture_screen_rect(frame.left, frame.top, frame.right - frame.left, frame.bottom - frame.top, MAX_SNAPSHOT_BYTES)
}

pub(super) fn visible_windows(own_pid: u32) -> Vec<WindowFrame> {
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut windows = Vec::new();
    for hwnd in top_level_windows() {
        if !is_app_window(hwnd) || ex_style(hwnd) & WS_EX_TOPMOST.0 != 0 {
            continue;
        }
        let pid = window_pid(hwnd);
        if pid == own_pid {
            continue;
        }
        let Some(rect) = frame_rect(hwnd) else {
            continue;
        };
        let (x, y, w, h) = logical(&rect);
        if w < 80.0 || h < 60.0 {
            continue;
        }
        let app = names.entry(pid).or_insert_with(|| process_name(pid)).clone();
        windows.push(WindowFrame { id: id_from_hwnd(hwnd), pid, app, x, y, w, h });
    }
    windows
}
