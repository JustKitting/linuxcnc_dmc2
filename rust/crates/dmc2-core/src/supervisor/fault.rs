#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum FaultCode {
    AdapterFailure,
    LinkFailure,
    QuadratureFailure,
    PacketTimeout,
    TaskHeartbeatTimeout,
    InvalidPendantPacket,
    StartupLimitMismatch,
    StartupLimitDuringHoming,
    LimitDuringStartupReset,
    HomingDuringStartupReset,
    LimitDuringRecovery,
    UnexpectedLimit,
    BounceLostLimitAttribution,
    BounceFeedbackUnavailable,
    BounceFeedbackIncoherent,
    BounceCountMismatch,
    JogCountMismatch,
    BounceLimitStillActive,
    BounceTimedOut,
    LimitLatchResetTimedOut,
    MesaStartupFailure,
    ControllerWatchdogFailure,
    CommandSequencerFailure,
    JogFeedbackUnavailable,
    JogFeedbackIncoherent,
}

impl FaultCode {
    pub const ALL: [Self; 25] = [
        Self::AdapterFailure,
        Self::LinkFailure,
        Self::QuadratureFailure,
        Self::PacketTimeout,
        Self::TaskHeartbeatTimeout,
        Self::InvalidPendantPacket,
        Self::StartupLimitMismatch,
        Self::StartupLimitDuringHoming,
        Self::LimitDuringStartupReset,
        Self::HomingDuringStartupReset,
        Self::LimitDuringRecovery,
        Self::UnexpectedLimit,
        Self::BounceLostLimitAttribution,
        Self::BounceFeedbackUnavailable,
        Self::BounceFeedbackIncoherent,
        Self::BounceCountMismatch,
        Self::JogCountMismatch,
        Self::BounceLimitStillActive,
        Self::BounceTimedOut,
        Self::LimitLatchResetTimedOut,
        Self::MesaStartupFailure,
        Self::ControllerWatchdogFailure,
        Self::CommandSequencerFailure,
        Self::JogFeedbackUnavailable,
        Self::JogFeedbackIncoherent,
    ];

    pub const fn wire_code(self) -> i32 {
        self as i32 + 1
    }

    pub const fn from_wire_code(code: i32) -> Option<Self> {
        if code < 1 || code > Self::ALL.len() as i32 {
            return None;
        }
        Some(Self::ALL[(code - 1) as usize])
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::AdapterFailure => "ADAPTER_FAILURE",
            Self::LinkFailure => "LINK_FAILURE",
            Self::QuadratureFailure => "QUADRATURE_FAILURE",
            Self::PacketTimeout => "PACKET_TIMEOUT",
            Self::TaskHeartbeatTimeout => "TASK_HEARTBEAT_TIMEOUT",
            Self::InvalidPendantPacket => "INVALID_PENDANT_PACKET",
            Self::StartupLimitMismatch => "STARTUP_LIMIT_MISMATCH",
            Self::StartupLimitDuringHoming => "STARTUP_LIMIT_DURING_HOMING",
            Self::LimitDuringStartupReset => "LIMIT_DURING_STARTUP_RESET",
            Self::HomingDuringStartupReset => "HOMING_DURING_STARTUP_RESET",
            Self::LimitDuringRecovery => "LIMIT_DURING_RECOVERY",
            Self::UnexpectedLimit => "UNEXPECTED_LIMIT",
            Self::BounceLostLimitAttribution => "BOUNCE_LOST_LIMIT_ATTRIBUTION",
            Self::BounceFeedbackUnavailable => "BOUNCE_FEEDBACK_UNAVAILABLE",
            Self::BounceFeedbackIncoherent => "BOUNCE_FEEDBACK_INCOHERENT",
            Self::BounceCountMismatch => "BOUNCE_COUNT_MISMATCH",
            Self::JogCountMismatch => "JOG_COUNT_MISMATCH",
            Self::BounceLimitStillActive => "BOUNCE_LIMIT_STILL_ACTIVE",
            Self::BounceTimedOut => "BOUNCE_TIMED_OUT",
            Self::LimitLatchResetTimedOut => "LIMIT_LATCH_RESET_TIMED_OUT",
            Self::MesaStartupFailure => "MESA_STARTUP_FAILURE",
            Self::ControllerWatchdogFailure => "CONTROLLER_WATCHDOG_FAILURE",
            Self::CommandSequencerFailure => "COMMAND_SEQUENCER_FAILURE",
            Self::JogFeedbackUnavailable => "JOG_FEEDBACK_UNAVAILABLE",
            Self::JogFeedbackIncoherent => "JOG_FEEDBACK_INCOHERENT",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FaultCode;

    #[test]
    fn every_fault_has_one_stable_nonzero_wire_code_and_name() {
        assert_eq!(FaultCode::ALL.len(), 25);
        for (index, fault) in FaultCode::ALL.iter().copied().enumerate() {
            let expected = index as i32 + 1;
            assert_eq!(fault.wire_code(), expected);
            assert_eq!(FaultCode::from_wire_code(expected), Some(fault));
            assert!(!fault.name().is_empty());
        }
        for unknown in [i32::MIN, -1, 0, 26, i32::MAX] {
            assert_eq!(FaultCode::from_wire_code(unknown), None);
        }
    }
}
