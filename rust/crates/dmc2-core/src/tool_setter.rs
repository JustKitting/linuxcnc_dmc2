//! Normally closed tool-setter contacts and operator-owned overtravel recovery.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Circuits {
    pub contact_closed: bool,
    pub overtravel_closed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Unavailable,
    Ready,
    ContactOpen,
    OvertravelOpen,
    NeedsManualIdle,
    NeedsAcknowledgement,
}

pub struct Inputs {
    /// None means no usable Mesa sample, not an open physical circuit.
    pub circuits: Option<Circuits>,
    pub manual_idle_stationary: bool,
    pub program_active: bool,
    pub clear_setter: bool,
    pub clear_fault: bool,
}

#[derive(Debug)]
pub struct Outputs {
    pub contact: bool,
    pub overtravel: bool,
    pub latched: bool,
    pub feed_inhibit: bool,
    pub abort_program: bool,
    pub status: Status,
}

#[derive(Default)]
pub struct ToolSetter {
    last_sample: Option<Circuits>,
    latched: bool,
    clear_setter_held: bool,
    clear_fault_held: bool,
}

impl ToolSetter {
    /// Accept the global operator command independently of sampled input state.
    pub fn clear_fault(&mut self) {
        self.latched = false;
    }

    pub fn update(&mut self, input: Inputs) -> Outputs {
        // An early/held acknowledgement is never queued for later recovery.
        let acknowledge = input.clear_setter && !self.clear_setter_held;
        let clear_fault = input.clear_fault && !self.clear_fault_held;
        self.clear_setter_held = input.clear_setter;
        self.clear_fault_held = input.clear_fault;
        // Global Clear Fault always clears retained state, even with a stale
        // task/homing snapshot. A currently open overtravel circuit below is
        // still an actual input; acknowledgement cannot make it closed.
        if clear_fault {
            self.clear_fault();
        }
        if let Some(sample) = input.circuits {
            self.last_sample = Some(sample);
            if !sample.overtravel_closed {
                self.latched = true;
            } else if sample.contact_closed && input.manual_idle_stationary && acknowledge {
                self.latched = false;
            }
        }
        let contact = self.last_sample.is_some_and(|s| !s.contact_closed);
        let overtravel = self.last_sample.is_some_and(|s| !s.overtravel_closed);
        let status = if input.circuits.is_none() {
            Status::Unavailable
        } else if overtravel {
            Status::OvertravelOpen
        } else if contact {
            Status::ContactOpen
        } else if self.latched && !input.manual_idle_stationary {
            Status::NeedsManualIdle
        } else if self.latched {
            Status::NeedsAcknowledgement
        } else {
            Status::Ready
        };
        Outputs {
            contact,
            overtravel,
            latched: self.latched,
            // Do not resume a stopped program merely because IN2 closes again.
            // LinuxCNC feed-inhibit leaves manual withdrawal available.
            feed_inhibit: self.latched || input.circuits.is_none(),
            abort_program: self.latched && input.program_active,
            status,
        }
    }
}

#[cfg(test)]
mod tests;
