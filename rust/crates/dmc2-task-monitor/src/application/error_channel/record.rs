use std::fmt;

use dmc2_linuxcnc_interface::{error_message_contract_by_type, ErrorMessageContract};

#[cfg(test)]
use dmc2_linuxcnc_interface::ERROR_MESSAGE_CONTRACTS;

use super::native::{RawErrorSnapshot, ERROR_MESSAGE_ABI_VERSION, ERROR_OBJECT_CAPACITY};

const BASE_TYPE_OFFSET: usize = 0;
const BASE_TYPE_SIZE: usize = 4;
const BASE_SIZE_OFFSET: usize = 8;
const BASE_SIZE_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::application) enum ErrorSeverity {
    Error,
    Info,
}

impl ErrorSeverity {
    pub(in crate::application) const fn journal_name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Info => "info",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::application) struct ErrorMessageRecord {
    pub(in crate::application) message_type: i32,
    pub(in crate::application) contract: Option<ErrorMessageContract>,
    pub(in crate::application) severity: ErrorSeverity,
    pub(in crate::application) object_size: usize,
    pub(in crate::application) declared_size: i64,
    pub(in crate::application) serial_number: Option<i32>,
    pub(in crate::application) operator_id: Option<i32>,
    pub(in crate::application) payload: Vec<u8>,
    pub(in crate::application) text: Vec<u8>,
    pub(in crate::application) padding: Vec<u8>,
    pub(in crate::application) object: [u8; ERROR_OBJECT_CAPACITY],
}

impl ErrorMessageRecord {
    pub(super) fn decode(snapshot: RawErrorSnapshot) -> Result<Self, DecodeError> {
        if snapshot.abi_version != ERROR_MESSAGE_ABI_VERSION {
            return Err(DecodeError::AbiVersion(snapshot.abi_version));
        }
        if usize::try_from(snapshot.struct_size).ok()
            != Some(std::mem::size_of::<RawErrorSnapshot>())
        {
            return Err(DecodeError::StructSize(snapshot.struct_size));
        }
        let object_size = usize::try_from(snapshot.object_size)
            .map_err(|_| DecodeError::ObjectSize(snapshot.object_size))?;
        if !(BASE_SIZE_OFFSET + BASE_SIZE_SIZE..=ERROR_OBJECT_CAPACITY).contains(&object_size) {
            return Err(DecodeError::ObjectSize(snapshot.object_size));
        }
        if snapshot.object[object_size..].iter().any(|byte| *byte != 0) {
            return Err(DecodeError::DirtyObjectTail);
        }

        let raw_type = read_i32(
            &snapshot.object[..object_size],
            BASE_TYPE_OFFSET,
            BASE_TYPE_SIZE,
            "type",
        )?;
        if raw_type != snapshot.message_type {
            return Err(DecodeError::MessageType {
                transport: snapshot.message_type,
                object: raw_type,
            });
        }
        let declared_size = read_i64(
            &snapshot.object[..object_size],
            BASE_SIZE_OFFSET,
            BASE_SIZE_SIZE,
            "size",
        )?;
        if declared_size <= 0 || usize::try_from(declared_size).ok() != Some(object_size) {
            return Err(DecodeError::DeclaredSize {
                declared: declared_size,
                object: object_size,
            });
        }

        let contract = error_message_contract_by_type(i64::from(raw_type));
        let mut owners = vec![false; object_size];
        claim(&mut owners, BASE_TYPE_OFFSET, BASE_TYPE_SIZE, "type")?;
        claim(&mut owners, BASE_SIZE_OFFSET, BASE_SIZE_SIZE, "size")?;

        let (severity, serial_number, operator_id, payload) = match contract {
            Some(contract) => {
                if contract.message_size != object_size
                    || contract.type_offset != BASE_TYPE_OFFSET
                    || contract.type_size != BASE_TYPE_SIZE
                    || contract.size_offset != BASE_SIZE_OFFSET
                    || contract.size_size != BASE_SIZE_SIZE
                {
                    return Err(DecodeError::ContractLayout(contract.class_name));
                }
                let serial_number = optional_i32(
                    &snapshot.object[..object_size],
                    contract.serial_offset,
                    contract.serial_size,
                    "serial_number",
                    &mut owners,
                )?;
                let operator_id = optional_i32(
                    &snapshot.object[..object_size],
                    contract.id_offset,
                    contract.id_size,
                    "id",
                    &mut owners,
                )?;
                claim(
                    &mut owners,
                    contract.payload_offset,
                    contract.payload_size,
                    contract.payload_member,
                )?;
                let payload = snapshot.object
                    [contract.payload_offset..contract.payload_offset + contract.payload_size]
                    .to_vec();
                let severity = if contract.class_name.ends_with("_ERROR") {
                    ErrorSeverity::Error
                } else {
                    ErrorSeverity::Info
                };
                (severity, serial_number, operator_id, payload)
            }
            None => {
                let uninterpreted_size = object_size - (BASE_SIZE_OFFSET + BASE_SIZE_SIZE);
                if uninterpreted_size > 0 {
                    claim(
                        &mut owners,
                        BASE_SIZE_OFFSET + BASE_SIZE_SIZE,
                        uninterpreted_size,
                        "uninterpreted",
                    )?;
                }
                (ErrorSeverity::Error, None, None, Vec::new())
            }
        };
        let text_length = payload
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(payload.len());
        let text = payload[..text_length].to_vec();
        let padding = owners
            .iter()
            .zip(snapshot.object[..object_size].iter())
            .filter_map(|(owned, byte)| (!owned).then_some(*byte))
            .collect();

        Ok(Self {
            message_type: raw_type,
            contract,
            severity,
            object_size,
            declared_size,
            serial_number,
            operator_id,
            payload,
            text,
            padding,
            object: snapshot.object,
        })
    }

