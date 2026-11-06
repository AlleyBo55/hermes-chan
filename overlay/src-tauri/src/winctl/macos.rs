//! macOS window observation/control via `osascript` and System Events, plus the
//! CoreGraphics window list and `screencapture` snapshots.
//!
//! All target selection stays in Rust. AppleScript receives only validated
//! numeric process IDs and coordinates, so plugin-provided target text is never
//! interpolated into a script.

use super::{ActiveWindow, ErrorKind, WindowControlError, WindowFrame};
use std::process::Command;

const FIELD_SEPARATOR: char = '\u{1f}';

// AppleScript's own wording ("100:121: execution error: …") would land verbatim in her status
// bubble. macOS checks these grants against whatever launched the Hermes gateway: `osascript`
// for the background gateway (Hermes starts it through /usr/bin/osascript), else the terminal.
const ACCESSIBILITY_HELP: &str = "Turn on Accessibility for osascript (Hermes' gateway) or your \
     terminal: System Settings ▸ Privacy & Security.";
const AUTOMATION_HELP: &str = "Allow osascript (Hermes' gateway) or your terminal to control \
     System Events: System Settings ▸ Privacy & Security ▸ Automation.";

fn osascript(script: &str) -> Result<String, WindowControlError> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| WindowControlError {
            kind: ErrorKind::Other,
            message: format!("could not launch System Events script: {error}"),
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let lower = stderr.to_ascii_lowercase();
    let help = if lower.contains("not authorized") || lower.contains("-1743") {
        Some(AUTOMATION_HELP)
    } else if lower.contains("not allowed assistive access")
        || lower.contains("accessibility")
        || lower.contains("-1719")
    {
        Some(ACCESSIBILITY_HELP)
    } else {
        None
    };
    Err(match help {
        Some(help) => WindowControlError { kind: ErrorKind::Permission, message: help.to_string() },
        None => WindowControlError {
            kind: ErrorKind::Other,
            message: if stderr.is_empty() {
                format!("System Events failed with status {}", output.status)
            } else {
                stderr
            },
        },
    })
}

/// Frontmost application window in global logical screen coordinates.
pub(super) fn frontmost_window() -> Result<Option<ActiveWindow>, WindowControlError> {
    let output = osascript(
        r#"tell application "System Events"
  set p to first application process whose frontmost is true
  if (count of windows of p) is 0 then return ""
  set windowTitle to ""
  try
    set windowTitle to name of front window of p as text
  end try
  set {px, py} to position of front window of p
  set {ww, wh} to size of front window of p
  set sep to ASCII character 31
  return (unix id of p as text) & sep & (name of p as text) & sep & windowTitle & sep & (px as text) & sep & (py as text) & sep & (ww as text) & sep & (wh as text)
end tell"#,
    )?;
    parse_window(&output)
}

/// Current front window for a process ID previously observed as active.
pub(super) fn window_for_pid(pid: u32) -> Result<Option<ActiveWindow>, WindowControlError> {
    let output = osascript(&format!(
        r#"tell application "System Events"
  set matches to every application process whose unix id is {pid}
  if (count of matches) is 0 then return ""
  set p to first item of matches
  if (count of windows of p) is 0 then return ""
  set windowTitle to ""
  try
    set windowTitle to name of front window of p as text
  end try
  set {{px, py}} to position of front window of p
  set {{ww, wh}} to size of front window of p
  set sep to ASCII character 31
  return (unix id of p as text) & sep & (name of p as text) & sep & windowTitle & sep & (px as text) & sep & (py as text) & sep & (ww as text) & sep & (wh as text)
end tell"#
    ))?;
    parse_window(&output)
}

/// Where the target's window is now (its process's front window).
pub(super) fn current_frame(target: &ActiveWindow) -> Result<Option<ActiveWindow>, WindowControlError> {
    window_for_pid(target.pid)
}

fn parse_window(output: &str) -> Result<Option<ActiveWindow>, WindowControlError> {
    if output.is_empty() {
        return Ok(None);
    }
    let fields: Vec<&str> = output.split(FIELD_SEPARATOR).collect();
    if fields.len() != 7 {
        return Err(WindowControlError {
            kind: ErrorKind::Other,
            message: "System Events returned an invalid active-window record".to_string(),
        });
    }
    let parse_u32 = |value: &str, name: &str| {
        value.parse::<u32>().map_err(|_| WindowControlError {
            kind: ErrorKind::Other,
            message: format!("System Events returned an invalid {name}"),
        })
    };
    let parse_i32 = |value: &str, name: &str| {
        value.parse::<i32>().map_err(|_| WindowControlError {
            kind: ErrorKind::Other,
            message: format!("System Events returned an invalid {name}"),
        })
    };

    let window = ActiveWindow {
        pid: parse_u32(fields[0], "process ID")?,
        app: fields[1].to_string(),
        title: fields[2].to_string(),
        x: parse_i32(fields[3], "window x coordinate")?,
        y: parse_i32(fields[4], "window y coordinate")?,
        w: parse_i32(fields[5], "window width")?,
        h: parse_i32(fields[6], "window height")?,
        handle: 0,
    };
    if window.w <= 0 || window.h <= 0 {
        return Err(WindowControlError {
            kind: ErrorKind::Other,
            message: "System Events returned a non-positive window size".to_string(),
        });
    }
    Ok(Some(window))
}

