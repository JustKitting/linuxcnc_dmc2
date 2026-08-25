#ifndef DMC2_SERIAL_SHIM_H
#define DMC2_SERIAL_SHIM_H

#include <stddef.h>

int dmc2_serial_open(const char *path, unsigned int baud);
int dmc2_serial_read(int fd, unsigned char *buffer, size_t capacity);
int dmc2_serial_close(int fd);

#endif
