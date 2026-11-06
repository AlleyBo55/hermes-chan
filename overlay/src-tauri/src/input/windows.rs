//! Windows input through SendInput. Windows refuses synthetic input into apps running
//! with higher privileges (UIPI); that comes back as a clean error.

use super::parse::{self, Button, Key, Modifier, Named, Scroll};
use super::InputError;
use crate::winctl::{
    scale_factor,
    windows::{is_app_window, process_name, top_level_windows, window_pid, window_title},
};
use serde_json::Value;
use std::{mem::size_of, thread, time::Duration};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC,
        MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
        VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT,
        VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
    },
    WindowsAndMessaging::{
        GetForegroundWindow, GetSystemMetrics, IsIconic, SetForegroundWindow, ShowWindow, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE,
    },
};

const WHEEL_DELTA: i32 = 120;

pub fn execute(verb: &str, input: &Value) -> Result<String, InputError> {
    match verb {
        "click" => click(input),
        "drag" => drag(input),
        "scroll" => scroll(input),
        "type" | "set_value" => type_text(input),
        "key" => key(input),
        "focus_app" => focus_app(input),
        _ => Err(InputError::invalid(format!("unsupported native input action: {verb}"))),
    }
}

fn send(inputs: &[INPUT]) -> Result<(), InputError> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(InputError::unavailable(
            "Windows blocked the input; the target app may be running as administrator",
        ))
    }
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn keyboard(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Absolute move to a logical point, across all monitors.
fn move_to(point: (f64, f64)) -> Result<(), InputError> {
    let scale = scale_factor();
    let (x, y) = ((point.0 * scale).round() as i32, (point.1 * scale).round() as i32);
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
        )
    };
    let nx = ((x - left) as f64 * 65535.0 / (width - 1) as f64).round().clamp(0.0, 65535.0) as i32;
    let ny = ((y - top) as f64 * 65535.0 / (height - 1) as f64).round().clamp(0.0, 65535.0) as i32;
    send(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0)])
}

fn button_flags(button: Button) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
    match button {
        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        Button::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
}

fn click(input: &Value) -> Result<String, InputError> {
    let at = parse::point(input, "x", "y")?;
    let (down, up) = button_flags(parse::button(input)?);
    move_to(at)?;
    for _ in 0..parse::click_count(input) {
        send(&[mouse(down, 0, 0, 0)])?;
        thread::sleep(Duration::from_millis(35));
        send(&[mouse(up, 0, 0, 0)])?;
        thread::sleep(Duration::from_millis(55));
    }
    Ok(format!("clicked at ({:.0}, {:.0})", at.0, at.1))
}

fn drag(input: &Value) -> Result<String, InputError> {
    let from = parse::xy_pair(input.get("from_xy"), "from_xy")?;
    let to = parse::xy_pair(input.get("to_xy"), "to_xy")?;
    let (down, up) = button_flags(parse::button(input)?);
    move_to(from)?;
    send(&[mouse(down, 0, 0, 0)])?;
    for step in 1..=16 {
        let t = step as f64 / 16.0;
        let eased = t * t * (3.0 - 2.0 * t);
        move_to((from.0 + (to.0 - from.0) * eased, from.1 + (to.1 - from.1) * eased))?;
        thread::sleep(Duration::from_millis(12));
    }
    send(&[mouse(up, 0, 0, 0)])?;
    Ok("dragged between screen coordinates".to_string())
}

fn scroll(input: &Value) -> Result<String, InputError> {
    if let Some(at) = parse::optional_point(input) {
        move_to(at)?;
    }
    let (direction, amount, name) = parse::scroll(input)?;
    let (flags, data) = match direction {
        Scroll::Up => (MOUSEEVENTF_WHEEL, amount * WHEEL_DELTA),
        Scroll::Down => (MOUSEEVENTF_WHEEL, -amount * WHEEL_DELTA),
        Scroll::Right => (MOUSEEVENTF_HWHEEL, amount * WHEEL_DELTA),
        Scroll::Left => (MOUSEEVENTF_HWHEEL, -amount * WHEEL_DELTA),
    };
    send(&[mouse(flags, 0, 0, data)])?;
    Ok(format!("scrolled {name} by {amount}"))
}

fn virtual_key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    let extended = matches!(
        vk,
        VK_DELETE | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN | VK_LWIN
    );
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    keyboard(vk, scan, flags)
}

fn tap(vk: VIRTUAL_KEY) -> [INPUT; 2] {
    [virtual_key(vk, false), virtual_key(vk, true)]
}

