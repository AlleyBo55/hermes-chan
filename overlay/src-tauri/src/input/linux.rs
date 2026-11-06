//! Linux input through the X11 XTest extension (X11 sessions, and X11/XWayland apps on
//! Wayland sessions). Characters missing from the keyboard layout are typed by briefly
//! binding them to a spare keycode, the way xdotool does.

use super::parse::{self, Button, Key, Modifier, Named, Scroll};
use super::InputError;
use crate::winctl::{linux::X11, scale_factor};
use serde_json::Value;
use std::{collections::HashMap, thread, time::Duration};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        xproto::{
            ClientMessageEvent, ConnectionExt as _, EventMask, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT,
            KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
        },
        xtest::{self, ConnectionExt as _},
    },
    CURRENT_TIME, NONE,
};

const KEY_DELAY: Duration = Duration::from_millis(6);

pub fn execute(verb: &str, input: &Value) -> Result<String, InputError> {
    let x11 = X11::connect().map_err(|error| InputError::unavailable(error.message))?;
    if verb != "focus_app" {
        let has_xtest = x11.conn.extension_information(xtest::X11_EXTENSION_NAME).ok().flatten().is_some();
        if !has_xtest {
            return Err(InputError::unavailable("the X server has no XTest extension for synthetic input"));
        }
    }
    match verb {
        "click" => click(&x11, input),
        "drag" => drag(&x11, input),
        "scroll" => scroll(&x11, input),
        "type" | "set_value" => type_text(&x11, input),
        "key" => key(&x11, input),
        "focus_app" => focus_app(&x11, input),
        _ => Err(InputError::invalid(format!("unsupported native input action: {verb}"))),
    }
}

fn sent<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, InputError> {
    result.map_err(|error| InputError::unavailable(format!("X11 input failed: {error}")))
}

fn flush(x11: &X11) -> Result<(), InputError> {
    sent(x11.conn.flush())
}

fn fake(x11: &X11, kind: u8, detail: u8, x: i16, y: i16) -> Result<(), InputError> {
    let root = if kind == MOTION_NOTIFY_EVENT { x11.root } else { NONE };
    sent(x11.conn.xtest_fake_input(kind, detail, CURRENT_TIME, root, x, y, 0))?;
    Ok(())
}

fn move_to(x11: &X11, point: (f64, f64)) -> Result<(), InputError> {
    let scale = scale_factor();
    let clamp = |value: f64| (value * scale).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    fake(x11, MOTION_NOTIFY_EVENT, 0, clamp(point.0), clamp(point.1))?;
    flush(x11)
}

fn button_number(button: Button) -> u8 {
    match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
    }
}

fn press_button(x11: &X11, button: u8, down: bool) -> Result<(), InputError> {
    fake(x11, if down { BUTTON_PRESS_EVENT } else { BUTTON_RELEASE_EVENT }, button, 0, 0)?;
    flush(x11)
}

fn click(x11: &X11, input: &Value) -> Result<String, InputError> {
    let at = parse::point(input, "x", "y")?;
    let button = button_number(parse::button(input)?);
    move_to(x11, at)?;
    for _ in 0..parse::click_count(input) {
        press_button(x11, button, true)?;
        thread::sleep(Duration::from_millis(35));
        press_button(x11, button, false)?;
        thread::sleep(Duration::from_millis(55));
    }
    Ok(format!("clicked at ({:.0}, {:.0})", at.0, at.1))
}

fn drag(x11: &X11, input: &Value) -> Result<String, InputError> {
    let from = parse::xy_pair(input.get("from_xy"), "from_xy")?;
    let to = parse::xy_pair(input.get("to_xy"), "to_xy")?;
    let button = button_number(parse::button(input)?);
    move_to(x11, from)?;
    press_button(x11, button, true)?;
    for step in 1..=16 {
        let t = step as f64 / 16.0;
        let eased = t * t * (3.0 - 2.0 * t);
        move_to(x11, (from.0 + (to.0 - from.0) * eased, from.1 + (to.1 - from.1) * eased))?;
        thread::sleep(Duration::from_millis(12));
    }
    press_button(x11, button, false)?;
    Ok("dragged between screen coordinates".to_string())
}

