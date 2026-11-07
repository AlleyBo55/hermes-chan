//! Linux through X11 (XWayland on Wayland sessions, see main.rs): EWMH for the window
//! list and the active window, `_NET_MOVERESIZE_WINDOW` for moves, MIT-SCREEN-SAVER for
//! idle time, GetImage (through Composite when a compositor runs) for snapshots.
//! Native Wayland apps are invisible to X11 clients, so on Wayland she only sees and
//! touches X11/XWayland windows. x11rb speaks the protocol directly: no Xlib, so no
//! process-wide X error handler to fight GTK over.

use super::{png_base64, scale_factor, ActiveWindow, WindowControlError, WindowFrame};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::{
        composite::{self, ConnectionExt as _},
        screensaver::ConnectionExt as _,
        xproto::{
            AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, GetPropertyReply, ImageFormat,
            ImageOrder, MapState, Window,
        },
    },
    rust_connection::RustConnection,
};

const MAX_SAMPLE_BYTES: usize = 6 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 12 * 1024 * 1024;

x11rb::atom_manager! {
    pub(crate) Atoms: AtomsCookie {
        _NET_ACTIVE_WINDOW,
        _NET_CLIENT_LIST_STACKING,
        _NET_CURRENT_DESKTOP,
        _NET_FRAME_EXTENTS,
        _NET_MOVERESIZE_WINDOW,
        _NET_WM_DESKTOP,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _GTK_FRAME_EXTENTS,
        UTF8_STRING,
    }
}

fn failed<E: std::fmt::Display>(context: &'static str) -> impl FnOnce(E) -> WindowControlError {
    move |error| WindowControlError::other(format!("{context}: {error}"))
}

/// One short-lived connection per operation: cheap on a local X server, and nothing
/// stale survives an X server restart.
pub(crate) struct X11 {
    pub conn: RustConnection,
    pub root: Window,
    screen: usize,
    pub atoms: Atoms,
}

