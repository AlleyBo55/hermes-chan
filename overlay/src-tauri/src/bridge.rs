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
pub fn spawn(handle: AppHandle, controller: crate::overlay::OverlayController) {
    thread::spawn(move || {
        let path = socket_path();
        loop {
            if let Ok(stream) = UnixStream::connect(&path) {
                if let Ok(reader_stream) = stream.try_clone() {
                    *WRITER.lock().unwrap() = Some(stream);
                    read_loop(&handle, &controller, reader_stream);
                }
                *WRITER.lock().unwrap() = None;
            }
            thread::sleep(Duration::from_millis(1000));
        }
    });
}

#[cfg(unix)]
fn read_loop(
    handle: &AppHandle,
    controller: &crate::overlay::OverlayController,
    stream: UnixStream,
) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        match line {
            Ok(text) if !text.trim().is_empty() => handle_line(handle, controller, text),
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

/// Forward every line to the webview, then safely handle native window effects.
#[cfg(unix)]
fn handle_line(handle: &AppHandle, controller: &crate::overlay::OverlayController, text: String) {
    use tauri::Emitter;
    let _ = handle.emit("from-plugin", text.clone());

    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return,
    };
    if value.get("type").and_then(|item| item.as_str()) == Some("native_input") {
        thread::spawn(move || {
            let response = crate::input::execute(&value);
            if let Ok(line) = serde_json::to_string(&response) {
                let _ = send_line(&line);
            }
        });
        return;
    }
    if value.get("type").and_then(|item| item.as_str()) == Some("active_window_request") {
        let request_id = value
            .get("request_id")
            .and_then(|item| item.as_str())
            .filter(|item| !item.is_empty() && item.len() <= 64)
            .unwrap_or("");
        if !request_id.is_empty() {
            let response = controller.active_window_response(
                handle,
                request_id,
                value
                    .get("include_screenshot")
                    .and_then(|item| item.as_bool())
                    .unwrap_or(false),
            );
            if let Ok(line) = serde_json::to_string(&response) {
                let _ = send_line(&line);
            }
        }
        return;
    }
    let needs_native = match value.get("type").and_then(|item| item.as_str()) {
        Some("point") => true,
        Some("action" | "native_action") => matches!(
            value.get("verb").and_then(|item| item.as_str()),
            Some("glitch" | "grab_window")
        ),
        Some("state") => value.get("state").and_then(|item| item.as_str()) == Some("peek"),
        _ => false,
    };
    if !needs_native {
        return;
    }

    // System Events calls block; serialize them off the socket-read thread.
    let handle = handle.clone();
    let controller = controller.clone();
    thread::spawn(move || controller.handle_plugin_message(&handle, &value));
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
pub fn spawn(_handle: AppHandle, _controller: crate::overlay::OverlayController) {}

#[cfg(not(unix))]
pub fn send_line(_msg: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unix socket bridge only on unix hosts",
    ))
}
