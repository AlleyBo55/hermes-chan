//! macOS input: CoreGraphics mouse and scroll events, System Events keystrokes.
//! macOS enforces Accessibility permission for both.

use super::InputError;
use core_graphics::{
    event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
        ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
};
use serde_json::Value;
use std::{process::Command, thread, time::Duration};

pub fn execute(verb: &str, input: &Value) -> Result<String, InputError> {
    match verb {
        "click" => click(input),
        "drag" => drag(input),
        "scroll" => scroll(input),
        "type" | "set_value" => type_text(input),
        "key" => key(input),
        "focus_app" => focus_app(input),
        _ => Err(InputError::invalid(format!("unsupported native input action: {verb}"))),
    }
}

fn source() -> Result<CGEventSource, InputError> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| InputError::unavailable("could not create a CoreGraphics event source"))
}

fn point(input: &Value, x_key: &str, y_key: &str) -> Result<CGPoint, InputError> {
    let x = input.get(x_key).and_then(Value::as_f64)
        .ok_or_else(|| InputError::invalid(format!("{x_key} is required")))?;
    let y = input.get(y_key).and_then(Value::as_f64)
        .ok_or_else(|| InputError::invalid(format!("{y_key} is required")))?;
    if !x.is_finite() || !y.is_finite() {
        return Err(InputError::invalid("coordinates must be finite"));
    }
    Ok(CGPoint::new(x, y))
}

fn mouse_button(input: &Value) -> Result<(CGMouseButton, CGEventType, CGEventType, CGEventType), InputError> {
    match input.get("button").and_then(Value::as_str).unwrap_or("left") {
        "left" => Ok((CGMouseButton::Left, CGEventType::LeftMouseDown,
                      CGEventType::LeftMouseUp, CGEventType::LeftMouseDragged)),
        "right" => Ok((CGMouseButton::Right, CGEventType::RightMouseDown,
                       CGEventType::RightMouseUp, CGEventType::RightMouseDragged)),
        "middle" => Ok((CGMouseButton::Center, CGEventType::OtherMouseDown,
                        CGEventType::OtherMouseUp, CGEventType::OtherMouseDragged)),
        other => Err(InputError::invalid(format!("unsupported mouse button: {other}"))),
    }
}

fn post_mouse(kind: CGEventType, point: CGPoint, button: CGMouseButton) -> Result<(), InputError> {
    let event = CGEvent::new_mouse_event(source()?, kind, point, button)
        .map_err(|_| InputError::unavailable("could not create a mouse event"))?;
    event.post(CGEventTapLocation::HID);
    Ok(())
}

