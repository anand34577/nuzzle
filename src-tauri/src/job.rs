//! Windows Job Object that kills `llama-server` when this app dies for any reason (crash, Task Manager),
//! so it can't linger and hold the GPU.

#[cfg(windows)]
pub fn adopt(handle: *mut core::ffi::c_void) {
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

#[cfg(not(windows))]
pub fn adopt(_handle: *mut core::ffi::c_void) {}
