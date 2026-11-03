//! Makes the companion wander along the bottom of the screen and hop onto the top edges of
//! open windows. It stays put while a conversation is going on, gives way if dragged, and keeps out of the user's
//! way: never standing on the text caret, the spot last clicked, or the pointer when it rests on him (see `avoid`).
//! Like a real little creature he falls when you let go of him in mid-air (landing on a window edge or the ground),
//! runs over when called, and now and then gets the zoomies.

use rand::Rng;
use serde_json::json;
use std::{
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant},
};
use tauri::{Manager, PhysicalPosition};

use crate::{
    audio::now_ms,
    avoid::{self, Dwell, Rect},
    settings, Core, LockExt,
};

struct Perch {
    x: i32,
    y: i32,
    w: i32,
}

fn perches(own_title: &str, area: (i32, i32, i32, i32), min_top: i32) -> Vec<Perch> {
    let Ok(all) = xcap::Window::all() else { return vec![] };
    all.into_iter()
        .filter_map(|w| {
            if w.is_minimized().ok()? || w.title().ok()?.is_empty() || w.title().ok()? == own_title {
                return None;
            }
            let (x, y) = (w.x().ok()?, w.y().ok()?);
            let (ww, wh) = (w.width().ok()? as i32, w.height().ok()? as i32);
            let inside = x < area.2 && x + ww > area.0 && y >= min_top && y < area.3 - 120;
            (ww >= 300 && wh >= 150 && inside).then_some(Perch { x, y, w: ww })
        })
        .collect()
}

/// Where the feet come down when let go of at (x, y): the top edge of a window right below, or the ground. Returns the
/// window position to land at and the perch (window top) if it is one.
fn landing(win: &tauri::WebviewWindow, core: &Core, x: f64, y: f64) -> Option<(f64, Option<i32>)> {
    let (size, mon) = (win.outer_size().ok()?, win.current_monitor().ok()??);
    let sf = win.scale_factor().unwrap_or(1.0);
    let (ww, wh) = (size.width as i32, size.height as i32);
    let wa = mon.work_area();
    let mut area = (wa.position.x, wa.position.y, wa.position.x + wa.size.width as i32, wa.position.y + wa.size.height as i32);
    if let Some(a) = core.settings.lk().roam_area {
        area = (a.x, a.y, a.x + a.w as i32, a.y + a.h as i32);
    }
    let pad = (8.0 * sf) as i32;
    let ground = f64::from(area.3 - wh + pad);
    let feet = y as i32 + wh - pad;
    let centre = x as i32 + ww / 2;
    let title = win.title().unwrap_or_default();
    let perch = perches(&title, area, area.1)
        .into_iter()
        .filter(|p| p.y >= feet - 2 && centre > p.x + 20 && centre < p.x + p.w - 20)
        .min_by_key(|p| p.y);
    Some(match perch {
        Some(p) if f64::from(p.y - wh + pad) < ground => (f64::from(p.y - wh + pad), Some(p.y)),
        _ => (ground, None),
    })
}

pub fn spawn(core: Arc<Core>) {
    std::thread::spawn(move || run(core));
}