fn click(input: &Value) -> Result<String, InputError> {
    let at = point(input, "x", "y")?;
    let (button, down, up, _) = mouse_button(input)?;
    let count = input.get("click_count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 3);
    post_mouse(CGEventType::MouseMoved, at, button)?;
    for _ in 0..count {
        post_mouse(down, at, button)?;
        thread::sleep(Duration::from_millis(35));
        post_mouse(up, at, button)?;
        thread::sleep(Duration::from_millis(55));
    }
    Ok(format!("clicked at ({:.0}, {:.0})", at.x, at.y))
}

fn xy_pair(value: Option<&Value>, name: &str) -> Result<CGPoint, InputError> {
    let values = value.and_then(Value::as_array)
        .ok_or_else(|| InputError::invalid(format!("{name} must be [x, y]")))?;
    if values.len() != 2 {
        return Err(InputError::invalid(format!("{name} must contain two coordinates")));
    }
    let x = values[0].as_f64().ok_or_else(|| InputError::invalid(format!("{name}[0] is invalid")))?;
    let y = values[1].as_f64().ok_or_else(|| InputError::invalid(format!("{name}[1] is invalid")))?;
    Ok(CGPoint::new(x, y))
}

fn drag(input: &Value) -> Result<String, InputError> {
    let from = xy_pair(input.get("from_xy"), "from_xy")?;
    let to = xy_pair(input.get("to_xy"), "to_xy")?;
    let (button, down, up, dragged) = mouse_button(input)?;
    post_mouse(CGEventType::MouseMoved, from, button)?;
    post_mouse(down, from, button)?;
    for step in 1..=16 {
        let t = step as f64 / 16.0;
        let eased = t * t * (3.0 - 2.0 * t);
        let at = CGPoint::new(
            from.x + (to.x - from.x) * eased,
            from.y + (to.y - from.y) * eased,
        );
        post_mouse(dragged, at, button)?;
        thread::sleep(Duration::from_millis(12));
    }
    post_mouse(up, to, button)?;
    Ok("dragged between screen coordinates".to_string())
}

fn current_pointer() -> Result<CGPoint, InputError> {
    let event = CGEvent::new(source()?)
        .map_err(|_| InputError::unavailable("could not read pointer location"))?;
    Ok(event.location())
}

fn scroll(input: &Value) -> Result<String, InputError> {
    let x = input.get("x").and_then(Value::as_f64);
    let y = input.get("y").and_then(Value::as_f64);
    if let (Some(x), Some(y)) = (x, y) {
        post_mouse(CGEventType::MouseMoved, CGPoint::new(x, y), CGMouseButton::Left)?;
    }
    let _ = current_pointer()?;
    let amount = input.get("amount").and_then(Value::as_i64).unwrap_or(3).clamp(1, 50) as i32;
    let direction = input.get("direction").and_then(Value::as_str).unwrap_or("down");
    let (vertical, horizontal) = match direction {
        "up" => (amount, 0),
        "down" => (-amount, 0),
        "left" => (0, amount),
        "right" => (0, -amount),
        other => return Err(InputError::invalid(format!("unsupported scroll direction: {other}"))),
    };
    let event = CGEvent::new_scroll_event(
        source()?, ScrollEventUnit::LINE, 2, vertical, horizontal, 0,
    ).map_err(|_| InputError::unavailable("could not create a scroll event"))?;
    event.post(CGEventTapLocation::HID);
    Ok(format!("scrolled {direction} by {amount}"))
}

fn run_system_events(lines: &[&str], argument: Option<&str>) -> Result<(), InputError> {
    let mut command = Command::new("osascript");
    for line in lines {
        command.arg("-e").arg(line);
    }
    if let Some(argument) = argument {
        command.arg("--").arg(argument);
    }
    let output = command.output().map_err(|error| {
        InputError::unavailable(format!("could not launch System Events: {error}"))
    })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(InputError::unavailable(if stderr.is_empty() {
        "System Events input failed; grant Accessibility permission".to_string()
    } else {
        stderr
    }))
}

fn type_text(input: &Value) -> Result<String, InputError> {
    let text = input.get("text").and_then(Value::as_str)
        .ok_or_else(|| InputError::invalid("text is required"))?;
    if text.len() > 1_000_000 {
        return Err(InputError::invalid("text exceeds the 1 MB input limit"));
    }
    run_system_events(&[
        "on run argv",
        "tell application \"System Events\" to keystroke (item 1 of argv)",
        "end run",
    ], Some(text))?;
    Ok(format!("typed {} characters", text.chars().count()))
}

fn modifier_clause(modifiers: &[&str]) -> Result<String, InputError> {
    let mut values = Vec::new();
    for modifier in modifiers {
        values.push(match *modifier {
            "cmd" | "command" | "meta" => "command down",
            "shift" => "shift down",
            "alt" | "option" => "option down",
            "ctrl" | "control" => "control down",
            other => return Err(InputError::invalid(format!("unsupported modifier: {other}"))),
        });
    }
    Ok(if values.is_empty() { String::new() } else { format!(" using {{{}}}", values.join(", ")) })
}

fn key(input: &Value) -> Result<String, InputError> {
    let combo = input.get("keys").and_then(Value::as_str)
        .ok_or_else(|| InputError::invalid("keys is required"))?;
    let parts: Vec<String> = combo.split('+').map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty()).collect();
    let (key, modifiers) = parts.split_last()
        .ok_or_else(|| InputError::invalid("keys is empty"))?;
    let modifier_refs: Vec<&str> = modifiers.iter().map(String::as_str).collect();
    let clause = modifier_clause(&modifier_refs)?;
    let special = match key.as_str() {
        "return" | "enter" => Some(36),
        "tab" => Some(48),
        "space" => Some(49),
        "backspace" | "delete" => Some(51),
        "escape" | "esc" => Some(53),
        "home" => Some(115),
        "end" => Some(119),
        "pageup" | "page_up" => Some(116),
        "pagedown" | "page_down" => Some(121),
        "left" | "left_arrow" => Some(123),
        "right" | "right_arrow" => Some(124),
        "down" | "down_arrow" => Some(125),
        "up" | "up_arrow" => Some(126),
        _ => None,
    };
    if let Some(code) = special {
        let script = format!("tell application \"System Events\" to key code {code}{clause}");
        run_system_events(&[&script], None)?;
    } else if key.chars().count() == 1 {
        let script = format!(
            "tell application \"System Events\" to keystroke (item 1 of argv){clause}"
        );
        run_system_events(&["on run argv", &script, "end run"], Some(key))?;
    } else {
        return Err(InputError::invalid(format!("unsupported key: {key}")));
    }
    Ok(format!("pressed {combo}"))
}

fn focus_app(input: &Value) -> Result<String, InputError> {
    let app = input.get("app").and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| InputError::invalid("app is required"))?;
    let output = Command::new("open").args(["-a", app]).output().map_err(|error| {
        InputError::unavailable(format!("could not focus app: {error}"))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(InputError::unavailable(if stderr.is_empty() {
            format!("could not focus app {app}")
        } else {
            stderr
        }));
    }
    Ok(format!("focused {app}"))
}

#[allow(dead_code)]
fn _modifier_flags(modifiers: &[&str]) -> Result<CGEventFlags, InputError> {
    let mut flags = CGEventFlags::empty();
    for modifier in modifiers {
        flags |= match *modifier {
            "cmd" | "command" | "meta" => CGEventFlags::CGEventFlagCommand,
            "shift" => CGEventFlags::CGEventFlagShift,
            "alt" | "option" => CGEventFlags::CGEventFlagAlternate,
            "ctrl" | "control" => CGEventFlags::CGEventFlagControl,
            other => return Err(InputError::invalid(format!("unsupported modifier: {other}"))),
        };
    }
    Ok(flags)
}
