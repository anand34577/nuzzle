//! Keeping out of the user's way. The companion's window sits on top of everything, so wherever he stands he covers
//! what is underneath. This works out places he should not stand: where the text caret is, where the user last clicked
//! (normally the box they are typing in), and the pointer when it rests on him (they want to click what is behind him).
//! All coordinates are physical pixels on the virtual desktop.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

impl Rect {
    pub fn around(cx: f64, cy: f64, hx: f64, hy: f64) -> Self {
        Self { l: cx - hx, t: cy - hy, r: cx + hx, b: cy + hy }
    }

    /// Strict overlap: rectangles that only touch don't count.
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.l < o.r && self.r > o.l && self.t < o.b && self.b > o.t
    }

    pub fn inflate(&self, dx: f64, dy: f64) -> Rect {
        Rect { l: self.l - dx, t: self.t - dy, r: self.r + dx, b: self.b + dy }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.l && x <= self.r && y >= self.t && y <= self.b
    }

    #[cfg(test)]
    pub fn width(&self) -> f64 {
        self.r - self.l
    }
}

/// The smallest sideways move, within `[min_dx, max_dx]`, that takes `pet` clear of every zone. `Some(0.0)` when it
/// already is, `None` when there is no room.
pub fn clear_shift(pet: &Rect, zones: &[Rect], min_dx: f64, max_dx: f64) -> Option<f64> {
    if !zones.iter().any(|z| pet.overlaps(z)) {
        return Some(0.0);
    }
    let moved = |dx: f64| Rect { l: pet.l + dx, t: pet.t, r: pet.r + dx, b: pet.b };
    zones
        .iter()
        .flat_map(|z| [z.r - pet.l, z.l - pet.r]) // stand just right of it, or just left of it
        .filter(|dx| (min_dx..=max_dx).contains(dx))
        .filter(|&dx| !zones.iter().any(|z| moved(dx).overlaps(z)))
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// Notices when the pointer has stopped on the companion (rather than passing over him, petting or poking him).
#[derive(Default)]
pub struct Dwell {
    since: Option<Instant>,
    at: (f64, f64),
}

impl Dwell {
    /// Feed the pointer position and whether it is over the companion; returns how long it has stayed put there.
    pub fn update(&mut self, inside: bool, pos: (f64, f64), now: Instant) -> Duration {
        if !inside {
            self.since = None;
            return Duration::ZERO;
        }
        let moved = (pos.0 - self.at.0).abs() > 8.0 || (pos.1 - self.at.1).abs() > 8.0;
        if self.since.is_none() || moved {
            self.since = Some(now);
            self.at = pos;
        }
        self.since.map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }
}

/// How long a click keeps marking "where the user is working".
pub const CLICK_MEMORY: Duration = Duration::from_secs(300);

/// The zone around the last click outside the companion: a line of text either side of it and a little above and below.
pub fn click_zone(spot: Option<(i32, i32, Instant)>, sf: f64, now: Instant) -> Option<Rect> {
    let (x, y, at) = spot?;
    (now.saturating_duration_since(at) < CLICK_MEMORY).then(|| Rect::around(f64::from(x), f64::from(y), 90.0 * sf, 26.0 * sf))
}

/// The zone around the text caret, with room to read what is being typed.
pub fn caret_zone(caret: Rect, sf: f64) -> Rect {
    caret.inflate(28.0 * sf, 12.0 * sf)
}

/// Screen rectangle of the text caret in the foreground app, when that app exposes one. Many native apps do;
/// some (browsers, game-style UIs) don't, and then the last click stands in for it.
#[cfg(windows)]
pub fn caret_rect() -> Option<Rect> {
    use windows_sys::Win32::{
        Foundation::POINT,
        Graphics::Gdi::ClientToScreen,
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO},
    };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        let thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndCaret.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(info.hwndCaret, &mut pid);
        if pid == GetCurrentProcessId() {
            return None; // the caret is in my own typing box
        }
        let r = info.rcCaret;
        let (mut tl, mut br) = (POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom });
        if ClientToScreen(info.hwndCaret, &mut tl) == 0 || ClientToScreen(info.hwndCaret, &mut br) == 0 {
            return None;
        }
        if br.y <= tl.y {
            return None; // an empty rectangle: no visible caret
        }
        Some(Rect { l: f64::from(tl.x), t: f64::from(tl.y), r: f64::from(br.x.max(tl.x + 1)), b: f64::from(br.y) })
    }
}

#[cfg(not(windows))]
pub fn caret_rect() -> Option<Rect> {
    None
}

