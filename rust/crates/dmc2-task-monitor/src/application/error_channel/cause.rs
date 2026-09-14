//! Classify pinned native diagnostics at their source boundary, not by severity.
use dmc2_diagnostics::RecoveryClass;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeCause {
    MesaReadFailure,
    MesaReceiveIncomplete,
    MesaWriteUnconfirmed,
    MesaWatchdogExpired,
    ProbeContactDuringJog,
    NativeErrorUnclassified,
    NativeInformation,
    UnknownMessageType,
}

pub(super) struct Contract {
    pub identity: &'static str,
    pub cause: &'static str,
    pub action: &'static str,
    pub recovery: RecoveryClass,
}

impl NativeCause {
    pub fn classify(message_type: i32, text: &[u8], known: bool) -> Self {
        if !known {
            return Self::UnknownMessageType;
        }
        let text = std::str::from_utf8(text).unwrap_or("").trim();
        if text.starts_with("hm2_eth:") {
            for word in text.split_ascii_whitespace() {
                match word {
                    "MESA_RECEIVE_INCOMPLETE" => return Self::MesaReceiveIncomplete,
                    "MESA_WRITE_UNCONFIRMED" => return Self::MesaWriteUnconfirmed,
                    _ => {}
                }
            }
        }
        // LinuxCNC 2.9.10: mesa-hostmot2/tram.c and watchdog.c. The board
        // instance name is supplied by the driver, not fixed to one machine.
        if let Some((_, message)) = text.strip_prefix("hm2/").and_then(|s| s.split_once(": ")) {
            if message
                .strip_prefix("error finishing read! iter=")
                .is_some_and(|value| value.parse::<u32>().is_ok())
            {
                return Self::MesaReadFailure;
            }
            if message == "Watchdog has bit! (set the .has-bit pin to False to resume)" {
                return Self::MesaWatchdogExpired;
            }
        }
        // LinuxCNC 2.9.10: emc/motion/control.c, probe edge during a jog.
        if matches!(
            text,
            "Probe tripped during a coordinate jog." | "Probe tripped during a joint jog."
        ) {
            return Self::ProbeContactDuringJog;
        }
        match message_type {
            1 | 11 => Self::NativeErrorUnclassified,
            _ => Self::NativeInformation,
        }
    }

    pub fn contract(self) -> Contract {
        let (identity, cause, action, recovery) = match self {
            Self::MesaReceiveIncomplete => (
                "MESA_RECEIVE_INCOMPLETE",
                "The Mesa receive deadline expired without the expected packet length and the existing error accumulator reached its fault threshold. The native record retains received/expected bytes, errno, elapsed time and deadline budget.",
                "Use Clear Fault, then Pendant Mode. Retain this event to distinguish a missing or short packet from a write-confirmation failure.",
                RecoveryClass::ClearController,
            ),
            Self::MesaWriteUnconfirmed => (
                "MESA_WRITE_UNCONFIRMED",
                "The returned Mesa write counter disagreed with the last transmitted write and the existing error accumulator reached its fault threshold. Expected and received read/write counters are retained below.",
                "Use Clear Fault, then Pendant Mode. Retain this event to trace the missing or stale confirmation.",
                RecoveryClass::ClearController,
            ),
            Self::MesaReadFailure => (
                "MESA_READ_FAILURE",
                "HostMot2 failed to complete its cyclic Mesa read. This native message does not distinguish the underlying receive failures; it is not a task-idle timeout report.",
                "Use Clear Fault to acknowledge the Mesa communication latch, then Pendant Mode. A new read failure is a new transport event and remains recorded.",
                RecoveryClass::ClearController,
            ),
            Self::MesaWatchdogExpired => (
                "MESA_WATCHDOG_EXPIRED",
                "The Mesa board reported its watchdog latch.",
                "Use Clear Fault to acknowledge the Mesa watchdog, then Pendant Mode.",
                RecoveryClass::ClearController,
            ),
            Self::ProbeContactDuringJog => (
                "PROBE_CONTACT_DURING_JOG",
                "LinuxCNC detected the probe input during manual jogging and stopped that jog.",
                "Use the visible Clear Fault and Pendant Mode controls to resume manual control; the contact record remains retained.",
                RecoveryClass::ClearController,
            ),
            Self::NativeErrorUnclassified => (
                "NATIVE_ERROR_UNCLASSIFIED",
                "LinuxCNC supplied the retained native error below. Its message type identifies severity, not the subsystem cause.",
                "Use Clear Fault and Pendant Mode to regain manual control. The original source message remains in the error journal for diagnosis.",
                RecoveryClass::ClearController,
            ),
            Self::NativeInformation => (
                "NATIVE_INFORMATION",
                "LinuxCNC supplied an informational message.",
                "Acknowledge the message in the UI.",
                RecoveryClass::RecheckSource,
            ),
            Self::UnknownMessageType => (
                "UNKNOWN_LINUXCNC_MESSAGE_TYPE",
                "The error-channel message type is outside the pinned LinuxCNC 2.9.10 ABI.",
                "Preserve the raw journal and reopen the matching standard application. Clear Fault remains available.",
                RecoveryClass::RelaunchApplication,
            ),
        };
        Contract {
            identity,
            cause,
            action,
            recovery,
        }
    }
}
