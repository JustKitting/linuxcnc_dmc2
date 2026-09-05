//! Closed recovery taxonomy shared by every operator-facing diagnostic.

use core::fmt;

use crate::{valid_hal_slug, valid_symbolic_identity, SelfDescribingDiagnostic};

macro_rules! define_recovery_operations {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        /// Every recovery step that must be reachable through an operator UI.
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        #[repr(u8)]
        pub enum RecoveryOperation {
            $($variant,)+
        }

        impl RecoveryOperation {
            pub const COUNT: usize = <[()]>::len(&[$(define_recovery_operations!(@unit $variant)),+]);
            pub const ALL: [Self; Self::COUNT] = [$(Self::$variant,)+];

            pub const fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)+
                }
            }

            pub const fn used_by_recovery_class(self) -> bool {
                let mut class_index = 0;
                while class_index < RECOVERY_CONTRACTS.len() {
                    let operations = RECOVERY_CONTRACTS[class_index].ui_operations;
                    let mut operation_index = 0;
                    while operation_index < operations.len() {
                        if operations[operation_index] as u8 == self as u8 {
                            return true;
                        }
                        operation_index += 1;
                    }
                    class_index += 1;
                }
                false
            }
        }
    };
    (@unit $variant:ident) => { () };
}

define_recovery_operations! {
    Abort => "machine.abort",
    ClearFault => "controller.clear-fault",
    EstopReset => "machine.estop-reset",
    MachineOn => "machine.on",
    LaunchApplication => "application.launch",
    PendantMode => "controller.pendant-mode",
    HomeAll => "machine.home-all",
}

macro_rules! define_recovery_contracts {
    ($(
        $class:ident = $class_code:literal => (
            $class_name:literal,
            $class_slug:literal,
            $transition:ident = $transition_code:literal,
            $transition_name:literal,
            $clear_transition:literal,
            [$($operation:ident),+ $(,)?]
        )
    ),+ $(,)?) => {
        /// The state observation that proves one recovery class has cleared.
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        #[repr(u8)]
        pub enum RecoveryTransition {
            $($transition = $transition_code,)+
        }

        impl RecoveryTransition {
            pub const COUNT: usize = <[()]>::len(&[$(define_recovery_contracts!(@unit $transition)),+]);
            pub const ALL: [Self; Self::COUNT] = [$(Self::$transition,)+];

            pub const fn wire_code(self) -> u8 {
                self as u8
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$transition => $transition_name,)+
                }
            }

            pub const fn description(self) -> &'static str {
                match self {
                    $(Self::$transition => $clear_transition,)+
                }
            }
        }

        /// The complete set of operator recovery state machines.
        ///
        /// The declaration below generates each variant together with its
        /// transition, clear condition, and ordered UI operation path.
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        #[repr(u8)]
        pub enum RecoveryClass {
            $($class = $class_code,)+
        }

        /// One complete recovery state-machine contract.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct RecoveryContract {
            recovery_class: RecoveryClass,
            transition: RecoveryTransition,
            ui_operations: &'static [RecoveryOperation],
        }

        impl RecoveryContract {
            pub const fn recovery_class(self) -> RecoveryClass {
                self.recovery_class
            }

            pub const fn transition(self) -> RecoveryTransition {
                self.transition
            }

            pub const fn ui_operations(self) -> &'static [RecoveryOperation] {
                self.ui_operations
            }
        }

        pub const RECOVERY_CONTRACTS: [RecoveryContract; RecoveryClass::COUNT] = [$(
            RecoveryContract {
                recovery_class: RecoveryClass::$class,
                transition: RecoveryTransition::$transition,
                ui_operations: &[$(RecoveryOperation::$operation,)+],
            },
        )+];

        impl RecoveryClass {
            pub const COUNT: usize = <[()]>::len(&[$(define_recovery_contracts!(@unit $class)),+]);
            pub const ALL: [Self; Self::COUNT] = [$(Self::$class,)+];

            pub const fn wire_code(self) -> u8 {
                self as u8
            }

            pub const fn from_wire_code(code: u8) -> Option<Self> {
                match code {
                    $($class_code => Some(Self::$class),)+
                    _ => None,
                }
            }

            pub fn from_slug(slug: &str) -> Option<Self> {
                match slug {
                    $($class_slug => Some(Self::$class),)+
                    _ => None,
                }
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$class => $class_name,)+
                }
            }

            pub const fn hal_slug(self) -> &'static str {
                match self {
                    $(Self::$class => $class_slug,)+
                }
            }

            pub const fn transition(self) -> RecoveryTransition {
                self.contract().transition
            }

            pub const fn clear_transition(self) -> &'static str {
                self.transition().description()
            }

            pub const fn ui_operations(self) -> &'static [RecoveryOperation] {
                self.contract().ui_operations
            }

            pub const fn contract(self) -> RecoveryContract {
                RECOVERY_CONTRACTS[self as usize - 1]
            }

            pub const fn contract_complete(self) -> bool {
                if !(valid_symbolic_identity(self.name())
                    && valid_hal_slug(self.hal_slug())
                    && valid_symbolic_identity(self.transition().name())
                    && !self.clear_transition().is_empty()
                    && !self.ui_operations().is_empty())
                {
                    return false;
                }
                let operations = self.ui_operations();
                let mut index = 0;
                while index < operations.len() {
                    if !valid_operation_id(operations[index].id()) {
                        return false;
                    }
                    let mut previous = 0;
                    while previous < index {
                        if operations[previous] as u8 == operations[index] as u8 {
                            return false;
                        }
                        previous += 1;
                    }
                    index += 1;
                }
                matches!(
                    operations[operations.len() - 1],
                    RecoveryOperation::PendantMode
                )
            }
        }
    };
    (@unit $variant:ident) => { () };
}

