use core::mem;

use dmc2_linuxcnc_interface::{
    DOMAINS, ENUM_CODE_COUNT, ENUM_DECLARATION_COUNT, ERROR_MESSAGE_CONTRACT_COUNT,
    GENERATED_CODE_COUNT, INTERPRETER_ERROR_TEMPLATES, LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION,
    NON_ENUM_CODE_COUNT, PUBLIC_ENUM_HEADER_COUNT, STATUS_MESSAGE_CONTRACT_COUNT,
};

use crate::snapshot::{
    dmc2_task_status_copy_self_test, dmc2_task_status_copy_signature_rounds,
    dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size, NativeSnapshot,
    SNAPSHOT_ABI_VERSION, SNAPSHOT_FIELDS, SNAPSHOT_LOGICAL_FIELD_COUNT, SNAPSHOT_SCHEMA_FNV64,
};

struct AuditReport {
    native_snapshot_size: usize,
    snapshot_field_bytes: usize,
    snapshot_padding_bytes: usize,
    snapshot_copy_fields: usize,
    snapshot_copy_signature_rounds: u32,
}

fn snapshot_byte_coverage() -> Result<(usize, usize), String> {
    if SNAPSHOT_FIELDS.len() != SNAPSHOT_LOGICAL_FIELD_COUNT {
        return Err(format!(
            "snapshot field inventory has {} entries, expected {SNAPSHOT_LOGICAL_FIELD_COUNT}",
            SNAPSHOT_FIELDS.len()
        ));
    }
    let snapshot_size = mem::size_of::<NativeSnapshot>();
    let mut owners = vec![false; snapshot_size];
    for field in SNAPSHOT_FIELDS {
        if field.path.is_empty()
            || field.c_type.is_empty()
            || field.element_count == 0
            || field.byte_size == 0
        {
            return Err(format!("invalid snapshot field contract: {}", field.path));
        }
        let end = field
            .byte_offset
            .checked_add(field.byte_size)
            .ok_or_else(|| format!("snapshot field range overflow: {}", field.path))?;
        if end > snapshot_size {
            return Err(format!("snapshot field is outside ABI: {}", field.path));
        }
        for owned in &mut owners[field.byte_offset..end] {
            if *owned {
                return Err(format!("overlapping snapshot field: {}", field.path));
            }
            *owned = true;
        }
    }
    let field_bytes = owners.iter().filter(|owned| **owned).count();
    Ok((field_bytes, snapshot_size - field_bytes))
}

impl AuditReport {
    fn collect() -> Result<Self, String> {
        let native_abi = unsafe { dmc2_task_status_snapshot_abi_version() };
        let native_snapshot_size = unsafe { dmc2_task_status_snapshot_size() };
        let rust_snapshot_size = mem::size_of::<NativeSnapshot>();
        if native_abi != SNAPSHOT_ABI_VERSION || native_snapshot_size != rust_snapshot_size {
            return Err(format!(
                "native snapshot ABI mismatch: C++ version=0x{native_abi:08x} size={native_snapshot_size}, Rust version=0x{SNAPSHOT_ABI_VERSION:08x} size={rust_snapshot_size}"
            ));
        }
        let (snapshot_field_bytes, snapshot_padding_bytes) = snapshot_byte_coverage()?;
        if snapshot_field_bytes + snapshot_padding_bytes != native_snapshot_size {
            return Err("snapshot byte inventory does not cover the complete ABI".to_owned());
        }

        let mut snapshot_copy_fields = 0_u32;
        let mut failure_offset = usize::MAX;
        let copy_result = unsafe {
            dmc2_task_status_copy_self_test(&mut snapshot_copy_fields, &mut failure_offset)
        };
        if copy_result != 0 {
            return Err(format!(
                "native status copy failed in signature round {copy_result} at destination byte {failure_offset}"
            ));
        }
        if snapshot_copy_fields as usize != SNAPSHOT_LOGICAL_FIELD_COUNT {
            return Err(format!(
                "native status copy mapped {snapshot_copy_fields} fields, expected {SNAPSHOT_LOGICAL_FIELD_COUNT}"
            ));
        }
        let snapshot_copy_signature_rounds = unsafe { dmc2_task_status_copy_signature_rounds() };
        if snapshot_copy_signature_rounds == 0 {
            return Err("native status-copy test reports zero signature rounds".to_owned());
        }

        Ok(Self {
            native_snapshot_size,
            snapshot_field_bytes,
            snapshot_padding_bytes,
            snapshot_copy_fields: snapshot_copy_fields as usize,
            snapshot_copy_signature_rounds,
        })
    }

