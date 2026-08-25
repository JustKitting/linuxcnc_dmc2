#include <cstddef>
#include <cstdint>
#include <new>
#include <type_traits>

#include "emc_nml.hh"

namespace {

constexpr std::uint32_t DMC2_SNAPSHOT_ABI_VERSION = 0x00020910U;
// LinuxCNC 2.9.10's installed linuxcnc.nml grants read access to emcStatus
// through the standard UI client identity used by AXIS, HALUI, and linuxcnc.stat.
constexpr const char *LINUXCNC_STATUS_CLIENT = "xemc";

struct dmc2_rcs_status_snapshot {
    std::int64_t command_type;
    std::int32_t echo_serial_number;
    std::int32_t status;
    std::int32_t state;
    std::int32_t reserved;
};

struct dmc2_task_snapshot {
    dmc2_rcs_status_snapshot rcs;
    std::uint32_t heartbeat;
    std::int32_t mode;
    std::int32_t state;
    std::int32_t exec_state;
    std::int32_t interp_state;
    std::int32_t program_units;
    std::int32_t interpreter_errcode;
    std::uint32_t input_timeout;
    std::uint32_t paused;
};

struct dmc2_trajectory_snapshot {
    dmc2_rcs_status_snapshot rcs;
    std::int32_t joints;
    std::int32_t spindles;
    std::int32_t axis_mask;
    std::int32_t mode;
    std::int32_t kinematics_type;
    std::int32_t motion_type;
    std::uint32_t enabled;
    std::uint32_t in_position;
    std::uint32_t queue_full;
    std::uint32_t paused;
    std::uint32_t probe_tripped;
    std::uint32_t probing;
    std::int32_t probe_value;
    std::uint32_t feed_override_enabled;
    std::uint32_t adaptive_feed_enabled;
    std::uint32_t feed_hold_enabled;
    std::uint64_t state_tag_flags;
};

struct dmc2_joint_snapshot {
    dmc2_rcs_status_snapshot rcs;
    std::int32_t joint_type;
    std::uint32_t in_position;
    std::uint32_t homing;
    std::uint32_t homed;
    std::uint32_t fault;
    std::uint32_t enabled;
    std::uint32_t min_soft_limit;
    std::uint32_t max_soft_limit;
    std::uint32_t min_hard_limit;
    std::uint32_t max_hard_limit;
    std::uint32_t override_limits;
};

struct dmc2_axis_snapshot {
    dmc2_rcs_status_snapshot rcs;
    std::uint32_t stopped;
};

struct dmc2_spindle_snapshot {
    dmc2_rcs_status_snapshot rcs;
    std::int32_t direction;
    std::int32_t brake;
    std::int32_t enabled;
    std::int32_t orient_state;
    std::int32_t orient_fault;
    std::uint32_t override_enabled;
};

struct dmc2_io_snapshot {
    dmc2_rcs_status_snapshot rcs;
    dmc2_rcs_status_snapshot tool_rcs;
    dmc2_rcs_status_snapshot aux_rcs;
    dmc2_rcs_status_snapshot coolant_rcs;
    dmc2_rcs_status_snapshot lube_rcs;
    std::uint32_t heartbeat;
    std::int32_t debug;
    std::int32_t reason;
    std::int32_t fault;
    std::int32_t estop;
    std::int32_t coolant_mist;
    std::int32_t coolant_flood;
    std::int32_t lube_on;
    std::int32_t lube_level;
};

struct dmc2_task_status_snapshot {
    std::uint32_t abi_version;
    std::uint32_t struct_size;
    dmc2_rcs_status_snapshot top_rcs;
    dmc2_task_snapshot task;
    dmc2_rcs_status_snapshot motion_rcs;
    std::uint32_t motion_heartbeat;
    std::int32_t motion_debug;
    std::int32_t num_extra_joints;
    std::uint32_t jogging_active;
    dmc2_trajectory_snapshot trajectory;
    dmc2_joint_snapshot joints[EMCMOT_MAX_JOINTS];
    dmc2_axis_snapshot axes[EMCMOT_MAX_AXIS];
    dmc2_spindle_snapshot spindles[EMCMOT_MAX_SPINDLES];
    std::int32_t misc_error[EMCMOT_MAX_MISC_ERROR];
    dmc2_io_snapshot io;
    std::int32_t top_debug;
};

static_assert(std::is_standard_layout_v<dmc2_rcs_status_snapshot>);
static_assert(std::is_standard_layout_v<dmc2_task_status_snapshot>);
static_assert(EMCMOT_MAX_JOINTS == 16);
static_assert(EMCMOT_MAX_AXIS == 9);
static_assert(EMCMOT_MAX_SPINDLES == 8);
static_assert(EMCMOT_MAX_MISC_ERROR == 64);
static_assert(sizeof(dmc2_task_status_snapshot) <= UINT32_MAX);

void copy_rcs_status(const RCS_STAT_MSG &source,
                     dmc2_rcs_status_snapshot &destination) noexcept {
    destination.command_type = static_cast<std::int64_t>(source.command_type);
    destination.echo_serial_number = source.echo_serial_number;
    destination.status = source.status;
    destination.state = source.state;
    destination.reserved = 0;
}

std::uint32_t flag(bool value) noexcept { return value ? 1U : 0U; }

void set_nml_error(std::int32_t *destination, NML_ERROR_TYPE error) noexcept {
    if (destination != nullptr) {
        *destination = static_cast<std::int32_t>(error);
    }
}

} // namespace