define_recovery_contracts! {
    RecheckSource = 1 => (
        "RECHECK_SOURCE",
        "recheck-source",
        SourceHealthy = 1,
        "SOURCE_HEALTHY",
        "a subsequent valid monitor evaluation no longer contains the exact typed issue",
        [PendantMode]
    ),
    ClearController = 2 => (
        "CLEAR_CONTROLLER",
        "clear-controller",
        ControllerFaultCleared = 2,
        "CONTROLLER_FAULT_CLEARED",
        "after the named cause is healthy, LinuxCNC's canonical E-stop Reset reaches the controller and its retained fault code returns to NONE",
        [ClearFault, PendantMode]
    ),
    RestorePendant = 3 => (
        "RESTORE_PENDANT",
        "restore-pendant",
        PendantStreamRestored = 3,
        "PENDANT_STREAM_RESTORED",
        "with E-stop and deadman released, use Clear Fault; a fresh unchanged-counter packet acknowledges the bounded request and clears both bridge and controller faults without replaying a detent; after a new decoder/transport error or timeout, restore the named cause and explicitly retry Clear Fault",
        [ClearFault, PendantMode]
    ),
    ReleaseLimit = 4 => (
        "RELEASE_LIMIT",
        "release-limit",
        LimitReleased = 4,
        "LIMIT_RELEASED",
        "Clear Fault resets only inactive raw-input latches without turning Machine On; for one remaining attributed switch, select Machine On and Pendant Mode and command the existing away-only release until raw and safety indications clear; conflicting active switches must be resolved before retrying Clear Fault",
        [ClearFault, MachineOn, PendantMode]
    ),
    AbortTask = 5 => (
        "ABORT_TASK",
        "abort-task",
        TaskIdle = 5,
        "TASK_IDLE",
        "LinuxCNC accepts Abort, reports the interpreter idle, and the exact typed task issue is absent from the next valid evaluation",
        [Abort, ClearFault, PendantMode]
    ),
    RestoreMachine = 6 => (
        "RESTORE_MACHINE",
        "restore-machine",
        MachineReady = 6,
        "MACHINE_READY",
        "after the physical E-stop is released, LinuxCNC reports E-stop reset and Machine On",
        [EstopReset, MachineOn, PendantMode]
    ),
    ResetSpindle = 7 => (
        "RESET_SPINDLE",
        "reset-spindle",
        SpindleReset = 7,
        "SPINDLE_RESET",
        "after Abort and correction of the named drive cause, Clear Fault reaches the H100 reset input and its fault, block, and current-fault indications clear",
        [Abort, ClearFault, PendantMode]
    ),
    RelaunchApplication = 8 => (
        "RELAUNCH_APPLICATION",
        "relaunch-application",
        ApplicationRelaunched = 8,
        "APPLICATION_RELAUNCHED",
        "after correction of the named installation or runtime cause, the Applications launcher starts a matching session that publishes a valid diagnostic catalog",
        [LaunchApplication, ClearFault, PendantMode]
    ),
    EstablishPosition = 9 => (
        "ESTABLISH_POSITION",
        "establish-position",
        PositionKnown = 9,
        "POSITION_KNOWN",
        "after E-stop Reset and Machine On, Home All completes and LinuxCNC reports every configured joint homed",
        [EstopReset, MachineOn, HomeAll, PendantMode]
    ),
}

