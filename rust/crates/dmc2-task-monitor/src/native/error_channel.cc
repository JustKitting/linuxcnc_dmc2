#include "error_message.h"

#include <cstdint>
#include <new>

#include "cms.hh"
#include "emc_nml.hh"
#include "error_message_copy.hh"
#include "nml.hh"
#include "nmlmsg.hh"

namespace {

constexpr const char *LINUXCNC_ERROR_CLIENT = "xemc";

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

} // namespace

struct dmc2_error_channel {
    NML *channel;
};

namespace {

CMS_STATUS current_cms_status(const dmc2_error_channel *holder) noexcept {
    if (holder == nullptr || holder->channel == nullptr ||
        holder->channel->cms == nullptr) {
        return CMS_STATUS_NOT_SET;
    }
    return holder->channel->cms->status;
}

} // namespace

extern "C" dmc2_error_channel *dmc2_error_channel_open(
    const char *nml_file,
    std::int32_t *nml_error,
    std::int32_t *cms_status) noexcept {
    set_nml_error(nml_error, NML_INVALID_CONFIGURATION);
    set_cms_status(cms_status, CMS_STATUS_NOT_SET);
    if (nml_file == nullptr || *nml_file == '\0' || nml_error == nullptr ||
        cms_status == nullptr) {
        return nullptr;
    }

    auto *holder = new (std::nothrow) dmc2_error_channel{nullptr};
    if (holder == nullptr) {
        set_nml_error(nml_error, NML_INTERNAL_CMS_ERROR);
        return nullptr;
    }
    try {
        holder->channel = new NML(
            emcFormat,
            "emcError",
            LINUXCNC_ERROR_CLIENT,
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
    set_nml_error(nml_error, NML_NO_ERROR);
    set_cms_status(cms_status, current_cms_status(holder));
    return holder;
}

extern "C" dmc2_error_native_result dmc2_error_channel_read(
    dmc2_error_channel *holder,
    dmc2_error_message_snapshot *snapshot) noexcept {
    if (snapshot == nullptr) {
        return DMC2_ERROR_NATIVE_INVALID_ARGUMENT;
    }
    dmc2_error_message_snapshot_initialize(snapshot);
    snapshot->nml_error = static_cast<std::int32_t>(NML_INVALID_CONFIGURATION);
    snapshot->cms_status = static_cast<std::int32_t>(current_cms_status(holder));
    if (holder == nullptr || holder->channel == nullptr ||
        !holder->channel->valid()) {
        return DMC2_ERROR_NATIVE_TRANSPORT_ERROR;
    }

    const NMLTYPE message_type = holder->channel->read();
    snapshot->message_type = static_cast<std::int32_t>(message_type);
    snapshot->nml_error = static_cast<std::int32_t>(holder->channel->error_type);
    snapshot->cms_status = static_cast<std::int32_t>(current_cms_status(holder));
    if (message_type == 0) {
        return DMC2_ERROR_NATIVE_EMPTY;
    }
    if (message_type < 0) {
        return DMC2_ERROR_NATIVE_TRANSPORT_ERROR;
    }

    const auto *message =
        static_cast<const NMLmsg *>(holder->channel->get_address());
    const dmc2_error_native_result result =
        dmc2_copy_error_message(message, snapshot);
    if (result != DMC2_ERROR_NATIVE_MESSAGE) {
        return result;
    }
    if (snapshot->message_type != static_cast<std::int32_t>(message_type)) {
        return DMC2_ERROR_NATIVE_INVALID_MESSAGE;
    }
    return DMC2_ERROR_NATIVE_MESSAGE;
}

extern "C" void dmc2_error_channel_close(
    dmc2_error_channel *holder) noexcept {
    if (holder == nullptr) {
        return;
    }
    delete holder->channel;
    delete holder;
}
