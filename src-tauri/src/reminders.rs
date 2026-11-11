//! Reminders and timers. They survive restarts; when one is due the companion speaks it.

use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

pub const MAX_REMINDERS: usize = 50;
pub const MAX_TEXT: usize = 200;
/// A year, so a typo can't schedule something forgotten for decades.
pub const MAX_SECS: u64 = 366 * 86_400;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reminder {
    pub id: u64,
    /// Unix seconds.
    pub due: u64,
    pub text: String,
    /// Seconds between repeats; 0 = said once.
    #[serde(default)]
    pub every: u64,
}

/// Shortest gap between repeats, so a slip can't make me nag every few seconds.
pub const MIN_EVERY: u64 = 60;

pub fn load(path: &Path) -> Vec<Reminder> {
    fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(path: &Path, list: &[Reminder]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(list).map_err(io::Error::other)?)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
pub fn add(list: &mut Vec<Reminder>, now: u64, in_secs: u64, text: &str) -> Result<Reminder, String> {
    add_repeating(list, now, in_secs, 0, text)
}

/// Like `add`, repeating every `every` seconds after the first time (0 = once).
pub fn add_repeating(list: &mut Vec<Reminder>, now: u64, in_secs: u64, every: u64, text: &str) -> Result<Reminder, String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err("What should I remind you about?".into());
    }
    if text.chars().count() > MAX_TEXT {
        return Err(format!("Keep a reminder under {MAX_TEXT} characters."));
    }
    if !(1..=MAX_SECS).contains(&in_secs) {
        return Err("I can set reminders from one second up to a year ahead.".into());
    }
    if every != 0 && !(MIN_EVERY..=MAX_SECS).contains(&every) {
        return Err("A repeating reminder can repeat every minute at the fastest.".into());
    }
    if list.len() >= MAX_REMINDERS {
        return Err(format!("You already have {MAX_REMINDERS} reminders. Clear a few first."));
    }
    let r = Reminder { id: list.iter().map(|r| r.id).max().unwrap_or(0) + 1, due: now + in_secs, text, every };
    list.push(r.clone());
    Ok(r)
}

/// Remove and return everything due at `now`, oldest first. Repeating reminders stay, moved on to their next time
/// (one that was missed several times over, e.g. while the computer slept, is said once, not once per miss).
pub fn take_due(list: &mut Vec<Reminder>, now: u64) -> Vec<Reminder> {
    let mut due: Vec<Reminder> = list.iter().filter(|r| r.due <= now).cloned().collect();
    list.retain(|r| r.due > now || r.every > 0);
    for r in list.iter_mut().filter(|r| r.due <= now) {
        r.due += ((now - r.due) / r.every + 1) * r.every;
    }
    due.sort_by_key(|r| r.due);
    due
}

pub fn matching(list: &[Reminder], needle: &str) -> Vec<u64> {
    let n = needle.trim().to_lowercase();
    if n.chars().count() < 2 {
        return vec![];
    }
    list.iter().filter(|r| r.text.to_lowercase().contains(&n)).map(|r| r.id).collect()
}

/// "in 5 minutes", "in 2 h 5 min", "in 40 seconds".
pub fn describe(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} second{}", if secs == 1 { "" } else { "s" }),
        60..=3599 => {
            let m = (secs + 30) / 60;
            format!("{m} minute{}", if m == 1 { "" } else { "s" })
        }
        _ => {
            let (h, m) = (secs / 3600, (secs % 3600) / 60);
            if m == 0 { format!("{h} hour{}", if h == 1 { "" } else { "s" }) } else { format!("{h} h {m} min") }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_validates_and_limits() {
        let mut l = vec![];
        let r = add(&mut l, 1000, 60, "  stretch   your legs ").unwrap();
        assert_eq!((r.due, r.text.as_str(), r.id), (1060, "stretch your legs", 1));
        assert!(add(&mut l, 1000, 0, "x").is_err());
        assert!(add(&mut l, 1000, MAX_SECS + 1, "x").is_err());
        assert!(add(&mut l, 1000, 5, "  ").is_err());
        assert!(add(&mut l, 1000, 5, &"y".repeat(201)).is_err());
        for i in 0..MAX_REMINDERS - 1 {
            add(&mut l, 1000, 5, &format!("r{i}")).unwrap();
        }
        assert!(add(&mut l, 1000, 5, "one too many").unwrap_err().contains("50"));
    }

    #[test]
    fn due_reminders_are_taken_once_in_order() {
        let mut l = vec![];
        add(&mut l, 0, 300, "late").unwrap();
        add(&mut l, 0, 100, "early").unwrap();
        add(&mut l, 0, 9000, "much later").unwrap();
        assert!(take_due(&mut l, 50).is_empty());
        let due = take_due(&mut l, 400);
        assert_eq!(due.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["early", "late"]);
        assert_eq!(l.len(), 1);
        assert!(take_due(&mut l, 400).is_empty());
    }

    #[test]
    fn repeating_reminders_come_back() {
        let mut l = vec![];
        add_repeating(&mut l, 0, 100, 3600, "drink water").unwrap();
        assert!(add_repeating(&mut l, 0, 100, 5, "too fast").is_err());
        assert_eq!(take_due(&mut l, 100).len(), 1);
        assert_eq!((l.len(), l[0].due), (1, 3700));
        // Missed three times over while asleep: said once, next one in the future.
        assert_eq!(take_due(&mut l, 3700 + 3 * 3600 + 5).len(), 1);
        assert_eq!(l[0].due, 3700 + 4 * 3600);
        // Files written before repeats existed still load.
        let old: Vec<Reminder> = serde_json::from_str(r#"[{"id":1,"due":5,"text":"x"}]"#).unwrap();
        assert_eq!(old[0].every, 0);
    }

    #[test]
    fn matching_and_describing() {
        let mut l = vec![];
        add(&mut l, 0, 10, "Call mum").unwrap();
        assert_eq!(matching(&l, "MUM"), vec![1]);
        assert!(matching(&l, "m").is_empty());
        assert_eq!(describe(1), "1 second");
        assert_eq!(describe(300), "5 minutes");
        assert_eq!(describe(3600), "1 hour");
        assert_eq!(describe(5400), "1 h 30 min");
    }

    #[test]
    fn persists() {
        let p = std::env::temp_dir().join(format!("nuzzle-rem-{}", std::process::id())).join("r.json");
        let mut l = vec![];
        add(&mut l, 5, 5, "persist me").unwrap();
        save(&p, &l).unwrap();
        assert_eq!(load(&p), l);
        assert!(load(&p.with_extension("missing")).is_empty());
    }
}
