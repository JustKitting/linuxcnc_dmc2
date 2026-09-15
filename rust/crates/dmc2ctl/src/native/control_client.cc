#include "control_client.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstring>
#include <new>
#include <thread>

#include "cms.hh"
#include "emc_nml.hh"

extern "C" int32_t dmc2_probe_capture_position(
    const char *nml_file, double xyz[3]) noexcept {
    if (nml_file == nullptr || xyz == nullptr) return -1;
    try {
        RCS_STAT_CHANNEL channel(emcFormat, "emcStatus", "xemc", nml_file);
        if (!channel.valid()) return -1;
        const NMLTYPE type = channel.peek();
        if (channel.error_type != NML_NO_ERROR ||
            (type != 0 && type != EMC_STAT_TYPE)) return -1;
        const auto *status = static_cast<const EMC_STAT *>(channel.get_address());
        if (status == nullptr || !status->motion.traj.probe_tripped) return -1;
        xyz[0] = status->motion.traj.probedPosition.tran.x;
        xyz[1] = status->motion.traj.probedPosition.tran.y;
        xyz[2] = status->motion.traj.probedPosition.tran.z;
        return 0;
    } catch (...) {
        return -1;
    }
}

// Status only. Return the exact byte length; no fixed Rust-side path buffer,
// command client, current-plan registry or machine state mutation is involved.
extern "C" int32_t dmc2_probe_capture_program(
    const char *nml_file, char *path, size_t capacity) noexcept {
    if (nml_file == nullptr) return -1;
    try {
        RCS_STAT_CHANNEL channel(emcFormat, "emcStatus", "xemc", nml_file);
        if (!channel.valid()) return -1;
        const NMLTYPE type = channel.peek();
        if (channel.error_type != NML_NO_ERROR ||
            (type != 0 && type != EMC_STAT_TYPE)) return -1;
        const auto *status = static_cast<const EMC_STAT *>(channel.get_address());
        if (status == nullptr) return -1;
        const auto length = strnlen(status->task.file, sizeof(status->task.file));
        if (length == 0 || length == sizeof(status->task.file)) return -1;
        if (path != nullptr) {
            if (capacity != length) return -1;
            std::memcpy(path, status->task.file, length);
        }
        return static_cast<int32_t>(length);
    } catch (...) {
        return -1;
    }
}

extern "C" int32_t dmc2_probe_capture_error(
    const char *nml_file, const char *message) noexcept {
    if (nml_file == nullptr || message == nullptr) return -1;
    try {
        // The stock NML configuration grants the tool process write access to
        // emcError. This is a diagnostic writer, never a command-channel client.
        NML channel(emcFormat, "emcError", "tool", nml_file);
        if (!channel.valid()) return -1;
        EMC_OPERATOR_ERROR error;
        error.id = 0;
        std::strncpy(error.error, message, sizeof(error.error) - 1);
        error.error[sizeof(error.error) - 1] = '\0';
        return channel.write(&error);
    } catch (...) {
        return -1;
    }
}

namespace {

constexpr const char *CLIENT_NAME = "xemc";
constexpr auto POLL_PERIOD = std::chrono::milliseconds(10);
constexpr auto COMMAND_TIMEOUT = std::chrono::seconds(5);

void initialize_receipt(dmc2_command_receipt *receipt) noexcept {
    if (receipt == nullptr) {
        return;
    }
    std::memset(receipt, 0, sizeof(*receipt));
    receipt->nml_error = static_cast<std::int32_t>(NML_INVALID_CONFIGURATION);
    receipt->cms_status = static_cast<std::int32_t>(CMS_STATUS_NOT_SET);
}

CMS_STATUS cms_status(const RCS_STAT_CHANNEL *channel) noexcept {
    if (channel == nullptr || channel->cms == nullptr) {
        return CMS_STATUS_NOT_SET;
    }
    return channel->cms->status;
}

} // namespace

struct dmc2_control_session {
    RCS_CMD_CHANNEL *command;
    RCS_STAT_CHANNEL *status_channel;
    EMC_STAT *status;
};

