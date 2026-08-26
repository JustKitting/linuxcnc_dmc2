#include "error_message_copy.hh"

#include <cstdint>
#include <cstring>
#include <limits>

#include "nmlmsg.hh"

extern "C" std::uint32_t dmc2_error_message_abi_version() noexcept {
    return DMC2_ERROR_MESSAGE_ABI_VERSION;
}

extern "C" std::size_t dmc2_error_message_snapshot_size() noexcept {
    return sizeof(dmc2_error_message_snapshot);
}

extern "C" void dmc2_error_message_snapshot_initialize(
    dmc2_error_message_snapshot *snapshot) noexcept {
    if (snapshot == nullptr) {
        return;
    }
    std::memset(snapshot, 0, sizeof(*snapshot));
    snapshot->abi_version = DMC2_ERROR_MESSAGE_ABI_VERSION;
    snapshot->struct_size = sizeof(*snapshot);
}

dmc2_error_native_result dmc2_copy_error_message(
    const NMLmsg *message,
    dmc2_error_message_snapshot *snapshot) noexcept {
    if (message == nullptr || snapshot == nullptr) {
        return DMC2_ERROR_NATIVE_INVALID_ARGUMENT;
    }
    if (message->size <= 0 ||
        static_cast<unsigned long>(message->size) >
            DMC2_ERROR_MESSAGE_OBJECT_CAPACITY ||
        static_cast<unsigned long>(message->size) >
            std::numeric_limits<std::uint32_t>::max()) {
        return DMC2_ERROR_NATIVE_INVALID_MESSAGE;
    }
    snapshot->message_type = static_cast<std::int32_t>(message->type);
    snapshot->object_size = static_cast<std::uint32_t>(message->size);
    std::memcpy(snapshot->object, message, snapshot->object_size);
    return DMC2_ERROR_NATIVE_MESSAGE;
}
