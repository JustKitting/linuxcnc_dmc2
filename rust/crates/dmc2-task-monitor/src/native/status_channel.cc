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

extern "C" int dmc2_task_status_poll(
    dmc2_task_status_channel *holder,
    dmc2_task_status_snapshot *snapshot,
    std::int32_t *nml_error) noexcept {
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
        set_nml_error(nml_error, NML_NO_ERROR);
        return 1;
    } else if (type != 0) {
        set_nml_error(nml_error, NML_INVALID_MESSAGE_ERROR);
        return -1;
    }

    dmc2_copy_status(*holder->status, *snapshot);
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
