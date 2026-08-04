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

void wpd_log_missing_feature(void *context, const char *feature, int want_sample)
{ (void)context; (void)want_sample; fprintf(stderr, "unsupported feature: %s\n", feature); }

int wpd_check_image_size(unsigned width, unsigned height, int log_offset, void *log_context)
{
    (void)log_offset; (void)log_context;
    return !width || !height || width > 16384 || height > 16384 ? WPD_ERROR(EINVAL) : 0;
}

void wpd_set_dimensions(WpdCodecContext *context, int width, int height)
{
    context->width = context->coded_width = width;
    context->height = context->coded_height = height;
}
