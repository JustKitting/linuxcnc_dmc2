#include "status_copy.hh"

#include <cstdint>
#include <cstring>
#include <limits>
#include <type_traits>

#include "emcmotcfg.h"
#include "motion.h"
#include "state_tag.h"

namespace {

static_assert(EMCMOT_MAX_JOINTS == DMC2_MAX_JOINTS);
static_assert(EMCMOT_MAX_AXIS == DMC2_MAX_AXES);
static_assert(EMCMOT_MAX_SPINDLES == DMC2_MAX_SPINDLES);
static_assert(EMCMOT_MAX_DIO == DMC2_MAX_DIGITAL_IO);
static_assert(EMCMOT_MAX_AIO == DMC2_MAX_ANALOG_IO);
static_assert(EMCMOT_MAX_MISC_ERROR == DMC2_MAX_MISC_ERRORS);
static_assert(LINELEN == DMC2_LINE_LENGTH);
static_assert(ACTIVE_G_CODES == DMC2_ACTIVE_G_CODES);
static_assert(ACTIVE_M_CODES == DMC2_ACTIVE_M_CODES);
static_assert(ACTIVE_SETTINGS == DMC2_ACTIVE_SETTINGS);
static_assert(GM_FIELD_FLOAT_MAX_FIELDS == DMC2_STATE_TAG_FLOAT_FIELDS);
static_assert(GM_FIELD_MAX_FIELDS == DMC2_STATE_TAG_FIELDS);
static_assert(sizeof(unsigned long int) == sizeof(std::uint64_t));
static_assert(sizeof(NMLTYPE) == sizeof(std::int32_t));
static_assert(sizeof(((RCS_STAT_MSG *)nullptr)->source_file) ==
              DMC2_SOURCE_FILE_LENGTH);
static_assert(sizeof(((EMC_TASK_STAT *)nullptr)->file) == DMC2_LINE_LENGTH);
static_assert(sizeof(((EMC_TASK_STAT *)nullptr)->command) == DMC2_LINE_LENGTH);
static_assert(sizeof(((EMC_TASK_STAT *)nullptr)->ini_filename) ==
              DMC2_LINE_LENGTH);
static_assert(std::numeric_limits<double>::is_iec559);

#ifdef TOOL_NML
#error "DMC2 status ABI requires LinuxCNC's installed mmap tool-table layout"
#endif

#define DMC2_REQUIRE_PLAIN_SNAPSHOT(type)                                      \
    static_assert(std::is_standard_layout_v<type>);                            \
    static_assert(std::is_trivially_copyable_v<type>)

DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_pose_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_state_tag_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_rcs_status_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_task_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_trajectory_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_joint_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_axis_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_spindle_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_tool_table_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_tool_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_aux_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_coolant_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_lube_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_io_snapshot);
DMC2_REQUIRE_PLAIN_SNAPSHOT(dmc2_task_status_snapshot);

#undef DMC2_REQUIRE_PLAIN_SNAPSHOT

std::uint32_t flag(bool value) noexcept { return value ? 1U : 0U; }

void copy_pose(const EmcPose &source, dmc2_pose_snapshot &destination) noexcept {
    destination.x = source.tran.x;
    destination.y = source.tran.y;
    destination.z = source.tran.z;
    destination.a = source.a;
    destination.b = source.b;
    destination.c = source.c;
    destination.u = source.u;
    destination.v = source.v;
    destination.w = source.w;
}

void copy_state_tag(
    const StateTag &source,
    dmc2_state_tag_snapshot &destination) noexcept {
    for (int index = 0; index < GM_FIELD_FLOAT_MAX_FIELDS; ++index) {
        destination.fields_float[index] = source.fields_float[index];
    }
    for (int index = 0; index < GM_FIELD_MAX_FIELDS; ++index) {
        destination.fields[index] = source.fields[index];
    }
    destination.packed_flags =
        static_cast<std::uint64_t>(source.packed_flags);
}

void copy_rcs_status(
    const RCS_STAT_MSG &source,
    dmc2_rcs_status_snapshot &destination) noexcept {
    destination.message_type = static_cast<std::int32_t>(source.type);
    destination.message_size = static_cast<std::int64_t>(source.size);
    destination.command_type = static_cast<std::int32_t>(source.command_type);
    destination.echo_serial_number = source.echo_serial_number;
    destination.status = source.status;
    destination.state = source.state;
    destination.line = source.line;
    destination.source_line = source.source_line;
    std::memcpy(
        destination.source_file,
        source.source_file,
        sizeof(destination.source_file));
    destination.reserved = 0;
}

