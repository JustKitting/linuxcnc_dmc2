#define _DEFAULT_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <termios.h>
#include <unistd.h>

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
        errno = EINVAL;
        return -1;
    }

    const int fd = open(path, O_RDWR | O_NOCTTY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        return -1;
    }
    struct termios options;
    if (tcgetattr(fd, &options) != 0) {
        close(fd);
        return -1;
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
        close(fd);
        return -1;
    }
    if (tcflush(fd, TCIFLUSH) != 0) {
        close(fd);
        return -1;
    }
    return fd;
}

int dmc2_serial_read(int fd, unsigned char *buffer, size_t capacity) {
    if (fd < 0 || buffer == NULL || capacity == 0) {
        errno = EINVAL;
        return -1;
    }
    const ssize_t result = read(fd, buffer, capacity);
    if (result < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR)) {
        return 0;
    }
    if (result < 0 || result > 2147483647) {
        return -1;
    }
    return (int)result;
}

void dmc2_serial_close(int fd) {
    if (fd >= 0) {
        close(fd);
    }
}
