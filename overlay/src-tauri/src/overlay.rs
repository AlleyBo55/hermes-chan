//! Native overlay state, active-window tracking, hit testing, and effects.

use crate::winctl::{self, ActiveWindow, ErrorKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};

const MAIN_WINDOW: &str = "hermeschan";
/// Her rest reminder owns the effect window while it is up: no prank may move or resize it.
static REST_SHOWING: AtomicBool = AtomicBool::new(false);
/// Which reminder is up, so a late failsafe never closes a newer one.
static REST_GENERATION: AtomicU64 = AtomicU64::new(0);
/// The card closes itself after about 27 s. If its page failed, never leave a screen-sized
/// window catching clicks for longer than this.
const REST_FAILSAFE: Duration = Duration::from_secs(40);
const EFFECT_WINDOW: &str = "effect";
const ACTIVE_POLL: Duration = Duration::from_millis(750);
const HIT_TEST_POLL: Duration = Duration::from_millis(50);
// Roaming moves the window every frame; write the remembered spot at most once a second.
const PERSIST_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct PersistentState {
    version: u8,
    follow_active: bool,
    pinned_position: Option<OverlayPosition>,
}

impl Default for PersistentState {
    fn default() -> Self {
        Self {
            version: 1,
            follow_active: true,
            pinned_position: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    pub follow_active: bool,
    pub pinned_position: Option<OverlayPosition>,
}

impl From<&PersistentState> for OverlayState {
    fn from(value: &PersistentState) -> Self {
        Self {
            follow_active: value.follow_active,
            pinned_position: value.pinned_position.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitRegion {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Debug, Serialize)]
struct ActiveWindowPayload<'a> {
    app: &'a str,
    title: &'a str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl<'a> From<&'a ActiveWindow> for ActiveWindowPayload<'a> {
    fn from(window: &'a ActiveWindow) -> Self {
        Self {
            app: &window.app,
            title: &window.title,
            x: window.x,
            y: window.y,
            w: window.w,
            h: window.h,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct WindowControlStatus<'a> {
    status: &'a str,
    operation: &'a str,
    message: Option<&'a str>,
}

#[derive(Clone, Debug, Serialize)]
struct EffectPayload<'a> {
    effect: &'a str,
    verb: &'a str,
    gesture: Option<&'a str>,
    app: &'a str,
    title: &'a str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    intensity: f64,
    duration_ms: u64,
    origin_x: f64,
    origin_y: f64,
}

struct Inner {
    persistent: PersistentState,
    persistence_dirty: bool,
    hit_regions: Vec<HitRegion>,
    // Linux shapes the window's input region instead of toggling click-through.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    ignoring_cursor: bool,
    active_window: Option<ActiveWindow>,
    force_perch: bool,
    statuses: HashMap<String, (String, String)>,
}

#[derive(Clone)]
pub struct OverlayController {
    inner: Arc<Mutex<Inner>>,
    state_path: Arc<PathBuf>,
    action_lock: Arc<Mutex<()>>,
    load_error: Arc<Option<String>>,
}

impl OverlayController {
    pub fn load() -> Self {
        let state_path = overlay_state_path();
        let (persistent, load_error) = match fs::read_to_string(&state_path) {
            Ok(text) => match serde_json::from_str::<PersistentState>(&text) {
                Ok(state) => (state, None),
                Err(error) => (
                    PersistentState::default(),
                    Some(format!("could not parse {}: {error}", state_path.display())),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (PersistentState::default(), None)
            }
            Err(error) => (
                PersistentState::default(),
                Some(format!("could not read {}: {error}", state_path.display())),
            ),
        };

        Self {
            inner: Arc::new(Mutex::new(Inner {
                persistent,
                persistence_dirty: false,
                hit_regions: Vec::new(),
                ignoring_cursor: false,
                active_window: None,
                force_perch: true,
                statuses: HashMap::new(),
            })),
            state_path: Arc::new(state_path),
            action_lock: Arc::new(Mutex::new(())),
            load_error: Arc::new(load_error),
        }
    }

    pub fn setup(&self, handle: &AppHandle) -> Result<(), String> {
        let main = handle
            .get_webview_window(MAIN_WINDOW)
            .ok_or_else(|| format!("missing {MAIN_WINDOW} window"))?;
        let effect = handle
            .get_webview_window(EFFECT_WINDOW)
            .ok_or_else(|| format!("missing {EFFECT_WINDOW} window"))?;

        if let Some(position) = self.snapshot().pinned_position {
            main.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                position.x, position.y,
            )))
            .map_err(|error| format!("could not restore overlay position: {error}"))?;
        }

        // tao's Linux click-through needs a realized window and panics on the hidden
        // effect window, so Linux sets the (empty) input shape through GTK instead.
        #[cfg(not(target_os = "linux"))]
        effect
            .set_ignore_cursor_events(true)
            .map_err(|error| format!("could not make effect window click-through: {error}"))?;
        #[cfg(target_os = "linux")]
        pass_all_clicks_through(&effect);

        winctl::set_scale_factor(main.scale_factor().unwrap_or(1.0));

        #[cfg(target_os = "macos")]
        lift_over_menu_bar(&main);
        #[cfg(target_os = "macos")]
        set_effect_level(&effect, false);

        // Follow you onto other workspaces, like CanJoinAllSpaces does on macOS.
        #[cfg(target_os = "linux")]
        let _ = main.set_visible_on_all_workspaces(true);

        let controller = self.clone();
        let app = handle.clone();
        main.on_window_event(move |event| match event {
            WindowEvent::Moved(position) => controller.record_pinned_position(position.x, position.y),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => winctl::set_scale_factor(*scale_factor),
            // Alt+F4 and friends: closing her window switches her off, like Ctrl+Shift+click.
            WindowEvent::CloseRequested { .. } => leave(&app),
            _ => {}
        });

        if let Some(message) = self.load_error.as_ref() {
            self.emit_status(handle, "error", "persistence", message);
        }

        self.spawn_runtime(handle.clone());
        Ok(())
    }

    pub fn snapshot(&self) -> OverlayState {
        let inner = self.inner.lock().unwrap();
        OverlayState::from(&inner.persistent)
    }

    pub fn active_window_response(
        &self,
        handle: &AppHandle,
        request_id: &str,
        include_screenshot: bool,
    ) -> serde_json::Value {
        let Some(window) = self.resolve_target(handle, "active_window_sample") else {
            return serde_json::json!({
                "type": "active_window",
                "request_id": request_id,
                "app": "",
                "title": "",
            });
        };
        let mut response = serde_json::json!({
            "type": "active_window",
            "request_id": request_id,
            "app": &window.app,
            "title": &window.title,
            "x": window.x,
            "y": window.y,
            "w": window.w,
            "h": window.h,
        });
        if include_screenshot {
            match winctl::capture_window(&window) {
                Ok(data) => {
                    response["screenshot"] = serde_json::json!({
                        "mime_type": "image/png",
                        "data": data,
                    });
                    self.emit_ready(handle, "screen_recording");
                }
                Err(error) => self.emit_winctl_error(handle, "screen_recording", &error),
            }
        }
        response
    }

    fn spawn_runtime(&self, handle: AppHandle) {
        let controller = self.clone();
        thread::spawn(move || {
            let mut last_active_poll = Instant::now() - ACTIVE_POLL;
            let mut last_flush = Instant::now() - PERSIST_INTERVAL;
            loop {
                // Linux shapes her window's input region once per layout change instead
                // (see set_overlay_hit_regions): XWayland cannot report the pointer over
                // other apps, so polling it would leave her unclickable there.
                #[cfg(not(target_os = "linux"))]
                controller.update_hit_testing(&handle);
                if last_flush.elapsed() >= PERSIST_INTERVAL {
                    controller.flush_dirty_state(&handle);
                    last_flush = Instant::now();
                }

                if last_active_poll.elapsed() >= ACTIVE_POLL {
                    if let Some(scale) = handle
                        .get_webview_window(MAIN_WINDOW)
                        .and_then(|window| window.scale_factor().ok())
                    {
                        winctl::set_scale_factor(scale);
                    }
                    controller.poll_active_window(&handle);
                    last_active_poll = Instant::now();
                }
                thread::sleep(HIT_TEST_POLL);
            }
        });
    }

    fn poll_active_window(&self, handle: &AppHandle) {
        let window = match winctl::frontmost_window() {
            Ok(Some(window)) if window.pid != std::process::id() => window,
            Ok(_) => return,
            Err(error) => {
                self.emit_winctl_error(handle, "active_window", &error);
                return;
            }
        };
        self.emit_ready(handle, "active_window");

        let (changed, should_perch) = {
            let mut inner = self.inner.lock().unwrap();
            let changed = inner.active_window.as_ref() != Some(&window);
            let should_perch = inner.persistent.follow_active && (changed || inner.force_perch);
            inner.active_window = Some(window.clone());
            inner.force_perch = false;
            (changed, should_perch)
        };

        if changed {
            let payload = ActiveWindowPayload::from(&window);
            let _ = handle.emit("active-window", &payload);
            if let Ok(line) = serde_json::to_string(&serde_json::json!({
                "type": "active_window",
                "app": &window.app,
                "title": &window.title,
                "x": window.x,
                "y": window.y,
                "w": window.w,
                "h": window.h,
            })) {
                let _ = crate::bridge::send_line(&line);
            }
        }
        if should_perch {
            self.perch(handle, &window);
        }
    }

    fn perch(&self, handle: &AppHandle, target: &ActiveWindow) {
        let Some(window) = handle.get_webview_window(MAIN_WINDOW) else {
            self.emit_status(
                handle,
                "error",
                "overlay_position",
                "main overlay window is missing",
            );
            return;
        };
        let destination_x = target.x + target.w - 150;
        let destination_y = target.y - 60;
        let scale = window.scale_factor().unwrap_or(1.0);
        let current = window.outer_position().ok();
        let start_x = current
            .as_ref()
            .map(|position| position.x as f64 / scale)
            .unwrap_or(destination_x as f64);
        let start_y = current
            .as_ref()
            .map(|position| position.y as f64 / scale)
            .unwrap_or(destination_y as f64);
        let delta_x = destination_x as f64 - start_x;
        let delta_y = destination_y as f64 - start_y;
        let direction = if delta_x < 0.0 { "left" } else { "right" };

        let _ = handle.emit("active-window", serde_json::json!({
            "app": &target.app,
            "title": &target.title,
            "x": target.x,
            "y": target.y,
            "w": target.w,
            "h": target.h,
            "direction": direction,
            "delta_x": delta_x,
            "delta_y": delta_y,
            "moving": true,
            "phase": "moving",
        }));

        // Match the visible travel to one walk or climb sprite cycle.
        let steps = 24;
        let step_delay = Duration::from_millis(if delta_y < -45.0 { 37 } else { 30 });
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let eased = t * t * (3.0 - 2.0 * t);
            let x = start_x + delta_x * eased;
            let y = start_y + delta_y * eased;
            if let Err(error) = window.set_position(tauri::Position::Logical(
                tauri::LogicalPosition::new(x, y),
            )) {
                self.emit_status(
                    handle,
                    "error",
                    "overlay_position",
                    &format!("could not perch overlay: {error}"),
                );
                return;
            }
            thread::sleep(step_delay);
        }
        self.emit_ready(handle, "overlay_position");
        let _ = handle.emit("active-window", serde_json::json!({
            "app": &target.app,
            "title": &target.title,
            "x": target.x,
            "y": target.y,
            "w": target.w,
            "h": target.h,
            "moving": false,
            "phase": "perched",
        }));
    }

    fn record_pinned_position(&self, x: i32, y: i32) {
        let mut inner = self.inner.lock().unwrap();
        if inner.persistent.follow_active {
            return;
        }
        let position = OverlayPosition { x, y };
        if inner.persistent.pinned_position.as_ref() != Some(&position) {
            inner.persistent.pinned_position = Some(position);
            inner.persistence_dirty = true;
        }
    }

    fn flush_dirty_state(&self, handle: &AppHandle) {
        let state = {
            let mut inner = self.inner.lock().unwrap();
            if !inner.persistence_dirty {
                return;
            }
            inner.persistence_dirty = false;
            inner.persistent.clone()
        };
        match write_state(&self.state_path, &state) {
            Ok(()) => self.emit_ready(handle, "persistence"),
            Err(error) => self.emit_status(handle, "error", "persistence", &error),
        }
    }

    #[cfg_attr(target_os = "linux", allow(dead_code))]
    fn update_hit_testing(&self, handle: &AppHandle) {
        let Some(window) = handle.get_webview_window(MAIN_WINDOW) else {
            return;
        };
        let (regions, was_ignoring) = {
            let inner = self.inner.lock().unwrap();
            (inner.hit_regions.clone(), inner.ignoring_cursor)
        };

        // Until the frontend supplies its actual character/chat rectangles, keep
        // the window interactive rather than guessing and locking out its UI.
        let should_ignore = if regions.is_empty() {
            false
        } else {
            let Ok(cursor) = window.cursor_position() else {
                return;
            };
            let Ok(origin) = window.outer_position() else {
                return;
            };
            let Ok(scale) = window.scale_factor() else {
                return;
            };
            let x = (cursor.x - origin.x as f64) / scale;
            let y = (cursor.y - origin.y as f64) / scale;
            !regions.iter().any(|region| {
                x >= region.x
                    && x < region.x + region.width
                    && y >= region.y
                    && y < region.y + region.height
            })
        };

        if should_ignore == was_ignoring {
            return;
        }
        match window.set_ignore_cursor_events(should_ignore) {
            Ok(()) => {
                self.inner.lock().unwrap().ignoring_cursor = should_ignore;
                self.emit_ready(handle, "hit_testing");
            }
            Err(error) => self.emit_status(
                handle,
                "error",
                "hit_testing",
                &format!("could not update overlay hit testing: {error}"),
            ),
        }
    }

    pub fn handle_plugin_message(&self, handle: &AppHandle, message: &serde_json::Value) {
        let _action = self.action_lock.lock().unwrap();
        match message.get("type").and_then(|value| value.as_str()) {
            Some("point") => self.handle_perch_message(handle, message),
            Some("state")
                if message.get("state").and_then(|value| value.as_str()) == Some("peek") =>
            {
                self.handle_perch_message(handle, message)
            }
            Some("action" | "native_action") => self.handle_action(handle, message),
            _ => {}
        }
    }

    fn handle_perch_message(&self, handle: &AppHandle, message: &serde_json::Value) {
        if !self.snapshot().follow_active {
            return;
        }
        let Some(target) = self.resolve_target(handle, "point_target") else {
            return;
        };
        if !target_matches(message.get("target"), &target) {
            self.emit_status(
                handle,
                "error",
                "point_target",
                "target is invalid or no longer matches the active window",
            );
            return;
        }
        self.emit_ready(handle, "point_target");
        self.perch(handle, &target);
    }

    fn handle_action(&self, handle: &AppHandle, message: &serde_json::Value) {
        if message.get("type").and_then(|value| value.as_str()) == Some("action")
            && message
                .get("defer_native")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        {
            return;
        }
        let Some(verb) = message.get("verb").and_then(|value| value.as_str()) else {
            return;
        };
        if verb != "glitch" && verb != "grab_window" {
            return;
        }

        let Some(target) = self.resolve_target(handle, "action_target") else {
            return;
        };
        if !target_matches(message.get("target"), &target) {
            self.emit_status(
                handle,
                "error",
                "action_target",
                "target is invalid or no longer matches the active window",
            );
            return;
        }
        self.emit_ready(handle, "action_target");

        if verb == "glitch" {
            let intensity = message
                .get("intensity")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.75)
                .clamp(0.0, 1.0);
            let (effect, duration_ms) = match message.get("effect").and_then(|value| value.as_str()) {
                None | Some("glitch") => ("glitch", 1200),
                Some("crack") => ("crack", 1400),
                Some("portal") => ("portal", 2200),
                Some("bend") => ("bend", 1100),
                Some("text_shatter") => ("text_shatter", 1650),
                Some("ripple") => ("ripple", 1400),
                Some("pixel_dissolve") => ("pixel_dissolve", 1600),
                Some("lightning") => ("lightning", 1300),
                Some("tear") => ("tear", 1700),
                Some("hijack") => ("hijack", 2000),
                Some("collapse") => ("collapse", 2600),
                _ => {
                    self.emit_status(handle, "error", "effect", "unsupported visual effect");
                    return;
                }
            };
            // Both break the window's own pixels. The hijack still glitches without them; the
            // collapse would have nothing to drop, so it cracks the window instead.
            let (effect, duration_ms, intensity) = match effect {
                "hijack" => {
                    self.send_window_snapshot(handle, &target);
                    ("hijack", duration_ms, 1.0)
                }
                "collapse" if !self.send_window_snapshot(handle, &target) => ("crack", 1400, intensity),
                _ => (effect, duration_ms, intensity),
            };
            self.show_effect(handle, effect, verb, None, &target, intensity, duration_ms);
            return;
        }

        let gesture = message
            .get("gesture")
            .and_then(|value| value.as_str())
            .unwrap_or("drag");
        if gesture == "bend" {
            self.show_effect(handle, "bend", verb, Some(gesture), &target, 0.85, 1100);
            return;
        }

        let result = match gesture {
            "drag" => winctl::nudge_window(&target, 40, 0, std::process::id()),
            "shake" => winctl::shake_window(&target, std::process::id()),
            "throw" => winctl::nudge_window(&target, 220, 140, std::process::id()),
            _ => {
                self.emit_status(
                    handle,
                    "error",
                    "window_action",
                    "unsupported grab_window gesture",
                );
                return;
            }
        };
        match result {
            Ok(()) => self.emit_ready(handle, "window_action"),
            Err(error) => self.emit_winctl_error(handle, "window_action", &error),
        }
    }

    fn resolve_target(&self, handle: &AppHandle, operation: &str) -> Option<ActiveWindow> {
        match winctl::frontmost_window() {
            Ok(Some(window)) if window.pid != std::process::id() => {
                self.inner.lock().unwrap().active_window = Some(window.clone());
                self.emit_ready(handle, operation);
                Some(window)
            }
            Ok(Some(_)) => {
                let cached = self.inner.lock().unwrap().active_window.clone()?;
                match winctl::window_for_pid(cached.pid) {
                    Ok(Some(window)) => Some(window),
                    Ok(None) => {
                        self.emit_status(
                            handle,
                            "error",
                            operation,
                            "the previously active window is no longer available",
                        );
                        None
                    }
                    Err(error) => {
                        self.emit_winctl_error(handle, operation, &error);
                        None
                    }
                }
            }
            Ok(None) => {
                self.emit_status(handle, "error", operation, "no active window is available");
                None
            }
            Err(error) => {
                self.emit_winctl_error(handle, operation, &error);
                None
            }
        }
    }

    /// The hijack and collapse break the window's own pixels: hand a snapshot of the target
    /// window to the effect window first. False when there is none; her bubble then says
    /// which permission it takes.
    fn send_window_snapshot(&self, handle: &AppHandle, target: &ActiveWindow) -> bool {
        let windows = winctl::visible_windows(std::process::id());
        let same_app: Vec<&winctl::WindowFrame> =
            windows.iter().filter(|win| win.pid == target.pid).collect();
        let near = |a: f64, b: i32| (a - b as f64).abs() <= 2.0;
        let Some(id) = same_app
            .iter()
            .find(|win| near(win.x, target.x) && near(win.y, target.y) && near(win.w, target.w) && near(win.h, target.h))
            .or_else(|| same_app.first())
            .map(|win| win.id)
        else {
            return false;
        };
        match winctl::capture_window_id(id) {
            Ok(snapshot) => {
                self.emit_ready(handle, "window_snapshot");
                handle
                    .get_webview_window(EFFECT_WINDOW)
                    .is_some_and(|effect| effect.emit("effect-image", snapshot).is_ok())
            }
            Err(error) => {
                self.emit_winctl_error(handle, "window_snapshot", &error);
                false
            }
        }
    }

    fn show_effect(
        &self,
        handle: &AppHandle,
        effect_name: &str,
        verb: &str,
        gesture: Option<&str>,
        target: &ActiveWindow,
        intensity: f64,
        duration_ms: u64,
    ) {
        self.show_effect_at(handle, effect_name, verb, gesture, target, intensity, duration_ms, None);
    }

    /// Like `show_effect`, with the exact point her hand is at when the caller knows it.
    fn show_effect_at(
        &self,
        handle: &AppHandle,
        effect_name: &str,
        verb: &str,
        gesture: Option<&str>,
        target: &ActiveWindow,
        intensity: f64,
        duration_ms: u64,
        hand: Option<(f64, f64)>,
    ) {
        if REST_SHOWING.load(Ordering::Relaxed) {
            return;
        }
        let Some(window) = handle.get_webview_window(EFFECT_WINDOW) else {
            self.emit_status(handle, "error", "effect", "effect window is missing");
            return;
        };
        let result = window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                target.x as f64,
                target.y as f64,
            )))
            .and_then(|_| {
                window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                    target.w as f64,
                    target.h as f64,
                )))
            })
            .and_then(|_| show_without_focus(&window));
        if let Err(error) = result {
            self.emit_status(
                handle,
                "error",
                "effect",
                &format!("could not show effect window: {error}"),
            );
            return;
        }

        // Anchor the effect near Hermes-chan's body within the active window.
        let (hand_px_x, hand_px_y) = match effect_name {
            "crack" => (151.0, 105.0),
            "portal" => (151.0, 108.0),
            "text_shatter" => (145.0, 109.0),
            "ripple" => (150.0, 107.0),
            "pixel_dissolve" => (154.0, 102.0),
            "lightning" => (153.0, 105.0),
            "tear" => (148.0, 110.0),
            _ => (151.0, 105.0),
        };
        let exact = hand.map(|(hand_x, hand_y)| {
            (
                ((hand_x - target.x as f64) / target.w.max(1) as f64).clamp(0.02, 0.98),
                ((hand_y - target.y as f64) / target.h.max(1) as f64).clamp(0.02, 0.98),
            )
        });
        let (origin_x, origin_y) = exact.or_else(|| handle
            .get_webview_window(MAIN_WINDOW)
            .and_then(|body| {
                let position = body.outer_position().ok()?;
                let size = body.inner_size().ok()?;
                let scale = body.scale_factor().ok()?;
                let body_x = position.x as f64 / scale;
                let body_y = position.y as f64 / scale;
                let body_w = size.width as f64 / scale;
                let body_h = size.height as f64 / scale;
                // 192x208 sprite is displayed at 96x104, centered above the
                // stage's 48px footer. Match the generated contact marks.
                let hand_x = body_x + (body_w - 96.0) / 2.0 + hand_px_x / 2.0;
                let hand_y = body_y + body_h - 48.0 - 104.0 + hand_px_y / 2.0;
                Some((
                    ((hand_x - target.x as f64) / target.w.max(1) as f64).clamp(0.08, 0.92),
                    ((hand_y - target.y as f64) / target.h.max(1) as f64).clamp(0.08, 0.92),
                ))
            }))
            .unwrap_or((0.5, 0.35));

        let payload = EffectPayload {
            effect: effect_name,
            verb,
            gesture,
            app: &target.app,
            title: &target.title,
            x: target.x,
            y: target.y,
            w: target.w,
            h: target.h,
            intensity,
            duration_ms,
            origin_x,
            origin_y,
        };
        match window.emit("effect", payload) {
            Ok(()) => self.emit_ready(handle, "effect"),
            Err(error) => self.emit_status(
                handle,
                "error",
                "effect",
                &format!("could not emit effect payload: {error}"),
            ),
        }
    }

    fn emit_winctl_error(
        &self,
        handle: &AppHandle,
        operation: &str,
        error: &winctl::WindowControlError,
    ) {
        let status = match error.kind {
            ErrorKind::Permission => "permission_required",
            ErrorKind::Other => "error",
        };
        self.emit_status(handle, status, operation, &error.message);
    }

    fn emit_status(&self, handle: &AppHandle, status: &str, operation: &str, message: &str) {
        let changed = {
            let mut inner = self.inner.lock().unwrap();
            let value = (status.to_string(), message.to_string());
            if inner.statuses.get(operation) == Some(&value) {
                false
            } else {
                inner.statuses.insert(operation.to_string(), value);
                true
            }
        };
        if changed {
            let _ = handle.emit(
                "window-control-status",
                WindowControlStatus {
                    status,
                    operation,
                    message: Some(message),
                },
            );
        }
    }

    fn emit_ready(&self, handle: &AppHandle, operation: &str) {
        let had_error = self
            .inner
            .lock()
            .unwrap()
            .statuses
            .remove(operation)
            .is_some();
        if had_error {
            let _ = handle.emit(
                "window-control-status",
                WindowControlStatus {
                    status: "ready",
                    operation,
                    message: None,
                },
            );
        }
    }
}