    pub(in crate::application) fn class_name(&self) -> &'static str {
        self.contract
            .map(|contract| contract.class_name)
            .unwrap_or("UNKNOWN")
    }

    pub(in crate::application) fn known(&self) -> bool {
        self.contract.is_some()
    }
}

fn claim(
    owners: &mut [bool],
    offset: usize,
    size: usize,
    member: &'static str,
) -> Result<(), DecodeError> {
    if size == 0 {
        return Err(DecodeError::MemberRange(member));
    }
    let end = offset
        .checked_add(size)
        .ok_or(DecodeError::MemberRange(member))?;
    if end > owners.len() || owners[offset..end].iter().any(|owned| *owned) {
        return Err(DecodeError::MemberRange(member));
    }
    owners[offset..end].fill(true);
    Ok(())
}

fn bytes<'a>(
    object: &'a [u8],
    offset: usize,
    size: usize,
    member: &'static str,
) -> Result<&'a [u8], DecodeError> {
    let end = offset
        .checked_add(size)
        .ok_or(DecodeError::MemberRange(member))?;
    object
        .get(offset..end)
        .ok_or(DecodeError::MemberRange(member))
}

fn read_i32(
    object: &[u8],
    offset: usize,
    size: usize,
    member: &'static str,
) -> Result<i32, DecodeError> {
    let field: [u8; 4] = bytes(object, offset, size, member)?
        .try_into()
        .map_err(|_| DecodeError::MemberWidth(member))?;
    Ok(i32::from_ne_bytes(field))
}

fn read_i64(
    object: &[u8],
    offset: usize,
    size: usize,
    member: &'static str,
) -> Result<i64, DecodeError> {
    let field: [u8; 8] = bytes(object, offset, size, member)?
        .try_into()
        .map_err(|_| DecodeError::MemberWidth(member))?;
    Ok(i64::from_ne_bytes(field))
}

