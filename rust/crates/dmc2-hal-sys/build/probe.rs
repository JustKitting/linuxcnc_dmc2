use std::fs;
use std::path::Path;
use std::process::Command;

use super::config::INCLUDE_ROOT;
use super::process;

const ABI_CONTRACT: &str = r#"
#define RTAPI 1
#include <hal.h>
#include <stdbool.h>
#include <stdint.h>

typedef int (*expected_hal_init)(const char *);
typedef int (*expected_hal_component_call)(int);
typedef void *(*expected_hal_malloc)(long);
typedef int (*expected_hal_pin_bit_new)(
    const char *, hal_pin_dir_t, volatile bool **, int);
typedef int (*expected_hal_pin_float_new)(
    const char *, hal_pin_dir_t, volatile double **, int);
typedef int (*expected_hal_pin_s32_new)(
    const char *, hal_pin_dir_t, volatile int32_t **, int);
typedef int (*expected_hal_pin_u32_new)(
    const char *, hal_pin_dir_t, volatile uint32_t **, int);
typedef int (*expected_hal_param_float_new)(
    const char *, hal_param_dir_t, volatile double *, int);
typedef int (*expected_hal_param_u32_new)(
    const char *, hal_param_dir_t, volatile uint32_t *, int);
typedef void (*expected_hal_realtime_function)(void *, long);
typedef int (*expected_hal_export_funct)(
    const char *, expected_hal_realtime_function, void *, int, int, int);
typedef void (*expected_rtapi_print_msg)(msg_level_t, const char *, ...);

#define ABI_COMPATIBLE(actual, expected, label) \
    _Static_assert(__builtin_types_compatible_p(actual, expected), label)

ABI_COMPATIBLE(__typeof__(&hal_init), expected_hal_init,
               "hal_init signature changed");
ABI_COMPATIBLE(__typeof__(&hal_exit), expected_hal_component_call,
               "hal_exit signature changed");
ABI_COMPATIBLE(__typeof__(&hal_ready), expected_hal_component_call,
               "hal_ready signature changed");
ABI_COMPATIBLE(__typeof__(&hal_malloc), expected_hal_malloc,
               "hal_malloc signature changed");
ABI_COMPATIBLE(__typeof__(&hal_pin_bit_new), expected_hal_pin_bit_new,
               "hal_pin_bit_new signature changed");
ABI_COMPATIBLE(__typeof__(&hal_pin_float_new), expected_hal_pin_float_new,
               "hal_pin_float_new signature changed");
ABI_COMPATIBLE(__typeof__(&hal_pin_s32_new), expected_hal_pin_s32_new,
               "hal_pin_s32_new signature changed");
ABI_COMPATIBLE(__typeof__(&hal_pin_u32_new), expected_hal_pin_u32_new,
               "hal_pin_u32_new signature changed");
ABI_COMPATIBLE(__typeof__(&hal_param_float_new), expected_hal_param_float_new,
               "hal_param_float_new signature changed");
ABI_COMPATIBLE(__typeof__(&hal_param_u32_new), expected_hal_param_u32_new,
               "hal_param_u32_new signature changed");
ABI_COMPATIBLE(__typeof__(&hal_export_funct), expected_hal_export_funct,
               "hal_export_funct signature changed");
ABI_COMPATIBLE(__typeof__(&rtapi_print_msg), expected_rtapi_print_msg,
               "rtapi_print_msg signature changed");

ABI_COMPATIBLE(hal_bit_t *, volatile bool *, "hal_bit_t changed");
ABI_COMPATIBLE(hal_float_t *, volatile double *, "hal_float_t changed");
ABI_COMPATIBLE(hal_s32_t *, volatile int32_t *, "hal_s32_t changed");
ABI_COMPATIBLE(hal_u32_t *, volatile uint32_t *, "hal_u32_t changed");

_Static_assert(sizeof(hal_pin_dir_t) == sizeof(int),
               "hal_pin_dir_t width changed");
_Static_assert(HAL_IN == 16, "HAL_IN value changed");
_Static_assert(HAL_OUT == 32, "HAL_OUT value changed");
_Static_assert(HAL_IO == 48, "HAL_IO value changed");
_Static_assert(HAL_RW == 192, "HAL_RW value changed");
_Static_assert(RTAPI_MSG_ERR == 1, "RTAPI_MSG_ERR value changed");
_Static_assert(EPERM == 1, "EPERM value changed");
_Static_assert(ENOMEM == 12, "ENOMEM value changed");
_Static_assert(EINVAL == 22, "EINVAL value changed");
"#;

pub(crate) fn compile_c_abi_contract(output_directory: &Path) {
    let source = output_directory.join("hal_abi_contract.c");
    let object = output_directory.join("hal_abi_contract.o");
    fs::write(&source, ABI_CONTRACT)
        .unwrap_or_else(|error| panic!("failed to write HAL ABI contract: {error}"));
    process::run(
        Command::new("cc")
            .args(["-std=gnu11", "-Wall", "-Wextra", "-Werror", "-I"])
            .arg(INCLUDE_ROOT)
            .arg("-c")
            .arg(&source)
            .arg("-o")
            .arg(&object),
        "compiled LinuxCNC HAL C ABI contract",
    );
}
