use crate::Pid;

pub(crate) fn getpid() -> Pid {
    let pid = unsafe { libc::getpid() };
    Pid(pid as i32)
}

pub(crate) fn gettid() -> Pid {
    let pthread = unsafe { libc::pthread_self() };
    let tid = unsafe { libc::pthread_mach_thread_np(pthread) };
    Pid(tid as i32)
}