#[tauri::command]
pub fn drag_overlay(
    window: WebviewWindow,
    controller: State<'_, OverlayController>,
) -> Result<OverlayState, String> {
    require_window(&window, MAIN_WINDOW)?;
    let position = window
        .outer_position()
        .map_err(|error| format!("could not read overlay position: {error}"))?;
    let old_state = {
        let inner = controller.inner.lock().unwrap();
        inner.persistent.clone()
    };
    let mut new_state = old_state.clone();
    new_state.follow_active = false;
    new_state.pinned_position = Some(OverlayPosition {
        x: position.x,
        y: position.y,
    });
    write_state(&controller.state_path, &new_state)?;
    {
        let mut inner = controller.inner.lock().unwrap();
        inner.persistent = new_state;
        inner.persistence_dirty = false;
    }

    if let Err(error) = window.start_dragging() {
        let _ = write_state(&controller.state_path, &old_state);
        controller.inner.lock().unwrap().persistent = old_state;
        return Err(format!("could not start overlay drag: {error}"));
    }
    Ok(controller.snapshot())
}

#[tauri::command]
pub fn set_follow_active(
    enabled: bool,
    app: AppHandle,
    controller: State<'_, OverlayController>,
) -> Result<OverlayState, String> {
    let current_position = if enabled {
        None
    } else {
        let window = app
            .get_webview_window(MAIN_WINDOW)
            .ok_or_else(|| format!("missing {MAIN_WINDOW} window"))?;
        let position = window
            .outer_position()
            .map_err(|error| format!("could not read overlay position: {error}"))?;
        Some(OverlayPosition {
            x: position.x,
            y: position.y,
        })
    };

    let (old_state, new_state) = {
        let inner = controller.inner.lock().unwrap();
        let old_state = inner.persistent.clone();
        let mut new_state = old_state.clone();
        new_state.follow_active = enabled;
        if let Some(position) = current_position {
            new_state.pinned_position = Some(position);
        }
        (old_state, new_state)
    };
    write_state(&controller.state_path, &new_state)?;
    {
        let mut inner = controller.inner.lock().unwrap();
        inner.persistent = new_state;
        inner.persistence_dirty = false;
        inner.force_perch = enabled;
    }
    let state = controller.snapshot();
    if state.follow_active != enabled {
        let _ = write_state(&controller.state_path, &old_state);
        return Err("overlay follow state changed concurrently".to_string());
    }
    Ok(state)
}