fn optional_i32(
    object: &[u8],
    offset: Option<usize>,
    size: usize,
    member: &'static str,
    owners: &mut [bool],
) -> Result<Option<i32>, DecodeError> {
    match offset {
        Some(offset) => {
            claim(owners, offset, size, member)?;
            read_i32(object, offset, size, member).map(Some)
        }
        None if size == 0 => Ok(None),
        None => Err(DecodeError::MemberRange(member)),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DecodeError {
    AbiVersion(u32),
    StructSize(u32),
    ObjectSize(u32),
    DirtyObjectTail,
    MessageType { transport: i32, object: i32 },
    DeclaredSize { declared: i64, object: usize },
    ContractLayout(&'static str),
    MemberRange(&'static str),
    MemberWidth(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbiVersion(value) => write!(formatter, "error-message ABI is 0x{value:08x}"),
            Self::StructSize(value) => write!(formatter, "error-message struct size is {value}"),
            Self::ObjectSize(value) => write!(formatter, "error-message object size is {value}"),
            Self::DirtyObjectTail => write!(formatter, "error-message object tail was not zero"),
            Self::MessageType { transport, object } => write!(
                formatter,
                "error-message transport type {transport} disagrees with object type {object}"
            ),
            Self::DeclaredSize { declared, object } => write!(
                formatter,
                "error-message declared size {declared} disagrees with copied size {object}"
            ),
            Self::ContractLayout(class_name) => {
                write!(formatter, "error-message contract changed for {class_name}")
            }
            Self::MemberRange(member) => {
                write!(formatter, "error-message member range is invalid: {member}")
            }
            Self::MemberWidth(member) => {
                write!(formatter, "error-message member width is invalid: {member}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_for(contract: ErrorMessageContract) -> RawErrorSnapshot {
        let mut snapshot = RawErrorSnapshot {
            abi_version: ERROR_MESSAGE_ABI_VERSION,
            struct_size: std::mem::size_of::<RawErrorSnapshot>() as u32,
            message_type: contract.message_type as i32,
            nml_error: 0,
            cms_status: 1,
            object_size: contract.message_size as u32,
            object: [0; ERROR_OBJECT_CAPACITY],
        };
        snapshot.object[contract.type_offset..contract.type_offset + contract.type_size]
            .copy_from_slice(&(contract.message_type as i32).to_ne_bytes());
        snapshot.object[contract.size_offset..contract.size_offset + contract.size_size]
            .copy_from_slice(&(contract.message_size as i64).to_ne_bytes());
        if let Some(offset) = contract.serial_offset {
            snapshot.object[offset..offset + contract.serial_size]
                .copy_from_slice(&0x1020_3040_i32.to_ne_bytes());
        }
        if let Some(offset) = contract.id_offset {
            snapshot.object[offset..offset + contract.id_size]
                .copy_from_slice(&(-123_i32).to_ne_bytes());
        }
        let payload = &mut snapshot.object
            [contract.payload_offset..contract.payload_offset + contract.payload_size];
        payload[..4].copy_from_slice(b"abc\0");
        for (index, byte) in payload[4..].iter_mut().enumerate() {
            *byte = (index as u8).wrapping_add(1);
        }
        snapshot
    }

    #[test]
    fn all_six_messages_preserve_every_field_payload_padding_and_object_byte() {
        for contract in ERROR_MESSAGE_CONTRACTS {
            let snapshot = snapshot_for(*contract);
            let record = ErrorMessageRecord::decode(snapshot).unwrap();
            assert_eq!(record.message_type, contract.message_type as i32);
            assert_eq!(record.contract, Some(*contract));
            assert_eq!(record.class_name(), contract.class_name);
            assert!(record.known());
            assert_eq!(record.object_size, contract.message_size);
            assert_eq!(record.declared_size, contract.message_size as i64);
            assert_eq!(record.text, b"abc");
            assert_eq!(record.payload.len(), contract.payload_size);
            assert_eq!(record.object, snapshot.object);
            assert_eq!(
                record.severity,
                if contract.class_name.ends_with("_ERROR") {
                    ErrorSeverity::Error
                } else {
                    ErrorSeverity::Info
                }
            );
            if contract.serial_offset.is_some() {
                assert_eq!(record.serial_number, Some(0x1020_3040));
                assert_eq!(record.operator_id, Some(-123));
                assert_eq!(record.padding.len(), 5);
            } else {
                assert_eq!(record.serial_number, None);
                assert_eq!(record.operator_id, None);
                assert_eq!(record.padding.len(), 4);
            }
        }
    }

    #[test]
    fn unknown_type_preserves_base_padding_and_every_remaining_byte() {
        let mut snapshot = snapshot_for(ERROR_MESSAGE_CONTRACTS[0]);
        snapshot.message_type = 77;
        snapshot.object[..4].copy_from_slice(&77_i32.to_ne_bytes());
        let record = ErrorMessageRecord::decode(snapshot).unwrap();
        assert!(!record.known());
        assert_eq!(record.class_name(), "UNKNOWN");
        assert_eq!(record.severity.journal_name(), "error");
        assert!(record.payload.is_empty());
        assert_eq!(record.padding.len(), 4);
    }

    #[test]
    fn every_corrupt_snapshot_boundary_is_rejected() {
        let valid = snapshot_for(ERROR_MESSAGE_CONTRACTS[0]);

        let mut changed = valid;
        changed.abi_version ^= 1;
        assert!(matches!(
            ErrorMessageRecord::decode(changed),
            Err(DecodeError::AbiVersion(_))
        ));

        let mut changed = valid;
        changed.struct_size -= 1;
        assert!(matches!(
            ErrorMessageRecord::decode(changed),
            Err(DecodeError::StructSize(_))
        ));

        for object_size in [0, 15, 281, u32::MAX] {
            let mut changed = valid;
            changed.object_size = object_size;
            assert!(matches!(
                ErrorMessageRecord::decode(changed),
                Err(DecodeError::ObjectSize(_))
            ));
        }

        let mut changed = valid;
        changed.object[valid.object_size as usize] = 1;
        assert_eq!(
            ErrorMessageRecord::decode(changed),
            Err(DecodeError::DirtyObjectTail)
        );

        let mut changed = valid;
        changed.message_type += 1;
        assert!(matches!(
            ErrorMessageRecord::decode(changed),
            Err(DecodeError::MessageType { .. })
        ));

        for declared in [i64::MIN, -1, 0, valid.object_size as i64 - 1, i64::MAX] {
            let mut changed = valid;
            changed.object[BASE_SIZE_OFFSET..BASE_SIZE_OFFSET + BASE_SIZE_SIZE]
                .copy_from_slice(&declared.to_ne_bytes());
            assert!(matches!(
                ErrorMessageRecord::decode(changed),
                Err(DecodeError::DeclaredSize { .. })
            ));
        }
    }
}
