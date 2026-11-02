//! Tiny append-only log file (`companion.log`, rotated at 1 MB). Errors shown in the speech bubble vanish
//! after a few seconds; this keeps the full text for troubleshooting.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

static PATH: OnceLock<PathBuf> = OnceLock::new();
const MAX_BYTES: u64 = 1_000_000;
/// Bytes written since the last size check (checking every line would mean a stat per line).
static SINCE_CHECK: AtomicU64 = AtomicU64::new(0);

pub fn init(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let p = dir.join("companion.log");
    rotate(&p);
    let _ = PATH.set(p);
}

/// Move a log over the size limit aside (replacing the previous `.old` one).
fn rotate(p: &Path) {
    if fs::metadata(p).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(p, p.with_file_name("companion.old.log"));
    }
}

/// Drop terminal colour codes (ESC `[` ... letter) that llama-server puts in its output.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            for n in it.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// "2026-11-02 15:04:05" in UTC.
fn stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn write(level: &str, msg: &str) {
    let Some(p) = PATH.get() else { return };
    let line = format!("{} {level:5} {}\n", stamp(), strip_ansi(msg).replace('\n', " "));
    if SINCE_CHECK.fetch_add(line.len() as u64, Ordering::Relaxed) > 50_000 {
        SINCE_CHECK.store(0, Ordering::Relaxed);
        rotate(p); // a long session must not grow the file without limit
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn warn(msg: &str) {
    write("WARN", msg);
}

#[cfg(test)]
mod tests {
    #[test]
    fn colour_codes_are_removed() {
        assert_eq!(super::strip_ansi("\x1b[34m0.01.778\x1b[0m \x1b[32mI \x1b[0msrv ok"), "0.01.778 I srv ok");
        assert_eq!(super::strip_ansi("plain [text] é"), "plain [text] é");
    }

    #[test]
    fn big_logs_are_rotated() {
        let d = std::env::temp_dir().join(format!("companion-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("companion.log");
        std::fs::write(&p, vec![b'x'; 1_100_000]).unwrap();
        super::rotate(&p);
        assert!(!p.exists() && d.join("companion.old.log").exists());
    }

    #[test]
    fn stamp_has_expected_shape() {
        let s = super::stamp();
        assert_eq!(s.len(), 19);
        assert!(s.starts_with("20"));
    }
}
