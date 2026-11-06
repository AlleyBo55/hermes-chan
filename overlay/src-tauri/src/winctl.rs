//! Minimal macOS window observation/control via `osascript` and System Events.
//!
//! All target selection stays in Rust. AppleScript receives only validated
//! numeric process IDs and coordinates, so plugin-provided target text is never
//! interpolated into a script.

use std::{fmt, process::Command};

const FIELD_SEPARATOR: char = '\u{1f}';

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveWindow {
    pub pid: u32,
    pub app: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
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

impl fmt::Display for WindowControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

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
    let kind = if lower.contains("not authorized")
        || lower.contains("not allowed assistive access")
        || lower.contains("accessibility")
        || lower.contains("-1743")
        || lower.contains("-1719")
    {
        ErrorKind::Permission
    } else {
        ErrorKind::Other
    };
    Err(WindowControlError {
        kind,
        message: if stderr.is_empty() {
            format!("System Events failed with status {}", output.status)
        } else {
            stderr
        },
    })
}

/// Frontmost application window in global logical screen coordinates.
pub fn frontmost_window() -> Result<Option<ActiveWindow>, WindowControlError> {
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
pub fn window_for_pid(pid: u32) -> Result<Option<ActiveWindow>, WindowControlError> {
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
    };
    if window.w <= 0 || window.h <= 0 {
        return Err(WindowControlError {
            kind: ErrorKind::Other,
            message: "System Events returned a non-positive window size".to_string(),
        });
    }
    Ok(Some(window))
}

fn move_window(pid: u32, x: i32, y: i32, overlay_pid: u32) -> Result<(), WindowControlError> {
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

/// Nudge the validated active window by a relative logical offset.
pub fn nudge_window(
    target: &ActiveWindow,
    dx: i32,
    dy: i32,
    overlay_pid: u32,
) -> Result<(), WindowControlError> {
    move_window(
        target.pid,
        target.x.saturating_add(dx),
        target.y.saturating_add(dy),
        overlay_pid,
    )
}

/// Shake the validated active window and restore its original position.
pub fn shake_window(target: &ActiveWindow, overlay_pid: u32) -> Result<(), WindowControlError> {
    for dx in [-18, 18, -14, 14, -8, 8, 0] {
        if let Err(error) = move_window(
            target.pid,
            target.x.saturating_add(dx),
            target.y,
            overlay_pid,
        ) {
            let _ = move_window(target.pid, target.x, target.y, overlay_pid);
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(45));
    }
    Ok(())
}

/// Capture one validated window rectangle for an opt-in autonomy sample.
/// Returns PNG base64. Screen Recording denial is reported as a permission error.
pub fn capture_window(target: &ActiveWindow) -> Result<String, WindowControlError> {
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
