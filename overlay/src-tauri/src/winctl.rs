//! Minimal macOS window control via `osascript` / System Events.
//!
//! No extra crates and no FFI, so it compiles anywhere and fails gracefully off
//! macOS (the command simply isn't found). It needs the host to hold the
//! **Accessibility** permission; without it, System Events returns an error and
//! these helpers return `None` / do nothing. A native `objc2` + AX backend can
//! replace this later for smooth per-frame dragging; osascript cadence
//! (~tens of ms per call) is enough for perch / nudge / shake gestures.

use std::process::Command;

fn osascript(script: &str) -> Option<String> {
    let out = Command::new("osascript").arg("-e").arg(script).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Name of the app that currently owns the frontmost window.
#[allow(dead_code)] // part of the winctl surface; used by point/targeting work
pub fn frontmost_app_name() -> Option<String> {
    osascript(
        "tell application \"System Events\" to get name of \
         (first process whose frontmost is true)",
    )
}

/// `(x, y, w, h)` of the frontmost window in global screen coordinates.
pub fn frontmost_frame() -> Option<(i32, i32, i32, i32)> {
    let pos = osascript(
        "tell application \"System Events\" to get position of front window of \
         (first process whose frontmost is true)",
    )?;
    let size = osascript(
        "tell application \"System Events\" to get size of front window of \
         (first process whose frontmost is true)",
    )?;
    let p = parse_pair(&pos)?;
    let s = parse_pair(&size)?;
    Some((p.0, p.1, s.0, s.1))
}

fn parse_pair(s: &str) -> Option<(i32, i32)> {
    let nums: Vec<i32> = s
        .split(',')
        .filter_map(|t| t.trim().parse::<i32>().ok())
        .collect();
    if nums.len() == 2 {
        Some((nums[0], nums[1]))
    } else {
        None
    }
}

/// Move the frontmost window's top-left corner to `(x, y)`.
pub fn move_frontmost(x: i32, y: i32) {
    let _ = osascript(&format!(
        "tell application \"System Events\" to set position of front window of \
         (first process whose frontmost is true) to {{{x}, {y}}}"
    ));
}

/// A quick left-right shake of the frontmost window, returning it to rest.
pub fn shake_frontmost() {
    if let Some((x, y, _, _)) = frontmost_frame() {
        for dx in [-18, 18, -14, 14, -8, 8, 0] {
            move_frontmost(x + dx, y);
            std::thread::sleep(std::time::Duration::from_millis(45));
        }
    }
}

/// Nudge the frontmost window by a relative offset (used by `grab`/`drag`).
pub fn nudge_frontmost(dx: i32, dy: i32) {
    if let Some((x, y, _, _)) = frontmost_frame() {
        move_frontmost(x + dx, y + dy);
    }
}
