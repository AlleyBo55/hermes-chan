// Hermechan overlay: a transparent, always-on-top body that mirrors the plugin.
// The Rust side owns the AF_UNIX connection to the plugin and relays JSON both
// ways; the webview draws the character and owns the speech-bubble input.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod winctl;

/// Called from the webview when the user types into the speech bubble, or the
/// body wants to send an intent back to the plugin.
#[tauri::command]
fn send_to_plugin(msg: String) -> Result<(), String> {
    bridge::send_line(&msg).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![send_to_plugin])
        .setup(|app| {
            // Start the socket client; inbound lines are emitted to the webview
            // as "from-plugin" events.
            bridge::spawn(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the hermes-chan overlay");
}
