#ifndef DMC2_STATUS_SNAPSHOT_H
#define DMC2_STATUS_SNAPSHOT_H

#include <stddef.h>
#include <stdint.h>

#define DMC2_SNAPSHOT_ABI_VERSION 0x00020911U
#define DMC2_SOURCE_FILE_LENGTH 64
#define DMC2_LINE_LENGTH 255
#define DMC2_ACTIVE_G_CODES 17
#define DMC2_ACTIVE_M_CODES 10
#define DMC2_ACTIVE_SETTINGS 5
#define DMC2_STATE_TAG_FLOAT_FIELDS 5
#define DMC2_STATE_TAG_FIELDS 8
#define DMC2_MAX_JOINTS 16
#define DMC2_MAX_AXES 9
#define DMC2_MAX_SPINDLES 8
#define DMC2_MAX_DIGITAL_IO 64
#define DMC2_MAX_ANALOG_IO 64
#define DMC2_MAX_MISC_ERRORS 64

typedef struct dmc2_pose_snapshot {
    double x;
    double y;
    double z;
    double a;
    double b;
    double c;
    double u;
    double v;
    double w;
} dmc2_pose_snapshot;

typedef struct dmc2_state_tag_snapshot {
    float fields_float[DMC2_STATE_TAG_FLOAT_FIELDS];
    int32_t fields[DMC2_STATE_TAG_FIELDS];
    uint64_t packed_flags;
} dmc2_state_tag_snapshot;

typedef struct dmc2_rcs_status_snapshot {
    int32_t message_type;
    int64_t message_size;
    int32_t command_type;
    int32_t echo_serial_number;
    int32_t status;
    int32_t state;
    int32_t line;
    int32_t source_line;
    uint8_t source_file[DMC2_SOURCE_FILE_LENGTH];
    uint32_t reserved;
} dmc2_rcs_status_snapshot;

typedef struct dmc2_task_snapshot {
    dmc2_rcs_status_snapshot rcs;
    uint32_t heartbeat;
    int32_t mode;
    int32_t state;
    int32_t exec_state;
    int32_t interp_state;
    int32_t call_level;
    int32_t motion_line;
    int32_t current_line;
    int32_t read_line;
    uint32_t optional_stop_state;
    uint32_t block_delete_state;
    uint32_t input_timeout;
    uint8_t file[DMC2_LINE_LENGTH];
    uint8_t command[DMC2_LINE_LENGTH];
    uint8_t ini_filename[DMC2_LINE_LENGTH];
    dmc2_pose_snapshot g5x_offset;
    int32_t g5x_index;
    dmc2_pose_snapshot g92_offset;
    double rotation_xy;
    dmc2_pose_snapshot tool_offset;
    int32_t active_g_codes[DMC2_ACTIVE_G_CODES];
    int32_t active_m_codes[DMC2_ACTIVE_M_CODES];
    double active_settings[DMC2_ACTIVE_SETTINGS];
    int32_t program_units;
    int32_t interpreter_errcode;
    int32_t task_paused;
    double delay_left;
    int32_t queued_mdi_commands;
} dmc2_task_snapshot;

typedef struct dmc2_trajectory_snapshot {
    dmc2_rcs_status_snapshot rcs;
    double linear_units;
    double angular_units;
    double cycle_time;
    int32_t joints;
    int32_t spindles;
    int32_t axis_mask;
    int32_t mode;
    uint32_t enabled;
    uint32_t in_position;
    int32_t queue;
    int32_t active_queue;
    uint32_t queue_full;
    int32_t id;
    uint32_t paused;
    double scale;
    double rapid_scale;
    dmc2_pose_snapshot position;
    dmc2_pose_snapshot actual_position;
    double velocity;
    double acceleration;
    double max_velocity;
    double max_acceleration;
    dmc2_pose_snapshot probed_position;
    uint32_t probe_tripped;
    uint32_t probing;
    int32_t probe_value;
    int32_t kinematics_type;
    int32_t motion_type;
    double distance_to_go;
    dmc2_pose_snapshot dtg;
    double current_velocity;
    uint32_t feed_override_enabled;
    uint32_t adaptive_feed_enabled;
    uint32_t feed_hold_enabled;
    dmc2_state_tag_snapshot state_tag;
} dmc2_trajectory_snapshot;

typedef struct dmc2_joint_snapshot {
    dmc2_rcs_status_snapshot rcs;
    int32_t joint_number;
    int32_t joint_type;
    double units;
    double backlash;
    double min_position_limit;
    double max_position_limit;
    double max_ferror;
    double min_ferror;
    double ferror_current;
    double ferror_high_mark;
    double output;
    double input;
    double velocity;
    uint32_t in_position;
    uint32_t homing;
    uint32_t homed;
    uint32_t fault;
    uint32_t enabled;
    uint32_t min_soft_limit;
    uint32_t max_soft_limit;
    uint32_t min_hard_limit;
    uint32_t max_hard_limit;
    uint32_t override_limits;
} dmc2_joint_snapshot;

typedef struct dmc2_axis_snapshot {
    dmc2_rcs_status_snapshot rcs;
    int32_t axis_number;
    double min_position_limit;
    double max_position_limit;
    double velocity;
    uint32_t stopped;
} dmc2_axis_snapshot;