impl X11 {
    pub(crate) fn connect() -> Result<Self, WindowControlError> {
        let (conn, screen) = x11rb::connect(None).map_err(|error| {
            WindowControlError::other(format!(
                "no X11 display ({error}); on a Wayland session she needs XWayland"
            ))
        })?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)
            .map_err(failed("X11 request failed"))?
            .reply()
            .map_err(failed("X11 request failed"))?;
        Ok(Self { conn, root, screen, atoms })
    }

    fn property(&self, window: Window, property: u32, kind: u32, length: u32) -> Option<GetPropertyReply> {
        self.conn
            .get_property(false, window, property, kind, 0, length)
            .ok()?
            .reply()
            .ok()
    }

    pub(crate) fn numbers(&self, window: Window, property: u32, kind: impl Into<u32>, length: u32) -> Vec<u32> {
        self.property(window, property, kind.into(), length)
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default()
    }

    fn text(&self, window: Window, property: u32, kind: u32) -> Option<String> {
        let reply = self.property(window, property, kind, 1024)?;
        (reply.format == 8 && !reply.value.is_empty()).then(|| String::from_utf8_lossy(&reply.value).into_owned())
    }

    pub(crate) fn active_client(&self) -> Option<Window> {
        self.numbers(self.root, self.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW, 1)
            .first()
            .copied()
            .filter(|window| *window != 0)
    }

    /// Client windows, front to back.
    pub(crate) fn clients(&self) -> Vec<Window> {
        let mut list = self.numbers(self.root, self.atoms._NET_CLIENT_LIST_STACKING, AtomEnum::WINDOW, 4096);
        if list.is_empty() {
            // No EWMH window manager: the clients are the root's own children.
            list = self
                .conn
                .query_tree(self.root)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|tree| tree.children)
                .unwrap_or_default();
        }
        // Both lists run bottom to top.
        list.reverse();
        list
    }

    pub(crate) fn pid(&self, window: Window) -> u32 {
        self.numbers(window, self.atoms._NET_WM_PID, AtomEnum::CARDINAL, 1)
            .first()
            .copied()
            .unwrap_or(0)
    }

    /// WM_CLASS as (instance, class), e.g. ("code", "Code").
    pub(crate) fn class(&self, window: Window) -> (String, String) {
        let Some(reply) = self.property(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into(), 256) else {
            return (String::new(), String::new());
        };
        let mut parts = reply
            .value
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned());
        let instance = parts.next().unwrap_or_default();
        let class = parts.next().unwrap_or_else(|| instance.clone());
        (instance, class)
    }

    /// The app's name: WM_CLASS's class ("Code", "firefox", "kitty"), else the process name.
    pub(crate) fn app(&self, window: Window, pid: u32) -> String {
        let (_, class) = self.class(window);
        if !class.is_empty() || pid == 0 {
            return class;
        }
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|name| name.trim().to_string())
            .unwrap_or_default()
    }

    pub(crate) fn title(&self, window: Window) -> String {
        self.text(window, self.atoms._NET_WM_NAME, self.atoms.UTF8_STRING)
            .or_else(|| self.text(window, AtomEnum::WM_NAME.into(), AtomEnum::ANY.into()))
            .unwrap_or_default()
    }

    fn states(&self, window: Window) -> Vec<u32> {
        self.numbers(window, self.atoms._NET_WM_STATE, AtomEnum::ATOM, 32)
    }

    /// The visible frame in device pixels: window-manager decorations added,
    /// client-side-decoration shadows trimmed.
    pub(crate) fn frame(&self, window: Window) -> Option<(i32, i32, i32, i32)> {
        let geometry = self.conn.get_geometry(window).ok()?.reply().ok()?;
        let origin = self.conn.translate_coordinates(window, self.root, 0, 0).ok()?.reply().ok()?;
        let (mut x, mut y) = (origin.dst_x as i32, origin.dst_y as i32);
        let (mut w, mut h) = (geometry.width as i32, geometry.height as i32);
        if let [left, right, top, bottom] = self.numbers(window, self.atoms._NET_FRAME_EXTENTS, AtomEnum::CARDINAL, 4)[..] {
            x -= left as i32;
            y -= top as i32;
            w += (left + right) as i32;
            h += (top + bottom) as i32;
        }
        if let [left, right, top, bottom] = self.numbers(window, self.atoms._GTK_FRAME_EXTENTS, AtomEnum::CARDINAL, 4)[..] {
            x += left as i32;
            y += top as i32;
            w -= (left + right) as i32;
            h -= (top + bottom) as i32;
        }
        (w > 0 && h > 0).then_some((x, y, w, h))
    }

    fn current_desktop(&self) -> Option<u32> {
        self.numbers(self.root, self.atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL, 1)
            .first()
            .copied()
    }

    /// Mapped, a normal window or dialog, not minimised, and on the current desktop.
    pub(crate) fn is_normal(&self, window: Window, desktop: Option<u32>) -> bool {
        let Some(attributes) = self.conn.get_window_attributes(window).ok().and_then(|cookie| cookie.reply().ok()) else {
            return false;
        };
        if attributes.map_state != MapState::VIEWABLE || attributes.override_redirect {
            return false;
        }
        let types = self.numbers(window, self.atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, 16);
        if !types.is_empty()
            && !types.iter().any(|kind| *kind == self.atoms._NET_WM_WINDOW_TYPE_NORMAL || *kind == self.atoms._NET_WM_WINDOW_TYPE_DIALOG)
        {
            return false;
        }
        if self.states(window).contains(&self.atoms._NET_WM_STATE_HIDDEN) {
            return false;
        }
        match (desktop, self.numbers(window, self.atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL, 1).first()) {
            (Some(current), Some(on)) => *on == current || *on == u32::MAX,
            _ => true,
        }
    }

    pub(crate) fn describe(&self, window: Window) -> Option<ActiveWindow> {
        let (x, y, w, h) = self.frame(window)?;
        let pid = self.pid(window);
        let scale = scale_factor();
        Some(ActiveWindow {
            pid,
            app: self.app(window, pid),
            title: self.title(window),
            x: (x as f64 / scale).round() as i32,
            y: (y as f64 / scale).round() as i32,
            w: (w as f64 / scale).round().max(1.0) as i32,
            h: (h as f64 / scale).round().max(1.0) as i32,
            handle: window as u64,
        })
    }

    fn screen_size(&self) -> (i32, i32) {
        let screen = &self.conn.setup().roots[self.screen];
        (screen.width_in_pixels as i32, screen.height_in_pixels as i32)
    }

    /// The window the window manager wraps this client in (the client itself if undecorated).
    fn toplevel(&self, window: Window) -> Window {
        let mut current = window;
        for _ in 0..16 {
            let Some(tree) = self.conn.query_tree(current).ok().and_then(|cookie| cookie.reply().ok()) else {
                break;
            };
            if tree.parent == self.root || tree.parent == 0 {
                break;
            }
            current = tree.parent;
        }
        current
    }

    /// RGBA pixels of a drawable area via GetImage (ZPixmap, 32 bits per pixel).
    fn image_rgba(&self, drawable: u32, x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>, WindowControlError> {
        if w <= 0 || h <= 0 || w > i16::MAX as i32 || h > i16::MAX as i32 {
            return Err(WindowControlError::other("snapshot area is empty or too large"));
        }
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, drawable, x as i16, y as i16, w as u16, h as u16, !0)
            .map_err(failed("could not read the screen"))?
            .reply()
            .map_err(failed("could not read the screen"))?;
        let setup = self.conn.setup();
        let bits = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == reply.depth)
            .map(|format| format.bits_per_pixel)
            .unwrap_or(0);
        if bits != 32 {
            return Err(WindowControlError::other("snapshots need a 24- or 32-bit X display"));
        }
        let (red, green, blue) = setup
            .roots
            .iter()
            .flat_map(|screen| screen.allowed_depths.iter())
            .flat_map(|depth| depth.visuals.iter())
            .find(|visual| visual.visual_id == reply.visual)
            .map(|visual| (visual.red_mask, visual.green_mask, visual.blue_mask))
            .unwrap_or((0x00ff_0000, 0x0000_ff00, 0x0000_00ff));
        let channel = |pixel: u32, mask: u32| -> u8 {
            if mask == 0 {
                return 0;
            }
            let shift = mask.trailing_zeros();
            let max = mask >> shift;
            (((pixel & mask) >> shift) * 255 / max.max(1)) as u8
        };
        let big_endian = setup.image_byte_order == ImageOrder::MSB_FIRST;
        let count = w as usize * h as usize;
        if reply.data.len() < count * 4 {
            return Err(WindowControlError::other("the X server returned a short image"));
        }
        let mut rgba = Vec::with_capacity(count * 4);
        for chunk in reply.data.chunks_exact(4).take(count) {
            let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
            let pixel = if big_endian { u32::from_be_bytes(bytes) } else { u32::from_le_bytes(bytes) };
            rgba.extend_from_slice(&[channel(pixel, red), channel(pixel, green), channel(pixel, blue), 255]);
        }
        Ok(rgba)
    }

    /// Screen pixels of a device-pixel rectangle, clipped to the screen.
    fn capture_rect(&self, x: i32, y: i32, w: i32, h: i32, max_bytes: usize) -> Result<String, WindowControlError> {
        let (screen_w, screen_h) = self.screen_size();
        let (left, top) = (x.max(0), y.max(0));
        let (right, bottom) = ((x + w).min(screen_w), (y + h).min(screen_h));
        if right <= left || bottom <= top {
            return Err(WindowControlError::other("the window is off screen"));
        }
        let rgba = self.image_rgba(self.root, left, top, right - left, bottom - top)?;
        png_base64((right - left) as u32, (bottom - top) as u32, &rgba, max_bytes)
    }

    /// With a compositor every top-level window keeps its own off-screen copy, so the
    /// picture holds nothing that is in front of it (her own body included).
    fn composited(&self, window: Window, frame: (i32, i32, i32, i32)) -> Option<(i32, i32, Vec<u8>)> {
        self.conn.extension_information(composite::X11_EXTENSION_NAME).ok()??;
        self.conn.composite_query_version(0, 4).ok()?.reply().ok()?;
        let toplevel = self.toplevel(window);
        let geometry = self.conn.get_geometry(toplevel).ok()?.reply().ok()?;
        let origin = self.conn.translate_coordinates(toplevel, self.root, 0, 0).ok()?.reply().ok()?;
        let border = geometry.border_width as i32;
        let (outer_x, outer_y) = (origin.dst_x as i32 - border, origin.dst_y as i32 - border);
        let (outer_w, outer_h) = (geometry.width as i32 + 2 * border, geometry.height as i32 + 2 * border);
        let left = (frame.0 - outer_x).clamp(0, outer_w);
        let top = (frame.1 - outer_y).clamp(0, outer_h);
        let w = frame.2.min(outer_w - left);
        let h = frame.3.min(outer_h - top);
        if w <= 0 || h <= 0 {
            return None;
        }
        let pixmap = self.conn.generate_id().ok()?;
        // Fails with BadMatch when nothing redirects the window (no compositor).
        self.conn.composite_name_window_pixmap(toplevel, pixmap).ok()?.check().ok()?;
        let pixels = self.image_rgba(pixmap, left, top, w, h).ok();
        let _ = self.conn.free_pixmap(pixmap);
        let _ = self.conn.flush();
        pixels.map(|rgba| (w, h, rgba))
    }
}