void copy_task(
    const EMC_TASK_STAT &source,
    dmc2_task_snapshot &destination) noexcept {
    copy_rcs_status(source, destination.rcs);
    destination.heartbeat = source.heartbeat;
    destination.mode = source.mode;
    destination.state = source.state;
    destination.exec_state = source.execState;
    destination.interp_state = source.interpState;
    destination.call_level = source.callLevel;
    destination.motion_line = source.motionLine;
    destination.current_line = source.currentLine;
    destination.read_line = source.readLine;
    destination.optional_stop_state = flag(source.optional_stop_state);
    destination.block_delete_state = flag(source.block_delete_state);
    destination.input_timeout = flag(source.input_timeout);
    std::memcpy(destination.file, source.file, sizeof(destination.file));
    std::memcpy(destination.command, source.command, sizeof(destination.command));
    std::memcpy(
        destination.ini_filename,
        source.ini_filename,
        sizeof(destination.ini_filename));
    copy_pose(source.g5x_offset, destination.g5x_offset);
    destination.g5x_index = source.g5x_index;
    copy_pose(source.g92_offset, destination.g92_offset);
    destination.rotation_xy = source.rotation_xy;
    copy_pose(source.toolOffset, destination.tool_offset);
    for (int index = 0; index < ACTIVE_G_CODES; ++index) {
        destination.active_g_codes[index] = source.activeGCodes[index];
    }
    for (int index = 0; index < ACTIVE_M_CODES; ++index) {
        destination.active_m_codes[index] = source.activeMCodes[index];
    }
    for (int index = 0; index < ACTIVE_SETTINGS; ++index) {
        destination.active_settings[index] = source.activeSettings[index];
    }
    destination.program_units = source.programUnits;
    destination.interpreter_errcode = source.interpreter_errcode;
    destination.task_paused = source.task_paused;
    destination.delay_left = source.delayLeft;
    destination.queued_mdi_commands = source.queuedMDIcommands;
}

void copy_trajectory(
    const EMC_TRAJ_STAT &source,
    dmc2_trajectory_snapshot &destination) noexcept {
    copy_rcs_status(source, destination.rcs);
    destination.linear_units = source.linearUnits;
    destination.angular_units = source.angularUnits;
    destination.cycle_time = source.cycleTime;
    destination.joints = source.joints;
    destination.spindles = source.spindles;
    destination.axis_mask = source.axis_mask;
    destination.mode = source.mode;
    destination.enabled = flag(source.enabled);
    destination.in_position = flag(source.inpos);
    destination.queue = source.queue;
    destination.active_queue = source.activeQueue;
    destination.queue_full = flag(source.queueFull);
    destination.id = source.id;
    destination.paused = flag(source.paused);
    destination.scale = source.scale;
    destination.rapid_scale = source.rapid_scale;
    copy_pose(source.position, destination.position);
    copy_pose(source.actualPosition, destination.actual_position);
    destination.velocity = source.velocity;
    destination.acceleration = source.acceleration;
    destination.max_velocity = source.maxVelocity;
    destination.max_acceleration = source.maxAcceleration;
    copy_pose(source.probedPosition, destination.probed_position);
    destination.probe_tripped = flag(source.probe_tripped);
    destination.probing = flag(source.probing);
    destination.probe_value = source.probeval;
    destination.kinematics_type = source.kinematics_type;
    destination.motion_type = source.motion_type;
    destination.distance_to_go = source.distance_to_go;
    copy_pose(source.dtg, destination.dtg);
    destination.current_velocity = source.current_vel;
    destination.feed_override_enabled = flag(source.feed_override_enabled);
    destination.adaptive_feed_enabled = flag(source.adaptive_feed_enabled);
    destination.feed_hold_enabled = flag(source.feed_hold_enabled);
    copy_state_tag(source.tag, destination.state_tag);
}

void copy_joint(
    const EMC_JOINT_STAT &source,
    dmc2_joint_snapshot &destination) noexcept {
    copy_rcs_status(source, destination.rcs);
    destination.joint_number = source.joint;
    destination.joint_type = source.jointType;
    destination.units = source.units;
    destination.backlash = source.backlash;
    destination.min_position_limit = source.minPositionLimit;
    destination.max_position_limit = source.maxPositionLimit;
    destination.max_ferror = source.maxFerror;
    destination.min_ferror = source.minFerror;
    destination.ferror_current = source.ferrorCurrent;
    destination.ferror_high_mark = source.ferrorHighMark;
    destination.output = source.output;
    destination.input = source.input;
    destination.velocity = source.velocity;
    destination.in_position = flag(source.inpos != 0);
    destination.homing = flag(source.homing != 0);
    destination.homed = flag(source.homed != 0);
    destination.fault = flag(source.fault != 0);
    destination.enabled = flag(source.enabled != 0);
    destination.min_soft_limit = flag(source.minSoftLimit != 0);
    destination.max_soft_limit = flag(source.maxSoftLimit != 0);
    destination.min_hard_limit = flag(source.minHardLimit != 0);
    destination.max_hard_limit = flag(source.maxHardLimit != 0);
    destination.override_limits = flag(source.overrideLimits != 0);
}

void copy_axis(
    const EMC_AXIS_STAT &source,
    dmc2_axis_snapshot &destination) noexcept {
    copy_rcs_status(source, destination.rcs);
    destination.axis_number = source.axis;
    destination.min_position_limit = source.minPositionLimit;
    destination.max_position_limit = source.maxPositionLimit;
    destination.velocity = source.velocity;
}

