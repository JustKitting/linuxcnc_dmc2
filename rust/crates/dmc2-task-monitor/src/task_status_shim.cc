#include <cstdint>
#include <new>

#include "emc_nml.hh"

struct dmc2_task_status_channel {
    RCS_STAT_CHANNEL *channel;
    EMC_STAT *status;
};

struct dmc2_task_status_snapshot {
    std::uint32_t heartbeat;
    std::uint32_t machine_on;
    std::uint32_t estopped;
    std::uint32_t manual_mode;
    std::uint32_t joint_mode;
    std::uint32_t teleop_mode;
    std::uint32_t interp_idle;
    std::uint32_t homed[3];
    std::uint32_t homing[3];
    std::uint32_t axis_stopped[3];
};

extern "C" dmc2_task_status_channel *dmc2_task_status_open(
    const char *nml_file) noexcept {
    if (nml_file == nullptr || *nml_file == '\0') {
        return nullptr;
    }

    auto *holder = new (std::nothrow) dmc2_task_status_channel{nullptr, nullptr};
    if (holder == nullptr) {
        return nullptr;
    }
    try {
        holder->channel =
            new RCS_STAT_CHANNEL(emcFormat, "emcStatus", "dmc2-task-monitor", nml_file);
    } catch (...) {
        delete holder;
        return nullptr;
    }
    if (holder->channel == nullptr || !holder->channel->valid()) {
        delete holder->channel;
        delete holder;
        return nullptr;
    }
    holder->status = static_cast<EMC_STAT *>(holder->channel->get_address());
    if (holder->status == nullptr) {
        delete holder->channel;
        delete holder;
        return nullptr;
    }
    return holder;
}

extern "C" int dmc2_task_status_poll(
    dmc2_task_status_channel *holder,
    dmc2_task_status_snapshot *snapshot) noexcept {
    if (holder == nullptr || snapshot == nullptr || holder->channel == nullptr ||
        holder->status == nullptr || !holder->channel->valid()) {
        return -1;
    }

    const NMLTYPE type = holder->channel->peek();
    if (type != 0 && type != EMC_STAT_TYPE) {
        return -1;
    }
    snapshot->heartbeat = holder->status->task.heartbeat;
    snapshot->machine_on = holder->status->motion.traj.enabled ? 1U : 0U;
    snapshot->estopped = holder->status->io.aux.estop ? 1U : 0U;
    snapshot->manual_mode =
        holder->status->task.mode == EMC_TASK_MODE_MANUAL ? 1U : 0U;
    snapshot->joint_mode =
        holder->status->motion.traj.mode == EMC_TRAJ_MODE_FREE ? 1U : 0U;
    snapshot->teleop_mode =
        holder->status->motion.traj.mode == EMC_TRAJ_MODE_TELEOP ? 1U : 0U;
    snapshot->interp_idle =
        holder->status->task.interpState == EMC_TASK_INTERP_IDLE ? 1U : 0U;
    for (int index = 0; index < 3; ++index) {
        const auto &joint = holder->status->motion.joint[index];
        const auto &axis = holder->status->motion.axis[index];
        snapshot->homed[index] = joint.homed ? 1U : 0U;
        snapshot->homing[index] = joint.homing ? 1U : 0U;
        snapshot->axis_stopped[index] =
            joint.inpos && axis.velocity >= -0.000001 && axis.velocity <= 0.000001
                ? 1U
                : 0U;
    }
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
