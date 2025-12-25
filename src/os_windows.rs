#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub(crate) struct Pid(u32);

pub(crate) fn getpid() -> Pid {
    Pid(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() })
}

pub(crate) fn gettid() -> Pid {
    Pid(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() })
}

impl Pid {
    pub(crate) fn as_i32(self) -> i32 {
        self.0 as i32
    }
}
