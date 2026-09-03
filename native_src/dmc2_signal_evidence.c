#define _GNU_SOURCE

#include <errno.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define DMC2_FD_ENV "DMC2_SIGNAL_EVIDENCE_FD"
#define DMC2_PROTOCOL_ENV "DMC2_SIGNAL_EVIDENCE_PROTOCOL"
#define DMC2_PROTOCOL_VALUE "1"

enum dmc2_record_kind {
    DMC2_INITIALIZED = 1,
    DMC2_HANDLER_ARMED = 2,
    DMC2_SIGNAL_DELIVERED = 3,
    DMC2_HANDLER_DISARMED = 4,
};

struct dmc2_record {
    unsigned char magic[8];
    uint32_t version;
    uint32_t record_size;
    uint32_t kind;
    int32_t signal_number;
    int32_t signal_code;
    int32_t signal_errno;
    uint32_t target_pid;
    uint32_t target_tid;
    uint32_t sender_pid;
    uint32_t sender_uid;
    int64_t realtime_seconds;
    int64_t realtime_nanoseconds;
};

_Static_assert(sizeof(struct dmc2_record) == 64,
               "signal-evidence record must remain 64 bytes");
_Static_assert(ATOMIC_INT_LOCK_FREE == 2,
               "signal evidence descriptor must be lock-free atomic");

typedef void (*dmc2_handler)(int);
typedef dmc2_handler (*dmc2_signal_function)(int, dmc2_handler);
typedef _Atomic(uintptr_t) dmc2_atomic_handler;

_Static_assert(sizeof(dmc2_handler) == sizeof(uintptr_t),
               "handler pointers must fit in uintptr_t");

union dmc2_handler_bits {
    dmc2_handler function;
    uintptr_t bits;
};

static _Atomic int evidence_fd = -1;
static _Atomic int initialization_state = 0;
static dmc2_atomic_handler sigint_handler = 0;
static dmc2_atomic_handler sigterm_handler = 0;
static dmc2_signal_function real_signal_function = NULL;
static bool handler_atomics_supported = false;

static void emit_record(uint32_t kind, int signal_number,
                        const siginfo_t *information)
{
    int saved_errno = errno;
    int fd = atomic_load_explicit(&evidence_fd, memory_order_acquire);
    if (fd < 0) {
        errno = saved_errno;
        return;
    }

    struct dmc2_record record = {0};
    struct timespec timestamp = {0};
    static const unsigned char magic[8] = {
        'D', 'M', 'C', '2', 'S', 'I', 'G', '1'
    };
    for (size_t index = 0; index < sizeof(record.magic); ++index) {
        record.magic[index] = magic[index];
    }
    record.version = 1;
    record.record_size = sizeof(record);
    record.kind = kind;
    record.signal_number = signal_number;
    record.target_pid = (uint32_t)getpid();
    record.target_tid = (uint32_t)gettid();
    if (information != NULL) {
        record.signal_code = information->si_code;
        record.signal_errno = information->si_errno;
        record.sender_pid = (uint32_t)information->si_pid;
        record.sender_uid = (uint32_t)information->si_uid;
    }
    if (clock_gettime(CLOCK_REALTIME, &timestamp) == 0) {
        record.realtime_seconds = timestamp.tv_sec;
        record.realtime_nanoseconds = timestamp.tv_nsec;
    }

    ssize_t sent;
    do {
        sent = send(fd, &record, sizeof(record), MSG_DONTWAIT | MSG_NOSIGNAL);
    } while (sent < 0 && errno == EINTR);
    errno = saved_errno;
}

static void resolve_real_signal(void)
{
    void *symbol = dlsym(RTLD_NEXT, "signal");
    _Static_assert(sizeof(symbol) == sizeof(real_signal_function),
                   "function and object pointers must have matching size");
    (void)memcpy(&real_signal_function, &symbol,
                 sizeof(real_signal_function));
}

static bool parse_fd(const char *text, int *result)
{
    if (text == NULL || *text == '\0') {
        return false;
    }
    unsigned long value = 0;
    const unsigned char *cursor = (const unsigned char *)text;
    while (*cursor != '\0') {
        if (*cursor < '0' || *cursor > '9') {
            return false;
        }
        value = (value * 10UL) + (unsigned long)(*cursor - '0');
        if (value > INT_MAX) {
            return false;
        }
        ++cursor;
    }
    *result = (int)value;
    return true;
}

