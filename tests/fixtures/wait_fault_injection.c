#define _GNU_SOURCE

#include <dlfcn.h>
#include <errno.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/types.h>
#include <sys/wait.h>

typedef int (*waitid_function)(idtype_t, id_t, siginfo_t *, int);
typedef pid_t (*wait4_function)(pid_t, int *, int, struct rusage *);

static waitid_function real_waitid_function = NULL;
static wait4_function real_wait4_function = NULL;
static _Atomic int failure_emitted = 0;
static _Atomic unsigned int wait4_failures_emitted = 0;
static bool fail_waitid_observation = true;
static bool fail_retained_reap = false;
static const char *retained_reap_release = NULL;

static void resolve_real_waitid(void)
{
    void *symbol = dlsym(RTLD_NEXT, "waitid");
    _Static_assert(sizeof(symbol) == sizeof(real_waitid_function),
                   "function and object pointers must have matching size");
    (void)memcpy(&real_waitid_function, &symbol,
                 sizeof(real_waitid_function));
}

static void resolve_real_wait4(void)
{
    void *symbol = dlsym(RTLD_NEXT, "wait4");
    _Static_assert(sizeof(symbol) == sizeof(real_wait4_function),
                   "function and object pointers must have matching size");
    (void)memcpy(&real_wait4_function, &symbol,
                 sizeof(real_wait4_function));
}

static bool is_lifecycle_test_owner(void)
{
    char name[16] = {0};
    return prctl(PR_GET_NAME, name, 0, 0, 0) == 0
        && (strcmp(name, "dmc2-test-owner") == 0
            || strcmp(name, "dmc2-test-sess") == 0);
}

__attribute__((constructor))
static void waitid_failure_constructor(void)
{
    resolve_real_waitid();
    resolve_real_wait4();
    const char *mode = getenv("DMC2_WAIT_FAULT_MODE");
    fail_waitid_observation = mode == NULL
        || strcmp(mode, "waitid-then-fallback-wait4") == 0;
    fail_retained_reap = mode != NULL
        && strcmp(mode, "retained-terminal-wait4") == 0;
    retained_reap_release = getenv("DMC2_WAIT_FAULT_RELEASE");
}

__attribute__((visibility("default")))
int waitid(idtype_t id_type, id_t id, siginfo_t *information, int options)
{
    int expected = 0;
    if (fail_waitid_observation
        && is_lifecycle_test_owner()
        && atomic_compare_exchange_strong_explicit(
            &failure_emitted, &expected, 1,
            memory_order_acq_rel, memory_order_acquire)) {
        errno = EPERM;
        return -1;
    }
    if (real_waitid_function == NULL) {
        errno = ENOSYS;
        return -1;
    }
    return real_waitid_function(id_type, id, information, options);
}

__attribute__((visibility("default")))
pid_t wait4(pid_t pid, int *status, int options, struct rusage *usage)
{
    if (is_lifecycle_test_owner()) {
        if (fail_retained_reap
            && retained_reap_release != NULL
            && access(retained_reap_release, F_OK) != 0) {
            atomic_fetch_add_explicit(&wait4_failures_emitted, 1,
                                      memory_order_acq_rel);
            errno = EPERM;
            return -1;
        }
        unsigned int observed = atomic_load_explicit(
            &wait4_failures_emitted, memory_order_acquire);
        while (fail_waitid_observation && observed < 3) {
            if (atomic_compare_exchange_weak_explicit(
                    &wait4_failures_emitted, &observed, observed + 1,
                    memory_order_acq_rel, memory_order_acquire)) {
                errno = EPERM;
                return -1;
            }
        }
    }
    if (real_wait4_function == NULL) {
        errno = ENOSYS;
        return -1;
    }
    return real_wait4_function(pid, status, options, usage);
}