fn type_text(input: &Value) -> Result<String, InputError> {
    let text = parse::text(input)?;
    let mut events: Vec<INPUT> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' | '\n' => events.extend(tap(VK_RETURN)),
            '\t' => events.extend(tap(VK_TAB)),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    events.push(keyboard(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE));
                    events.push(keyboard(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
    }
    // Small batches keep slow apps from dropping keystrokes.
    for batch in events.chunks(32) {
        send(batch)?;
        thread::sleep(Duration::from_millis(4));
    }
    Ok(format!("typed {} characters", text.chars().count()))
}

fn modifier_key(modifier: Modifier) -> VIRTUAL_KEY {
    match modifier {
        Modifier::Control => VK_CONTROL,
        Modifier::Shift => VK_SHIFT,
        Modifier::Alt => VK_MENU,
        Modifier::Meta => VK_LWIN,
    }
}

fn named_key(named: Named) -> VIRTUAL_KEY {
    match named {
        Named::Return => VK_RETURN,
        Named::Tab => VK_TAB,
        Named::Space => VK_SPACE,
        Named::Backspace => VK_BACK,
        Named::Delete => VK_DELETE,
        Named::Escape => VK_ESCAPE,
        Named::Home => VK_HOME,
        Named::End => VK_END,
        Named::PageUp => VK_PRIOR,
        Named::PageDown => VK_NEXT,
        Named::Left => VK_LEFT,
        Named::Right => VK_RIGHT,
        Named::Up => VK_UP,
        Named::Down => VK_DOWN,
    }
}

fn key(input: &Value) -> Result<String, InputError> {
    let (mut modifiers, key, combo) = parse::combo(input)?;
    let mut middle: Vec<INPUT> = Vec::new();
    match key {
        Key::Named(named) => middle.extend(tap(named_key(named))),
        Key::Char(ch) => {
            let mut units = [0u16; 2];
            let encoded = ch.encode_utf16(&mut units);
            let scan = if encoded.len() == 1 { unsafe { VkKeyScanW(encoded[0]) } } else { -1 };
            if scan == -1 {
                // Not on this keyboard layout: send the character itself.
                for unit in encoded.iter() {
                    middle.push(keyboard(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE));
                    middle.push(keyboard(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            } else {
                let shift_state = (scan as u16 >> 8) & 0xff;
                for (bit, modifier) in [(1, Modifier::Shift), (2, Modifier::Control), (4, Modifier::Alt)] {
                    if shift_state & bit != 0 && !modifiers.contains(&modifier) {
                        modifiers.push(modifier);
                    }
                }
                middle.extend(tap(VIRTUAL_KEY(scan as u16 & 0xff)));
            }
        }
    }
    let mut events: Vec<INPUT> = modifiers.iter().map(|m| virtual_key(modifier_key(*m), false)).collect();
    events.extend(middle);
    events.extend(modifiers.iter().rev().map(|m| virtual_key(modifier_key(*m), true)));
    send(&events)?;
    Ok(format!("pressed {combo}"))
}

/// Bring an open window of the app to the front. Unlike macOS, nothing is launched.
fn focus_app(input: &Value) -> Result<String, InputError> {
    let app = parse::app(input)?;
    let wanted = app.to_lowercase();
    let wanted_exe = wanted.trim_end_matches(".exe").to_string();
    let candidates: Vec<_> = top_level_windows().into_iter().filter(|hwnd| is_app_window(*hwnd)).collect();
    let by_process = candidates
        .iter()
        .find(|hwnd| process_name(window_pid(**hwnd)).to_lowercase() == wanted_exe)
        .or_else(|| candidates.iter().find(|hwnd| process_name(window_pid(**hwnd)).to_lowercase().contains(&wanted_exe)))
        .or_else(|| candidates.iter().find(|hwnd| window_title(**hwnd).to_lowercase().contains(&wanted)));
    let Some(hwnd) = by_process.copied() else {
        return Err(InputError::unavailable(format!("no open window for {app}; open it first")));
    };
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Windows lets the process that sent the last input change the foreground window.
        let _ = send(&[mouse(MOUSEEVENTF_MOVE, 0, 0, 0)]);
        let _ = SetForegroundWindow(hwnd);
    }
    for _ in 0..10 {
        if unsafe { GetForegroundWindow() } == hwnd {
            return Ok(format!("focused {app}"));
        }
        thread::sleep(Duration::from_millis(30));
    }
    Err(InputError::unavailable(format!("Windows did not let her bring {app} to the front")))
}
