#![no_std]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeName {
    pub code: i64,
    pub name: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct CodeDomain {
    pub name: &'static str,
    pub codes: &'static [CodeName],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageTemplate {
    pub name: &'static str,
    pub template: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusMessageContract {
    pub class_name: &'static str,
    pub message_type_name: &'static str,
    pub message_type: i64,
    pub message_size: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ErrorMessageContract {
    pub class_name: &'static str,
    pub message_type_name: &'static str,
    pub message_type: i64,
    pub message_size: usize,
    pub type_offset: usize,
    pub type_size: usize,
    pub size_offset: usize,
    pub size_size: usize,
    pub serial_member: Option<&'static str>,
    pub serial_offset: Option<usize>,
    pub serial_size: usize,
    pub payload_member: &'static str,
    pub payload_offset: usize,
    pub payload_size: usize,
    pub id_member: Option<&'static str>,
    pub id_offset: Option<usize>,
    pub id_size: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnumDomainContract {
    pub header_name: &'static str,
    pub declaration_kind: &'static str,
    pub declaration_name: &'static str,
    pub domain_name: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicEnumHeaderContract {
    pub header_name: &'static str,
    pub declaration_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PublicInteger {
    Signed(i128),
    Unsigned(u128),
}

impl PublicInteger {
    pub const fn as_i128(self) -> Option<i128> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Unsigned(value) if value <= i128::MAX as u128 => Some(value as i128),
            Self::Unsigned(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicMacroKind {
    Inactive,
    FunctionLike,
    ObjectWithoutValue,
    SignedInteger,
    UnsignedInteger,
    ObjectNotIntegerConstant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicMacroContract {
    pub header_name: &'static str,
    pub name: &'static str,
    pub declaration_count: usize,
    pub object_declaration_count: usize,
    pub function_declaration_count: usize,
    pub active_replacement: Option<&'static str>,
    pub kind: PublicMacroKind,
    pub value: Option<PublicInteger>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicHeaderContract {
    pub header_name: &'static str,
    pub source_relative_path: &'static str,
    pub source_byte_count: usize,
    pub source_fnv64: u64,
    pub macro_declaration_count: usize,
    pub macro_name_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalPinDirection {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HalValueType {
    Bit,
    S32,
    Float,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMotionHalPinContract {
    pub name_pattern: &'static str,
    pub value_type: HalValueType,
    /// Direction from LinuxCNC motion's perspective.
    pub direction: HalPinDirection,
}

/// Every LinuxCNC 2.9.10 HAL endpoint used by the realtime finite-jog path.
pub const NATIVE_MOTION_HAL_PINS: [NativeMotionHalPinContract; 18] = [
    NativeMotionHalPinContract {
        name_pattern: "motion.jog-stop",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "motion.jog-stop-immediate",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "motion.motion-enabled",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "motion.in-position",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "motion.coord-mode",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "motion.teleop-mode",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "motion.jog-is-active",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "axis.%c.jog-enable",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "axis.%c.jog-scale",
        value_type: HalValueType::Float,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "axis.%c.jog-counts",
        value_type: HalValueType::S32,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "axis.%c.jog-vel-mode",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "axis.%c.wheel-jog-active",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "joint.%d.jog-counts",
        value_type: HalValueType::S32,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "joint.%d.jog-enable",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "joint.%d.jog-scale",
        value_type: HalValueType::Float,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "joint.%d.jog-vel-mode",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Input,
    },
    NativeMotionHalPinContract {
        name_pattern: "joint.%d.wheel-jog-active",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
    NativeMotionHalPinContract {
        name_pattern: "joint.%d.in-position",
        value_type: HalValueType::Bit,
        direction: HalPinDirection::Output,
    },
];

impl CodeDomain {
    pub fn names(self, code: i64) -> impl Iterator<Item = &'static str> {
        self.codes
            .iter()
            .filter(move |entry| entry.code == code)
            .map(|entry| entry.name)
    }

    pub fn lookup(self, code: i64) -> Option<&'static str> {
        self.names(code).next()
    }

    pub fn contains(self, code: i64) -> bool {
        self.lookup(code).is_some()
    }
}

include!(concat!(env!("OUT_DIR"), "/linuxcnc_code_catalog.rs"));

pub fn domain(name: &str) -> Option<CodeDomain> {
    DOMAINS.iter().copied().find(|domain| domain.name == name)
}

pub fn status_message_contract(class_name: &str) -> Option<StatusMessageContract> {
    STATUS_MESSAGE_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.class_name == class_name)
}

pub fn error_message_contract(class_name: &str) -> Option<ErrorMessageContract> {
    ERROR_MESSAGE_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.class_name == class_name)
}

pub fn error_message_contract_by_type(message_type: i64) -> Option<ErrorMessageContract> {
    ERROR_MESSAGE_CONTRACTS
        .iter()
        .copied()
        .find(|contract| contract.message_type == message_type)
}

pub fn public_macro(header_name: &str, name: &str) -> Option<PublicMacroContract> {
    PUBLIC_MACROS
        .iter()
        .copied()
        .find(|contract| contract.header_name == header_name && contract.name == name)
}

pub fn public_integer_macro_names(
    header_name: &str,
    value: PublicInteger,
) -> impl Iterator<Item = &'static str> + '_ {
    PUBLIC_MACROS
        .iter()
        .filter(move |contract| {
            contract.header_name == header_name && contract.value == Some(value)
        })
        .map(|contract| contract.name)
}

pub fn public_integer_macro_names_i128(
    header_name: &str,
    value: i128,
) -> impl Iterator<Item = &'static str> + '_ {
    PUBLIC_MACROS
        .iter()
        .filter(move |contract| {
            contract.header_name == header_name
                && contract
                    .value
                    .and_then(PublicInteger::as_i128)
                    .is_some_and(|candidate| candidate == value)
        })
        .map(|contract| contract.name)
}

#[cfg(test)]
extern crate std;