struct dmc2_task_status_channel {
    RCS_STAT_CHANNEL *channel;
    EMC_STAT *status;
    bool received_status;
};

extern "C" std::uint32_t dmc2_task_status_snapshot_abi_version() noexcept {
    return DMC2_SNAPSHOT_ABI_VERSION;
}

extern "C" std::size_t dmc2_task_status_snapshot_size() noexcept {
    return sizeof(dmc2_task_status_snapshot);
}

extern "C" dmc2_task_status_channel *dmc2_task_status_open(
    const char *nml_file, std::int32_t *nml_error) noexcept {
    set_nml_error(nml_error, NML_INVALID_CONFIGURATION);
    if (nml_file == nullptr || *nml_file == '\0') {
        return nullptr;
    }

    auto *holder =
        new (std::nothrow) dmc2_task_status_channel{nullptr, nullptr, false};
    if (holder == nullptr) {
        set_nml_error(nml_error, NML_INTERNAL_CMS_ERROR);
        return nullptr;
    }
    try {
        holder->channel = new RCS_STAT_CHANNEL(
            emcFormat, "emcStatus", LINUXCNC_STATUS_CLIENT, nml_file);
    } catch (...) {
        set_nml_error(nml_error, NML_INTERNAL_CMS_ERROR);
        delete holder;
        return nullptr;
    }
    if (holder->channel == nullptr || !holder->channel->valid()) {
        if (holder->channel != nullptr && holder->channel->error_type != NML_NO_ERROR) {
            set_nml_error(nml_error, holder->channel->error_type);
        }
        delete holder->channel;
        delete holder;
        return nullptr;
    }
    holder->status = static_cast<EMC_STAT *>(holder->channel->get_address());
    if (holder->status == nullptr) {
        set_nml_error(nml_error, NML_INTERNAL_CMS_ERROR);
        delete holder->channel;
        delete holder;
        return nullptr;
    }
    set_nml_error(nml_error, NML_NO_ERROR);
    return holder;
}

