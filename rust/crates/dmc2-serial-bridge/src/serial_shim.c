#define _DEFAULT_SOURCE

#include "serial_shim.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stddef.h>
#include <termios.h>
#include <unistd.h>

static int dmc2_negative_errno(void) {
    return errno > 0 ? -errno : -EIO;
}

static int dmc2_close_after_failure(int fd, int operation_error) {
    if (close(fd) != 0) {
        return dmc2_negative_errno();
    }
    return operation_error;
}

static speed_t dmc2_baud(unsigned int baud) {
    switch (baud) {
    case 115200:
        return B115200;
    default:
        return (speed_t)0;
    }
}

int dmc2_serial_open(const char *path, unsigned int baud) {
    const speed_t speed = dmc2_baud(baud);
    if (path == NULL || *path == '\0' || speed == (speed_t)0) {
        return -EINVAL;
    }

    const int fd = open(path, O_RDWR | O_NOCTTY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        return dmc2_negative_errno();
    }
    struct termios options;
    if (tcgetattr(fd, &options) != 0) {
        const int operation_error = dmc2_negative_errno();
        return dmc2_close_after_failure(fd, operation_error);
    }
    cfmakeraw(&options);
    options.c_cflag |= CLOCAL | CREAD;
    options.c_cflag &= ~(CSTOPB | CRTSCTS);
    options.c_cflag &= ~CSIZE;
    options.c_cflag |= CS8;
    options.c_cc[VMIN] = 0;
    options.c_cc[VTIME] = 0;
    if (cfsetispeed(&options, speed) != 0 ||
        cfsetospeed(&options, speed) != 0 ||
        tcsetattr(fd, TCSANOW, &options) != 0) {
        const int operation_error = dmc2_negative_errno();
        return dmc2_close_after_failure(fd, operation_error);
    }
    if (tcflush(fd, TCIFLUSH) != 0) {
        const int operation_error = dmc2_negative_errno();
        return dmc2_close_after_failure(fd, operation_error);
    }
    return fd;
}

int dmc2_serial_read(int fd, unsigned char *buffer, size_t capacity) {
    if (fd < 0 || buffer == NULL || capacity == 0 || capacity > INT_MAX) {
        return -EINVAL;
    }
    const ssize_t result = read(fd, buffer, capacity);
    if (result < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR)) {
        return 0;
    }
    if (result < 0) {
        return dmc2_negative_errno();
    }
    return (int)result;
}

int dmc2_serial_close(int fd) {
    if (fd < 0) {
        return -EINVAL;
    }
    return close(fd) == 0 ? 0 : dmc2_negative_errno();
}