void copy_spindle(
    const EMC_SPINDLE_STAT &source,
    dmc2_spindle_snapshot &destination) noexcept {
    copy_rcs_status(source, destination.rcs);
    destination.speed = source.speed;
    destination.spindle_scale = source.spindle_scale;
    destination.css_maximum = source.css_maximum;
    destination.css_factor = source.css_factor;
    destination.state = source.state;
    destination.direction = source.direction;
    destination.brake = source.brake;
    destination.increasing = source.increasing;
    destination.enabled = source.enabled;
    destination.orient_state = source.orient_state;
    destination.orient_fault = source.orient_fault;
    destination.override_enabled = flag(source.spindle_override_enabled);
    destination.homed = flag(source.homed);
}

void copy_tool_table(
    const CANON_TOOL_TABLE &source,
    dmc2_tool_table_snapshot &destination) noexcept {
    destination.tool_number = source.toolno;
    destination.pocket_number = source.pocketno;
    copy_pose(source.offset, destination.offset);
    destination.diameter = source.diameter;
    destination.front_angle = source.frontangle;
    destination.back_angle = source.backangle;
    destination.orientation = source.orientation;
}

void copy_io(
    const EMC_IO_STAT &source,
    dmc2_io_snapshot &destination) noexcept {
    copy_rcs_status(source, destination.rcs);
    destination.heartbeat = source.heartbeat;
    destination.cycle_time = source.cycleTime;
    destination.debug = source.debug;
    destination.reason = source.reason;
    destination.fault = source.fault;

    copy_rcs_status(source.tool, destination.tool.rcs);
    destination.tool.pocket_prepped = source.tool.pocketPrepped;
    destination.tool.tool_in_spindle = source.tool.toolInSpindle;
    destination.tool.tool_from_pocket = source.tool.toolFromPocket;
    copy_tool_table(source.tool.toolTableCurrent, destination.tool.current_tool);

    copy_rcs_status(source.coolant, destination.coolant.rcs);
    destination.coolant.mist = source.coolant.mist;
    destination.coolant.flood = source.coolant.flood;

    copy_rcs_status(source.aux, destination.aux.rcs);
    destination.aux.estop = source.aux.estop;

    copy_rcs_status(source.lube, destination.lube.rcs);
    destination.lube.on = source.lube.on;
    destination.lube.level = source.lube.level;
}

} // namespace

extern "C" void dmc2_task_status_snapshot_initialize(
    dmc2_task_status_snapshot *snapshot) noexcept {
    if (snapshot == nullptr) {
        return;
    }
    std::memset(snapshot, 0, sizeof(*snapshot));
    snapshot->abi_version = DMC2_SNAPSHOT_ABI_VERSION;
    snapshot->struct_size = sizeof(*snapshot);
}

void dmc2_copy_status(
    const EMC_STAT &source,
    dmc2_task_status_snapshot &destination) noexcept {
    dmc2_task_status_snapshot_initialize(&destination);
    copy_rcs_status(source, destination.top_rcs);
    copy_task(source.task, destination.task);
    copy_rcs_status(source.motion, destination.motion_rcs);
    destination.motion_heartbeat = source.motion.heartbeat;
    copy_trajectory(source.motion.traj, destination.trajectory);

    for (int index = 0; index < EMCMOT_MAX_JOINTS; ++index) {
        copy_joint(source.motion.joint[index], destination.joints[index]);
    }
    for (int index = 0; index < EMCMOT_MAX_AXIS; ++index) {
        copy_axis(source.motion.axis[index], destination.axes[index]);
    }
    for (int index = 0; index < EMCMOT_MAX_SPINDLES; ++index) {
        copy_spindle(source.motion.spindle[index], destination.spindles[index]);
    }
    for (int index = 0; index < EMCMOT_MAX_DIO; ++index) {
        destination.synchronized_digital_inputs[index] =
            source.motion.synch_di[index];
        destination.synchronized_digital_outputs[index] =
            source.motion.synch_do[index];
    }
    for (int index = 0; index < EMCMOT_MAX_AIO; ++index) {
        destination.analog_inputs[index] = source.motion.analog_input[index];
        destination.analog_outputs[index] = source.motion.analog_output[index];
    }
    for (int index = 0; index < EMCMOT_MAX_MISC_ERROR; ++index) {
        destination.misc_error[index] = source.motion.misc_error[index];
    }
    destination.motion_debug = source.motion.debug;
    destination.on_soft_limit = source.motion.on_soft_limit;
    destination.external_offsets_applied = source.motion.external_offsets_applied;
    copy_pose(source.motion.eoffset_pose, destination.external_offset_pose);
    destination.num_extra_joints = source.motion.numExtraJoints;
    destination.jogging_active = flag(source.motion.jogging_active);
    copy_io(source.io, destination.io);
    destination.top_debug = source.debug;
}
