#include "status_snapshot.h"

#include <cstdint>
#include <new>

#include "emc_nml.hh"
#include "status_copy.hh"

namespace {

// LinuxCNC 2.9.10's installed linuxcnc.nml grants read access to emcStatus
// through the same standard UI client identity used by AXIS and HALUI.
constexpr const char *LINUXCNC_STATUS_CLIENT = "xemc";

void set_nml_error(std::int32_t *destination, NML_ERROR_TYPE error) noexcept {
    if (destination != nullptr) {
        *destination = static_cast<std::int32_t>(error);
    }
}

struct PollDecision {
    dmc2_task_status_poll_result result;
    NML_ERROR_TYPE error;
    bool received_status;
    bool copy_snapshot;
};

constexpr PollDecision classify_poll(
    NMLTYPE type,
    NML_ERROR_TYPE error,
    bool received_status) noexcept {
    if (error != NML_NO_ERROR) {
        return {
            DMC2_TASK_STATUS_POLL_ERROR,
            error,
            received_status,
            false,
        };
    }
    if (type == EMC_STAT_TYPE) {
        return {
            DMC2_TASK_STATUS_POLL_OK,
            NML_NO_ERROR,
            true,
            true,
        };
    }
    if (type == 0 && !received_status) {
        return {
            DMC2_TASK_STATUS_POLL_NOT_READY,
            NML_NO_ERROR,
            false,
            false,
        };
    }
    if (type != 0) {
        return {
            DMC2_TASK_STATUS_POLL_ERROR,
            NML_INVALID_MESSAGE_ERROR,
            received_status,
            false,
        };
    }
    return {
        DMC2_TASK_STATUS_POLL_OK,
        NML_NO_ERROR,
        true,
        true,
    };
}

constexpr PollDecision POLL_TRANSPORT_ERROR = classify_poll(
    EMC_STAT_TYPE,
    NML_INTERNAL_CMS_ERROR,
    false);
static_assert(
    POLL_TRANSPORT_ERROR.result == DMC2_TASK_STATUS_POLL_ERROR &&
    POLL_TRANSPORT_ERROR.error == NML_INTERNAL_CMS_ERROR &&
    !POLL_TRANSPORT_ERROR.received_status &&
    !POLL_TRANSPORT_ERROR.copy_snapshot);

constexpr PollDecision POLL_NEW_STATUS =
    classify_poll(EMC_STAT_TYPE, NML_NO_ERROR, false);
static_assert(
    POLL_NEW_STATUS.result == DMC2_TASK_STATUS_POLL_OK &&
    POLL_NEW_STATUS.error == NML_NO_ERROR &&
    POLL_NEW_STATUS.received_status &&
    POLL_NEW_STATUS.copy_snapshot);

constexpr PollDecision POLL_FIRST_WAIT = classify_poll(0, NML_NO_ERROR, false);
static_assert(
    POLL_FIRST_WAIT.result == DMC2_TASK_STATUS_POLL_NOT_READY &&
    POLL_FIRST_WAIT.error == NML_NO_ERROR &&
    !POLL_FIRST_WAIT.received_status &&
    !POLL_FIRST_WAIT.copy_snapshot);

constexpr PollDecision POLL_INVALID_MESSAGE =
    classify_poll(-1, NML_NO_ERROR, true);
static_assert(
    POLL_INVALID_MESSAGE.result == DMC2_TASK_STATUS_POLL_ERROR &&
    POLL_INVALID_MESSAGE.error == NML_INVALID_MESSAGE_ERROR &&
    POLL_INVALID_MESSAGE.received_status &&
    !POLL_INVALID_MESSAGE.copy_snapshot);

constexpr PollDecision POLL_CURRENT_STATUS =
    classify_poll(0, NML_NO_ERROR, true);
static_assert(
    POLL_CURRENT_STATUS.result == DMC2_TASK_STATUS_POLL_OK &&
    POLL_CURRENT_STATUS.error == NML_NO_ERROR &&
    POLL_CURRENT_STATUS.received_status &&
    POLL_CURRENT_STATUS.copy_snapshot);

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
    const char *nml_file,
    std::int32_t *nml_error) noexcept {
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
            emcFormat,
            "emcStatus",
            LINUXCNC_STATUS_CLIENT,
            nml_file);
    } catch (...) {
        set_nml_error(nml_error, NML_INTERNAL_CMS_ERROR);
        delete holder;
        return nullptr;
    }
    if (holder->channel == nullptr || !holder->channel->valid()) {
        if (holder->channel != nullptr &&
            holder->channel->error_type != NML_NO_ERROR) {
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

extern "C" dmc2_task_status_poll_result dmc2_task_status_poll(
    dmc2_task_status_channel *holder,
    dmc2_task_status_snapshot *snapshot,
    std::int32_t *nml_error) noexcept {
    set_nml_error(nml_error, NML_INVALID_CONFIGURATION);
    if (holder == nullptr || snapshot == nullptr || holder->channel == nullptr ||
        holder->status == nullptr || !holder->channel->valid()) {
        return DMC2_TASK_STATUS_POLL_ERROR;
    }

    const NMLTYPE type = holder->channel->peek();
    const PollDecision decision = classify_poll(
        type,
        holder->channel->error_type,
        holder->received_status);
    holder->received_status = decision.received_status;
    set_nml_error(nml_error, decision.error);
    if (!decision.copy_snapshot) {
        return decision.result;
    }

    dmc2_copy_status(*holder->status, *snapshot);
    return decision.result;
}

extern "C" void dmc2_task_status_close(
    dmc2_task_status_channel *holder) noexcept {
    if (holder == nullptr) {
        return;
    }
    delete holder->channel;
    delete holder;
}