fn run(core: Arc<Core>) {
    let mut rng = rand::rng();
    let win = loop {
        if let Some(w) = core.app.get_webview_window("main") {
            break w;
        }
        sleep(Duration::from_millis(100));
    };
    sleep(Duration::from_millis(600));

    // The page only needs to hear when something changed, not 30 times a second.
    let last_move = std::cell::Cell::new((false, 1, false, false));
    let emit_move = |walking: bool, dir: i32, hop: bool, run: bool| {
        if last_move.replace((walking, dir, hop, run)) != (walking, dir, hop, run) {
            core.emit("move", json!({"walking": walking, "dir": dir, "hop": hop, "run": run}))
        }
    };

    let (mut x, mut y, mut last_set) = {
        let p = win.outer_position().unwrap_or(PhysicalPosition::new(200, 200));
        (p.x as f64, p.y as f64, (p.x, p.y))
    };
    let mut dest: Option<(f64, f64)> = None;
    let mut walking = false;
    let mut next_decision = Instant::now() + Duration::from_secs(2);
    let mut paused_until = Instant::now();
    let mut feet_y: Option<i32> = None; // set while perched on a window edge
    let (mut dragging, mut drag_last) = (false, Instant::now());
    let mut last_perch_check = Instant::now();
    // Keeping out of the way.
    let mut dwell = Dwell::default();
    let mut zones: Vec<Rect> = vec![];
    let mut last_zone_check = Instant::now() - Duration::from_secs(1);
    let mut next_dodge = Instant::now();
    let mut last_dodge_line = Instant::now() - Duration::from_secs(120);
    let mut dodging = false;
    let mut last_cursor = (0.0, 0.0);
    let mut cursor_moved = Instant::now() - Duration::from_secs(60);
    // Falling after being let go of: (speed in px/s, where the feet land, the perch landed on if any).
    let mut falling: Option<(f64, f64, Option<i32>)> = None;
    // Running: to the user when called, or the zoomies.
    let (mut summoned, mut running) = (false, false);

    loop {
        sleep(Duration::from_millis(33));

        // Dragged by the user? Adopt the new spot and leave them alone for a while.
        if let Ok(p) = win.outer_position() {
            if (p.x - last_set.0).abs() > 2 || (p.y - last_set.1).abs() > 2 {
                if !dragging {
                    dragging = true;
                    core.emit("drag", json!(true));
                }
                drag_last = Instant::now();
                x = p.x as f64;
                y = p.y as f64;
                last_set = (p.x, p.y);
                dest = None;
                dodging = false;
                feet_y = None;
                paused_until = Instant::now() + Duration::from_secs(20);
            }
        }

        if dragging && drag_last.elapsed() > Duration::from_millis(350) {
            dragging = false;
            core.emit("drag", json!(false));
            // Let go of in mid-air: fall to the window edge or ground below.
            if let Some(f) = landing(&win, &core, x, y) {
                if f.0 > y + 4.0 {
                    falling = Some((0.0, f.0, f.1));
                    core.emit("fall", json!(true));
                }
            }
        }
        if let Some((vy, target, perch)) = falling.as_mut() {
            let sf = win.scale_factor().unwrap_or(1.0);
            *vy += 2600.0 * sf * 0.033;
            y = (y + *vy * 0.033).min(*target);
            last_set = (x as i32, y as i32);
            let _ = win.set_position(PhysicalPosition::new(last_set.0, last_set.1));
            if y >= *target {
                core.emit("land", json!({"speed": *vy / sf}));
                feet_y = *perch;
                falling = None;
                paused_until = Instant::now() + Duration::from_secs(4);
            }
            continue;
        }
        if core.summon.swap(false, std::sync::atomic::Ordering::SeqCst) {
            summoned = true;
            dest = None;
            paused_until = Instant::now();
        }

        // Stay exactly where I am while anything is going on (thinking, speaking, hearing, typing, approvals),
        // and for a few seconds after, so I don't wander off mid-conversation.
        let busy = core.is_engaged() || now_ms().saturating_sub(core.last_activity.load(std::sync::atomic::Ordering::Relaxed)) < 5000;
        let parked = !core.roam.load(std::sync::atomic::Ordering::Relaxed) || !win.is_visible().unwrap_or(true);
        if !summoned && (busy || parked || Instant::now() < paused_until) {
            dodging = false;
            if walking {
                walking = false;
                dest = None;
                running = false;
                emit_move(false, 1, false, false);
            }
            continue;
        }

        let sf = win.scale_factor().unwrap_or(1.0);
        let scale = sf * f64::from(core.pet_pct.load(std::sync::atomic::Ordering::Relaxed)) / 100.0; // bigger = quicker strides
        let (Ok(size), Ok(Some(mon))) = (win.outer_size(), win.current_monitor()) else { continue };
        let (ww, wh) = (size.width as i32, size.height as i32);
        let wa = mon.work_area();
        let area = (
            wa.position.x,
            wa.position.y,
            wa.position.x + wa.size.width as i32,
            wa.position.y + wa.size.height as i32,
        );
        // A user-defined roaming area replaces the whole work area.
        let area = core.settings.lk().roam_area.map_or(area, |a| (a.x, a.y, a.x + a.w as i32, a.y + a.h as i32));
        let pad = (8.0 * sf) as i32; // the window reaches this far below the work area's bottom edge
        let ground_y = (area.3 - wh + pad) as f64;
        let min_x = area.0 as f64;
        let max_x = (area.2 - ww).max(area.0) as f64;

        // ---- keep out of the user's way ----
        let avoid_on = core.settings.lk().avoid_cursor;
        let f = f64::from(core.pet_pct.load(std::sync::atomic::Ordering::Relaxed)) / 100.0;
        let pet_at = |px: f64, py: f64| {
            let (l, t, r, b) = settings::pet_rect(px, py, f, sf);
            Rect { l, t, r, b }
        };
        let now = Instant::now();
        let cursor = core.app.cursor_position().ok().map(|c| (c.x, c.y));
        if let Some(c) = cursor {
            if (c.0 - last_cursor.0).abs() > 4.0 || (c.1 - last_cursor.1).abs() > 4.0 {
                last_cursor = c;
                cursor_moved = now;
            }
        }
        let typing = avoid::is_typing(avoid::input_idle(), now.saturating_duration_since(cursor_moved));
        // While the pointer is on the move, or the user is typing, don't pick spots near their work or the pointer.
        let pointer_busy = now.saturating_duration_since(cursor_moved) < Duration::from_millis(3000) || typing;
        if !avoid_on {
            zones.clear();
            dodging = false;
        } else if last_zone_check.elapsed() > Duration::from_millis(100) {
            last_zone_check = now;
            zones.clear();
            if let Some(c) = avoid::caret_rect() {
                zones.push(avoid::caret_zone(c, sf));
            }
            if let Some(z) = avoid::click_zone(*core.click_spot.lk(), sf, now) {
                zones.push(if typing { z.inflate(60.0 * sf, 30.0 * sf) } else { z });
            }
        }
        // Where I may choose to stand: clear of what the user is working on, and of the pointer while it is busy.
        let personal = cursor.filter(|_| avoid_on && pointer_busy).map(|c| avoid::personal_space(c, sf));
        let wide = |tx: f64, ty: f64| {
            let me = pet_at(tx, ty);
            !zones.iter().any(|z| me.overlaps(&z.inflate(100.0 * sf, 60.0 * sf))) && !personal.is_some_and(|p| me.overlaps(&p))
        };
        // The pointer resting on me (not petting or poking: those move or press) means they want what is behind me.
        let touched = now_ms().saturating_sub(core.last_pet_touch.load(std::sync::atomic::Ordering::Relaxed)) < 10_000;
        let rest = match (avoid_on, cursor) {
            (true, Some(c)) => dwell.update(pet_at(x, y).inflate(4.0, 4.0).contains(c.0, c.1), c, now),
            _ => Duration::ZERO,
        };
        let mut hot = zones.clone();
        if let (true, false, Some(c)) = (rest > Duration::from_millis(900), touched || dragging, cursor) {
            hot.push(Rect::around(c.0, c.1, 30.0 * sf, 30.0 * sf));
        }
        if avoid_on && now >= next_dodge && !dodging && hot.iter().any(|z| pet_at(x, y).overlaps(z)) {
            if let Some(dx) = avoid::clear_shift(&pet_at(x, y), &hot, min_x - x, max_x - x).filter(|d| d.abs() > 1.0) {
                // A little hop to the side instead of a slow shuffle.
                dest = Some((x + dx, y));
                dodging = true;
                next_dodge = now + Duration::from_millis(1500);
                if last_dodge_line.elapsed() > Duration::from_secs(90) {
                    last_dodge_line = now;
                    let kind = if zones.iter().any(|z| pet_at(x, y).overlaps(z)) { "text" } else { "pointer" };
                    core.emit("dodge", json!({"kind": kind}));
                }
            }
        }

        // Sitting on a window edge: if that window closes or moves, drop to the ground within a second
        // instead of floating until the next decision.
        if let (None, Some(fy)) = (dest, feet_y) {
            if last_perch_check.elapsed() > Duration::from_secs(1) {
                last_perch_check = Instant::now();
                let title = win.title().unwrap_or_default();
                if !perches(&title, area, area.1 + wh - pad).iter().any(|p| (p.y - fy).abs() < 6) {
                    feet_y = None;
                    dest = Some((x.clamp(min_x, max_x), ground_y));
                }
            }
        }

        if summoned && dest.is_none() {
            // Called: run to the ground below the pointer (or stay put if it is already right here).
            let cx = cursor.map_or(x, |c| c.0 - f64::from(ww) / 2.0).clamp(min_x, max_x);
            feet_y = None;
            dest = Some((cx, ground_y));
            running = true;
            core.emit("summoned", json!(null));
        }

        if dest.is_none() {
            if Instant::now() < next_decision {
                continue;
            }
            let title = win.title().unwrap_or_default();
            let spots = perches(&title, area, area.1 + wh - pad);
            // A perch that vanished or moved: fall back to the ground.
            let still_ok = feet_y.is_none_or(|fy| spots.iter().any(|p| (p.y - fy).abs() < 6));
            if !still_ok {
                feet_y = None;
                dest = Some((x.clamp(min_x, max_x), ground_y));
            } else if !spots.is_empty() && rng.random_bool(0.45) {
                let p = &spots[rng.random_range(0..spots.len())];
                let lo = p.x.max(area.0);
                let hi = (p.x + p.w - ww).max(lo + 1).min(area.2 - ww);
                let ty = (p.y - wh + pad) as f64;
                // A few tries for a spot on this edge that isn't on top of what the user is working on.
                let tx = (0..6)
                    .map(|_| rng.random_range(lo.min(hi)..=hi.max(lo)) as f64)
                    .find(|&tx| wide(tx, ty));
                match tx {
                    Some(tx) => {
                        feet_y = Some(p.y);
                        dest = Some((tx, ty));
                    }
                    None => next_decision = Instant::now() + Duration::from_secs(3),
                }
            } else {
                feet_y = None;
                let tx = (0..10)
                    .map(|_| rng.random_range(min_x..=max_x.max(min_x + 1.0)))
                    .find(|&tx| wide(tx, ground_y));
                // Now and then, out of nowhere: the zoomies (a quick dash, only when the way is long enough).
                running = tx.is_some_and(|tx| (tx - x).abs() > 300.0 * scale) && rng.random_bool(0.12);
                match tx {
                    Some(tx) => dest = Some((tx, ground_y)),
                    None => next_decision = Instant::now() + Duration::from_secs(3),
                }
            }
        }

        if let Some((tx, ty)) = dest {
            let (dx, dy) = (tx - x, ty - y);
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 3.0 {
                dest = None;
                dodging = false;
                walking = false;
                if summoned {
                    summoned = false;
                    core.emit("arrived", json!(null));
                    next_decision = Instant::now() + Duration::from_secs(20); // stay with them a while
                    paused_until = next_decision;
                }
                running = false;
                next_decision = Instant::now() + Duration::from_secs(rng.random_range(5..14));
                emit_move(false, 1, false, false);
            } else {
                let hop = dodging || dy.abs() > 40.0 * scale;
                let speed = if dodging { 420.0 } else if running { 400.0 } else { 150.0 };
                let step = (speed * scale * 0.033).min(dist);
                x += dx / dist * step;
                y += dy / dist * step;
                if !walking {
                    walking = true;
                }
                emit_move(true, if dx >= 0.0 { 1 } else { -1 }, hop, running);
                last_set = (x as i32, y as i32);
                let _ = win.set_position(PhysicalPosition::new(last_set.0, last_set.1));
            }
        }
    }
}
