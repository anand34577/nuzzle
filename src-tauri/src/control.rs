use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread::sleep,
    time::Duration,
};

use crate::screen::Geom;

pub const TOOK_OVER: &str = "the user moved the mouse, so control was handed back";

pub enum Action {
    Move { x: f64, y: f64 },
    Click { x: f64, y: f64, button: String, count: u32 },
    Scroll { dir: String, amount: i32, at: Option<(f64, f64)> },
    SelectAll,
    Drag { x1: f64, y1: f64, x2: f64, y2: f64 },
    Type { text: String },
    Key { combo: String },
}

struct Hand {
    e: Enigo,
    stop: Arc<AtomicU64>,
    gen: u64,
    geom: Geom,
}

type R<T = ()> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

impl Hand {
    fn alive(&self) -> R {
        if self.stop.load(Ordering::SeqCst) != self.gen {
            Err("stopped by the user".into())
        } else {
            Ok(())
        }
    }

    /// Glide the pointer; if the real pointer ever strays from where we put it, the user
    /// has grabbed the mouse and we abort immediately.
    fn goto(&mut self, tx: i32, ty: i32) -> R {
        let (cx, cy) = self.e.location().map_err(err)?;
        let dist = (((tx - cx).pow(2) + (ty - cy).pow(2)) as f64).sqrt();
        // Quick but still a visible glide: the user can see where it is going and grab the mouse to stop it.
        let steps = (dist / 30.0).clamp(6.0, 24.0) as i32;
        for i in 1..=steps {
            self.alive()?;
            let t = i as f64 / steps as f64;
            let t = t * t * (3.0 - 2.0 * t);
            let x = cx + ((tx - cx) as f64 * t) as i32;
            let y = cy + ((ty - cy) as f64 * t) as i32;
            self.e.move_mouse(x, y, Coordinate::Abs).map_err(err)?;
            sleep(Duration::from_millis(8));
            let (ax, ay) = self.e.location().map_err(err)?;
            if (ax - x).abs() > 30 || (ay - y).abs() > 30 {
                self.stop.fetch_add(1, Ordering::SeqCst);
                return Err(TOOK_OVER.into());
            }
        }
        self.e.move_mouse(tx, ty, Coordinate::Abs).map_err(err)?;
        sleep(Duration::from_millis(40));
        Ok(())
    }

    fn pt(&self, x: f64, y: f64) -> (i32, i32) {
        self.geom.map(x, y)
    }

    /// Just before pressing anything: is the pointer still where we left it?
    fn verify_at(&mut self, tx: i32, ty: i32) -> R {
        self.alive()?;
        let (ax, ay) = self.e.location().map_err(err)?;
        if (ax - tx).abs() > 12 || (ay - ty).abs() > 12 {
            self.stop.fetch_add(1, Ordering::SeqCst);
            return Err(TOOK_OVER.into());
        }
        Ok(())
    }

    fn chord(&mut self, combo: &str) -> R {
        let mut mods = vec![];
        let mut main = None;
        for part in combo.split('+').map(|p| p.trim().to_lowercase()) {
            match part.as_str() {
                "ctrl" | "control" => mods.push(Key::Control),
                "shift" => mods.push(Key::Shift),
                "alt" | "option" => mods.push(Key::Alt),
                "cmd" | "meta" | "win" | "super" => mods.push(Key::Meta),
                other => main = Some(parse_key(other)?),
            }
        }
        let main = main.ok_or("no key in combo")?;
        for m in &mods {
            self.e.key(*m, Direction::Press).map_err(err)?;
        }
        let res = self.e.key(main, Direction::Click).map_err(err);
        for m in mods.iter().rev() {
            let _ = self.e.key(*m, Direction::Release);
        }
        res
    }
}

fn parse_key(k: &str) -> R<Key> {
    Ok(match k {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "space" => Key::Space,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap_or(' ')),
        other => return Err(format!("unknown key '{other}'")),
    })
}

/// Blocking; run on a blocking thread. Returns a short human-readable result.
pub fn run(stop: Arc<AtomicU64>, gen: u64, geom: Geom, action: Action) -> R<String> {
    let e = Enigo::new(&Settings::default()).map_err(err)?;
    let mut h = Hand { e, stop, gen, geom };
    h.alive()?;
    match action {
        Action::Move { x, y } => {
            let (px, py) = h.pt(x, y);
            h.goto(px, py)?;
            Ok("moved".into())
        }
        Action::Click { x, y, button, count } => {
            let (px, py) = h.pt(x, y);
            h.goto(px, py)?;
            let b = match button.as_str() {
                "right" => Button::Right,
                "middle" => Button::Middle,
                _ => Button::Left,
            };
            for _ in 0..count.clamp(1, 3) {
                h.verify_at(px, py)?;
                h.e.button(b, Direction::Click).map_err(err)?;
                sleep(Duration::from_millis(70));
            }
            Ok(format!("clicked x{count}"))
        }
        Action::Scroll { dir, amount, at } => {
            if let Some((x, y)) = at {
                let (px, py) = h.pt(x, y);
                h.goto(px, py)?;
            }
            let (axis, sign) = match dir.as_str() {
                "up" => (Axis::Vertical, -1),
                "left" => (Axis::Horizontal, -1),
                "right" => (Axis::Horizontal, 1),
                _ => (Axis::Vertical, 1),
            };
            for _ in 0..amount.clamp(1, 20) {
                h.alive()?;
                h.e.scroll(sign, axis).map_err(err)?;
                sleep(Duration::from_millis(60));
            }
            Ok("scrolled".into())
        }
        Action::SelectAll => {
            h.chord(if cfg!(target_os = "macos") { "cmd+a" } else { "ctrl+a" })?;
            Ok("selected all".into())
        }
        Action::Drag { x1, y1, x2, y2 } => {
            let (ax, ay) = h.pt(x1, y1);
            let (bx, by) = h.pt(x2, y2);
            h.goto(ax, ay)?;
            h.verify_at(ax, ay)?;
            h.e.button(Button::Left, Direction::Press).map_err(err)?;
            let res = h.goto(bx, by);
            let _ = h.e.button(Button::Left, Direction::Release);
            res?;
            Ok("dragged".into())
        }
        Action::Type { text } => {
            for ch in text.chars().take(4000) {
                h.alive()?;
                h.e.text(&ch.to_string()).map_err(err)?;
                sleep(Duration::from_millis(12));
            }
            Ok("typed".into())
        }
        Action::Key { combo } => {
            h.chord(&combo)?;
            Ok(format!("pressed {combo}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_single_keys() {
        assert!(matches!(parse_key("enter"), Ok(Key::Return)));
        assert!(matches!(parse_key("f5"), Ok(Key::F5)));
        assert!(matches!(parse_key("a"), Ok(Key::Unicode('a'))));
        assert!(parse_key("banana").is_err());
    }
}