#[tauri::command]
pub fn get_overlay_state(controller: State<'_, OverlayController>) -> OverlayState {
    // The page asks once it is listening: forget the last window so the next poll re-sends it.
    controller.inner.lock().unwrap().active_window = None;
    controller.snapshot()
}

#[tauri::command]
pub fn set_overlay_hit_regions(
    regions: Vec<HitRegion>,
    window: WebviewWindow,
    app: AppHandle,
    controller: State<'_, OverlayController>,
) -> Result<(), String> {
    require_window(&window, MAIN_WINDOW)?;
    if regions.len() > 64 {
        return Err("at most 64 hit regions are allowed".to_string());
    }
    if regions.iter().any(|region| {
        !region.x.is_finite()
            || !region.y.is_finite()
            || !region.width.is_finite()
            || !region.height.is_finite()
            || region.width <= 0.0
            || region.height <= 0.0
            || region.width > 10_000.0
            || region.height > 10_000.0
    }) {
        return Err("hit regions must contain finite, positive dimensions".to_string());
    }
    #[cfg(target_os = "linux")]
    shape_input(&window, &regions);
    controller.inner.lock().unwrap().hit_regions = regions;
    #[cfg(not(target_os = "linux"))]
    controller.update_hit_testing(&app);
    #[cfg(target_os = "linux")]
    let _ = app;
    Ok(())
}