fn scroll(x11: &X11, input: &Value) -> Result<String, InputError> {
    if let Some(at) = parse::optional_point(input) {
        move_to(x11, at)?;
    }
    let (direction, amount, name) = parse::scroll(input)?;
    // X11 scrolls with buttons 4-7, one notch per click.
    let button = match direction {
        Scroll::Up => 4,
        Scroll::Down => 5,
        Scroll::Left => 6,
        Scroll::Right => 7,
    };
    for _ in 0..amount {
        press_button(x11, button, true)?;
        press_button(x11, button, false)?;
        thread::sleep(Duration::from_millis(8));
    }
    Ok(format!("scrolled {name} by {amount}"))
}

/// The keyboard layout as keysym -> (keycode, needs shift), plus a keycode with no
/// symbols that can be borrowed for anything the layout lacks.
struct Keymap {
    per_keycode: u8,
    symbols: HashMap<u32, (u8, bool)>,
    spare: Option<u8>,
}

impl Keymap {
    fn load(x11: &X11) -> Result<Self, InputError> {
        let setup = x11.conn.setup();
        let (first, last) = (setup.min_keycode, setup.max_keycode);
        let reply = sent(sent(x11.conn.get_keyboard_mapping(first, last - first + 1))?.reply())?;
        let per_keycode = reply.keysyms_per_keycode.max(1);
        let mut symbols = HashMap::new();
        let mut spare = None;
        for (index, row) in reply.keysyms.chunks(per_keycode as usize).enumerate() {
            let keycode = first + index as u8;
            if row.iter().all(|keysym| *keysym == 0) {
                // Prefer a high, unused keycode, the way xdotool does.
                spare = Some(keycode);
                continue;
            }
            for (level, keysym) in row.iter().take(2).enumerate() {
                if *keysym != 0 {
                    symbols.entry(*keysym).or_insert((keycode, level == 1));
                }
            }
        }
        Ok(Self { per_keycode, symbols, spare })
    }

    fn keycode(&self, keysym: u32) -> Option<(u8, bool)> {
        self.symbols.get(&keysym).copied()
    }
}

fn keysym_for_char(ch: char) -> u32 {
    match ch {
        '\n' | '\r' => 0xff0d,
        '\t' => 0xff09,
        '\u{8}' => 0xff08,
        // Latin-1 keysyms are the code points themselves; the rest live at 0x0100_0000 + U.
        ' '..='~' | '\u{a0}'..='\u{ff}' => ch as u32,
        _ => 0x0100_0000 | ch as u32,
    }
}

fn keysym_for_named(named: Named) -> u32 {
    match named {
        Named::Return => 0xff0d,
        Named::Tab => 0xff09,
        Named::Space => 0x0020,
        Named::Backspace => 0xff08,
        Named::Delete => 0xffff,
        Named::Escape => 0xff1b,
        Named::Home => 0xff50,
        Named::End => 0xff57,
        Named::PageUp => 0xff55,
        Named::PageDown => 0xff56,
        Named::Left => 0xff51,
        Named::Up => 0xff52,
        Named::Right => 0xff53,
        Named::Down => 0xff54,
    }
}

fn keysym_for_modifier(modifier: Modifier) -> u32 {
    match modifier {
        Modifier::Control => 0xffe3,
        Modifier::Shift => 0xffe1,
        Modifier::Alt => 0xffe9,
        Modifier::Meta => 0xffeb,
    }
}

fn press_key(x11: &X11, keycode: u8, down: bool) -> Result<(), InputError> {
    fake(x11, if down { KEY_PRESS_EVENT } else { KEY_RELEASE_EVENT }, keycode, 0, 0)
}

fn modifier_keycode(keymap: &Keymap, modifier: Modifier) -> Result<u8, InputError> {
    keymap
        .keycode(keysym_for_modifier(modifier))
        .map(|(keycode, _)| keycode)
        .ok_or_else(|| InputError::unavailable("this keyboard layout has no key for that modifier"))
}

/// Bind `keysym` to the spare keycode for one key press.
fn borrow_spare(x11: &X11, keymap: &Keymap, keysym: u32) -> Result<u8, InputError> {
    let spare = keymap
        .spare
        .ok_or_else(|| InputError::unavailable("no free keycode to type that character"))?;
    let row = vec![keysym; keymap.per_keycode as usize];
    sent(x11.conn.change_keyboard_mapping(1, spare, keymap.per_keycode, &row))?;
    // A round trip, then a moment for apps to pick up the MappingNotify.
    sent(sent(x11.conn.get_input_focus())?.reply())?;
    thread::sleep(Duration::from_millis(25));
    Ok(spare)
}