typedef struct dmc2_spindle_snapshot {
    dmc2_rcs_status_snapshot rcs;
    double speed;
    double spindle_scale;
    double css_maximum;
    double css_factor;
    int32_t state;
    int32_t direction;
    int32_t brake;
    int32_t increasing;
    int32_t enabled;
    int32_t orient_state;
    int32_t orient_fault;
    uint32_t override_enabled;
    uint32_t homed;
} dmc2_spindle_snapshot;

typedef struct dmc2_tool_table_snapshot {
    int32_t tool_number;
    int32_t pocket_number;
    dmc2_pose_snapshot offset;
    double diameter;
    double front_angle;
    double back_angle;
    int32_t orientation;
} dmc2_tool_table_snapshot;

typedef struct dmc2_tool_snapshot {
    dmc2_rcs_status_snapshot rcs;
    int32_t pocket_prepped;
    int32_t tool_in_spindle;
    int32_t tool_from_pocket;
    dmc2_tool_table_snapshot current_tool;
} dmc2_tool_snapshot;

typedef struct dmc2_aux_snapshot {
    dmc2_rcs_status_snapshot rcs;
    int32_t estop;
} dmc2_aux_snapshot;

typedef struct dmc2_coolant_snapshot {
    dmc2_rcs_status_snapshot rcs;
    int32_t mist;
    int32_t flood;
} dmc2_coolant_snapshot;

typedef struct dmc2_lube_snapshot {
    dmc2_rcs_status_snapshot rcs;
    int32_t on;
    int32_t level;
} dmc2_lube_snapshot;

typedef struct dmc2_io_snapshot {
    dmc2_rcs_status_snapshot rcs;
    uint32_t heartbeat;
    double cycle_time;
    int32_t debug;
    int32_t reason;
    int32_t fault;
    dmc2_tool_snapshot tool;
    dmc2_coolant_snapshot coolant;
    dmc2_aux_snapshot aux;
    dmc2_lube_snapshot lube;
} dmc2_io_snapshot;

typedef struct dmc2_task_status_snapshot {
    uint32_t abi_version;
    uint32_t struct_size;
    dmc2_rcs_status_snapshot top_rcs;
    dmc2_task_snapshot task;
    dmc2_rcs_status_snapshot motion_rcs;
    uint32_t motion_heartbeat;
    dmc2_trajectory_snapshot trajectory;
    dmc2_joint_snapshot joints[DMC2_MAX_JOINTS];
    dmc2_axis_snapshot axes[DMC2_MAX_AXES];
    dmc2_spindle_snapshot spindles[DMC2_MAX_SPINDLES];
    int32_t synchronized_digital_inputs[DMC2_MAX_DIGITAL_IO];
    int32_t synchronized_digital_outputs[DMC2_MAX_DIGITAL_IO];
    double analog_inputs[DMC2_MAX_ANALOG_IO];
    double analog_outputs[DMC2_MAX_ANALOG_IO];
    int32_t misc_error[DMC2_MAX_MISC_ERRORS];
    int32_t motion_debug;
    int32_t on_soft_limit;
    int32_t external_offsets_applied;
    dmc2_pose_snapshot external_offset_pose;
    int32_t num_extra_joints;
    uint32_t jogging_active;
    dmc2_io_snapshot io;
    int32_t top_debug;
} dmc2_task_status_snapshot;

typedef struct dmc2_task_status_channel dmc2_task_status_channel;

typedef enum dmc2_task_status_native_result {
    DMC2_TASK_STATUS_NATIVE_ERROR = -1,
    DMC2_TASK_STATUS_NATIVE_OK = 0
} dmc2_task_status_native_result;

#ifdef __cplusplus
extern "C" {
#define DMC2_NOEXCEPT noexcept
#else
#define DMC2_NOEXCEPT
#endif

uint32_t dmc2_task_status_snapshot_abi_version(void) DMC2_NOEXCEPT;
size_t dmc2_task_status_snapshot_size(void) DMC2_NOEXCEPT;
void dmc2_task_status_snapshot_initialize(
    dmc2_task_status_snapshot *snapshot) DMC2_NOEXCEPT;
int dmc2_task_status_copy_self_test(
    uint32_t *native_copy_fields,
    size_t *failure_offset) DMC2_NOEXCEPT;
uint32_t dmc2_task_status_copy_signature_rounds(void) DMC2_NOEXCEPT;
dmc2_task_status_channel *dmc2_task_status_open(
    const char *nml_file,
    int32_t *nml_error) DMC2_NOEXCEPT;
dmc2_task_status_native_result dmc2_task_status_observe(
    dmc2_task_status_channel *channel,
    int32_t *message_type,
    int32_t *nml_error) DMC2_NOEXCEPT;
dmc2_task_status_native_result dmc2_task_status_copy(
    dmc2_task_status_channel *channel,
    dmc2_task_status_snapshot *snapshot) DMC2_NOEXCEPT;
void dmc2_task_status_close(
    dmc2_task_status_channel *channel) DMC2_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#undef DMC2_NOEXCEPT

#endif
