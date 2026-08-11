#include "wpd_codec.h"

#include <stdarg.h>

void *wpd_mallocz(size_t size) { return calloc(1, size); }
void  wpd_free(void *pointer) { free(pointer); }
void  wpd_freep(void *pointer) {
    void **p = pointer;
    free(*p);
    *p = NULL;
}

unsigned    wpd_version(void) { return WPD_VERSION_NUM; }
const char *wpd_version_string(void) { return WPD_VERSION_STR; }

void wpd_log(void *context, int level, const char *format, ...) {
    va_list args;
    (void)context;
    (void)level;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
}

int wpd_check_image_size(unsigned width, unsigned height) {
    return !width || !height || width > 16383 || height > 16383
        ? WPD_ERROR(EINVAL)
        : 0;
}
