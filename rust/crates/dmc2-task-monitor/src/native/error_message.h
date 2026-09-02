#ifndef DMC2_ERROR_MESSAGE_H
#define DMC2_ERROR_MESSAGE_H

#include <stddef.h>
#include <stdint.h>

#define DMC2_ERROR_MESSAGE_ABI_VERSION 0x00020910U
#define DMC2_ERROR_MESSAGE_OBJECT_CAPACITY 280U

typedef struct dmc2_error_message_snapshot {
    uint32_t abi_version;
    uint32_t struct_size;
    int32_t message_type;
    int32_t nml_error;
    int32_t cms_status;
    uint32_t object_size;
    uint8_t object[DMC2_ERROR_MESSAGE_OBJECT_CAPACITY];
} dmc2_error_message_snapshot;

#if defined(__cplusplus)
static_assert(sizeof(dmc2_error_message_snapshot) == 304U);
static_assert(offsetof(dmc2_error_message_snapshot, abi_version) == 0U);
static_assert(offsetof(dmc2_error_message_snapshot, struct_size) == 4U);
static_assert(offsetof(dmc2_error_message_snapshot, message_type) == 8U);
static_assert(offsetof(dmc2_error_message_snapshot, nml_error) == 12U);
static_assert(offsetof(dmc2_error_message_snapshot, cms_status) == 16U);
static_assert(offsetof(dmc2_error_message_snapshot, object_size) == 20U);
static_assert(offsetof(dmc2_error_message_snapshot, object) == 24U);
#else
_Static_assert(sizeof(dmc2_error_message_snapshot) == 304U, "snapshot size");
_Static_assert(offsetof(dmc2_error_message_snapshot, abi_version) == 0U, "abi offset");
_Static_assert(offsetof(dmc2_error_message_snapshot, struct_size) == 4U, "struct size offset");
_Static_assert(offsetof(dmc2_error_message_snapshot, message_type) == 8U, "type offset");
_Static_assert(offsetof(dmc2_error_message_snapshot, nml_error) == 12U, "NML offset");
_Static_assert(offsetof(dmc2_error_message_snapshot, cms_status) == 16U, "CMS offset");
_Static_assert(offsetof(dmc2_error_message_snapshot, object_size) == 20U, "object size offset");
_Static_assert(offsetof(dmc2_error_message_snapshot, object) == 24U, "object offset");
#endif

typedef struct dmc2_error_channel dmc2_error_channel;

typedef enum dmc2_error_native_result {
    DMC2_ERROR_NATIVE_INVALID_ARGUMENT = -3,
    DMC2_ERROR_NATIVE_INVALID_MESSAGE = -2,
    DMC2_ERROR_NATIVE_TRANSPORT_ERROR = -1,
    DMC2_ERROR_NATIVE_EMPTY = 0,
    DMC2_ERROR_NATIVE_MESSAGE = 1
} dmc2_error_native_result;

#ifdef __cplusplus
extern "C" {
#define DMC2_ERROR_NOEXCEPT noexcept
#else
#define DMC2_ERROR_NOEXCEPT
#endif

uint32_t dmc2_error_message_abi_version(void) DMC2_ERROR_NOEXCEPT;
size_t dmc2_error_message_snapshot_size(void) DMC2_ERROR_NOEXCEPT;
void dmc2_error_message_snapshot_initialize(
    dmc2_error_message_snapshot *snapshot) DMC2_ERROR_NOEXCEPT;
dmc2_error_channel *dmc2_error_channel_open(
    const char *nml_file,
    int32_t *nml_error,
    int32_t *cms_status) DMC2_ERROR_NOEXCEPT;
dmc2_error_native_result dmc2_error_channel_read(
    dmc2_error_channel *channel,
    dmc2_error_message_snapshot *snapshot) DMC2_ERROR_NOEXCEPT;
void dmc2_error_channel_close(
    dmc2_error_channel *channel) DMC2_ERROR_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#undef DMC2_ERROR_NOEXCEPT

#endif
