#include "error_message.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>

#include "emc_nml.hh"
#include "error_message_copy.hh"
#include "nml_oi.hh"
#include "nmlmsg.hh"

namespace {

template <typename Message>
bool copied_exactly(
    Message &message,
    dmc2_error_message_snapshot &snapshot,
    std::size_t &failure_offset) noexcept {
    dmc2_error_message_snapshot_initialize(&snapshot);
    if (dmc2_copy_error_message(&message, &snapshot) !=
            DMC2_ERROR_NATIVE_MESSAGE ||
        snapshot.message_type != static_cast<std::int32_t>(message.type) ||
        snapshot.object_size != sizeof(message)) {
        failure_offset = 0;
        return false;
    }
    const auto *source = reinterpret_cast<const std::uint8_t *>(&message);
    for (std::size_t index = 0; index < sizeof(message); ++index) {
        if (snapshot.object[index] != source[index]) {
            failure_offset = index;
            return false;
        }
    }
    for (std::size_t index = sizeof(message);
         index < DMC2_ERROR_MESSAGE_OBJECT_CAPACITY;
         ++index) {
        if (snapshot.object[index] != 0) {
            failure_offset = index;
            return false;
        }
    }
    return true;
}

template <std::size_t Size>
void fill_payload(char (&payload)[Size], std::uint8_t seed) noexcept {
    for (std::size_t index = 0; index < Size; ++index) {
        payload[index] = static_cast<char>(seed + index);
    }
    payload[Size - 1] = '\0';
}

class OversizedMessage final : public NMLmsg {
  public:
    OversizedMessage():NMLmsg(999, sizeof(OversizedMessage)) {}
    std::uint8_t bytes[DMC2_ERROR_MESSAGE_OBJECT_CAPACITY + 1];
};

} // namespace

extern "C" int dmc2_error_message_copy_self_test(
    std::uint32_t *tested_message_types,
    std::size_t *failure_offset) noexcept {
    if (tested_message_types == nullptr || failure_offset == nullptr) {
        return 1;
    }
    *tested_message_types = 0;
    *failure_offset = std::numeric_limits<std::size_t>::max();

    dmc2_error_message_snapshot snapshot;
    std::memset(&snapshot, 0xa5, sizeof(snapshot));
    dmc2_error_message_snapshot_initialize(&snapshot);
    if (snapshot.abi_version != DMC2_ERROR_MESSAGE_ABI_VERSION ||
        snapshot.struct_size != sizeof(snapshot)) {
        *failure_offset = 0;
        return 2;
    }
    const auto *initialized = reinterpret_cast<const std::uint8_t *>(&snapshot);
    for (std::size_t index = sizeof(snapshot.abi_version) +
                             sizeof(snapshot.struct_size);
         index < sizeof(snapshot);
         ++index) {
        if (initialized[index] != 0) {
            *failure_offset = index;
            return 3;
        }
    }

    NML_ERROR nml_error;
    fill_payload(nml_error.error, 0x11);
    if (!copied_exactly(nml_error, snapshot, *failure_offset)) {
        return 4;
    }
    ++*tested_message_types;

    NML_TEXT nml_text;
    fill_payload(nml_text.text, 0x22);
    if (!copied_exactly(nml_text, snapshot, *failure_offset)) {
        return 5;
    }
    ++*tested_message_types;

    NML_DISPLAY nml_display;
    fill_payload(nml_display.display, 0x33);
    if (!copied_exactly(nml_display, snapshot, *failure_offset)) {
        return 6;
    }
    ++*tested_message_types;

    EMC_OPERATOR_ERROR operator_error;
    operator_error.serial_number = 0x10203040;
    operator_error.id = 0x11223344;
    fill_payload(operator_error.error, 0x44);
    if (!copied_exactly(operator_error, snapshot, *failure_offset)) {
        return 7;
    }
    ++*tested_message_types;

    EMC_OPERATOR_TEXT operator_text;
    operator_text.serial_number = 0x20304050;
    operator_text.id = 0x22334455;
    fill_payload(operator_text.text, 0x55);
    if (!copied_exactly(operator_text, snapshot, *failure_offset)) {
        return 8;
    }
    ++*tested_message_types;

    EMC_OPERATOR_DISPLAY operator_display;
    operator_display.serial_number = 0x30405060;
    operator_display.id = 0x33445566;
    fill_payload(operator_display.display, 0x66);
    if (!copied_exactly(operator_display, snapshot, *failure_offset)) {
        return 9;
    }
    ++*tested_message_types;

    const std::int64_t saved_size = nml_error.size;
    nml_error.size = 0;
    dmc2_error_message_snapshot_initialize(&snapshot);
    if (dmc2_copy_error_message(&nml_error, &snapshot) !=
        DMC2_ERROR_NATIVE_INVALID_MESSAGE) {
        *failure_offset = 0;
        return 10;
    }
    nml_error.size = saved_size;

    OversizedMessage oversized;
    dmc2_error_message_snapshot_initialize(&snapshot);
    if (dmc2_copy_error_message(&oversized, &snapshot) !=
        DMC2_ERROR_NATIVE_INVALID_MESSAGE) {
        *failure_offset = DMC2_ERROR_MESSAGE_OBJECT_CAPACITY;
        return 11;
    }
    if (dmc2_copy_error_message(nullptr, &snapshot) !=
            DMC2_ERROR_NATIVE_INVALID_ARGUMENT ||
        dmc2_copy_error_message(&nml_error, nullptr) !=
            DMC2_ERROR_NATIVE_INVALID_ARGUMENT) {
        *failure_offset = 0;
        return 12;
    }
    return 0;
}
