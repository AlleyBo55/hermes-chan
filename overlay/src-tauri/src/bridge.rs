//! Duplex client to the plugin's AF_UNIX socket.
//!
//! The plugin is the server (it binds `HERMESCHAN_SOCKET`); the overlay connects
//! as a client, retrying until the plugin is up. Inbound newline-JSON lines are
//! (1) forwarded to the webview via the `from-plugin` event so the character
//! animates, and (2) acted on natively when they affect real windows — she
//! perches on the active window and shakes/nudges it on `grab_window`.
//! Outbound lines come from the `send_to_plugin` command (the chat bubble).

use std::sync::Mutex;
use tauri::AppHandle;

#[cfg(unix)]
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    thread,
    time::Duration,
};

#[cfg(unix)]
static WRITER: Mutex<Option<UnixStream>> = Mutex::new(None);

/// Resolve the socket path: `$HERMESCHAN_SOCKET` or `~/.hermes/hermes-chan.sock`.
#[cfg(unix)]
fn socket_path() -> String {
    if let Ok(p) = std::env::var("HERMESCHAN_SOCKET") {
        if !p.is_empty() {
            return p;
        }
    }
    let home = std::env::var("HERMES_HOME")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let base = if home.ends_with(".hermes") {
        home
    } else {
        format!("{home}/.hermes")
    };
    format!("{base}/hermes-chan.sock")
}

#[cfg(unix)]
pub fn spawn(handle: AppHandle) {
    thread::spawn(move || {
        let path = socket_path();
        loop {
            if let Ok(stream) = UnixStream::connect(&path) {
                if let Ok(reader_stream) = stream.try_clone() {
                    *WRITER.lock().unwrap() = Some(stream);
                    read_loop(&handle, reader_stream);
                }
                *WRITER.lock().unwrap() = None;
            }
            thread::sleep(Duration::from_millis(1000));
        }
    });
}

#[cfg(unix)]
fn read_loop(handle: &AppHandle, stream: UnixStream) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(text) if !text.trim().is_empty() => handle_line(handle, text),
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

/// Forward every line to the webview, then act natively on window-affecting ones.
#[cfg(unix)]
fn handle_line(handle: &AppHandle, text: String) {
    use tauri::Emitter;
    let _ = handle.emit("from-plugin", text.clone());

    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return,
    };
    let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let needs_native = match kind {
        "point" => true,
        "action" => true,
        "state" => v.get("state").and_then(|s| s.as_str()) == Some("peek"),
        _ => false,
    };
    if !needs_native {
        return;
    }
    // osascript blocks for tens of ms; do it off the socket-read thread.
    let handle = handle.clone();
    thread::spawn(move || native_action(&handle, &v));
}

#[cfg(unix)]
fn native_action(handle: &AppHandle, v: &serde_json::Value) {
    if v.get("type").and_then(|x| x.as_str()) == Some("action") {
        if v.get("verb").and_then(|s| s.as_str()) == Some("grab_window") {
            match v.get("gesture").and_then(|s| s.as_str()).unwrap_or("drag") {
                "shake" => crate::winctl::shake_frontmost(),
                "throw" => crate::winctl::nudge_frontmost(220, 140),
                _ => crate::winctl::nudge_frontmost(40, 0),
            }
        }
    }
    perch_on_active(handle);
}

/// Move the overlay window so she stands at the active window's top-right edge.
#[cfg(unix)]
fn perch_on_active(handle: &AppHandle) {
    use tauri::Manager;
    if let Some((x, y, w, _h)) = crate::winctl::frontmost_frame() {
        if let Some(win) = handle.get_webview_window("hermeschan") {
            let ox = (x + w - 150).max(0) as f64;
            let oy = (y - 60).max(0) as f64;
            let _ = win.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(ox, oy)));
        }
    }
}

#[cfg(unix)]
pub fn send_line(msg: &str) -> std::io::Result<()> {
    let mut guard = WRITER.lock().unwrap();
    match guard.as_mut() {
        Some(stream) => {
            stream.write_all(msg.as_bytes())?;
            stream.write_all(b"\n")?;
            stream.flush()
        }
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "overlay not connected to plugin",
        )),
    }
}

// --- non-unix stubs so the crate still compiles elsewhere -----------------
#[cfg(not(unix))]
pub fn spawn(_handle: AppHandle) {}

#[cfg(not(unix))]
pub fn send_line(_msg: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unix socket bridge only on unix hosts",
    ))
}
