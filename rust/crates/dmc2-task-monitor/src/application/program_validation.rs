mod interface;

use core::mem;

use dmc2_linuxcnc_interface::{LINUXCNC_SOURCE_COMMIT, LINUXCNC_VERSION};

use crate::snapshot::{
    dmc2_task_status_copy_self_test, dmc2_task_status_copy_signature_rounds,
    dmc2_task_status_snapshot_abi_version, dmc2_task_status_snapshot_size,
    snapshot_schema_fingerprint, NativeSnapshot, RUST_DERIVED_FIELD_COUNT, SNAPSHOT_ABI_VERSION,
    SNAPSHOT_FIELDS, SNAPSHOT_LOGICAL_FIELD_COUNT, SNAPSHOT_SCHEMA_FNV64,
    SNAPSHOT_SCHEMA_STRUCT_ALIGNMENT, SNAPSHOT_SCHEMA_STRUCT_SIZE,
};

struct ValidationReport {
    interface: interface::InterfaceCoverage,
    native_snapshot_size: usize,
    snapshot_field_bytes: usize,
    snapshot_padding_bytes: usize,
    snapshot_native_copy_fields: usize,
    snapshot_rust_derived_fields: usize,
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

impl ValidationReport {
    fn collect() -> Result<Self, String> {
        let interface = interface::InterfaceCoverage::collect()?;
        let native_abi = unsafe { dmc2_task_status_snapshot_abi_version() };
        let native_snapshot_size = unsafe { dmc2_task_status_snapshot_size() };
        let rust_snapshot_size = mem::size_of::<NativeSnapshot>();
        if native_abi != SNAPSHOT_ABI_VERSION || native_snapshot_size != rust_snapshot_size {
            return Err(format!(
                "native snapshot ABI mismatch: C++ version=0x{native_abi:08x} size={native_snapshot_size}, Rust version=0x{SNAPSHOT_ABI_VERSION:08x} size={rust_snapshot_size}"
            ));
        }
        if SNAPSHOT_SCHEMA_STRUCT_SIZE != rust_snapshot_size
            || SNAPSHOT_SCHEMA_STRUCT_ALIGNMENT != mem::align_of::<NativeSnapshot>()
        {
            return Err(format!(
                "generated snapshot layout mismatch: generated size={} alignment={}, Rust size={} alignment={}",
                SNAPSHOT_SCHEMA_STRUCT_SIZE,
                SNAPSHOT_SCHEMA_STRUCT_ALIGNMENT,
                rust_snapshot_size,
                mem::align_of::<NativeSnapshot>()
            ));
        }
        let recomputed_fingerprint = snapshot_schema_fingerprint();
        if recomputed_fingerprint != SNAPSHOT_SCHEMA_FNV64 {
            return Err(format!(
                "snapshot layout fingerprint mismatch: generated=0x{SNAPSHOT_SCHEMA_FNV64:016x} recomputed=0x{recomputed_fingerprint:016x}"
            ));
        }
        let (snapshot_field_bytes, snapshot_padding_bytes) = snapshot_byte_coverage()?;
        if snapshot_field_bytes + snapshot_padding_bytes != native_snapshot_size {
            return Err(
                "snapshot byte inventory does not cover the complete program ABI".to_owned(),
            );
        }

        let mut snapshot_native_copy_fields = 0_u32;
        let mut failure_offset = usize::MAX;
        let copy_result = unsafe {
            dmc2_task_status_copy_self_test(&mut snapshot_native_copy_fields, &mut failure_offset)
        };
        if copy_result != 0 {
            return Err(format!(
                "native status copy failed in signature round {copy_result} at destination byte {failure_offset}"
            ));
        }
        if snapshot_native_copy_fields as usize + RUST_DERIVED_FIELD_COUNT
            != SNAPSHOT_LOGICAL_FIELD_COUNT
        {
            return Err(format!(
                "program status mapping owns {} native-copy fields and {RUST_DERIVED_FIELD_COUNT} Rust-derived fields, expected {SNAPSHOT_LOGICAL_FIELD_COUNT} total fields",
                snapshot_native_copy_fields
            ));
        }
        let snapshot_copy_signature_rounds = unsafe { dmc2_task_status_copy_signature_rounds() };
        if snapshot_copy_signature_rounds == 0 {
            return Err("native status-copy test reports zero signature rounds".to_owned());
        }

        Ok(Self {
            interface,
            native_snapshot_size,
            snapshot_field_bytes,
            snapshot_padding_bytes,
            snapshot_native_copy_fields: snapshot_native_copy_fields as usize,
            snapshot_rust_derived_fields: RUST_DERIVED_FIELD_COUNT,
            snapshot_copy_signature_rounds,
        })
    }

