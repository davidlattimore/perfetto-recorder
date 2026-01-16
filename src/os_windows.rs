use crate::Pid;

pub(crate) fn getpid() -> Pid {
    Pid(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() } as i32)
}

pub(crate) fn gettid() -> Pid {
    Pid(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() } as i32)
}