namespace {

bool session_valid(const dmc2_control_session *session) noexcept {
    return session != nullptr && session->command != nullptr &&
           session->status_channel != nullptr && session->status != nullptr &&
           session->command->valid() && session->status_channel->valid();
}

void transport_diagnostic(
    const dmc2_control_session *session,
    dmc2_command_receipt *receipt) noexcept {
    if (receipt == nullptr || session == nullptr) {
        return;
    }
    if (session->command != nullptr) {
        receipt->nml_error = static_cast<std::int32_t>(session->command->error_type);
    }
    receipt->cms_status = static_cast<std::int32_t>(cms_status(session->status_channel));
}

dmc2_control_result observe_status(
    dmc2_control_session *session,
    dmc2_command_receipt *diagnostic) noexcept {
    if (!session_valid(session)) {
        return DMC2_CONTROL_STATUS_FAILED;
    }
    const NMLTYPE message_type = session->status_channel->peek();
    transport_diagnostic(session, diagnostic);
    if (session->status_channel->error_type != NML_NO_ERROR) {
        return DMC2_CONTROL_STATUS_FAILED;
    }
    if (message_type != 0 && message_type != EMC_STAT_TYPE) {
        return DMC2_CONTROL_STATUS_FAILED;
    }
    if (message_type == EMC_STAT_TYPE) {
        session->status = static_cast<EMC_STAT *>(session->status_channel->get_address());
    }
    return session->status == nullptr ? DMC2_CONTROL_STATUS_FAILED : DMC2_CONTROL_OK;
}

template <typename Command>
dmc2_control_result send_command(
    dmc2_control_session *session,
    Command &command,
    bool wait_until_done,
    dmc2_command_receipt *receipt) noexcept {
    initialize_receipt(receipt);
    if (!session_valid(session) || receipt == nullptr) {
        return DMC2_CONTROL_INVALID_ARGUMENT;
    }
    if (session->command->write(&command) != 0) {
        transport_diagnostic(session, receipt);
        return DMC2_CONTROL_WRITE_FAILED;
    }
    receipt->command_serial_number = command.serial_number;
    const auto deadline = std::chrono::steady_clock::now() + COMMAND_TIMEOUT;
    while (std::chrono::steady_clock::now() < deadline) {
        const auto observed = observe_status(session, receipt);
        if (observed != DMC2_CONTROL_OK) {
            return observed;
        }
        receipt->echo_serial_number = session->status->echo_serial_number;
        receipt->rcs_status = session->status->status;
        const int serial_difference =
            receipt->echo_serial_number - receipt->command_serial_number;
        if (serial_difference > 0) {
            return DMC2_CONTROL_OK;
        }
        if (serial_difference == 0) {
            // Receipt-only operations still must report an observed rejection.
            // RCS_ERROR belongs to this command only when the serials match.
            if (receipt->rcs_status == RCS_ERROR) {
                return DMC2_CONTROL_REJECTED;
            }
            if (!wait_until_done) {
                return DMC2_CONTROL_OK;
            }
            if (receipt->rcs_status == RCS_DONE) {
                return DMC2_CONTROL_OK;
            }
        }
        std::this_thread::sleep_for(POLL_PERIOD);
    }
    return DMC2_CONTROL_TIMEOUT;
}

} // namespace

extern "C" std::uint32_t dmc2_control_abi_version() noexcept {
    return DMC2_CONTROL_ABI_VERSION;
}

extern "C" std::size_t dmc2_control_status_size() noexcept {
    return sizeof(dmc2_control_status);
}

extern "C" dmc2_control_session *dmc2_control_open(
    const char *nml_file,
    dmc2_command_receipt *diagnostic) noexcept {
    initialize_receipt(diagnostic);
    if (nml_file == nullptr || *nml_file == '\0' || diagnostic == nullptr) {
        return nullptr;
    }
    auto *session = new (std::nothrow) dmc2_control_session{nullptr, nullptr, nullptr};
    if (session == nullptr) {
        return nullptr;
    }
    try {
        session->command =
            new RCS_CMD_CHANNEL(emcFormat, "emcCommand", CLIENT_NAME, nml_file);
        session->status_channel =
            new RCS_STAT_CHANNEL(emcFormat, "emcStatus", CLIENT_NAME, nml_file);
    } catch (...) {
        delete session->command;
        delete session->status_channel;
        delete session;
        return nullptr;
    }
    if (session->command == nullptr || session->status_channel == nullptr ||
        !session->command->valid() || !session->status_channel->valid()) {
        transport_diagnostic(session, diagnostic);
        delete session->command;
        delete session->status_channel;
        delete session;
        return nullptr;
    }
    session->status = static_cast<EMC_STAT *>(session->status_channel->get_address());
    if (session->status == nullptr) {
        transport_diagnostic(session, diagnostic);
        delete session->command;
        delete session->status_channel;
        delete session;
        return nullptr;
    }
    const auto deadline = std::chrono::steady_clock::now() + COMMAND_TIMEOUT;
    while (std::chrono::steady_clock::now() < deadline) {
        if (observe_status(session, diagnostic) == DMC2_CONTROL_OK &&
            session->status_channel->cms != nullptr &&
            (session->status_channel->cms->status == CMS_READ_OK ||
             session->status_channel->cms->status == CMS_READ_OLD)) {
            diagnostic->nml_error = static_cast<std::int32_t>(NML_NO_ERROR);
            return session;
        }
        std::this_thread::sleep_for(POLL_PERIOD);
    }
    delete session->command;
    delete session->status_channel;
    delete session;
    return nullptr;
}

extern "C" void dmc2_control_close(dmc2_control_session *session) noexcept {
    if (session == nullptr) {
        return;
    }
    delete session->command;
    delete session->status_channel;
    delete session;
}