pub(super) fn frontmost_window() -> Result<Option<ActiveWindow>, WindowControlError> {
    let x11 = X11::connect()?;
    Ok(x11.active_client().and_then(|window| x11.describe(window)))
}

pub(crate) fn find_window_for_pid(x11: &X11, pid: u32) -> Option<Window> {
    let desktop = x11.current_desktop();
    x11.clients()
        .into_iter()
        .find(|window| x11.pid(*window) == pid && x11.is_normal(*window, desktop))
}

pub(super) fn window_for_pid(pid: u32) -> Result<Option<ActiveWindow>, WindowControlError> {
    let x11 = X11::connect()?;
    Ok(find_window_for_pid(&x11, pid).and_then(|window| x11.describe(window)))
}

fn target_window(x11: &X11, target: &ActiveWindow) -> Option<Window> {
    if target.handle != 0 {
        let window = target.handle as Window;
        let pid = x11.pid(window);
        return (pid == target.pid || pid == 0).then_some(window);
    }
    find_window_for_pid(x11, target.pid)
}

pub(super) fn current_frame(target: &ActiveWindow) -> Result<Option<ActiveWindow>, WindowControlError> {
    let x11 = X11::connect()?;
    Ok(target_window(&x11, target).and_then(|window| x11.describe(window)))
}

