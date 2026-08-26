#ifndef DMC2_ERROR_MESSAGE_COPY_HH
#define DMC2_ERROR_MESSAGE_COPY_HH

#include "error_message.h"

class NMLmsg;

dmc2_error_native_result dmc2_copy_error_message(
    const NMLmsg *message,
    dmc2_error_message_snapshot *snapshot) noexcept;

#endif