extern "C" dmc2_control_result dmc2_control_read_status(
    dmc2_control_session *session,
    dmc2_control_status *destination,
    dmc2_command_receipt *diagnostic) noexcept {
    initialize_receipt(diagnostic);
    if (destination == nullptr || diagnostic == nullptr || !session_valid(session)) {
        return DMC2_CONTROL_INVALID_ARGUMENT;
    }
    const auto result = observe_status(session, diagnostic);
    if (result != DMC2_CONTROL_OK) {
        return result;
    }
    std::memset(destination, 0, sizeof(*destination));
    destination->abi_version = DMC2_CONTROL_ABI_VERSION;
    destination->struct_size = sizeof(*destination);
    destination->task_state = session->status->task.state;
    destination->task_mode = session->status->task.mode;
    destination->interpreter_state = session->status->task.interpState;
    destination->execution_state = session->status->task.execState;
    destination->rcs_status = session->status->status;
    destination->echo_serial_number = session->status->echo_serial_number;
    destination->joint_count =
        std::clamp(session->status->motion.traj.joints, 0, EMCMOT_MAX_JOINTS);
    for (int index = 0; index < destination->joint_count; ++index) {
        if (session->status->motion.joint[index].homed != 0) {
            destination->homed_mask |= (std::uint32_t{1} << index);
        }
        if (session->status->motion.joint[index].homing != 0) {
            destination->homing_mask |= (std::uint32_t{1} << index);
        }
    }
    destination->position_x = session->status->motion.traj.actualPosition.tran.x;
    destination->position_y = session->status->motion.traj.actualPosition.tran.y;
    destination->position_z = session->status->motion.traj.actualPosition.tran.z;
    destination->spindle_speed = session->status->motion.spindle[0].speed;
    destination->spindle_direction = session->status->motion.spindle[0].direction;
    destination->auxiliary_estop = session->status->io.aux.estop;
    std::memcpy(
        destination->loaded_file,
        session->status->task.file,
        sizeof(destination->loaded_file));
    destination->loaded_file[sizeof(destination->loaded_file) - 1] = 0;
    diagnostic->echo_serial_number = session->status->echo_serial_number;
    diagnostic->rcs_status = session->status->status;
    return DMC2_CONTROL_OK;
}

extern "C" dmc2_control_result dmc2_control_set_state(
    dmc2_control_session *session,
    std::int32_t state,
    dmc2_command_receipt *receipt) noexcept {
    if (state < EMC_TASK_STATE_ESTOP || state > EMC_TASK_STATE_ON) {
        return DMC2_CONTROL_INVALID_ARGUMENT;
    }
    EMC_TASK_SET_STATE command;
    command.state = static_cast<EMC_TASK_STATE_ENUM>(state);
    return send_command(session, command, true, receipt);
}

extern "C" dmc2_control_result dmc2_control_set_mode(
    dmc2_control_session *session,
    std::int32_t mode,
    dmc2_command_receipt *receipt) noexcept {
    if (mode < EMC_TASK_MODE_MANUAL || mode > EMC_TASK_MODE_MDI) {
        return DMC2_CONTROL_INVALID_ARGUMENT;
    }
    EMC_TASK_SET_MODE command;
    command.mode = static_cast<EMC_TASK_MODE_ENUM>(mode);
    return send_command(session, command, true, receipt);
}

extern "C" dmc2_control_result dmc2_control_abort(
    dmc2_control_session *session,
    dmc2_command_receipt *receipt) noexcept {
    EMC_TASK_ABORT command;
    return send_command(session, command, true, receipt);
}

extern "C" dmc2_control_result dmc2_control_set_teleop(
    dmc2_control_session *session,
    std::int32_t enabled,
    dmc2_command_receipt *receipt) noexcept {
    EMC_TRAJ_SET_TELEOP_ENABLE command;
    command.enable = enabled != 0;
    return send_command(session, command, true, receipt);
}

extern "C" dmc2_control_result dmc2_control_home(
    dmc2_control_session *session,
    std::int32_t joint,
    dmc2_command_receipt *receipt) noexcept {
    EMC_JOINT_HOME command;
    command.joint = joint;
    // Homing duration is machine-dependent. Confirm command receipt here;
    // the Rust operation waits on returned joint homed/homing state without
    // imposing an arbitrary homing deadline.
    return send_command(session, command, false, receipt);
}

extern "C" dmc2_control_result dmc2_control_program_close(
    dmc2_control_session *session,
    dmc2_command_receipt *receipt) noexcept {
    EMC_TASK_PLAN_CLOSE command;
    return send_command(session, command, true, receipt);
}

extern "C" dmc2_control_result dmc2_control_program_open(
    dmc2_control_session *session,
    const char *file,
    dmc2_command_receipt *receipt) noexcept {
    if (file == nullptr || *file == '\0' || std::strlen(file) >= LINELEN) {
        return DMC2_CONTROL_INVALID_ARGUMENT;
    }
    EMC_TASK_PLAN_OPEN command;
    std::memcpy(command.file, file, std::strlen(file) + 1);
    return send_command(session, command, true, receipt);
}

extern "C" dmc2_control_result dmc2_control_program_run(
    dmc2_control_session *session,
    std::int32_t line,
    dmc2_command_receipt *receipt) noexcept {
    EMC_TASK_PLAN_RUN command;
    command.line = line;
    return send_command(session, command, false, receipt);
}
