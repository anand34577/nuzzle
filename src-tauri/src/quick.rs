//! Quick commands: a message that starts with a slash (`/timer 10m tea`, `/note buy milk`, `/calc 19.99*3`) is done
//! at once, without the model. They work before a model is downloaded, while it is loading, from the typing box,
//! from Telegram and through the agent API, and they always do exactly what was typed.

use serde_json::json;
use std::sync::Arc;

use crate::{reminders, sys, tools, Core, LockExt};

pub const HELP: &str = "Quick commands (instant, no thinking needed):
/timer 10m tea
/remind 3pm call mum
/remind every 1h drink water
/reminders   /cancel tea
/note buy milk   /notes
/calc 19.99 * 3
/convert 5 km to miles
/flip   /roll 2d6   /pick tea, coffee
/remember I like green tea
/today   /weather Pune
/time   /new (fresh chat)   /stop
Anything else you type or say, I think about.";

#[derive(Debug, PartialEq)]
pub enum Cmd {
    Help,
    Remind { secs: u64, every: u64, text: String },
    Reminders,
    Cancel(String),
    Note(String),
    Notes,
    Calc(String),
    Convert { value: f64, from: String, to: String },
    Time,
    Today,
    Weather(String),
    Flip,
    Roll(String),
    Pick(Vec<String>),
    Remember(String),
    NewChat,
    Stop,
    /// A known command typed in a way I couldn't read: say how it goes.
    Usage(&'static str),
}

/// The "when" at the start of a reminder: (seconds until the first time, seconds between repeats, the rest).
/// Understands "10m", "in 2 hours", "at 3pm", "15:30", "every 30 min", "every day at 9am".
fn split_when(rest: &str) -> Option<(u64, u64, String)> {
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    let first = words.first()?.to_lowercase();
    let every = first == "every";
    if every || first == "in" || first == "at" {
        words.remove(0);
    }
    for k in (1..=words.len().min(4)).rev() {
        let head = words[..k].concat();
        if every {
            // "every hour" means "every 1 hour".
            let head = if head.starts_with(|c: char| c.is_ascii_digit()) { head } else { format!("1{head}") };
            let Some(gap) = sys::parse_duration(&head) else { continue };
            let mut tail = &words[k..];
            let mut first_in = gap;
            // "every day at 9am ...": the first time is that clock time.
            if tail.first().is_some_and(|w| w.eq_ignore_ascii_case("at")) {
                for j in (1..=(tail.len() - 1).min(2)).rev() {
                    if let Some((h, m)) = sys::parse_clock(&tail[1..=j].concat()) {
                        first_in = sys::secs_until_clock(h, m);
                        tail = &tail[j + 1..];
                        break;
                    }
                }
            }
            return Some((first_in, gap, tail.join(" ")));
        }
        let secs = sys::parse_duration(&head).or_else(|| sys::parse_clock(&head).map(|(h, m)| sys::secs_until_clock(h, m)));
        if let Some(secs) = secs {
            return Some((secs, 0, words[k..].join(" ")));
        }
    }
    None
}

/// "5 km to miles", "72f in c", "2.5 cups ml" -> (value, from, to).
fn split_convert(rest: &str) -> Option<(f64, String, String)> {
    let rest = rest.trim();
    let end = rest.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'))).unwrap_or(rest.len());
    let value: f64 = rest[..end].replace(',', "").parse().ok()?;
    let words: Vec<&str> = rest[end..].split_whitespace().filter(|w| !["to", "in", "into", "as", "="].contains(&w.to_lowercase().as_str())).collect();
    match words[..] {
        [from, to] => Some((value, from.to_string(), to.to_string())),
        _ => None,
    }
}

/// Read a quick command. `None` means "not one of mine": the message goes to the model as usual (so a path such as
/// `/home/sam/notes.txt` or an unknown `/word` is never swallowed).
pub fn parse(text: &str) -> Option<Cmd> {
    let t = text.trim().strip_prefix('/')?;
    let (name, rest) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
    let name = name.split('@').next().unwrap_or(name).to_lowercase(); // Telegram groups send /timer@mybot
    let rest = rest.trim();
    let need = |cmd: Cmd, usage: &'static str| if rest.is_empty() { Cmd::Usage(usage) } else { cmd };
    Some(match name.as_str() {
        "help" | "commands" | "?" => Cmd::Help,
        "timer" => match split_when(rest) {
            Some((secs, every, label)) => Cmd::Remind { secs, every, text: if label.is_empty() { "Your timer is done!".into() } else { label } },
            None => Cmd::Usage("Like this: /timer 10m tea   (or 90s, 1h30m, 25 minutes)"),
        },
        "remind" | "reminder" | "remindme" => match split_when(rest) {
            Some((secs, every, text)) if !text.is_empty() => Cmd::Remind { secs, every, text },
            _ => Cmd::Usage("Like this: /remind 20m stretch   /remind 3pm call mum   /remind every 1h drink water"),
        },
        "reminders" | "timers" => Cmd::Reminders,
        "cancel" => need(Cmd::Cancel(rest.into()), "Like this: /cancel tea   (cancels reminders containing that word)"),
        "note" => need(Cmd::Note(rest.into()), "Like this: /note buy milk"),
        "notes" => Cmd::Notes,
        "calc" | "calculate" | "math" => need(Cmd::Calc(rest.into()), "Like this: /calc (19.99 * 3) * 1.2"),
        "convert" | "conv" => match split_convert(rest) {
            Some((value, from, to)) => Cmd::Convert { value, from, to },
            None => Cmd::Usage("Like this: /convert 5 km to miles   /convert 72 f to c"),
        },
        "time" | "date" | "now" => Cmd::Time,
        "today" | "morning" | "briefing" => Cmd::Today,
        "weather" | "forecast" => Cmd::Weather(rest.into()),
        "flip" | "coin" => Cmd::Flip,
        "roll" | "dice" => Cmd::Roll(rest.into()),
        "pick" | "choose" => {
            let sep = if rest.contains(',') { ',' } else if rest.contains('|') { '|' } else { ' ' };
            let options: Vec<String> = rest.split(sep).map(str::trim).filter(|o| !o.is_empty() && !o.eq_ignore_ascii_case("or")).map(String::from).collect();
            if options.len() < 2 { Cmd::Usage("Like this: /pick pizza, pasta, salad") } else { Cmd::Pick(options) }
        }
        "remember" => need(Cmd::Remember(rest.into()), "Like this: /remember My sister's name is Maya"),
        "new" | "reset" | "newchat" => Cmd::NewChat,
        "stop" => Cmd::Stop,
        _ => return None,
    })
}

/// Do it and say what happened, in one or two short lines.
pub async fn run(core: &Arc<Core>, cmd: Cmd) -> String {
    match cmd {
        Cmd::Help => HELP.into(),
        Cmd::Usage(u) => u.into(),
        Cmd::Remind { secs, every, text } => match core.remind_add(secs, every, &text) {
            Ok(r) => format!(
                "Okay! In {}{}: {}",
                reminders::describe(r.due.saturating_sub(sys::unix_now())),
                if r.every > 0 { format!(", then every {}", reminders::describe(r.every)) } else { String::new() },
                r.text
            ),
            Err(e) => e,
        },
        Cmd::Reminders => {
            let now = sys::unix_now();
            let mut list = core.reminders.lk().clone();
            list.sort_by_key(|r| r.due);
            if list.is_empty() {
                return "No reminders or timers are set. Try /timer 10m tea".into();
            }
            list.iter()
                .map(|r| {
                    let repeat = if r.every > 0 { format!(" (every {})", reminders::describe(r.every)) } else { String::new() };
                    format!("In {}{repeat}: {}", reminders::describe(r.due.saturating_sub(now)), r.text)
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Cmd::Cancel(text) => {
            let ids = reminders::matching(&core.reminders.lk(), &text);
            let n = ids.into_iter().filter(|id| core.remind_delete(*id).is_ok()).count();
            match n {
                0 => format!("I don't have a reminder containing \"{text}\". /reminders lists them."),
                1 => "Cancelled that reminder.".into(),
                n => format!("Cancelled {n} reminders."),
            }
        }
        Cmd::Note(text) => {
            let res = tools::notes_tool(&json!({"action": "append", "title": "Quick notes", "text": format!("- {text}")})).await;
            if res["ok"] == true {
                "Noted in Quick notes (Documents/Nuzzle Notes).".into()
            } else {
                format!("I couldn't save that note. {}", res["error"].as_str().unwrap_or(""))
            }
        }
        Cmd::Notes => {
            let res = tools::notes_tool(&json!({"action": "list"})).await;
            let names: Vec<&str> = res["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()).collect();
            if names.is_empty() {
                "No notes yet. /note buy milk starts one.".into()
            } else {
                format!("Your notes: {}\nThey live in {}", names.join(", "), res["folder"].as_str().unwrap_or("Documents/Nuzzle Notes"))
            }
        }
        Cmd::Calc(expr) => match tools::calculate(&expr) {
            Ok(v) => format!("{expr} = {}", (v * 1e10).round() / 1e10),
            Err(e) => format!("I couldn't work that out: {e}."),
        },
        Cmd::Convert { value, from, to } => match tools::convert(value, &from, &to) {
            Ok(v) => format!("{} {from} = {} {to}", tools::pretty(value), tools::pretty(v)),
            Err(e) => e,
        },
        Cmd::Time => format!("It's {}.", sys::format_local(&sys::local_now())),
        Cmd::Today => crate::hello::today(core).await,
        Cmd::Weather(place) => {
            let place = if place.is_empty() { core.settings.lk().home_place.trim().to_string() } else { place };
            if place.is_empty() {
                return "Like this: /weather Pune   (or set your town in Settings -> Character and just type /weather)".into();
            }
            match tools::weather(core, &place).await {
                Ok(w) => {
                    let days: Vec<String> = w["next_days"].as_array().into_iter().flatten().skip(1).map(|d| {
                        format!("{}: {}, {} to {}", d["date"].as_str().unwrap_or(""), d["sky"].as_str().unwrap_or(""), d["low_c"], d["high_c"])
                    }).collect();
                    format!("{}: {} C (feels {}), {}. Today {} to {}.\n{}", w["place"].as_str().unwrap_or(&place), w["now"]["temp_c"], w["now"]["feels_like_c"],
                        w["now"]["sky"].as_str().unwrap_or(""), w["next_days"][0]["low_c"], w["next_days"][0]["high_c"], days.join("\n"))
                }
                Err(e) => e,
            }
        }
        Cmd::Flip => {
            let options = ["Heads!".to_string(), "Tails!".to_string()];
            tools::pick_one(&options).cloned().unwrap_or_default()
        }
        Cmd::Roll(spec) => match tools::roll_dice(&spec) {
            Ok((rolls, total)) if rolls.len() == 1 => format!("You rolled {total}."),
            Ok((rolls, total)) => format!("You rolled {} = {total}.", rolls.iter().map(u32::to_string).collect::<Vec<_>>().join(" + ")),
            Err(e) => e,
        },
        Cmd::Pick(options) => match tools::pick_one(&options) {
            Some(p) => format!("I pick: {p}"),
            None => "Give me a few things to pick from.".into(),
        },
        Cmd::Remember(text) => {
            if !core.settings.lk().memory_enabled {
                return "Memory is turned off in Settings -> Memory.".into();
            }
            match core.mem_add(&text, "user") {
                Ok(_) => "Got it. I'll remember that.".into(),
                Err(e) => e,
            }
        }
        Cmd::NewChat => {
            core.forget_chat();
            "Fresh start! I've let go of this chat (the things you asked me to remember are safe).".into()
        }
        Cmd::Stop => {
            core.stop_everything();
            "Stopped.".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_my_commands_are_taken() {
        for not_mine in ["hello", "/home/sam/notes.txt what is this?", "/unknown thing", "  what does /timer do", "/"] {
            assert_eq!(parse(not_mine), None, "{not_mine}");
        }
        assert_eq!(parse("/help"), Some(Cmd::Help));
        assert_eq!(parse("  /HELP  "), Some(Cmd::Help));
        assert_eq!(parse("/stop@my_bot"), Some(Cmd::Stop), "Telegram group form");
        assert_eq!(parse("/new"), Some(Cmd::NewChat));
    }

    #[test]
    fn timers_and_reminders() {
        assert_eq!(parse("/timer 10m tea"), Some(Cmd::Remind { secs: 600, every: 0, text: "tea".into() }));
        assert_eq!(parse("/timer 25"), Some(Cmd::Remind { secs: 1500, every: 0, text: "Your timer is done!".into() }));
        assert_eq!(parse("/timer 1h 30m pasta is ready"), Some(Cmd::Remind { secs: 5400, every: 0, text: "pasta is ready".into() }));
        assert_eq!(parse("/remind in 2 hours call mum"), Some(Cmd::Remind { secs: 7200, every: 0, text: "call mum".into() }));
        assert_eq!(parse("/remind every 30 min drink water"), Some(Cmd::Remind { secs: 1800, every: 1800, text: "drink water".into() }));
        assert_eq!(parse("/remind every hour stretch"), Some(Cmd::Remind { secs: 3600, every: 3600, text: "stretch".into() }));
        // Clock times depend on the time of day; they must be read, land within a day, and keep the text.
        for clock in ["/remind 3pm call mum", "/remind at 15:30 call mum", "/remind 3 pm call mum"] {
            match parse(clock) {
                Some(Cmd::Remind { secs, every: 0, text }) => assert!((1..=86_400).contains(&secs) && text == "call mum", "{clock}"),
                other => panic!("{clock}: {other:?}"),
            }
        }
        match parse("/remind every day at 9am take vitamins") {
            Some(Cmd::Remind { secs, every: 86_400, text }) => assert!(secs <= 86_400 && text == "take vitamins"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse("/timer soon"), Some(Cmd::Usage(_))));
        assert!(matches!(parse("/remind 10m"), Some(Cmd::Usage(_))), "a reminder needs words");
        assert!(matches!(parse("/remind"), Some(Cmd::Usage(_))));
    }

    #[test]
    fn the_small_tools() {
        assert_eq!(parse("/calc 2 + 2"), Some(Cmd::Calc("2 + 2".into())));
        assert_eq!(parse("/convert 5 km to miles"), Some(Cmd::Convert { value: 5.0, from: "km".into(), to: "miles".into() }));
        assert_eq!(parse("/convert 72f in c"), Some(Cmd::Convert { value: 72.0, from: "f".into(), to: "c".into() }));
        assert_eq!(parse("/convert 1,500 m km"), Some(Cmd::Convert { value: 1500.0, from: "m".into(), to: "km".into() }));
        assert!(matches!(parse("/convert lots"), Some(Cmd::Usage(_))));
        assert_eq!(parse("/pick tea, iced coffee, water"), Some(Cmd::Pick(vec!["tea".into(), "iced coffee".into(), "water".into()])));
        assert_eq!(parse("/pick tea or coffee"), Some(Cmd::Pick(vec!["tea".into(), "coffee".into()])));
        assert!(matches!(parse("/pick tea"), Some(Cmd::Usage(_))));
        assert_eq!(parse("/roll 2d6"), Some(Cmd::Roll("2d6".into())));
        assert_eq!(parse("/note buy milk"), Some(Cmd::Note("buy milk".into())));
        assert!(matches!(parse("/note"), Some(Cmd::Usage(_))));
        assert_eq!(parse("/today"), Some(Cmd::Today));
        assert_eq!(parse("/weather New Delhi"), Some(Cmd::Weather("New Delhi".into())));
        assert_eq!(parse("/weather"), Some(Cmd::Weather(String::new())));
        assert!(HELP.lines().count() <= 13, "the help has to fit in a speech bubble");
    }
}
