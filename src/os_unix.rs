#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub(crate) struct Pid(nix::unistd::Pid);

pub(crate) fn getpid() -> Pid {
    Pid(nix::unistd::getpid())
}

pub(crate) fn gettid() -> Pid {
    Pid(nix::unistd::gettid())
}

impl Pid {
    pub(crate) fn as_i32(self) -> i32 {
        self.0.as_raw()
    }
}