extern "C" int dmc2_task_status_poll(
    dmc2_task_status_channel *holder,
    dmc2_task_status_snapshot *snapshot, std::int32_t *nml_error) noexcept {
    set_nml_error(nml_error, NML_INVALID_CONFIGURATION);
    if (holder == nullptr || snapshot == nullptr || holder->channel == nullptr ||
        holder->status == nullptr || !holder->channel->valid()) {
        return -1;
    }

    const NMLTYPE type = holder->channel->peek();
    if (holder->channel->error_type != NML_NO_ERROR) {
        set_nml_error(nml_error, holder->channel->error_type);
        return -1;
    }
    if (type == EMC_STAT_TYPE) {
        holder->received_status = true;
    } else if (type == 0 && !holder->received_status) {
        // The channel exists, but LinuxCNC has not published its first status
        // frame.  This is startup latency, not a zero-valued machine status.
        set_nml_error(nml_error, NML_NO_ERROR);
        return 1;
    } else if (type != 0) {
        set_nml_error(nml_error, NML_INVALID_MESSAGE_ERROR);
        return -1;
    }

    *snapshot = {};
    snapshot->abi_version = DMC2_SNAPSHOT_ABI_VERSION;
    snapshot->struct_size = sizeof(*snapshot);

    const EMC_STAT &status = *holder->status;
    copy_rcs_status(status, snapshot->top_rcs);
    snapshot->top_debug = status.debug;

    copy_rcs_status(status.task, snapshot->task.rcs);
    snapshot->task.heartbeat = status.task.heartbeat;
    snapshot->task.mode = status.task.mode;
    snapshot->task.state = status.task.state;
    snapshot->task.exec_state = status.task.execState;
    snapshot->task.interp_state = status.task.interpState;
    snapshot->task.program_units = status.task.programUnits;
    snapshot->task.interpreter_errcode = status.task.interpreter_errcode;
    snapshot->task.input_timeout = flag(status.task.input_timeout);
    snapshot->task.paused = flag(status.task.task_paused != 0);

    copy_rcs_status(status.motion, snapshot->motion_rcs);
    snapshot->motion_heartbeat = status.motion.heartbeat;
    snapshot->motion_debug = status.motion.debug;
    snapshot->num_extra_joints = status.motion.numExtraJoints;
    snapshot->jogging_active = flag(status.motion.jogging_active);

    const EMC_TRAJ_STAT &trajectory = status.motion.traj;
    copy_rcs_status(trajectory, snapshot->trajectory.rcs);
    snapshot->trajectory.joints = trajectory.joints;
    snapshot->trajectory.spindles = trajectory.spindles;
    snapshot->trajectory.axis_mask = trajectory.axis_mask;
    snapshot->trajectory.mode = trajectory.mode;
    snapshot->trajectory.kinematics_type = trajectory.kinematics_type;
    snapshot->trajectory.motion_type = trajectory.motion_type;
    snapshot->trajectory.enabled = flag(trajectory.enabled);
    snapshot->trajectory.in_position = flag(trajectory.inpos);
    snapshot->trajectory.queue_full = flag(trajectory.queueFull);
    snapshot->trajectory.paused = flag(trajectory.paused);
    snapshot->trajectory.probe_tripped = flag(trajectory.probe_tripped);
    snapshot->trajectory.probing = flag(trajectory.probing);
    snapshot->trajectory.probe_value = trajectory.probeval;
    snapshot->trajectory.feed_override_enabled = flag(trajectory.feed_override_enabled);
    snapshot->trajectory.adaptive_feed_enabled = flag(trajectory.adaptive_feed_enabled);
    snapshot->trajectory.feed_hold_enabled = flag(trajectory.feed_hold_enabled);
    snapshot->trajectory.state_tag_flags =
        static_cast<std::uint64_t>(trajectory.tag.packed_flags);

    for (int index = 0; index < EMCMOT_MAX_JOINTS; ++index) {
        const EMC_JOINT_STAT &joint = status.motion.joint[index];
        dmc2_joint_snapshot &target = snapshot->joints[index];
        copy_rcs_status(joint, target.rcs);
        target.joint_type = joint.jointType;
        target.in_position = flag(joint.inpos != 0);
        target.homing = flag(joint.homing != 0);
        target.homed = flag(joint.homed != 0);
        target.fault = flag(joint.fault != 0);
        target.enabled = flag(joint.enabled != 0);
        target.min_soft_limit = flag(joint.minSoftLimit != 0);
        target.max_soft_limit = flag(joint.maxSoftLimit != 0);
        target.min_hard_limit = flag(joint.minHardLimit != 0);
        target.max_hard_limit = flag(joint.maxHardLimit != 0);
        target.override_limits = flag(joint.overrideLimits != 0);
    }

    for (int index = 0; index < EMCMOT_MAX_AXIS; ++index) {
        const EMC_AXIS_STAT &axis = status.motion.axis[index];
        dmc2_axis_snapshot &target = snapshot->axes[index];
        copy_rcs_status(axis, target.rcs);
        if ((trajectory.axis_mask & (1 << index)) == 0) {
            target.stopped = 1U;
            continue;
        }
        const bool joint_stopped = index >= trajectory.joints ||
            (status.motion.joint[index].inpos != 0 &&
             status.motion.joint[index].velocity >= -0.000001 &&
             status.motion.joint[index].velocity <= 0.000001);
        target.stopped = flag(joint_stopped && axis.velocity >= -0.000001 &&
                              axis.velocity <= 0.000001);
    }

    for (int index = 0; index < EMCMOT_MAX_SPINDLES; ++index) {
        const EMC_SPINDLE_STAT &spindle = status.motion.spindle[index];
        dmc2_spindle_snapshot &target = snapshot->spindles[index];
        copy_rcs_status(spindle, target.rcs);
        if (index >= trajectory.spindles) {
            continue;
        }
        target.direction = spindle.direction;
        target.brake = spindle.brake;
        target.enabled = spindle.enabled;
        target.orient_state = spindle.orient_state;
        target.orient_fault = spindle.orient_fault;
        target.override_enabled = flag(spindle.spindle_override_enabled);
    }

    for (int index = 0; index < EMCMOT_MAX_MISC_ERROR; ++index) {
        snapshot->misc_error[index] = status.motion.misc_error[index];
    }

    copy_rcs_status(status.io, snapshot->io.rcs);
    copy_rcs_status(status.io.tool, snapshot->io.tool_rcs);
    copy_rcs_status(status.io.aux, snapshot->io.aux_rcs);
    copy_rcs_status(status.io.coolant, snapshot->io.coolant_rcs);
    copy_rcs_status(status.io.lube, snapshot->io.lube_rcs);
    snapshot->io.heartbeat = status.io.heartbeat;
    snapshot->io.debug = status.io.debug;
    snapshot->io.reason = status.io.reason;
    snapshot->io.fault = status.io.fault;
    snapshot->io.estop = status.io.aux.estop;
    snapshot->io.coolant_mist = status.io.coolant.mist;
    snapshot->io.coolant_flood = status.io.coolant.flood;
    snapshot->io.lube_on = status.io.lube.on;
    snapshot->io.lube_level = status.io.lube.level;
    set_nml_error(nml_error, NML_NO_ERROR);
    return 0;
}

extern "C" void dmc2_task_status_close(
    dmc2_task_status_channel *holder) noexcept {
    if (holder == nullptr) {
        return;
    }
    delete holder->channel;
    delete holder;
}