static void initialize_evidence(void)
{
    int expected = 0;
    if (!atomic_compare_exchange_strong_explicit(
            &initialization_state, &expected, 1,
            memory_order_acq_rel, memory_order_acquire)) {
        while (atomic_load_explicit(&initialization_state,
                                    memory_order_acquire) == 1) {
        }
        return;
    }

    int fd = -1;
    resolve_real_signal();
    handler_atomics_supported = real_signal_function != NULL
        && atomic_is_lock_free(&sigint_handler)
        && atomic_is_lock_free(&sigterm_handler);
    const char *protocol = getenv(DMC2_PROTOCOL_ENV);
    const char *fd_text = getenv(DMC2_FD_ENV);
    if (handler_atomics_supported
        && protocol != NULL
        && strcmp(protocol, DMC2_PROTOCOL_VALUE) == 0
        && parse_fd(fd_text, &fd)) {
        int flags = fcntl(fd, F_GETFD);
        if (flags >= 0 && fcntl(fd, F_SETFD, flags | FD_CLOEXEC) == 0) {
            atomic_store_explicit(&evidence_fd, fd, memory_order_release);
        }
    }

    (void)unsetenv("LD_PRELOAD");
    (void)unsetenv(DMC2_FD_ENV);
    (void)unsetenv(DMC2_PROTOCOL_ENV);
    atomic_store_explicit(&initialization_state, 2, memory_order_release);
    emit_record(DMC2_INITIALIZED, 0, NULL);
}

__attribute__((constructor))
static void signal_evidence_constructor(void)
{
    initialize_evidence();
}

static dmc2_atomic_handler *handler_slot(int signal_number)
{
    if (signal_number == SIGINT) {
        return &sigint_handler;
    }
    if (signal_number == SIGTERM) {
        return &sigterm_handler;
    }
    return NULL;
}

static void caught_signal(int signal_number, siginfo_t *information,
                          void *context)
{
    (void)context;
    emit_record(DMC2_SIGNAL_DELIVERED, signal_number, information);

    dmc2_atomic_handler *slot = handler_slot(signal_number);
    if (slot == NULL) {
        return;
    }
    union dmc2_handler_bits original = {
        .bits = atomic_load_explicit(slot, memory_order_acquire)
    };
    if (original.function != SIG_DFL
        && original.function != SIG_IGN
        && original.function != SIG_ERR) {
        original.function(signal_number);
    }
}

static dmc2_handler previous_handler(const struct sigaction *previous,
                                     uintptr_t previous_saved)
{
    if ((previous->sa_flags & SA_SIGINFO) != 0
        && previous->sa_sigaction == caught_signal) {
        union dmc2_handler_bits saved = {.bits = previous_saved};
        return saved.function;
    }
    return previous->sa_handler;
}

__attribute__((visibility("default")))
dmc2_handler signal(int signal_number, dmc2_handler handler)
{
    initialize_evidence();
    if (!handler_atomics_supported) {
        if (real_signal_function == NULL) {
            errno = ENOSYS;
            return SIG_ERR;
        }
        return real_signal_function(signal_number, handler);
    }
    dmc2_atomic_handler *slot = handler_slot(signal_number);
    if (slot == NULL) {
        if (real_signal_function == NULL) {
            errno = ENOSYS;
            return SIG_ERR;
        }
        return real_signal_function(signal_number, handler);
    }
    if (handler == SIG_ERR) {
        errno = EINVAL;
        return SIG_ERR;
    }

    union dmc2_handler_bits requested = {.function = handler};
    uintptr_t previous_saved = atomic_exchange_explicit(
        slot, requested.bits, memory_order_acq_rel);
    struct sigaction action = {0};
    struct sigaction previous = {0};
    if (handler == SIG_DFL || handler == SIG_IGN) {
        action.sa_handler = handler;
        action.sa_flags = SA_RESTART;
    } else {
        action.sa_sigaction = caught_signal;
        action.sa_flags = SA_RESTART | SA_SIGINFO;
    }
    if (sigemptyset(&action.sa_mask) != 0
        || sigaction(signal_number, &action, &previous) != 0) {
        atomic_store_explicit(slot, previous_saved, memory_order_release);
        return SIG_ERR;
    }
    uint32_t kind = (handler == SIG_DFL || handler == SIG_IGN)
        ? DMC2_HANDLER_DISARMED
        : DMC2_HANDLER_ARMED;
    emit_record(kind, signal_number, NULL);
    return previous_handler(&previous, previous_saved);
}
