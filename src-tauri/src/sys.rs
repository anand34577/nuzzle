//! Small system helpers: local time (no time-zone crate needed on Windows) and a machine summary.

use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 0 = Sunday
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
#[cfg_attr(windows, allow(dead_code))]
fn civil(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(m <= 2)) as i32, m, d)
}

#[cfg_attr(windows, allow(dead_code))]
pub fn from_unix_utc(secs: u64) -> LocalTime {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let (year, month, day) = civil(days);
    LocalTime { year, month, day, weekday: ((days + 4).rem_euclid(7)) as u32, hour: rem / 3600, minute: rem % 3600 / 60, second: rem % 60 }
}

#[cfg(windows)]
pub fn local_now() -> LocalTime {
    use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    LocalTime {
        year: i32::from(t.wYear),
        month: u32::from(t.wMonth),
        day: u32::from(t.wDay),
        weekday: u32::from(t.wDayOfWeek),
        hour: u32::from(t.wHour),
        minute: u32::from(t.wMinute),
        second: u32::from(t.wSecond),
    }
}

#[cfg(not(windows))]
pub fn local_now() -> LocalTime {
    from_unix_utc(unix_now())
}

pub fn format_local(t: &LocalTime) -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!(
        "{}, {} {} {}, {:02}:{:02}",
        DAYS[(t.weekday % 7) as usize],
        t.day,
        MONTHS[(t.month.clamp(1, 12) - 1) as usize],
        t.year,
        t.hour,
        t.minute
    )
}

/// "15:30", "9:05", "3pm", "3:05 pm" -> (hour, minute).
pub fn parse_clock(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().to_lowercase().replace(' ', "");
    let (body, pm, am) = if let Some(b) = s.strip_suffix("pm") {
        (b, true, false)
    } else if let Some(b) = s.strip_suffix("am") {
        (b, false, true)
    } else {
        (s.as_str(), false, false)
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if pm || am => (body.parse::<u32>().ok()?, 0),
        None => return None,
    };
    let h = match (pm, am) {
        (true, _) if (1..=12).contains(&h) => h % 12 + 12,
        (_, true) if (1..=12).contains(&h) => h % 12,
        (false, false) if h < 24 => h,
        _ => return None,
    };
    (m < 60).then_some((h, m))
}

/// Seconds from `now` (seconds since local midnight) to the next occurrence of hh:mm (today, or tomorrow if passed).
pub fn secs_until(now_of_day: u32, h: u32, m: u32) -> u64 {
    let target = h * 3600 + m * 60;
    if target > now_of_day { u64::from(target - now_of_day) } else { u64::from(86_400 - now_of_day + target) }
}

pub fn secs_until_clock(h: u32, m: u32) -> u64 {
    let t = local_now();
    secs_until(t.hour * 3600 + t.minute * 60 + t.second, h, m)
}

#[cfg(windows)]
fn memory_and_uptime() -> (Option<(u64, u64)>, Option<u64>) {
    use windows_sys::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut m) } != 0;
    (ok.then_some((m.ullTotalPhys, m.ullAvailPhys)), Some(unsafe { GetTickCount64() } / 1000))
}

#[cfg(not(windows))]
fn memory_and_uptime() -> (Option<(u64, u64)>, Option<u64>) {
    (None, None)
}

pub fn system_info() -> Value {
    let (mem, up) = memory_and_uptime();
    let gb = |b: u64| (b as f64 / 1_073_741_824.0 * 10.0).round() / 10.0;
    json!({
        "time": format_local(&local_now()),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "computer": std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default(),
        "user": std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default(),
        "cpu_threads": std::thread::available_parallelism().map_or(0, |n| n.get()),
        "memory_gb_total": mem.map(|m| gb(m.0)),
        "memory_gb_free": mem.map(|m| gb(m.1)),
        "uptime_hours": up.map(|s| (s as f64 / 360.0).round() / 10.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_to_civil() {
        // 2026-11-02 15:04:05 UTC was a Monday.
        let t = from_unix_utc(1_793_631_845);
        assert_eq!((t.year, t.month, t.day, t.weekday), (2026, 11, 2, 1));
        assert_eq!((t.hour, t.minute, t.second), (15, 4, 5));
        assert_eq!(from_unix_utc(0), LocalTime { year: 1970, month: 1, day: 1, weekday: 4, hour: 0, minute: 0, second: 0 });
        assert_eq!(format_local(&t), "Monday, 2 November 2026, 15:04");
    }

    #[test]
    fn clock_parsing() {
        assert_eq!(parse_clock("15:30"), Some((15, 30)));
        assert_eq!(parse_clock("9:05"), Some((9, 5)));
        assert_eq!(parse_clock("3pm"), Some((15, 0)));
        assert_eq!(parse_clock("12 am"), Some((0, 0)));
        assert_eq!(parse_clock("12:15 PM"), Some((12, 15)));
        for bad in ["", "25:00", "7", "10:75", "0pm", "13pm", "abc"] {
            assert_eq!(parse_clock(bad), None, "{bad}");
        }
    }

    #[test]
    fn seconds_until_next_clock_time() {
        assert_eq!(secs_until(10 * 3600, 10, 30), 1800);
        assert_eq!(secs_until(23 * 3600, 1, 0), 2 * 3600); // rolls over midnight
        assert_eq!(secs_until(10 * 3600, 10, 0), 86_400); // exactly now means tomorrow
    }

    #[test]
    fn system_info_has_basics() {
        let v = system_info();
        assert!(v["os"].is_string());
        assert!(v["cpu_threads"].as_u64().unwrap_or(0) >= 1);
    }
}
