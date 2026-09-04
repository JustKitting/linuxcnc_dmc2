use std::io;
use std::os::raw::{c_int, c_ulong};
use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::catalog::CoreDumpPolicy;
use crate::event::Event;

const RLIMIT_CORE: c_int = 4;
const RLIM_INFINITY: c_ulong = c_ulong::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreDumpPlan {
    policy: CoreDumpPolicy,
    inherited_soft: c_ulong,
    inherited_hard: c_ulong,
    requested_soft: c_ulong,
}

impl CoreDumpPlan {
    pub fn capture(policy: CoreDumpPolicy) -> io::Result<Self> {
        let inherited = get_core_limit()?;
        let requested_soft = match policy {
            CoreDumpPolicy::Inherit => inherited.current,
            CoreDumpPolicy::EnableToHardLimit => inherited.maximum,
        };
        Ok(Self {
            policy,
            inherited_soft: inherited.current,
            inherited_hard: inherited.maximum,
            requested_soft,
        })
    }

    pub fn configure(self, command: &mut Command) {
        if self.policy == CoreDumpPolicy::Inherit {
            return;
        }
        let requested = RawLimit {
            current: self.requested_soft,
            maximum: self.inherited_hard,
        };
        // SAFETY: the closure invokes only setrlimit between fork and exec,
        // passes a copied POD object, allocates nothing, and returns any errno
        // through Command's normal exec-error pipe.
        unsafe {
            command.pre_exec(move || set_core_limit(requested));
        }
    }

    pub fn event_fields(self, event: Event) -> Event {
        event
            .field("core_limit_plan_policy", self.policy.name())
            .field(
                "core_limit_inherited_soft",
                render_limit(self.inherited_soft),
            )
            .field(
                "core_limit_inherited_hard",
                render_limit(self.inherited_hard),
            )
            .field(
                "core_limit_requested_soft",
                render_limit(self.requested_soft),
            )
    }
}

fn get_core_limit() -> io::Result<RawLimit> {
    let mut limit = RawLimit {
        current: 0,
        maximum: 0,
    };
    // SAFETY: `limit` is a valid writable rlimit object for the duration of
    // the syscall wrapper and RLIMIT_CORE is a valid Linux resource selector.
    if unsafe { getrlimit(RLIMIT_CORE, &mut limit) } == 0 {
        Ok(limit)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn set_core_limit(limit: RawLimit) -> io::Result<()> {
    // SAFETY: `limit` is a valid immutable rlimit object and the kernel does
    // not retain the pointer after setrlimit returns.
    if unsafe { setrlimit(RLIMIT_CORE, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn render_limit(value: c_ulong) -> String {
    if value == RLIM_INFINITY {
        "UNLIMITED".to_owned()
    } else {
        value.to_string()
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawLimit {
    current: c_ulong,
    maximum: c_ulong,
}

unsafe extern "C" {
    fn getrlimit(resource: c_int, limit: *mut RawLimit) -> c_int;
    fn setrlimit(resource: c_int, limit: *const RawLimit) -> c_int;
}
