#include "status_snapshot.h"

#include <cstdint>
#include <new>

#include "cms.hh"
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

void set_cms_status(std::int32_t *destination, CMS_STATUS status) noexcept {
    if (destination != nullptr) {
        *destination = static_cast<std::int32_t>(status);
    }
}

CMS_STATUS current_cms_status(
    const dmc2_task_status_channel *holder) noexcept;

} // namespace

struct dmc2_task_status_channel {
    RCS_STAT_CHANNEL *channel;
    EMC_STAT *status;
};

namespace {

CMS_STATUS current_cms_status(
    const dmc2_task_status_channel *holder) noexcept {
    if (holder == nullptr || holder->channel == nullptr ||
        holder->channel->cms == nullptr) {
        return CMS_STATUS_NOT_SET;
    }
    return holder->channel->cms->status;
}

} // namespace

extern "C" std::uint32_t dmc2_task_status_snapshot_abi_version() noexcept {
    return DMC2_SNAPSHOT_ABI_VERSION;
}

extern "C" std::size_t dmc2_task_status_snapshot_size() noexcept {
    return sizeof(dmc2_task_status_snapshot);
}

extern "C" dmc2_task_status_channel *dmc2_task_status_open(
    const char *nml_file,
    std::int32_t *nml_error,
    std::int32_t *cms_status) noexcept {
    set_nml_error(nml_error, NML_INVALID_CONFIGURATION);
    set_cms_status(cms_status, CMS_STATUS_NOT_SET);
    if (nml_file == nullptr || *nml_file == '\0') {
        return nullptr;
    }

    auto *holder = new (std::nothrow) dmc2_task_status_channel{nullptr, nullptr};
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
        set_cms_status(cms_status, current_cms_status(holder));
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
        set_cms_status(cms_status, current_cms_status(holder));
        set_nml_error(nml_error, NML_INTERNAL_CMS_ERROR);
        delete holder->channel;
        delete holder;
        return nullptr;
    }
    set_nml_error(nml_error, NML_NO_ERROR);
    set_cms_status(cms_status, current_cms_status(holder));
    return holder;
}

extern "C" dmc2_task_status_native_result dmc2_task_status_observe(
    dmc2_task_status_channel *holder,
    std::int32_t *message_type,
    std::int32_t *nml_error,
    std::int32_t *cms_status) noexcept {
    if (message_type != nullptr) {
        *message_type = 0;
    }
    set_nml_error(nml_error, NML_INVALID_CONFIGURATION);
    set_cms_status(cms_status, current_cms_status(holder));
    if (holder == nullptr || message_type == nullptr || nml_error == nullptr ||
        cms_status == nullptr || holder->channel == nullptr || holder->status == nullptr ||
        !holder->channel->valid()) {
        return DMC2_TASK_STATUS_NATIVE_ERROR;
    }

    *message_type = static_cast<std::int32_t>(holder->channel->peek());
    set_nml_error(nml_error, holder->channel->error_type);
    set_cms_status(cms_status, current_cms_status(holder));
    return DMC2_TASK_STATUS_NATIVE_OK;
}

extern "C" dmc2_task_status_native_result dmc2_task_status_copy(
    dmc2_task_status_channel *holder,
    dmc2_task_status_snapshot *snapshot) noexcept {
    if (holder == nullptr || snapshot == nullptr || holder->channel == nullptr ||
        holder->status == nullptr || !holder->channel->valid()) {
        return DMC2_TASK_STATUS_NATIVE_ERROR;
    }
    dmc2_copy_status(*holder->status, *snapshot);
    return DMC2_TASK_STATUS_NATIVE_OK;
}

extern "C" void dmc2_task_status_close(
    dmc2_task_status_channel *holder) noexcept {
    if (holder == nullptr) {
        return;
    }
    delete holder->channel;
    delete holder;
}
