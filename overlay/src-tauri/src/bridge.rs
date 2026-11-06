//! Duplex client to the plugin's bridge.
//!
//! The plugin is the server; the overlay connects as a client and retries until the
//! plugin is up. macOS and Linux use the plugin's AF_UNIX socket (owner-only file
//! permissions). Windows Python has no AF_UNIX, so there the plugin listens on
//! 127.0.0.1 and the overlay proves it is the one the plugin meant with a random
//! token: `HERMESCHAN_TCP` + `HERMESCHAN_TOKEN` when the plugin started her, otherwise
//! the plugin's `bridge.json`.
//!
//! Inbound newline-JSON lines are (1) forwarded to the webview via the `from-plugin`
//! event so the character animates, and (2) acted on natively when they affect real
//! windows — she perches on the active window and shakes/nudges it on `grab_window`.
//! Outbound lines come from the `send_to_plugin` command (the chat bubble).

#[cfg(not(unix))]
use std::net::SocketAddr;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    sync::Mutex,
    thread,
    time::Duration,
};
use tauri::AppHandle;

enum Stream {
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    // Only Windows dials TCP; the variant exists everywhere to keep one code path.
    #[allow(dead_code)]
    Tcp(TcpStream),
}

impl Stream {
    fn try_clone(&self) -> io::Result<Self> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.try_clone().map(Self::Unix),
            Self::Tcp(stream) => stream.try_clone().map(Self::Tcp),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buffer),
            Self::Tcp(stream) => stream.read(buffer),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buffer),
            Self::Tcp(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
            Self::Tcp(stream) => stream.flush(),
        }
    }
}

static WRITER: Mutex<Option<Stream>> = Mutex::new(None);

/// One connection attempt to wherever the plugin is listening right now.
#[cfg(unix)]
fn connect() -> Option<Stream> {
    std::os::unix::net::UnixStream::connect(crate::paths::socket_path())
        .ok()
        .map(Stream::Unix)
}

#[cfg(not(unix))]
fn connect() -> Option<Stream> {
    let (address, token) = tcp_endpoint()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(800)).ok()?;
    let _ = stream.set_nodelay(true);
    let hello = serde_json::json!({ "type": "hello", "token": token, "pid": std::process::id() });
    let mut line = serde_json::to_vec(&hello).ok()?;
    line.push(b'\n');
    stream.write_all(&line).ok()?;
    Some(Stream::Tcp(stream))
}

/// The plugin's loopback address and token, from the environment or its bridge file.
#[cfg(not(unix))]
fn tcp_endpoint() -> Option<(SocketAddr, String)> {
    let from_env = std::env::var("HERMESCHAN_TCP")
        .ok()
        .zip(std::env::var("HERMESCHAN_TOKEN").ok())
        .and_then(|(address, token)| Some((address.parse::<SocketAddr>().ok()?, token)));
    let (address, token) = from_env.or_else(|| {
        let text = std::fs::read_to_string(crate::paths::state_dir().join("bridge.json")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let port = u16::try_from(value.get("port")?.as_u64()?).ok()?;
        let token = value.get("token")?.as_str()?.to_string();
        Some((SocketAddr::from(([127, 0, 0, 1], port)), token))
    })?;
    // Never send the token anywhere but this machine.
    (address.ip().is_loopback() && !token.is_empty()).then_some((address, token))
}

pub fn spawn(handle: AppHandle, controller: crate::overlay::OverlayController) {
    thread::spawn(move || loop {
        if let Some(stream) = connect() {
            if let Ok(reader_stream) = stream.try_clone() {
                *WRITER.lock().unwrap() = Some(stream);
                read_loop(&handle, &controller, reader_stream);
            }
            *WRITER.lock().unwrap() = None;
        }
        thread::sleep(Duration::from_millis(1000));
    });
}

fn read_loop(handle: &AppHandle, controller: &crate::overlay::OverlayController, stream: Stream) {
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

    // Window queries block; serialize them off the socket-read thread.
    let handle = handle.clone();
    let controller = controller.clone();
    thread::spawn(move || controller.handle_plugin_message(&handle, &value));
}

pub fn send_line(msg: &str) -> io::Result<()> {
    let mut guard = WRITER.lock().unwrap();
    match guard.as_mut() {
        Some(stream) => {
            let mut line = Vec::with_capacity(msg.len() + 1);
            line.extend_from_slice(msg.as_bytes());
            line.push(b'\n');
            stream.write_all(&line)?;
            stream.flush()
        }
        None => Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "overlay not connected to plugin",
        )),
    }
}
