#define _GNU_SOURCE

#include <fcntl.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static volatile sig_atomic_t caught_signal = 0;

static void quit_cleanly(int signal_number)
{
    if (signal(signal_number, quit_cleanly) == SIG_ERR) {
        _exit(68);
    }
    caught_signal = signal_number;
}

int main(int argc, char **argv)
{
    if (argc < 2 || argc > 3) {
        return 64;
    }
    if (getenv("LD_PRELOAD") != NULL
        || getenv("DMC2_SIGNAL_EVIDENCE_FD") != NULL
        || getenv("DMC2_SIGNAL_EVIDENCE_PROTOCOL") != NULL) {
        return 72;
    }
    void (*previous)(int) = signal(SIGUSR1, SIG_IGN);
    if (previous == SIG_ERR || signal(SIGUSR1, previous) == SIG_ERR) {
        return 70;
    }
    if (signal(SIGINT, quit_cleanly) == SIG_ERR
        || signal(SIGTERM, quit_cleanly) == SIG_ERR) {
        return 65;
    }
    int ready = open(argv[1], O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
    if (ready < 0) {
        return 66;
    }
    if (write(ready, "1", 1) != 1 || close(ready) != 0) {
        return 67;
    }
    if (argc == 3) {
        return strcmp(argv[2], "exit-clean") == 0 ? 0 : 71;
    }
    while (caught_signal == 0) {
        pause();
    }
    return caught_signal == SIGINT || caught_signal == SIGTERM ? 0 : 69;
}