pub(super) fn move_window(
    target: &ActiveWindow,
    x: i32,
    y: i32,
    overlay_pid: u32,
) -> Result<(), WindowControlError> {
    let pid = target.pid;
    osascript(&format!(
        r#"tell application "System Events"
  set matches to every application process whose unix id is {pid}
  if (count of matches) is 0 then error "target window is no longer available"
  set p to first item of matches
  if (count of windows of p) is 0 then error "target window is no longer available"
  set activeProcess to first application process whose frontmost is true
  set activePid to unix id of activeProcess
  if activePid is not {pid} and activePid is not {overlay_pid} then error "target window is no longer active"
  set position of front window of p to {{{x}, {y}}}
end tell"#
    ))?;
    Ok(())
}

/// Seconds since the last keyboard, mouse or trackpad input.
pub(super) fn user_idle_seconds() -> f64 {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateCombinedSessionState and kCGAnyInputEventType.
    unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) }
}

/// Capture one validated window rectangle for an opt-in autonomy sample.
/// Returns PNG base64. Screen Recording denial is reported as a permission error.
pub(super) fn capture_window(target: &ActiveWindow) -> Result<String, WindowControlError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "hermes-chan-{}-{nonce}.png",
        std::process::id()
    ));
    let region = format!("{},{},{},{}", target.x, target.y, target.w, target.h);
    let output = Command::new("screencapture")
        .args(["-x", "-t", "png", "-R", &region])
        .arg(&path)
        .output()
        .map_err(|error| WindowControlError {
            kind: ErrorKind::Other,
            message: format!("could not launch screencapture: {error}"),
        })?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&path);
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let lower = stderr.to_ascii_lowercase();
        let kind = if lower.contains("permission")
            || lower.contains("not authorized")
            || lower.contains("screen recording")
        {
            ErrorKind::Permission
        } else {
            ErrorKind::Other
        };
        return Err(WindowControlError {
            kind,
            message: if stderr.is_empty() {
                "screencapture failed; Screen Recording permission may be required".to_string()
            } else {
                stderr
            },
        });
    }

    let bytes = std::fs::read(&path).map_err(|error| WindowControlError {
        kind: ErrorKind::Other,
        message: format!("could not read active-window screenshot: {error}"),
    });
    let _ = std::fs::remove_file(&path);
    let bytes = bytes?;
    if bytes.is_empty() || bytes.len() > 6 * 1024 * 1024 {
        return Err(WindowControlError {
            kind: ErrorKind::Other,
            message: "active-window screenshot was empty or exceeded 6 MiB".to_string(),
        });
    }
    Ok(STANDARD.encode(bytes))
}

/// Snapshot one window by its CGWindowID, without anything in front of it (so not her
/// own overlay either). PNG base64; needs Screen Recording like `capture_window`.
pub(super) fn capture_window_id(window_id: u32) -> Result<String, WindowControlError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("hermes-chan-{}-{nonce}-w.png", std::process::id()));
    let output = Command::new("screencapture")
        .args(["-x", "-o", "-t", "png", &format!("-l{window_id}")])
        .arg(&path)
        .output()
        .map_err(|error| WindowControlError {
            kind: ErrorKind::Other,
            message: format!("could not launch screencapture: {error}"),
        })?;
    let bytes = std::fs::read(&path);
    let _ = std::fs::remove_file(&path);
    match bytes {
        Ok(bytes) if output.status.success() && !bytes.is_empty() && bytes.len() <= 12 * 1024 * 1024 => {
            Ok(STANDARD.encode(bytes))
        }
        _ => Err(WindowControlError {
            kind: ErrorKind::Permission,
            message: "window snapshot failed; Screen Recording permission may be required".to_string(),
        }),
    }
}

/// Normal app windows on screen, front to back, in logical points. Reads only the
/// owner, layer and bounds, which need no Screen Recording permission.
pub(super) fn visible_windows(own_pid: u32) -> Vec<WindowFrame> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::{CFNumber, CFNumberRef};
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowNumber,
        kCGWindowOwnerName, kCGWindowOwnerPID,
    };
    use std::ffi::c_void;

    let Some(list) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };
    let key = |name: CFStringRef| unsafe { CFString::wrap_under_get_rule(name) };
    let number = |dict: &CFDictionary<CFString, CFType>, name: CFStringRef| -> Option<f64> {
        dict.find(key(name))
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_f64())
    };
    let mut windows = Vec::new();
    for item in list.iter() {
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*item as CFDictionaryRef) };
        let (layer, alpha, pid) = unsafe {
            (
                number(&dict, kCGWindowLayer),
                number(&dict, kCGWindowAlpha),
                number(&dict, kCGWindowOwnerPID),
            )
        };
        if layer != Some(0.0) || alpha.unwrap_or(1.0) <= 0.0 {
            continue;
        }
        let pid = pid.unwrap_or(0.0) as u32;
        if pid == own_pid {
            continue;
        }
        let Some(bounds) = dict
            .find(key(unsafe { kCGWindowBounds }))
            .and_then(|value| value.downcast::<CFDictionary>())
        else {
            continue;
        };
        let field = |name: &'static str| -> Option<f64> {
            let name = CFString::from_static_string(name);
            bounds
                .find(name.as_CFTypeRef() as *const c_void)
                .and_then(|value| unsafe { CFNumber::wrap_under_get_rule(*value as CFNumberRef) }.to_f64())
        };
        let (Some(x), Some(y), Some(w), Some(h)) = (field("X"), field("Y"), field("Width"), field("Height")) else {
            continue;
        };
        if w < 80.0 || h < 60.0 {
            continue;
        }
        let app = dict
            .find(key(unsafe { kCGWindowOwnerName }))
            .and_then(|value| value.downcast::<CFString>())
            .map(|name| name.to_string())
            .unwrap_or_default();
        let id = unsafe { number(&dict, kCGWindowNumber) }.unwrap_or(0.0) as u32;
        windows.push(WindowFrame { id, pid, app, x, y, w, h });
    }
    windows
}