/// Linux: only her body, bubble and buttons take clicks; the rest of her window passes
/// them to whatever is underneath. No regions yet means the whole window is interactive.
#[cfg(target_os = "linux")]
fn shape_input(window: &WebviewWindow, regions: &[HitRegion]) {
    use gtk::{cairo::RectangleInt, prelude::WidgetExt};
    let rectangles: Vec<RectangleInt> = regions
        .iter()
        .map(|region| {
            RectangleInt::new(
                region.x.floor() as i32,
                region.y.floor() as i32,
                region.width.ceil() as i32 + 1,
                region.height.ceil() as i32 + 1,
            )
        })
        .collect();
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(gtk_window) = target.gtk_window() else {
            return;
        };
        if rectangles.is_empty() {
            gtk_window.input_shape_combine_region(None);
        } else {
            let region = gtk::cairo::Region::create_rectangles(&rectangles);
            gtk_window.input_shape_combine_region(Some(&region));
        }
    });
}

/// Linux: an empty input shape, stored by GTK and applied whenever the window is realized.
#[cfg(target_os = "linux")]
fn pass_all_clicks_through(window: &WebviewWindow) {
    use gtk::prelude::WidgetExt;
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(gtk_window) = target.gtk_window() {
            gtk_window.input_shape_combine_region(Some(&gtk::cairo::Region::create()));
        }
    });
}