const fn valid_operation_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || !bytes[0].is_ascii_lowercase()
        || bytes[bytes.len() - 1] == b'.'
        || bytes[bytes.len() - 1] == b'-'
    {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-') {
            return false;
        }
        if (byte == b'.' || byte == b'-') && (bytes[index - 1] == b'.' || bytes[index - 1] == b'-')
        {
            return false;
        }
        index += 1;
    }
    true
}

/// Any error that crosses an operator or state boundary must classify all of
/// its variants into the closed recovery taxonomy.
pub trait RecoveryClassified {
    fn recovery_class(&self) -> RecoveryClass;
}

/// Display adapter used at every process/report boundary. A top-level error
/// cannot be rendered through this path until it has an exhaustive recovery
/// classification.
pub struct RecoveryDisplay<'a, T: ?Sized>(pub &'a T);

impl<T> fmt::Display for RecoveryDisplay<'_, T>
where
    T: fmt::Display + RecoveryClassified + ?Sized,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let recovery = self.0.recovery_class();
        write!(
            formatter,
            "{}; recovery-class={}; recovery-transition={}; clear-condition={:?}; ui-path=",
            self.0,
            recovery.name(),
            recovery.transition().name(),
            recovery.clear_transition(),
        )?;
        for (index, operation) in recovery.ui_operations().iter().enumerate() {
            if index != 0 {
                formatter.write_str(" -> ")?;
            }
            formatter.write_str(operation.id())?;
        }
        Ok(())
    }
}

/// Marker for numeric diagnostics that provide both complete diagnostic
/// metadata and an exhaustive recovery classification.
pub trait RecoverableDiagnostic: SelfDescribingDiagnostic + RecoveryClassified {}

impl<T> RecoverableDiagnostic for T where T: SelfDescribingDiagnostic + RecoveryClassified {}

const _: () = {
    const fn same_text(left: &str, right: &str) -> bool {
        let left = left.as_bytes();
        let right = right.as_bytes();
        if left.len() != right.len() {
            return false;
        }
        let mut index = 0;
        while index < left.len() {
            if left[index] != right[index] {
                return false;
            }
            index += 1;
        }
        true
    }

    let mut index = 0;
    while index < RecoveryClass::COUNT {
        assert!(RECOVERY_CONTRACTS[index].recovery_class.wire_code() as usize == index + 1);
        assert!(RecoveryClass::ALL[index].contract_complete());
        assert!(RecoveryClass::ALL[index].wire_code() as usize == index + 1);
        assert!(RecoveryClass::ALL[index].transition().wire_code() as usize == index + 1);
        assert!(RecoveryTransition::ALL[index].wire_code() as usize == index + 1);
        let mut previous = 0;
        while previous < index {
            assert!(!same_text(
                RecoveryClass::ALL[index].name(),
                RecoveryClass::ALL[previous].name()
            ));
            assert!(!same_text(
                RecoveryClass::ALL[index].hal_slug(),
                RecoveryClass::ALL[previous].hal_slug()
            ));
            assert!(!same_text(
                RecoveryTransition::ALL[index].name(),
                RecoveryTransition::ALL[previous].name()
            ));
            previous += 1;
        }
        index += 1;
    }
    index = 0;
    while index < RecoveryOperation::COUNT {
        assert!(valid_operation_id(RecoveryOperation::ALL[index].id()));
        assert!(RecoveryOperation::ALL[index].used_by_recovery_class());
        let mut previous = 0;
        while previous < index {
            assert!(!same_text(
                RecoveryOperation::ALL[index].id(),
                RecoveryOperation::ALL[previous].id()
            ));
            previous += 1;
        }
        index += 1;
    }
};