pub(super) fn move_window(
    target: &ActiveWindow,
    x: i32,
    y: i32,
    overlay_pid: u32,
) -> Result<(), WindowControlError> {
    let x11 = X11::connect()?;
    let active_pid = x11.active_client().map(|window| x11.pid(window)).unwrap_or(0);
    if active_pid != target.pid && active_pid != overlay_pid {
        return Err(WindowControlError::other("target window is no longer active"));
    }
    let window = target_window(&x11, target)
        .ok_or_else(|| WindowControlError::other("target window is no longer available"))?;
    let states = x11.states(window);
    if [
        x11.atoms._NET_WM_STATE_MAXIMIZED_HORZ,
        x11.atoms._NET_WM_STATE_MAXIMIZED_VERT,
        x11.atoms._NET_WM_STATE_FULLSCREEN,
    ]
    .iter()
    .any(|state| states.contains(state))
    {
        return Err(WindowControlError::other("a maximised window stays where it is"));
    }
    let (frame_x, frame_y, _, _) = x11
        .frame(window)
        .ok_or_else(|| WindowControlError::other("target window has no frame"))?;
    let client = x11
        .conn
        .translate_coordinates(window, x11.root, 0, 0)
        .map_err(failed("could not read the window position"))?
        .reply()
        .map_err(failed("could not read the window position"))?;
    let scale = scale_factor();
    let client_x = client.dst_x as i32 + (x as f64 * scale).round() as i32 - frame_x;
    let client_y = client.dst_y as i32 + (y as f64 * scale).round() as i32 - frame_y;
    // StaticGravity (10): x/y place the client window itself. Bits 8 and 9: x and y are
    // given. Source 2: a pager or tool, which window managers honour like the user.
    let flags: u32 = 10 | (1 << 8) | (1 << 9) | (2 << 12);
    let event = ClientMessageEvent::new(
        32,
        window,
        x11.atoms._NET_MOVERESIZE_WINDOW,
        [flags, client_x as u32, client_y as u32, 0, 0],
    );
    x11.conn
        .send_event(
            false,
            x11.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(failed("could not move the window"))?;
    x11.conn.flush().map_err(failed("could not move the window"))?;
    Ok(())
}

pub(super) fn user_idle_seconds_if_known() -> Option<f64> {
    let x11 = X11::connect().ok()?;
    x11.conn
        .screensaver_query_info(x11.root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|info| info.ms_since_user_input as f64 / 1000.0)
}
pub(super) fn user_idle_seconds() -> f64 {
    user_idle_seconds_if_known().unwrap_or(0.0)
}

pub(super) fn capture_window(target: &ActiveWindow) -> Result<String, WindowControlError> {
    let x11 = X11::connect()?;
    let scale = scale_factor();
    let px = |value: i32| (value as f64 * scale).round() as i32;
    x11.capture_rect(px(target.x), px(target.y), px(target.w), px(target.h), MAX_SAMPLE_BYTES)
}

pub(super) fn capture_window_id(window_id: u32) -> Result<String, WindowControlError> {
    let x11 = X11::connect()?;
    let frame = x11
        .frame(window_id)
        .ok_or_else(|| WindowControlError::other("that window is gone"))?;
    if let Some((w, h, rgba)) = x11.composited(window_id, frame) {
        return png_base64(w as u32, h as u32, &rgba, MAX_SNAPSHOT_BYTES);
    }
    x11.capture_rect(frame.0, frame.1, frame.2, frame.3, MAX_SNAPSHOT_BYTES)
}

pub(super) fn visible_windows(own_pid: u32) -> Vec<WindowFrame> {
    let Ok(x11) = X11::connect() else {
        return Vec::new();
    };
    let desktop = x11.current_desktop();
    let scale = scale_factor();
    let mut windows = Vec::new();
    for window in x11.clients() {
        if !x11.is_normal(window, desktop) || x11.states(window).contains(&x11.atoms._NET_WM_STATE_ABOVE) {
            continue;
        }
        let pid = x11.pid(window);
        if pid == own_pid {
            continue;
        }
        let Some((x, y, w, h)) = x11.frame(window) else {
            continue;
        };
        let (x, y, w, h) = (x as f64 / scale, y as f64 / scale, w as f64 / scale, h as f64 / scale);
        if w < 80.0 || h < 60.0 {
            continue;
        }
        windows.push(WindowFrame { id: window, pid, app: x11.app(window, pid), x, y, w, h });
    }
    windows
}
