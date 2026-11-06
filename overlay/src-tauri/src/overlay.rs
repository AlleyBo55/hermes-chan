//! Native overlay state, active-window tracking, hit testing, and effects.

use crate::winctl::{self, ActiveWindow, ErrorKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};

const MAIN_WINDOW: &str = "hermeschan";
const EFFECT_WINDOW: &str = "effect";
const ACTIVE_POLL: Duration = Duration::from_millis(750);
const HIT_TEST_POLL: Duration = Duration::from_millis(50);

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
}

struct Inner {
    persistent: PersistentState,
    persistence_dirty: bool,
    hit_regions: Vec<HitRegion>,
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

        effect
            .set_ignore_cursor_events(true)
            .map_err(|error| format!("could not make effect window click-through: {error}"))?;

        let controller = self.clone();
        main.on_window_event(move |event| {
            if let WindowEvent::Moved(position) = event {
                controller.record_pinned_position(position.x, position.y);
            }
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
            loop {
                controller.update_hit_testing(&handle);
                controller.flush_dirty_state(&handle);

                if last_active_poll.elapsed() >= ACTIVE_POLL {
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

        let steps = 16;
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
            thread::sleep(Duration::from_millis(18));
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
            self.show_effect(handle, "glitch", verb, None, &target, intensity, 1200);
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
            .and_then(|_| window.show());
        if let Err(error) = result {
            self.emit_status(
                handle,
                "error",
                "effect",
                &format!("could not show effect window: {error}"),
            );
            return;
        }

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
    controller.inner.lock().unwrap().hit_regions = regions;
    controller.update_hit_testing(&app);
    Ok(())
}

#[tauri::command]
pub fn hide_effect(window: WebviewWindow) -> Result<(), String> {
    require_window(&window, EFFECT_WINDOW)?;
    window
        .hide()
        .map_err(|error| format!("could not hide effect window: {error}"))
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
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".hermes/hermes-chan/overlay.json")
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