fn release_spare(x11: &X11, keymap: &Keymap) {
    if let Some(spare) = keymap.spare {
        let row = vec![0u32; keymap.per_keycode as usize];
        let _ = x11.conn.change_keyboard_mapping(1, spare, keymap.per_keycode, &row);
        let _ = x11.conn.flush();
    }
}

/// Press `keysym` with `modifiers` held, borrowing the spare keycode when needed.
fn tap_keysym(x11: &X11, keymap: &Keymap, keysym: u32, modifiers: &[Modifier]) -> Result<bool, InputError> {
    let (keycode, needs_shift, borrowed) = match keymap.keycode(keysym) {
        Some((keycode, shifted)) => (keycode, shifted, false),
        None => (borrow_spare(x11, keymap, keysym)?, false, true),
    };
    let mut held: Vec<u8> = Vec::new();
    for modifier in modifiers {
        held.push(modifier_keycode(keymap, *modifier)?);
    }
    if needs_shift && !modifiers.contains(&Modifier::Shift) {
        held.push(modifier_keycode(keymap, Modifier::Shift)?);
    }
    for code in &held {
        press_key(x11, *code, true)?;
    }
    press_key(x11, keycode, true)?;
    press_key(x11, keycode, false)?;
    for code in held.iter().rev() {
        press_key(x11, *code, false)?;
    }
    flush(x11)?;
    Ok(borrowed)
}

fn type_text(x11: &X11, input: &Value) -> Result<String, InputError> {
    let text = parse::text(input)?;
    let keymap = Keymap::load(x11)?;
    let mut borrowed = false;
    let mut chars = text.chars().peekable();
    let result = (|| {
        while let Some(ch) = chars.next() {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                continue;
            }
            borrowed |= tap_keysym(x11, &keymap, keysym_for_char(ch), &[])?;
            thread::sleep(KEY_DELAY);
        }
        Ok::<(), InputError>(())
    })();
    if borrowed {
        release_spare(x11, &keymap);
    }
    result?;
    Ok(format!("typed {} characters", text.chars().count()))
}

fn key(x11: &X11, input: &Value) -> Result<String, InputError> {
    let (modifiers, key, combo) = parse::combo(input)?;
    let keymap = Keymap::load(x11)?;
    let keysym = match key {
        Key::Named(named) => keysym_for_named(named),
        Key::Char(ch) => keysym_for_char(ch),
    };
    let borrowed = tap_keysym(x11, &keymap, keysym, &modifiers);
    if matches!(borrowed, Ok(true)) {
        release_spare(x11, &keymap);
    }
    borrowed?;
    Ok(format!("pressed {combo}"))
}

/// Ask the window manager to activate an open window of the app. Nothing is launched.
fn focus_app(x11: &X11, input: &Value) -> Result<String, InputError> {
    let app = parse::app(input)?;
    let wanted = app.to_lowercase();
    let clients = x11.clients();
    let matches_class = |window: u32| {
        let (instance, class) = x11.class(window);
        instance.to_lowercase() == wanted || class.to_lowercase() == wanted
    };
    let window = clients
        .iter()
        .copied()
        .find(|window| matches_class(*window))
        .or_else(|| clients.iter().copied().find(|window| x11.app(*window, x11.pid(*window)).to_lowercase().contains(&wanted)))
        .or_else(|| clients.iter().copied().find(|window| x11.title(*window).to_lowercase().contains(&wanted)))
        .ok_or_else(|| InputError::unavailable(format!("no open window for {app}; open it first")))?;
    // Source 2: a pager or tool; window managers honour it like a click on the taskbar.
    let event = ClientMessageEvent::new(32, window, x11.atoms._NET_ACTIVE_WINDOW, [2, CURRENT_TIME, 0, 0, 0]);
    sent(x11.conn.send_event(
        false,
        x11.root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    ))?;
    flush(x11)?;
    for _ in 0..10 {
        if x11.active_client() == Some(window) {
            return Ok(format!("focused {app}"));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(InputError::unavailable(format!("the window manager did not bring {app} to the front")))
}