/// How long since the last keyboard or mouse input anywhere in the system. Only the timestamp is read, never which
/// keys were pressed.
#[cfg(windows)]
pub fn input_idle() -> Option<Duration> {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    };
    unsafe {
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        (GetLastInputInfo(&mut info) != 0).then(|| Duration::from_millis(u64::from(GetTickCount().wrapping_sub(info.dwTime))))
    }
}

#[cfg(target_os = "macos")]
pub fn input_idle() -> Option<Duration> {
    let secs = unsafe { mac::CGEventSourceSecondsSinceLastEventType(mac::COMBINED_SESSION, mac::ANY_INPUT) };
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

#[cfg(target_os = "linux")]
pub fn input_idle() -> Option<Duration> {
    x11::idle()
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn input_idle() -> Option<Duration> {
    None
}

/// Is the left mouse button held down right now (anywhere on screen)? Lets a press on the character be told from
/// a drag even though the window manager swallows the release while it moves the window.
#[cfg(windows)]
pub fn left_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON)) as u16 & 0x8000 != 0 }
}

#[cfg(target_os = "macos")]
pub fn left_button_down() -> bool {
    unsafe { mac::CGEventSourceButtonState(mac::COMBINED_SESSION, mac::LEFT_BUTTON) }
}

#[cfg(target_os = "linux")]
pub fn left_button_down() -> bool {
    x11::left_button_down()
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn left_button_down() -> bool {
    false
}

/// CoreGraphics input state. Reading it needs no special permission.
#[cfg(target_os = "macos")]
mod mac {
    pub const COMBINED_SESSION: i32 = 0; // kCGEventSourceStateCombinedSessionState
    pub const LEFT_BUTTON: u32 = 0; // kCGMouseButtonLeft
    pub const ANY_INPUT: u32 = !0; // kCGAnyInputEventType

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
        pub fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
}

/// X11 pointer state and idle time (also covers XWayland). Under pure Wayland these answer `None`/false and the
/// companion simply does without. Each thread keeps its own connection, opened on first use.
#[cfg(target_os = "linux")]
mod x11 {
    use std::time::Duration;
    use x11rb::{connection::Connection, protocol::xproto, rust_connection::RustConnection};

    thread_local! {
        static CONN: Option<(RustConnection, xproto::Window)> = x11rb::connect(None).ok().and_then(|(c, n)| {
            let root = c.setup().roots.get(n)?.root;
            Some((c, root))
        });
    }

    pub fn left_button_down() -> bool {
        CONN.with(|c| {
            let Some((c, root)) = c else { return false };
            xproto::ConnectionExt::query_pointer(c, *root)
                .ok()
                .and_then(|r| r.reply().ok())
                .is_some_and(|r| u16::from(r.mask) & u16::from(xproto::KeyButMask::BUTTON1) != 0)
        })
    }

    pub fn idle() -> Option<Duration> {
        CONN.with(|c| {
            let (c, root) = c.as_ref()?;
            let r = x11rb::protocol::screensaver::ConnectionExt::screensaver_query_info(c, *root).ok()?.reply().ok()?;
            Some(Duration::from_millis(u64::from(r.ms_since_user_input)))
        })
    }
}

/// Typing (or at least keyboard-driven work): the user is active, but the pointer has been still for a moment.
pub fn is_typing(input_idle: Option<Duration>, pointer_still: Duration) -> bool {
    input_idle.is_some_and(|i| i < Duration::from_millis(2500)) && pointer_still > Duration::from_millis(1500)
}

/// The area around the pointer the companion should not choose to stand in while the user is moving it about or
/// working beside it. It only steers where he walks to: he never runs away from a pointer that comes to him.
pub fn personal_space(pointer: (f64, f64), sf: f64) -> Rect {
    Rect::around(pointer.0, pointer.1, 260.0 * sf, 190.0 * sf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(l: f64, t: f64, rr: f64, b: f64) -> Rect {
        Rect { l, t, r: rr, b }
    }

    #[test]
    fn overlap_ignores_touching_edges() {
        assert!(r(0.0, 0.0, 10.0, 10.0).overlaps(&r(5.0, 5.0, 15.0, 15.0)));
        assert!(!r(0.0, 0.0, 10.0, 10.0).overlaps(&r(10.0, 0.0, 20.0, 10.0)));
        assert!(!r(0.0, 0.0, 10.0, 10.0).overlaps(&r(0.0, 20.0, 10.0, 30.0)));
    }

    #[test]
    fn already_clear_needs_no_move() {
        assert_eq!(clear_shift(&r(0.0, 0.0, 100.0, 200.0), &[r(300.0, 0.0, 400.0, 50.0)], -1000.0, 1000.0), Some(0.0));
        assert_eq!(clear_shift(&r(0.0, 0.0, 100.0, 200.0), &[], -1000.0, 1000.0), Some(0.0));
    }

    #[test]
    fn steps_to_the_nearest_free_side() {
        let pet = r(100.0, 0.0, 200.0, 200.0);
        let zone = r(180.0, 50.0, 260.0, 100.0);
        // Nearest is leftwards: the pet's right edge must reach the zone's left edge (180): shift -20.
        assert_eq!(clear_shift(&pet, &[zone], -1000.0, 1000.0), Some(-20.0));
        // With no room on the left it goes right: left edge to 260, shift +160.
        assert_eq!(clear_shift(&pet, &[zone], -5.0, 1000.0), Some(160.0));
        // No room either way.
        assert_eq!(clear_shift(&pet, &[zone], -5.0, 10.0), None);
    }

    #[test]
    fn a_shift_that_lands_in_another_zone_is_skipped() {
        let pet = r(100.0, 0.0, 200.0, 200.0);
        let (a, b) = (r(180.0, 0.0, 260.0, 100.0), r(-50.0, 0.0, 99.0, 100.0));
        // Going left would land inside `b`; so it has to go right of `a`.
        assert_eq!(clear_shift(&pet, &[a, b], -1000.0, 1000.0), Some(160.0));
    }

    #[test]
    fn zones_above_the_character_do_not_matter() {
        // A text box near the top of the screen; the character stands at the bottom.
        let pet = r(100.0, 800.0, 200.0, 1000.0);
        assert_eq!(clear_shift(&pet, &[Rect::around(150.0, 100.0, 90.0, 26.0)], -500.0, 500.0), Some(0.0));
    }

    #[test]
    fn resting_pointer_is_noticed_but_moving_or_leaving_resets() {
        let mut d = Dwell::default();
        let t0 = Instant::now();
        assert_eq!(d.update(true, (100.0, 100.0), t0), Duration::ZERO);
        assert_eq!(d.update(true, (102.0, 101.0), t0 + Duration::from_millis(900)), Duration::from_millis(900));
        // petting: moving back and forth restarts the clock
        assert_eq!(d.update(true, (160.0, 101.0), t0 + Duration::from_millis(1000)), Duration::ZERO);
        // leaving forgets it
        assert_eq!(d.update(false, (900.0, 900.0), t0 + Duration::from_millis(1100)), Duration::ZERO);
        assert_eq!(d.update(true, (160.0, 101.0), t0 + Duration::from_millis(1200)), Duration::ZERO);
    }

    #[test]
    fn clicks_mark_a_work_spot_for_a_while() {
        let t0 = Instant::now();
        let spot = Some((500, 400, t0));
        let z = click_zone(spot, 1.5, t0 + Duration::from_secs(10)).unwrap();
        assert!(z.contains(500.0, 400.0) && z.width() > 200.0);
        assert!(click_zone(spot, 1.5, t0 + CLICK_MEMORY + Duration::from_secs(1)).is_none());
        assert!(click_zone(None, 1.0, t0).is_none());
        let c = caret_zone(r(300.0, 200.0, 302.0, 220.0), 1.0);
        assert!(c.l < 300.0 && c.r > 302.0 && c.t < 200.0 && c.b > 220.0);
    }

    #[test]
    fn typing_means_recent_input_with_a_still_pointer() {
        let (recent, long, still) = (Some(Duration::from_millis(400)), Some(Duration::from_secs(30)), Duration::from_secs(3));
        assert!(is_typing(recent, still));
        assert!(!is_typing(recent, Duration::from_millis(300))); // the pointer is moving: not typing
        assert!(!is_typing(long, still)); // nothing pressed lately
        assert!(!is_typing(None, still));
    }

    #[test]
    fn personal_space_surrounds_the_pointer() {
        let z = personal_space((500.0, 400.0), 1.0);
        assert!(z.contains(500.0, 400.0) && z.contains(700.0, 500.0) && !z.contains(900.0, 400.0));
    }

    #[test]
    fn input_idle_never_panics() {
        let _ = input_idle();
    }

    #[test]
    fn caret_lookup_never_panics() {
        let _ = caret_rect(); // may be None or a rect depending on what has focus; it must simply not crash
    }
}
