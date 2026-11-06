// Hermechan overlay: a transparent, always-on-top body that mirrors the plugin.
// The Rust side owns the AF_UNIX connection to the plugin and relays JSON both
// ways; the webview draws the character and owns the speech-bubble input.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod input;
mod overlay;
mod winctl;

use tauri::Manager;

/// Called from the webview when the user types into the speech bubble, or the
/// body wants to send an intent back to the plugin.
#[tauri::command]
fn send_to_plugin(msg: String) -> Result<(), String> {
    bridge::send_line(&msg).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            send_to_plugin,
            overlay::drag_overlay,
            overlay::set_follow_active,
            overlay::get_overlay_state,
            overlay::set_overlay_hit_regions,
            overlay::hide_effect,
            overlay::list_windows,
            overlay::play_effect,
            overlay::self_grab,
            overlay::prepare_break_ui
        ])
        .setup(|app| {
            // Dockless desktop companion: Accessory keeps her out of the Dock
            // and app switcher while still showing the floating window.
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let controller = overlay::OverlayController::load();
            controller
                .setup(app.handle())
                .map_err(std::io::Error::other)?;
            app.manage(controller.clone());

            // Start the socket client; inbound lines are emitted to the webview
            // as "from-plugin" events.
            bridge::spawn(app.handle().clone(), controller);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the hermes-chan overlay");
}
