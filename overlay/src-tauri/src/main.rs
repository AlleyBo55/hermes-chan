// Hermes-chan overlay: a transparent, always-on-top body that mirrors the plugin.
// The Rust side owns the connection to the plugin and relays JSON both ways; the
// webview draws the character and owns the speech-bubble input.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod input;
mod overlay;
mod paths;
mod watchdog;
mod winctl;

use tauri::Manager;

/// Called from the webview when the user types into the speech bubble, or the
/// body wants to send an intent back to the plugin.
#[tauri::command]
fn send_to_plugin(msg: String) -> Result<(), String> {
    bridge::send_line(&msg).map_err(|e| e.to_string())
}

/// Wayland gives apps no way to place their own windows, see other windows or send
/// input, which is everything she does. Run her through XWayland when it is there.
#[cfg(target_os = "linux")]
fn prefer_x11() {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    if set("WAYLAND_DISPLAY") && set("DISPLAY") && !set("GDK_BACKEND") {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

fn main() {
    // Before GTK starts and before any other thread exists.
    #[cfg(target_os = "linux")]
    prefer_x11();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            send_to_plugin,
            overlay::deactivate,
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
            watchdog::spawn(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the hermes-chan overlay");
}
