#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub(crate) struct Pid(u32);

pub(crate) fn getpid() -> Pid {
    let pid = unsafe { libc::getpid() };
    Pid(pid as u32)
}

pub(crate) fn gettid() -> Pid {
    let pthread = unsafe { libc::pthread_self() };
    let tid = unsafe { libc::pthread_mach_thread_np(pthread) };
    Pid(tid)
}

impl Pid {
    pub(crate) fn as_i32(self) -> i32 {
        self.0 as i32
    }
}
