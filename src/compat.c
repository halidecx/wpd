#include "compat.h"

#include <stdarg.h>
#if defined(__arm__) && defined(__linux__)
#include <sys/auxv.h>
#ifndef HWCAP_NEON
#define HWCAP_NEON (1UL << 12)
#endif
#endif

uint8_t ff_cropTbl[256 + 2 * MAX_NEG_CROP];
static int crop_initialized;

#if defined(__i386__) || defined(__x86_64__)
#define BYTE_VECTOR(name, value) const uint8_t name[16] __attribute__((aligned(16))) = { \
    value,value,value,value,value,value,value,value,value,value,value,value,value,value,value,value }
#define WORD_VECTOR(name, value) const uint16_t name[8] __attribute__((aligned(16))) = { \
    value,value,value,value,value,value,value,value }
BYTE_VECTOR(ff_pb_1, 1);
BYTE_VECTOR(ff_pb_3, 3);
BYTE_VECTOR(ff_pb_4, 4);
BYTE_VECTOR(ff_pb_80, 0x80);
BYTE_VECTOR(ff_pb_F8, 0xF8);
BYTE_VECTOR(ff_pb_FE, 0xFE);
WORD_VECTOR(ff_pw_3, 3);
WORD_VECTOR(ff_pw_4, 4);
WORD_VECTOR(ff_pw_8, 8);
WORD_VECTOR(ff_pw_9, 9);
WORD_VECTOR(ff_pw_18, 18);
WORD_VECTOR(ff_pw_27, 27);
WORD_VECTOR(ff_pw_63, 63);
WORD_VECTOR(ff_pw_64, 64);
WORD_VECTOR(ff_pw_256, 256);
#endif

void *av_malloc(size_t size) { return malloc(size); }
void *av_mallocz(size_t size) { return calloc(1, size); }
void av_free(void *pointer) { free(pointer); }
void av_freep(void *pointer)
{
    void **p = pointer;
    free(*p);
    *p = NULL;
}

void av_log(void *context, int level, const char *format, ...)
{
    va_list args;
    (void)context; (void)level;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
}

void av_log_missing_feature(void *context, const char *feature, int want_sample)
{ (void)context; (void)want_sample; fprintf(stderr, "unsupported feature: %s\n", feature); }

int av_image_check_size(unsigned width, unsigned height, int log_offset, void *log_context)
{
    (void)log_offset; (void)log_context;
    return !width || !height || width > 16384 || height > 16384 ? AVERROR(EINVAL) : 0;
}

void avcodec_set_dimensions(AVCodecContext *context, int width, int height)
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
        int sy = av_clip(src_y + y, 0, height - 1);
        for (int x = 0; x < block_w; x++) {
            int sx = av_clip(src_x + x, 0, width - 1);
            dst[(ptrdiff_t)y * dst_stride + x] = origin[(ptrdiff_t)sy * src_stride + sx];
        }
    }
}

void dsputil_init(DSPContext *dsp, AVCodecContext *context)
{
    (void)context;
    if (!crop_initialized) {
        for (int i = -MAX_NEG_CROP; i < 256 + MAX_NEG_CROP; i++)
            ff_cropTbl[i + MAX_NEG_CROP] = av_clip_uint8(i);
        crop_initialized = 1;
    }
    dsp->prefetch = prefetch;
    dsp->emulated_edge_mc = emulated_edge_mc;
#if ARCH_X86 && HAVE_MMX
    ff_dsputil_init_x86(dsp);
#endif
#if ARCH_ARM
    ff_dsputil_init_arm(dsp);
#endif
#if ARCH_AARCH64
    ff_dsputil_init_aarch64(dsp);
#endif
}

int ff_thread_get_buffer(AVCodecContext *context, AVFrame *frame)
{
    const int widths[3] = { context->width, (context->width + 1) / 2, (context->width + 1) / 2 };
    const int heights[3] = { context->height, (context->height + 1) / 2, (context->height + 1) / 2 };
    for (int p = 0; p < 3; p++) {
        int stride = (widths[p] + 63) & ~31;
        size_t size = (size_t)(heights[p] + 64) * stride;
        frame->allocation[p] = calloc(1, size);
        if (!frame->allocation[p]) {
            ff_thread_release_buffer(context, frame);
            return AVERROR(ENOMEM);
        }
        frame->linesize[p] = stride;
        frame->data[p] = frame->allocation[p] + 32 * stride + 32;
    }
    return 0;
}

void ff_thread_release_buffer(AVCodecContext *context, AVFrame *frame)
{
    (void)context;
    for (int p = 0; p < 3; p++) {
        free(frame->allocation[p]);
        frame->allocation[p] = frame->data[p] = NULL;
        frame->linesize[p] = 0;
    }
}

void ff_thread_await_progress(AVFrame *frame, int progress, int field)
{ (void)frame; (void)progress; (void)field; }
void ff_thread_report_progress(AVFrame *frame, int progress, int field)
{ (void)frame; (void)progress; (void)field; }
void ff_thread_finish_setup(AVCodecContext *context) { (void)context; }

static int cpu_flags_for_test = -1;

void ffvp8_set_cpu_flags_for_test(int flags)
{
    cpu_flags_for_test = flags;
}

int av_get_cpu_flags(void)
{
    int flags = 0;
#if defined(__i386__) || defined(__x86_64__)
    __builtin_cpu_init();
    if (__builtin_cpu_supports("mmx")) flags |= AV_CPU_FLAG_MMX;
    if (__builtin_cpu_supports("sse")) flags |= AV_CPU_FLAG_MMX2 | AV_CPU_FLAG_SSE;
    if (__builtin_cpu_supports("sse2")) flags |= AV_CPU_FLAG_SSE2;
    if (__builtin_cpu_supports("ssse3")) flags |= AV_CPU_FLAG_SSSE3;
    if (__builtin_cpu_supports("sse4.1")) flags |= AV_CPU_FLAG_SSE4;
    if (__builtin_cpu_supports("avx2")) flags |= AV_CPU_FLAG_AVX2;
#endif
#if defined(__arm__)
    flags |= AV_CPU_FLAG_ARMV6;
#if defined(__linux__)
    if (getauxval(AT_HWCAP) & HWCAP_NEON) flags |= AV_CPU_FLAG_NEON;
#elif defined(__ARM_NEON)
    flags |= AV_CPU_FLAG_NEON;
#endif
#endif
#if defined(__aarch64__)
    flags |= AV_CPU_FLAG_NEON;
#endif
    return cpu_flags_for_test < 0 ? flags : flags & cpu_flags_for_test;
}