    fn print_human(&self) {
        println!(
            "dmc2-task-monitor: offline validation passed; LinuxCNC={} source={} catalog_domains={} catalog_codes={} enum_headers={} enum_declarations={} interpreter_errors={} status_messages={} error_messages={} snapshot_abi=0x{:08x} snapshot_size={} snapshot_fields={} copy_rounds={} all_bytes_accounted=1",
            LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
            DOMAINS.len(),
            GENERATED_CODE_COUNT,
            PUBLIC_ENUM_HEADER_COUNT,
            ENUM_DECLARATION_COUNT,
            INTERPRETER_ERROR_TEMPLATES.len(),
            STATUS_MESSAGE_CONTRACT_COUNT,
            ERROR_MESSAGE_CONTRACT_COUNT,
            SNAPSHOT_ABI_VERSION,
            self.native_snapshot_size,
            self.snapshot_copy_fields,
            self.snapshot_copy_signature_rounds,
        );
    }

    fn print_json(&self) {
        println!(
            concat!(
                "{{",
                "\"schema_version\":1,",
                "\"linuxcnc_version\":\"{}\",",
                "\"linuxcnc_source_commit\":\"{}\",",
                "\"catalog_domains\":{},",
                "\"catalog_codes\":{},",
                "\"enum_codes\":{},",
                "\"non_enum_codes\":{},",
                "\"public_enum_headers\":{},",
                "\"enum_declarations\":{},",
                "\"interpreter_error_templates\":{},",
                "\"status_message_contracts\":{},",
                "\"error_message_contracts\":{},",
                "\"snapshot_abi_version\":{},",
                "\"snapshot_schema_fnv64\":\"0x{:016x}\",",
                "\"snapshot_size\":{},",
                "\"snapshot_logical_fields\":{},",
                "\"snapshot_field_bytes\":{},",
                "\"snapshot_padding_bytes\":{},",
                "\"snapshot_copy_signature_rounds\":{},",
                "\"snapshot_copy_all_bytes\":true",
                "}}"
            ),
            LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
            DOMAINS.len(),
            GENERATED_CODE_COUNT,
            ENUM_CODE_COUNT,
            NON_ENUM_CODE_COUNT,
            PUBLIC_ENUM_HEADER_COUNT,
            ENUM_DECLARATION_COUNT,
            INTERPRETER_ERROR_TEMPLATES.len(),
            STATUS_MESSAGE_CONTRACT_COUNT,
            ERROR_MESSAGE_CONTRACT_COUNT,
            SNAPSHOT_ABI_VERSION,
            SNAPSHOT_SCHEMA_FNV64,
            self.native_snapshot_size,
            self.snapshot_copy_fields,
            self.snapshot_field_bytes,
            self.snapshot_padding_bytes,
            self.snapshot_copy_signature_rounds,
        );
    }
}

pub(super) fn run(json: bool) -> Result<(), String> {
    let report = AuditReport::collect()?;
    if json {
        report.print_json();
    } else {
        report.print_human();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_audit_executes_the_native_copy_and_accounts_for_every_byte() {
        let report = AuditReport::collect().unwrap();
        assert_eq!(report.native_snapshot_size, 11_672);
        assert_eq!(report.snapshot_copy_fields, 1_109);
        assert_eq!(report.snapshot_copy_signature_rounds, 21);
        assert_eq!(report.snapshot_field_bytes, 11_165);
        assert_eq!(report.snapshot_padding_bytes, 507);
        assert_eq!(
            report.snapshot_field_bytes + report.snapshot_padding_bytes,
            report.native_snapshot_size
        );
    }
}
