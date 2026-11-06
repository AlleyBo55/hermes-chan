//! Native input for the public Hermes `ComputerUseBackend` implementation.
//!
//! The Python plugin never imports Hermes private drivers. It waits for the
//! sprite's contact frame, sends one `native_input` request here, and receives a
//! structured result. Coordinates are logical points, like everything she reports.
//! macOS uses CoreGraphics events and System Events (Accessibility permission),
//! Windows uses SendInput, Linux uses the X11 XTest extension.

use serde_json::{json, Value};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use self::windows as platform;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::InputError;
    use serde_json::Value;

    pub fn execute(_verb: &str, _input: &Value) -> Result<String, InputError> {
        Err(InputError::unavailable(
            "Hermes-chan native input is implemented for macOS, Windows and Linux (X11)",
        ))
    }
}

pub fn execute(message: &Value) -> Value {
    let action_id = message
        .get("action_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let verb = message.get("verb").and_then(Value::as_str).unwrap_or("");
    let input = message.get("input").unwrap_or(&Value::Null);

    let result = platform::execute(verb, input);
    match result {
        Ok(summary) => json!({
            "type": "native_result",
            "action_id": action_id,
            "ok": true,
            "action": verb,
            "message": summary,
        }),
        Err(error) => json!({
            "type": "native_result",
            "action_id": action_id,
            "ok": false,
            "action": verb,
            "code": error.code,
            "message": error.message,
        }),
    }
}

struct InputError {
    code: &'static str,
    message: String,
}

impl InputError {
    fn invalid(message: impl Into<String>) -> Self {
        Self { code: "invalid_input", message: message.into() }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self { code: "input_unavailable", message: message.into() }
    }
}

// --- request parsing shared by the Windows and Linux drivers ------------------------
// (the macOS driver predates these and keeps its own, equivalent parsing)

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod parse {
    use super::InputError;
    use serde_json::Value;

    pub(crate) fn point(input: &Value, x_key: &str, y_key: &str) -> Result<(f64, f64), InputError> {
        let x = input.get(x_key).and_then(Value::as_f64)
            .ok_or_else(|| InputError::invalid(format!("{x_key} is required")))?;
        let y = input.get(y_key).and_then(Value::as_f64)
            .ok_or_else(|| InputError::invalid(format!("{y_key} is required")))?;
        if !x.is_finite() || !y.is_finite() {
            return Err(InputError::invalid("coordinates must be finite"));
        }
        Ok((x, y))
    }

    pub(crate) fn optional_point(input: &Value) -> Option<(f64, f64)> {
        let x = input.get("x").and_then(Value::as_f64)?;
        let y = input.get("y").and_then(Value::as_f64)?;
        (x.is_finite() && y.is_finite()).then_some((x, y))
    }

    pub(crate) fn xy_pair(value: Option<&Value>, name: &str) -> Result<(f64, f64), InputError> {
        let values = value.and_then(Value::as_array)
            .ok_or_else(|| InputError::invalid(format!("{name} must be [x, y]")))?;
        if values.len() != 2 {
            return Err(InputError::invalid(format!("{name} must contain two coordinates")));
        }
        let x = values[0].as_f64().filter(|v| v.is_finite())
            .ok_or_else(|| InputError::invalid(format!("{name}[0] is invalid")))?;
        let y = values[1].as_f64().filter(|v| v.is_finite())
            .ok_or_else(|| InputError::invalid(format!("{name}[1] is invalid")))?;
        Ok((x, y))
    }

    #[derive(Clone, Copy)]
    pub(crate) enum Button {
        Left,
        Right,
        Middle,
    }

    pub(crate) fn button(input: &Value) -> Result<Button, InputError> {
        match input.get("button").and_then(Value::as_str).unwrap_or("left") {
            "left" => Ok(Button::Left),
            "right" => Ok(Button::Right),
            "middle" => Ok(Button::Middle),
            other => Err(InputError::invalid(format!("unsupported mouse button: {other}"))),
        }
    }

    pub(crate) fn click_count(input: &Value) -> u64 {
        input.get("click_count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 3)
    }

    #[derive(Clone, Copy)]
    pub(crate) enum Scroll {
        Up,
        Down,
        Left,
        Right,
    }

    pub(crate) fn scroll(input: &Value) -> Result<(Scroll, i32, &str), InputError> {
        let amount = input.get("amount").and_then(Value::as_i64).unwrap_or(3).clamp(1, 50) as i32;
        let name = input.get("direction").and_then(Value::as_str).unwrap_or("down");
        let direction = match name {
            "up" => Scroll::Up,
            "down" => Scroll::Down,
            "left" => Scroll::Left,
            "right" => Scroll::Right,
            other => return Err(InputError::invalid(format!("unsupported scroll direction: {other}"))),
        };
        Ok((direction, amount, name))
    }

    pub(crate) fn text(input: &Value) -> Result<&str, InputError> {
        let text = input.get("text").and_then(Value::as_str)
            .ok_or_else(|| InputError::invalid("text is required"))?;
        if text.len() > 1_000_000 {
            return Err(InputError::invalid("text exceeds the 1 MB input limit"));
        }
        Ok(text)
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Modifier {
        Control,
        Shift,
        Alt,
        /// cmd / meta / super / win: the Windows key, or Super on Linux.
        Meta,
    }

    #[derive(Clone, Copy)]
    pub(crate) enum Named {
        Return,
        Tab,
        Space,
        Backspace,
        Delete,
        Escape,
        Home,
        End,
        PageUp,
        PageDown,
        Left,
        Right,
        Up,
        Down,
    }

    #[derive(Clone, Copy)]
    pub(crate) enum Key {
        Named(Named),
        Char(char),
    }

    /// "ctrl+shift+t", "return", "alt+f4"-style combos: modifiers, then one key.
    pub(crate) fn combo(input: &Value) -> Result<(Vec<Modifier>, Key, String), InputError> {
        let combo = input.get("keys").and_then(Value::as_str)
            .ok_or_else(|| InputError::invalid("keys is required"))?;
        let parts: Vec<String> = combo.split('+').map(|part| part.trim().to_ascii_lowercase())
            .filter(|part| !part.is_empty()).collect();
        let (key, modifiers) = parts.split_last()
            .ok_or_else(|| InputError::invalid("keys is empty"))?;
        let mut held = Vec::new();
        for modifier in modifiers {
            let value = match modifier.as_str() {
                "ctrl" | "control" => Modifier::Control,
                "shift" => Modifier::Shift,
                "alt" | "option" => Modifier::Alt,
                "cmd" | "command" | "meta" | "super" | "win" | "windows" => Modifier::Meta,
                other => return Err(InputError::invalid(format!("unsupported modifier: {other}"))),
            };
            if !held.contains(&value) {
                held.push(value);
            }
        }
        let key = match key.as_str() {
            "return" | "enter" => Key::Named(Named::Return),
            "tab" => Key::Named(Named::Tab),
            "space" => Key::Named(Named::Space),
            "backspace" => Key::Named(Named::Backspace),
            "delete" | "del" => Key::Named(Named::Delete),
            "escape" | "esc" => Key::Named(Named::Escape),
            "home" => Key::Named(Named::Home),
            "end" => Key::Named(Named::End),
            "pageup" | "page_up" => Key::Named(Named::PageUp),
            "pagedown" | "page_down" => Key::Named(Named::PageDown),
            "left" | "left_arrow" => Key::Named(Named::Left),
            "right" | "right_arrow" => Key::Named(Named::Right),
            "down" | "down_arrow" => Key::Named(Named::Down),
            "up" | "up_arrow" => Key::Named(Named::Up),
            single if single.chars().count() == 1 => Key::Char(single.chars().next().unwrap_or(' ')),
            other => return Err(InputError::invalid(format!("unsupported key: {other}"))),
        };
        Ok((held, key, combo.to_string()))
    }

    pub(crate) fn app(input: &Value) -> Result<&str, InputError> {
        input.get("app").and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| InputError::invalid("app is required"))
    }
}
