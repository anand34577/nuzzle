//! Makes sure child programs (`llama-server`, MCP servers) die with this app, however it ends (crash, Task Manager,
//! `kill -9`), so they can't linger and hold the GPU.
//!
//! - Windows: every child joins a Job Object that kills its members when the last handle to it (ours) closes.
//! - macOS and Linux: every child leads its own process group, and a tiny detached watchdog waits for this app to
//!   disappear and then kills that group. The watchdog is a separate process, so it outlives a crash of ours.

use tokio::process::{Child, Command};

/// Set a command up so `adopt` can clean up after it (call before spawning).
pub fn prepare(cmd: &mut Command) {
    #[cfg(unix)]
    cmd.process_group(0); // its own group: the watchdog kills it and anything it started, and Ctrl+C in a terminal doesn't
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Tie a freshly spawned child's life to this app's.
pub fn adopt(child: &Child) {
    #[cfg(windows)]
    if let Some(h) = child.raw_handle() {
        windows_job(h);
    }
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        watchdog(std::process::id(), pid);
    }
}

/// Kill a child's whole process tree now (used on a normal quit).
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).output();
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL); // the group `prepare` gave it
    }
}

#[cfg(unix)]
fn watchdog(parent: u32, child: u32) {
    use std::os::unix::process::CommandExt;
    // Polls once a second; does nothing if the child ends first. "kill -0" only asks whether a process exists.
    const SCRIPT: &str =
        r#"while kill -0 "$1" 2>/dev/null && kill -0 "$2" 2>/dev/null; do sleep 1; done; kill -0 "$1" 2>/dev/null || kill -9 -"$2" 2>/dev/null"#;
    let spawned = std::process::Command::new("sh")
        .args(["-c", SCRIPT, "companion-watchdog", &parent.to_string(), &child.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0) // not in our group, so whatever stops us doesn't stop it first
        .spawn();
    match spawned {
        // Reap it when it finishes so it doesn't linger as a zombie while we run.
        Ok(mut w) => {
            std::thread::spawn(move || w.wait());
        }
        Err(e) => crate::log::warn(&format!("couldn't start the child watchdog: {e}")),
    }
}

#[cfg(windows)]
fn windows_job(handle: *mut core::ffi::c_void) {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let j = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if j.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            j,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        j as usize
    });
    if job != 0 && !handle.is_null() {
        unsafe {
            AssignProcessToJobObject(job as *mut core::ffi::c_void, handle);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_group_kill_ends_the_child() {
        let mut c = Command::new("sleep");
        c.arg("30");
        prepare(&mut c);
        let mut child = c.spawn().unwrap();
        adopt(&child);
        kill_tree(child.id().unwrap());
        let st = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.unwrap().unwrap();
        assert!(!st.success());
    }
}
