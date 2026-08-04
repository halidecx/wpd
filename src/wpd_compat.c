#include "wpd_codec.h"

#include <stdarg.h>

void *wpd_mallocz(size_t size) { return calloc(1, size); }
void wpd_free(void *pointer) { free(pointer); }
void wpd_freep(void *pointer)
{
    void **p = pointer;
    free(*p);
    *p = NULL;
}

void wpd_log(void *context, int level, const char *format, ...)
{
    va_list args;
    (void)context; (void)level;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
}

int wpd_check_image_size(unsigned width, unsigned height)
{
    /* WebP codes the picture size in 14 bits. */
    return !width || !height || width > 16383 || height > 16383 ? WPD_ERROR(EINVAL) : 0;
}
