use libc::{sched_param, sched_setscheduler, SCHED_FIFO, SCHED_OTHER};
use std::{io, marker::PhantomData};

pub struct RealtimeGuard {
    marker: PhantomData<*const ()>,
}

impl Drop for RealtimeGuard {
    fn drop(&mut self) {
        self.set_priority(false)
            .expect("Couldn't drop real-time priority!");
    }
}

impl Default for RealtimeGuard {
    fn default() -> Self {
        let mut guard = Self {
            marker: PhantomData,
        };
        guard
            .set_priority(true)
            .expect("Couldn't escalate to real-time priority!");
        guard
    }
}

impl RealtimeGuard {
    fn set_priority(&mut self, real_time: bool) -> io::Result<()> {
        let policy = if real_time { SCHED_FIFO } else { SCHED_OTHER };
        let sched_priority = if real_time { 10 } else { 0 };
        let params = sched_param { sched_priority };
        let res = unsafe { sched_setscheduler(0, policy, &params) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
