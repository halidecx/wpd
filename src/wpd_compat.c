#include "wpd_codec.h"

#include <stdarg.h>

void *wpd_malloc(size_t size) { return malloc(size); }
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

static void prefetch(uint8_t *buf, int stride, int h)
{ (void)buf; (void)stride; (void)h; }

static void emulated_edge_mc(uint8_t *dst, const uint8_t *src,
                             ptrdiff_t dst_stride, ptrdiff_t src_stride,
                             int block_w, int block_h, int src_x, int src_y,
                             int width, int height)
{
    const uint8_t *origin = src - (ptrdiff_t)src_y * src_stride - src_x;
    for (int y = 0; y < block_h; y++) {
        int sy = wpd_clip(src_y + y, 0, height - 1);
        for (int x = 0; x < block_w; x++) {
            int sx = wpd_clip(src_x + x, 0, width - 1);
            dst[(ptrdiff_t)y * dst_stride + x] = origin[(ptrdiff_t)sy * src_stride + sx];
        }
    }
}

void wpd_dsp_init(WpdDSPContext *dsp, WpdCodecContext *context)
{
    (void)context;
    wpd_dsp_data_init();
    dsp->prefetch = prefetch;
    dsp->emulated_edge_mc = emulated_edge_mc;
#if WPD_ARCH_X86 && WPD_HAVE_MMX
    wpd_dsp_init_x86(dsp);
#endif
#if WPD_ARCH_ARM
    wpd_dsp_init_arm(dsp);
#endif
#if WPD_ARCH_AARCH64
    wpd_dsp_init_aarch64(dsp);
#endif
}
