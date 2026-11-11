//! The morning hello: the first time I see you each morning I say what is on today (reminders) and what the weather
//! is like at home. Only when there is something to say, never in quiet mode, and only once a day. `/today` gives
//! the same summary any time.

use serde_json::Value;
use std::{sync::Arc, time::Duration};

use crate::{reminders, sys, tools, Core, LockExt};

/// "2026-11-11", to remember which day was already greeted.
fn day_key(t: &sys::LocalTime) -> String {
    format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
}

/// The reminders that come due before midnight, soonest first, as one sentence ("" when there are none).
fn reminders_line(list: &[reminders::Reminder], now: u64, secs_left_today: u64) -> String {
    let mut today: Vec<&reminders::Reminder> = list.iter().filter(|r| r.due.saturating_sub(now) <= secs_left_today).collect();
    today.sort_by_key(|r| r.due);
    if today.is_empty() {
        return String::new();
    }
    let items: Vec<String> = today.iter().take(4).map(|r| format!("{} in {}", r.text, reminders::describe(r.due.saturating_sub(now)))).collect();
    let more = if today.len() > 4 { format!(", and {} more", today.len() - 4) } else { String::new() };
    format!("You have {} today: {}{more}.", if today.len() == 1 { "one reminder".to_string() } else { format!("{} reminders", today.len()) }, items.join("; "))
}

/// The forecast as one spoken sentence, from the `weather` tool's answer.
fn weather_line(w: &Value) -> String {
    let n = |v: &Value| v.as_f64().map(|x| x.round() as i64);
    let place = w["place"].as_str().unwrap_or("").split(',').next().unwrap_or("").trim().to_string();
    let Some(temp) = n(&w["now"]["temp_c"]) else { return String::new() };
    let mut s = format!("In {place} it's {temp} degrees with {}", w["now"]["sky"].as_str().unwrap_or("mixed weather"));
    let d = &w["next_days"][0];
    if let (Some(hi), Some(lo)) = (n(&d["high_c"]), n(&d["low_c"])) {
        s.push_str(&format!(", {lo} to {hi} today"));
    }
    match n(&d["rain_chance_pct"]) {
        Some(p) if p >= 50 => s.push_str(&format!(". Take an umbrella: {p}% chance of rain")),
        Some(p) if p >= 20 => s.push_str(&format!(", {p}% chance of rain")),
        _ => {}
    }
    s + "."
}

/// What is on today: (the reminders sentence, the weather sentence), either may be empty.
async fn parts(core: &Arc<Core>) -> (String, String) {
    let t = sys::local_now();
    let left = u64::from(86_400 - (t.hour * 3600 + t.minute * 60 + t.second).min(86_399));
    let rem = reminders_line(&core.reminders.lk(), sys::unix_now(), left);
    let place = core.settings.lk().home_place.trim().to_string();
    let weather = if place.is_empty() { String::new() } else { tools::weather(core, &place).await.map(|w| weather_line(&w)).unwrap_or_default() };
    (rem, weather)
}

/// The summary for `/today` (always says something).
pub async fn today(core: &Arc<Core>) -> String {
    let (rem, weather) = parts(core).await;
    let date = sys::format_local(&sys::local_now());
    let rem = if rem.is_empty() { "No reminders left for today.".to_string() } else { rem };
    let hint = if weather.is_empty() && core.settings.lk().home_place.trim().is_empty() { "\nTell me your town in Settings -> Character and I'll add the weather." } else { "" };
    format!("It's {date}.\n{rem}{}{hint}", if weather.is_empty() { String::new() } else { format!("\n{weather}") })
}

pub fn spawn(core: Arc<Core>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await; // let the character arrive and say hi first
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let (on, quiet, done) = {
                let s = core.settings.lk();
                (s.morning_hello, s.playfulness == "quiet", s.hello_day.clone())
            };
            let t = sys::local_now();
            if !on || quiet || !(5..12).contains(&t.hour) || done == day_key(&t) {
                continue;
            }
            // Only when you are here (touched the mouse or keyboard in the last two minutes) and I'm not busy.
            let here = crate::avoid::input_idle().is_none_or(|d| d < Duration::from_secs(120));
            let shown = tauri::Manager::get_webview_window(&core.app, "main").is_some_and(|w| w.is_visible().unwrap_or(false));
            if !here || !shown || core.is_engaged() {
                continue;
            }
            let mut s = core.settings.lk().clone();
            s.hello_day = day_key(&t);
            if core.save_settings_now(s).is_err() {
                continue; // can't remember having said it: better silent than repeating all morning
            }
            let (rem, weather) = parts(&core).await;
            if rem.is_empty() && weather.is_empty() {
                continue; // nothing worth interrupting for
            }
            let text = ["Good morning!", &weather, &rem].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ");
            core.touch();
            core.transcript_add("assistant", &text);
            core.emit("quick", serde_json::json!(text));
            crate::brain::speak(&core, &text);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn todays_reminders_are_listed_soonest_first() {
        let mut l = vec![];
        reminders::add(&mut l, 1000, 7200, "call mum").unwrap();
        reminders::add(&mut l, 1000, 600, "tea").unwrap();
        reminders::add(&mut l, 1000, 200_000, "next week thing").unwrap();
        assert_eq!(reminders_line(&l, 1000, 36_000), "You have 2 reminders today: tea in 10 minutes; call mum in 2 hours.");
        assert_eq!(reminders_line(&l, 1000, 900), "You have one reminder today: tea in 10 minutes.");
        assert_eq!(reminders_line(&[], 1000, 900), "");
    }

    #[test]
    fn weather_reads_naturally() {
        let w = json!({"place": "Pune, India", "now": {"sky": "partly cloudy", "temp_c": 23.6},
            "next_days": [{"high_c": 29.2, "low_c": 17.8, "rain_chance_pct": 60}]});
        assert_eq!(weather_line(&w), "In Pune it's 24 degrees with partly cloudy, 18 to 29 today. Take an umbrella: 60% chance of rain.");
        let dry = json!({"place": "Oslo, Norway", "now": {"sky": "clear sky", "temp_c": -2.2}, "next_days": [{"high_c": 1.0, "low_c": -5.0, "rain_chance_pct": 5}]});
        assert_eq!(weather_line(&dry), "In Oslo it's -2 degrees with clear sky, -5 to 1 today.");
        assert_eq!(weather_line(&json!({})), "");
        assert_eq!(day_key(&sys::from_unix_utc(1_793_631_845)), "2026-11-02");
    }
}