    fn print_human(&self) {
        println!(
            "dmc2-task-monitor: program validation passed; linuxcnc_version={} source_commit={} interface_domains={} interface_codes={} handled_codes={} enum_declarations={} public_headers={} public_header_bytes={} public_header_fnv64=0x{:016x} public_macros={} macro_declarations={} integer_macros={} handled_integer_macros={} macro_kinds={}/{}/{}/{}/{}/{} interpreter_errors={} handled_interpreter_errors={} status_contracts={} error_contracts={} error_object_bytes={} error_field_bytes={} error_padding_bytes={} snapshot_abi=0x{:08x} snapshot_size={} snapshot_fields={} native_copy_fields={} rust_derived_fields={} copy_rounds={} all_codes_accounted=1 error_all_bytes_accounted=1 all_bytes_accounted=1",
            LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
            self.interface.domain_count,
            self.interface.code_count,
            self.interface.handled_code_count,
            self.interface.enum_declaration_count,
            self.interface.public_header_count,
            self.interface.public_header_source_byte_count,
            self.interface.public_header_source_fnv64,
            self.interface.public_macro_name_count,
            self.interface.public_macro_declaration_count,
            self.interface.public_integer_macro_count,
            self.interface.handled_public_integer_macro_count,
            self.interface.public_macro_kind_counts[0],
            self.interface.public_macro_kind_counts[1],
            self.interface.public_macro_kind_counts[2],
            self.interface.public_macro_kind_counts[3],
            self.interface.public_macro_kind_counts[4],
            self.interface.public_macro_kind_counts[5],
            self.interface.interpreter_error_count,
            self.interface.handled_interpreter_error_count,
            self.interface.status_message_count,
            self.interface.error_message_count,
            self.interface.error_message_object_bytes,
            self.interface.error_message_field_bytes,
            self.interface.error_message_padding_bytes,
            SNAPSHOT_ABI_VERSION,
            self.native_snapshot_size,
            self.snapshot_native_copy_fields + self.snapshot_rust_derived_fields,
            self.snapshot_native_copy_fields,
            self.snapshot_rust_derived_fields,
            self.snapshot_copy_signature_rounds,
        );
    }

