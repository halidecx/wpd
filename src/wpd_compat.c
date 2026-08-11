#include "wpd_codec.h"

#include <stdarg.h>
#include <stdatomic.h>

void *wpd_mallocz(size_t size) { return calloc(1, size); }
void  wpd_free(void *pointer) { free(pointer); }
void  wpd_freep(void *pointer) {
    void **p = pointer;
    free(*p);
    *p = NULL;
}

unsigned    wpd_version(void) { return WPD_VERSION_NUM; }
const char *wpd_version_string(void) { return WPD_VERSION_STR; }

static _Atomic(WPDLogCallback) log_callback;
static _Atomic(void *)         log_opaque;

void wpd_set_log_callback(WPDLogCallback callback, void *opaque) {
    atomic_store_explicit(&log_opaque, opaque, memory_order_release);
    atomic_store_explicit(&log_callback, callback, memory_order_release);
}

void wpd_log(void *context, int level, const char *format, ...) {
    WPDLogCallback callback = atomic_load_explicit(&log_callback,
                                                   memory_order_acquire);
    void          *opaque;
    char           message[512];
    va_list        args;
    int            length;

    (void)context;
    if (!callback)
        return;

    opaque = atomic_load_explicit(&log_opaque, memory_order_relaxed);

    va_start(args, format);
    length = vsnprintf(message, sizeof(message), format, args);
    va_end(args);
    if (length < 0)
        return;
    if (length > (int)sizeof(message) - 1)
        length = (int)sizeof(message) - 1;
    while (length > 0 && message[length - 1] == '\n') message[--length] = '\0';

    callback(opaque, (WPDLogLevel)level, message);
}

int wpd_check_image_size(unsigned width, unsigned height) {
    return !width || !height || width > 16383 || height > 16383
        ? WPD_ERROR(EINVAL)
        : 0;
}