/// Show a window without taking focus from the app you are using. Windows activates a
/// window it shows unless asked not to; elsewhere `show` already leaves focus alone.
fn show_without_focus(window: &WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        let hwnd = window.hwnd()?;
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        Ok(())
    }
    #[cfg(not(windows))]
    window.show()
}

/// Hide a window shown with `show_without_focus`.
fn hide_shown_window(window: &WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        let hwnd = window.hwnd()?;
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        Ok(())
    }
    #[cfg(not(windows))]
    window.hide()
}

/// Ctrl+Shift+click (or closing her window): tell the plugin she was switched off, so it
/// does not start her again, then disappear and end her process.
pub fn leave(app: &AppHandle) {
    let _ = crate::bridge::send_line(r#"{"type":"intent","name":"deactivate"}"#);
    if let Some(window) = app.get_webview_window(EFFECT_WINDOW) {
        let _ = hide_shown_window(&window);
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    app.exit(0);
}

#[tauri::command]
pub fn deactivate(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, MAIN_WINDOW)?;
    leave(&app);
    Ok(())
}

/// No keyboard or mouse input for this long before she may move a real window on her own.
const SELF_GRAB_IDLE_SECS: f64 = 20.0;
/// How long a moved window stays moved before she puts it back.
const SELF_GRAB_RESTORE: Duration = Duration::from_millis(2500);

/// Her own prank on the real active window: shake it, nudge it, fling it or pull it down,
/// then put it back. Refused while you are using the computer, so she never yanks a
/// window out from under your hands. Every move goes through the same active-window
/// checks as the agent's grab_window tool.
#[tauri::command]
pub fn self_grab(
    gesture: String,
    direction: i32,
    window: WebviewWindow,
    app: AppHandle,
    controller: State<'_, OverlayController>,
) -> Result<(), String> {
    require_window(&window, MAIN_WINDOW)?;
    if !matches!(gesture.as_str(), "shake" | "nudge" | "throw" | "pull_down") {
        return Err(format!("unknown gesture {gesture}"));
    }
    let idle = winctl::user_idle_seconds();
    if idle < SELF_GRAB_IDLE_SECS {
        return Err(format!("you were active {idle:.0}s ago"));
    }
    let target = controller
        .resolve_target(&app, "self_grab")
        .ok_or_else(|| "no active window to grab".to_string())?;
    let side = if direction < 0 { -1 } else { 1 };
    let lock = controller.action_lock.clone();
    let overlay_pid = std::process::id();
    thread::spawn(move || {
        // One window action at a time, shared with the agent's own window actions.
        let Ok(_guard) = lock.try_lock() else {
            return;
        };
        let moved = match gesture.as_str() {
            "shake" => winctl::shake_window(&target, overlay_pid).map(|_| None),
            "nudge" => winctl::nudge_window(&target, 40 * side, 0, overlay_pid)
                .map(|_| Some((target.x + 40 * side, target.y))),
            "throw" => winctl::nudge_window(&target, 220 * side, 140, overlay_pid)
                .map(|_| Some((target.x + 220 * side, target.y + 140))),
            _ => winctl::pull_window(&target, 90, overlay_pid).map(|_| Some((target.x, target.y + 90))),
        };
        if let Ok(Some(moved_to)) = moved {
            thread::sleep(SELF_GRAB_RESTORE);
            let _ = winctl::restore_window(&target, moved_to, overlay_pid);
        }
    });
    Ok(())
}

/// The fake "her prank broke your app" effect, step one: snapshot the window and hand the
/// picture to the effect window. Her cast starts only after this, so the collapse can then
/// fire on her effect frame like every other prank. The app itself is never touched.
#[tauri::command]
pub async fn prepare_break_ui(window_id: u32, window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, MAIN_WINDOW)?;
    let snapshot = tauri::async_runtime::spawn_blocking(move || winctl::capture_window_id(window_id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let effect = app
        .get_webview_window(EFFECT_WINDOW)
        .ok_or_else(|| "effect window is missing".to_string())?;
    effect.emit("effect-image", snapshot).map_err(|error| error.to_string())
}

/// On-screen app windows, front to back, so she can stand on, climb or jump to any of them.
/// Async so the X11 and Win32 round trips run off the UI thread.
#[tauri::command]
pub async fn list_windows() -> Vec<winctl::WindowFrame> {
    tauri::async_runtime::spawn_blocking(|| winctl::visible_windows(std::process::id()))
        .await
        .unwrap_or_default()
}

/// Her own harmless mischief: draw a short visual effect over a screen rectangle.
/// Purely decorative, on the click-through effect window; the app underneath is untouched.
#[tauri::command]
pub fn play_effect(
    effect: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    hand_x: Option<f64>,
    hand_y: Option<f64>,
    window: WebviewWindow,
    app: AppHandle,
    controller: State<'_, OverlayController>,
) -> Result<(), String> {
    require_window(&window, MAIN_WINDOW)?;
    if !matches!(effect.as_str(), "glitch" | "bend" | "crack" | "portal"
        | "text_shatter" | "ripple" | "pixel_dissolve" | "lightning" | "tear" | "collapse" | "hijack") {
        return Err(format!("unknown effect {effect}"));
    }
    if ![x, y, w, h].iter().all(|value| value.is_finite()) || w < 40.0 || h < 40.0 {
        return Err("effect area must be finite and at least 40x40".to_string());
    }
    let target = ActiveWindow {
        pid: 0,
        app: String::new(),
        title: String::new(),
        x: x.round() as i32,
        y: y.round() as i32,
        w: w.min(4000.0).round() as i32,
        h: h.min(4000.0).round() as i32,
        handle: 0,
    };
    let duration_ms = match effect.as_str() {
        "crack" | "ripple" => 1400,
        "portal" => 2200,
        "bend" => 1100,
        "text_shatter" => 1650,
        "pixel_dissolve" => 1600,
        "lightning" => 1300,
        "tear" => 1700,
        "collapse" => 2600,
        "hijack" => 2000,
        _ => 1200,
    };
    let hand = match (hand_x, hand_y) {
        (Some(hx), Some(hy)) if hx.is_finite() && hy.is_finite() => Some((hx, hy)),
        _ => None,
    };
    // The hijack is the window's own pixels, glitched: opaque, so the real app can't show through.
    let intensity = if effect == "hijack" { 1.0 } else { 0.7 };
    controller.show_effect_at(&app, &effect, "mischief", None, &target, intensity, duration_ms, hand);
    Ok(())
}

#[tauri::command]
pub fn hide_effect(window: WebviewWindow, app: AppHandle, promised: Option<bool>) -> Result<(), String> {
    require_window(&window, EFFECT_WINDOW)?;
    let hidden = hide_shown_window(&window).map_err(|error| format!("could not hide effect window: {error}"));
    if REST_SHOWING.swap(false, Ordering::Relaxed) {
        release_rest(&window);
        // Her reaction: `true` you clicked the card to promise a rest, `false` it timed out.
        if let Some(promised) = promised {
            let _ = app.emit_to(MAIN_WINDOW, "rest-done", promised);
        }
    }
    hidden
}

/// The rest reminder let go of the effect window: click-through again, and below her body
/// again for pranks.
fn release_rest(window: &WebviewWindow) {
    let _ = pass_clicks_through(window);
    #[cfg(target_os = "macos")]
    set_effect_level(window, false);
}

/// Seconds since your last keyboard or mouse input, for her rest reminder; None when this
/// desktop cannot tell.
#[tauri::command]
pub fn user_idle_seconds(window: WebviewWindow) -> Result<Option<f64>, String> {
    require_window(&window, MAIN_WINDOW)?;
    Ok(winctl::user_idle_seconds_if_known())
}

#[derive(Clone, Serialize)]
struct RestPayload {
    hours: u32,
}

/// Her rest reminder: take over the whole screen she is on with a "please rest" card. The
/// card closes on one click (it takes clicks while it is up) and never takes keyboard focus.
#[tauri::command]
pub fn show_rest_reminder(hours: u32, window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    require_window(&window, MAIN_WINDOW)?;
    let effect = app
        .get_webview_window(EFFECT_WINDOW)
        .ok_or_else(|| "effect window is missing".to_string())?;
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "no screen to show the rest reminder on".to_string())?;
    REST_SHOWING.store(true, Ordering::Relaxed);
    let generation = REST_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    // Over the menu bar and full-screen apps on macOS, and over her small body too.
    #[cfg(target_os = "macos")]
    set_effect_level(&effect, true);
    let shown = effect
        .set_position(tauri::Position::Physical(*monitor.position()))
        .and_then(|_| effect.set_size(tauri::Size::Physical(*monitor.size())))
        .and_then(|_| take_clicks(&effect))
        .and_then(|_| show_without_focus(&effect))
        .and_then(|_| effect.emit("rest", RestPayload { hours: hours.clamp(1, 99) }));
    if let Err(error) = shown {
        REST_SHOWING.store(false, Ordering::Relaxed);
        release_rest(&effect);
        let _ = hide_shown_window(&effect);
        return Err(format!("could not show the rest reminder: {error}"));
    }
    let handle = app.clone();
    thread::spawn(move || {
        thread::sleep(REST_FAILSAFE);
        if REST_GENERATION.load(Ordering::Relaxed) != generation {
            return;
        }
        let _ = handle.clone().run_on_main_thread(move || {
            if REST_GENERATION.load(Ordering::Relaxed) == generation && REST_SHOWING.swap(false, Ordering::Relaxed) {
                if let Some(effect) = handle.get_webview_window(EFFECT_WINDOW) {
                    let _ = hide_shown_window(&effect);
                    release_rest(&effect);
                }
            }
        });
    });
    Ok(())
}

/// The rest reminder closes on a click, so the effect window takes clicks while it is up.
fn take_clicks(window: &WebviewWindow) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    {
        shape_input(window, &[]);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    window.set_ignore_cursor_events(false)
}

/// Back to click-through, as the effect window always is otherwise.
fn pass_clicks_through(window: &WebviewWindow) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    {
        pass_all_clicks_through(window);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    window.set_ignore_cursor_events(true)
}

/// Let her climb and hang over the menu bar, and follow you onto other Spaces and into
/// full-screen apps. macOS keeps floating windows below the menu bar; status-bar ones are not.
#[cfg(target_os = "macos")]
fn lift_over_menu_bar(window: &WebviewWindow) {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    // SAFETY: Tauri hands back the live NSWindow of this webview window; setup runs on the main thread.
    let ns_window: &NSWindow = unsafe { &*pointer.cast::<NSWindow>() };
    ns_window.setLevel(NSStatusWindowLevel);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
}

/// The effect window follows you onto other Spaces and into full-screen apps like her body.
/// Pranks draw at the floating level, below her body; only the rest reminder goes over the
/// menu bar. Called on the main thread (setup and synchronous commands).
#[cfg(target_os = "macos")]
fn set_effect_level(window: &WebviewWindow, over_menu_bar: bool) {
    use objc2_app_kit::{NSFloatingWindowLevel, NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    // SAFETY: Tauri hands back the live NSWindow of this webview window, on the main thread.
    let ns_window: &NSWindow = unsafe { &*pointer.cast::<NSWindow>() };
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    // One level above her body (which sits at the status level), so the card covers her too.
    ns_window.setLevel(if over_menu_bar { NSStatusWindowLevel + 1 } else { NSFloatingWindowLevel });
}

fn require_window(window: &WebviewWindow, expected: &str) -> Result<(), String> {
    if window.label() == expected {
        Ok(())
    } else {
        Err(format!(
            "command is only available to the {expected} window"
        ))
    }
}

fn target_matches(target: Option<&serde_json::Value>, active: &ActiveWindow) -> bool {
    let Some(target) = target else {
        return true;
    };
    if target.is_null() {
        return true;
    }
    if let Some(name) = target.as_str() {
        return matches!(name, "active" | "active_window" | "frontmost") || name == active.app;
    }
    let Some(object) = target.as_object() else {
        return false;
    };

    let mut checked = false;
    if let Some(value) = object.get("app") {
        checked = true;
        let app = value.as_str();
        if !matches!(app, Some("active" | "active_window" | "frontmost"))
            && app != Some(active.app.as_str())
        {
            return false;
        }
    }
    if let Some(value) = object.get("title").or_else(|| object.get("window_title")) {
        checked = true;
        if value.as_str() != Some(active.title.as_str()) {
            return false;
        }
    }
    for (key, expected) in [
        ("x", active.x),
        ("y", active.y),
        ("w", active.w),
        ("h", active.h),
    ] {
        if let Some(value) = object.get(key) {
            checked = true;
            if value.as_i64() != Some(expected as i64) {
                return false;
            }
        }
    }
    checked
}

fn overlay_state_path() -> PathBuf {
    crate::paths::state_dir().join("overlay.json")
}

fn write_state(path: &Path, state: &PersistentState) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "overlay state path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("could not encode overlay state: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("could not replace {}: {error}", path.display()))
}