    fn print_json(&self) {
        println!(
            concat!(
                "{{",
                "\"schema_version\":4,",
                "\"linuxcnc_version\":\"{}\",",
                "\"linuxcnc_source_commit\":\"{}\",",
                "\"interface_domains\":{},",
                "\"interface_codes\":{},",
                "\"interface_handled_codes\":{},",
                "\"interface_enum_codes\":{},",
                "\"interface_non_enum_codes\":{},",
                "\"interface_enum_headers\":{},",
                "\"interface_enum_declarations\":{},",
                "\"public_headers\":{},",
                "\"public_header_source_fnv64\":\"0x{:016x}\",",
                "\"public_header_source_bytes\":{},",
                "\"public_macro_declarations\":{},",
                "\"public_macros\":{},",
                "\"public_macro_inactive\":{},",
                "\"public_macro_function_like\":{},",
                "\"public_macro_object_without_value\":{},",
                "\"public_macro_signed_integer\":{},",
                "\"public_macro_unsigned_integer\":{},",
                "\"public_macro_not_integer_constant\":{},",
                "\"public_integer_macros\":{},",
                "\"handled_public_integer_macros\":{},",
                "\"interpreter_errors\":{},",
                "\"handled_interpreter_errors\":{},",
                "\"status_message_contracts\":{},",
                "\"error_message_contracts\":{},",
                "\"error_message_object_bytes\":{},",
                "\"error_message_field_bytes\":{},",
                "\"error_message_padding_bytes\":{},",
                "\"snapshot_abi_version\":{},",
                "\"snapshot_schema_fnv64\":\"0x{:016x}\",",
                "\"snapshot_size\":{},",
                "\"snapshot_logical_fields\":{},",
                "\"snapshot_native_copy_fields\":{},",
                "\"snapshot_rust_derived_fields\":{},",
                "\"snapshot_field_bytes\":{},",
                "\"snapshot_padding_bytes\":{},",
                "\"snapshot_copy_signature_rounds\":{},",
                "\"interface_all_codes_accounted\":true,",
                "\"interface_all_public_macros_classified\":true,",
                "\"error_message_all_bytes_accounted\":true,",
                "\"snapshot_copy_all_bytes\":true",
                "}}"
            ),
            LINUXCNC_VERSION,
            LINUXCNC_SOURCE_COMMIT,
            self.interface.domain_count,
            self.interface.code_count,
            self.interface.handled_code_count,
            self.interface.enum_code_count,
            self.interface.non_enum_code_count,
            self.interface.enum_header_count,
            self.interface.enum_declaration_count,
            self.interface.public_header_count,
            self.interface.public_header_source_fnv64,
            self.interface.public_header_source_byte_count,
            self.interface.public_macro_declaration_count,
            self.interface.public_macro_name_count,
            self.interface.public_macro_kind_counts[0],
            self.interface.public_macro_kind_counts[1],
            self.interface.public_macro_kind_counts[2],
            self.interface.public_macro_kind_counts[3],
            self.interface.public_macro_kind_counts[4],
            self.interface.public_macro_kind_counts[5],
            self.interface.public_integer_macro_count,
            self.interface.handled_public_integer_macro_count,
            self.interface.interpreter_error_count,
            self.interface.handled_interpreter_error_count,
            self.interface.status_message_count,
            self.interface.error_message_count,
            self.interface.error_message_object_bytes,
            self.interface.error_message_field_bytes,
            self.interface.error_message_padding_bytes,
            SNAPSHOT_ABI_VERSION,
            SNAPSHOT_SCHEMA_FNV64,
            self.native_snapshot_size,
            self.snapshot_native_copy_fields + self.snapshot_rust_derived_fields,
            self.snapshot_native_copy_fields,
            self.snapshot_rust_derived_fields,
            self.snapshot_field_bytes,
            self.snapshot_padding_bytes,
            self.snapshot_copy_signature_rounds,
        );
    }
}

pub(super) fn run(json: bool) -> Result<(), String> {
    let report = ValidationReport::collect()?;
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
    fn validation_executes_the_native_copy_and_accounts_for_every_program_byte() {
        let report = ValidationReport::collect().unwrap();
        assert_eq!(report.interface.domain_count, 91);
        assert_eq!(report.interface.code_count, 920);
        assert_eq!(report.interface.handled_code_count, 920);
        assert_eq!(report.interface.enum_code_count, 711);
        assert_eq!(report.interface.non_enum_code_count, 209);
        assert_eq!(report.interface.enum_header_count, 30);
        assert_eq!(report.interface.enum_declaration_count, 79);
        assert_eq!(report.interface.public_header_count, 120);
        assert_eq!(report.interface.public_header_source_byte_count, 635_278);
        assert_eq!(
            report.interface.public_header_source_fnv64,
            0x8f2986fcf6b52329
        );
        assert_eq!(report.interface.public_macro_declaration_count, 1_106);
        assert_eq!(report.interface.public_macro_name_count, 1_029);
        assert_eq!(
            report.interface.public_macro_kind_counts,
            [86, 120, 126, 166, 315, 216]
        );
        assert_eq!(report.interface.public_integer_macro_count, 481);
        assert_eq!(report.interface.handled_public_integer_macro_count, 481);
        assert_eq!(report.interface.interpreter_error_count, 198);
        assert_eq!(report.interface.handled_interpreter_error_count, 198);
        assert_eq!(report.interface.status_message_count, 12);
        assert_eq!(report.interface.error_message_count, 6);
        assert_eq!(report.interface.error_message_object_bytes, 1_656);
        assert_eq!(report.interface.error_message_field_bytes, 1_629);
        assert_eq!(report.interface.error_message_padding_bytes, 27);
        assert_eq!(
            report.interface.error_message_field_bytes
                + report.interface.error_message_padding_bytes,
            report.interface.error_message_object_bytes
        );
        assert_eq!(report.native_snapshot_size, 11_672);
        assert_eq!(report.snapshot_native_copy_fields, 1_100);
        assert_eq!(report.snapshot_rust_derived_fields, 9);
        assert_eq!(
            report.snapshot_native_copy_fields + report.snapshot_rust_derived_fields,
            SNAPSHOT_LOGICAL_FIELD_COUNT
        );
        assert_eq!(report.snapshot_copy_signature_rounds, 21);
        assert_eq!(report.snapshot_field_bytes, 11_165);
        assert_eq!(report.snapshot_padding_bytes, 507);
        assert_eq!(
            report.snapshot_field_bytes + report.snapshot_padding_bytes,
            report.native_snapshot_size
        );
    }
}
