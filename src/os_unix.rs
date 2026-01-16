use crate::Pid;

pub(crate) fn getpid() -> Pid {
    Pid(nix::unistd::getpid().as_raw())
}

pub(crate) fn gettid() -> Pid {
    Pid(nix::unistd::gettid().as_raw())
}
