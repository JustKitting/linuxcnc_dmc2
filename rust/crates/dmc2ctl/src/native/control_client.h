#ifndef DMC2_CONTROL_CLIENT_H
#define DMC2_CONTROL_CLIENT_H

#include <stddef.h>
#include <stdint.h>

#define DMC2_CONTROL_ABI_VERSION 0x00020910U
#define DMC2_CONTROL_FILE_LENGTH 255

typedef struct dmc2_control_status {
    uint32_t abi_version;
    uint32_t struct_size;
    int32_t task_state;
    int32_t task_mode;
    int32_t interpreter_state;
    int32_t execution_state;
    int32_t rcs_status;
    int32_t echo_serial_number;
    int32_t joint_count;
    uint32_t homed_mask;
    uint32_t homing_mask;
    double position_x;
    double position_y;
    double position_z;
    double spindle_speed;
    int32_t spindle_direction;
    int32_t auxiliary_estop;
    uint8_t loaded_file[DMC2_CONTROL_FILE_LENGTH];
} dmc2_control_status;

typedef struct dmc2_command_receipt {
    int32_t command_serial_number;
    int32_t echo_serial_number;
    int32_t rcs_status;
    int32_t nml_error;
    int32_t cms_status;
} dmc2_command_receipt;

typedef struct dmc2_control_session dmc2_control_session;

typedef enum dmc2_control_result {
    DMC2_CONTROL_OK = 0,
    DMC2_CONTROL_INVALID_ARGUMENT = 1,
    DMC2_CONTROL_OPEN_FAILED = 2,
    DMC2_CONTROL_STATUS_FAILED = 3,
    DMC2_CONTROL_WRITE_FAILED = 4,
    DMC2_CONTROL_TIMEOUT = 5,
    DMC2_CONTROL_REJECTED = 6
} dmc2_control_result;

#ifdef __cplusplus
extern "C" {
#define DMC2_NOEXCEPT noexcept
#else
#define DMC2_NOEXCEPT
#endif

uint32_t dmc2_control_abi_version(void) DMC2_NOEXCEPT;
/* Operator error only: opens no command channel and changes no machine state. */
int32_t dmc2_probe_capture_error(
    const char *nml_file,
    const char *message) DMC2_NOEXCEPT;
/* Read the original machine-frame G38 trigger; never read a post-stop position. */
int32_t dmc2_probe_capture_position(
    const char *nml_file,
    double xyz[3]) DMC2_NOEXCEPT;
size_t dmc2_control_status_size(void) DMC2_NOEXCEPT;
dmc2_control_session *dmc2_control_open(
    const char *nml_file,
    dmc2_command_receipt *diagnostic) DMC2_NOEXCEPT;
void dmc2_control_close(dmc2_control_session *session) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_read_status(
    dmc2_control_session *session,
    dmc2_control_status *status,
    dmc2_command_receipt *diagnostic) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_set_state(
    dmc2_control_session *session,
    int32_t state,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_set_mode(
    dmc2_control_session *session,
    int32_t mode,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_abort(
    dmc2_control_session *session,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_set_teleop(
    dmc2_control_session *session,
    int32_t enabled,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_home(
    dmc2_control_session *session,
    int32_t joint,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_program_close(
    dmc2_control_session *session,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_program_open(
    dmc2_control_session *session,
    const char *file,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;
dmc2_control_result dmc2_control_program_run(
    dmc2_control_session *session,
    int32_t line,
    dmc2_command_receipt *receipt) DMC2_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#undef DMC2_NOEXCEPT
#endif
