#ifndef FFVP8_COMPAT_H
#define FFVP8_COMPAT_H

#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__GNUC__) || defined(__clang__)
#define av_always_inline inline __attribute__((always_inline))
#define av_cold __attribute__((cold))
#define av_noinline __attribute__((noinline))
#define av_unused __attribute__((unused))
#define av_const __attribute__((const))
#define DECLARE_ALIGNED(n,t,v) t __attribute__((aligned(n))) v
#else
#define av_always_inline inline
#define av_cold
#define av_noinline
#define av_unused
#define av_const
#define DECLARE_ALIGNED(n,t,v) t v
#endif

#define LOCAL_ALIGNED(a,t,n,s) DECLARE_ALIGNED(a,t,n) s

#if defined(__aarch64__)
#define ARCH_AARCH64 1
#else
#define ARCH_AARCH64 0
#endif
#if defined(__arm__)
#define ARCH_ARM 1
#else
#define ARCH_ARM 0
#endif
#if defined(__i386__) || defined(__x86_64__)
#define ARCH_X86 1
#if defined(FFVP8_ENABLE_X86_SIMD)
#define HAVE_MMX 1
#else
#define HAVE_MMX 0
#endif
#else
#define ARCH_X86 0
#define HAVE_MMX 0
#endif
#define HAVE_ALTIVEC 0
#define HAVE_BIGENDIAN 0
#define AV_CPU_FLAG_MMX      (1 << 0)
#define AV_CPU_FLAG_MMX2     (1 << 1)
#define AV_CPU_FLAG_SSE      (1 << 2)
#define AV_CPU_FLAG_SSE2     (1 << 3)
#define AV_CPU_FLAG_SSE2SLOW (1 << 4)
#define AV_CPU_FLAG_SSSE3    (1 << 5)
#define AV_CPU_FLAG_SSE4     (1 << 6)
#define AV_CPU_FLAG_NEON     (1 << 7)
#define AV_CPU_FLAG_AVX2     (1 << 8)
#define AV_CPU_FLAG_ARMV6    (1 << 9)
#define EXTERNAL_MMX(f)      ((f) & AV_CPU_FLAG_MMX)
#define EXTERNAL_MMXEXT(f)   ((f) & AV_CPU_FLAG_MMX2)
#define EXTERNAL_SSE(f)      ((f) & AV_CPU_FLAG_SSE)
#define EXTERNAL_SSE2(f)     ((f) & AV_CPU_FLAG_SSE2)
#define EXTERNAL_SSE2_SLOW(f) ((f) & (AV_CPU_FLAG_SSE2 | AV_CPU_FLAG_SSE2SLOW))
#define EXTERNAL_SSSE3(f)    ((f) & AV_CPU_FLAG_SSSE3)
#define EXTERNAL_SSE4(f)     ((f) & AV_CPU_FLAG_SSE4)
#define EXTERNAL_AVX2(f)     ((f) & AV_CPU_FLAG_AVX2)
#if ARCH_ARM || ARCH_AARCH64
static av_always_inline int have_armv6(int flags) { return !!(flags & AV_CPU_FLAG_ARMV6); }
static av_always_inline int have_neon(int flags) { return !!(flags & AV_CPU_FLAG_NEON); }
#endif
#define CODEC_ID_VP8 1
#define CODEC_ID_RV40 2
#define CODEC_ID_SVQ3 3
#define PIX_FMT_YUV420P 0
#define CODEC_FLAG_EMU_EDGE 1
#define AV_PICTURE_TYPE_I 1
#define AV_PICTURE_TYPE_P 2
#define AV_LOG_ERROR 0
#define AV_LOG_WARNING 1
#define AV_LOG_FATAL 2
#define AVERROR(e) (-(e))
#define AVERROR_INVALIDDATA (-1094995529)
#define AVDISCARD_NONE -16
#define AVDISCARD_DEFAULT 0
#define AVDISCARD_NONREF 8
#define AVDISCARD_NONKEY 32
#define AVDISCARD_ALL 48
#define FFMIN(a,b) ((a) > (b) ? (b) : (a))
#define FFMAX(a,b) ((a) > (b) ? (a) : (b))
#define FFABS(a) ((a) >= 0 ? (a) : -(a))
#define FF_ARRAY_ELEMS(a) (sizeof(a) / sizeof((a)[0]))
#define INT_BIT (sizeof(int) * CHAR_BIT)
#define FFSWAP(type,a,b) do { type ffvp8_swap = (a); (a) = (b); (b) = ffvp8_swap; } while (0)
#define av_uninit(x) x

typedef int16_t DCTELEM;
enum AVDiscard {
    FFVP8_DISCARD_NONE = AVDISCARD_NONE,
    FFVP8_DISCARD_DEFAULT = AVDISCARD_DEFAULT,
    FFVP8_DISCARD_NONREF = AVDISCARD_NONREF,
    FFVP8_DISCARD_NONKEY = AVDISCARD_NONKEY,
    FFVP8_DISCARD_ALL = AVDISCARD_ALL
};

#define MAX_NEG_CROP 1024
extern uint8_t ff_cropTbl[256 + 2 * MAX_NEG_CROP];

typedef struct AVFrame {
    uint8_t *data[4];
    uint8_t *allocation[3];
    int linesize[4];
    uint8_t *ref_index[4];
    int key_frame;
    int pict_type;
    int reference;
} AVFrame;

typedef struct AVCodecContext {
    void *priv_data;
    int width, height;
    int coded_width, coded_height;
    int flags;
    int skip_frame;
    int skip_loop_filter;
    int is_copy;
    int pix_fmt;
} AVCodecContext;

typedef struct AVPacket {
    const uint8_t *data;
    int size;
} AVPacket;

typedef struct DSPContext {
    void (*prefetch)(uint8_t *buf, int stride, int h);
    void (*emulated_edge_mc)(uint8_t *buf, const uint8_t *src,
                             ptrdiff_t dst_linesize, ptrdiff_t src_linesize,
                             int block_w, int block_h, int src_x, int src_y,
                             int width, int height);
} DSPContext;

static av_always_inline int av_clip(int value, int low, int high)
{ return value < low ? low : value > high ? high : value; }
static av_always_inline unsigned av_clip_uint8(int value)
{ return value & ~255 ? (unsigned)((-value >> 31) & 255) : (unsigned)value; }
static av_always_inline unsigned av_clip_uintp2(int value, int bits)
{ int max = (1 << bits) - 1; return value < 0 ? 0 : value > max ? max : value; }

static av_always_inline uint16_t ffvp8_r16(const void *p)
{ uint16_t v; memcpy(&v, p, 2); return v; }
static av_always_inline uint32_t ffvp8_r32(const void *p)
{ uint32_t v; memcpy(&v, p, 4); return v; }
static av_always_inline uint64_t ffvp8_r64(const void *p)
{ uint64_t v; memcpy(&v, p, 8); return v; }
static av_always_inline void ffvp8_w16(void *p, uint16_t v) { memcpy(p, &v, 2); }
static av_always_inline void ffvp8_w32(void *p, uint32_t v) { memcpy(p, &v, 4); }
static av_always_inline void ffvp8_w64(void *p, uint64_t v) { memcpy(p, &v, 8); }

#define AV_RN16(p) ffvp8_r16(p)
#define AV_RN32(p) ffvp8_r32(p)
#define AV_RN32A(p) ffvp8_r32(p)
#define AV_WN16(p,v) ffvp8_w16(p,v)
#define AV_WN32(p,v) ffvp8_w32(p,v)
#define AV_WN32A(p,v) ffvp8_w32(p,v)
#define AV_WN64(p,v) ffvp8_w64(p,v)
#define AV_RL16(p) ((uint16_t)((p)[0] | (p)[1] << 8))
#define AV_RL24(p) ((uint32_t)((p)[0] | (p)[1] << 8 | (p)[2] << 16))
#define AV_RL32(p) ((uint32_t)((p)[0] | (p)[1] << 8 | (p)[2] << 16 | (uint32_t)(p)[3] << 24))
#define AV_COPY32(d,s) ffvp8_w32(d, ffvp8_r32(s))
#define AV_COPY64(d,s) ffvp8_w64(d, ffvp8_r64(s))
#define AV_COPY128(d,s) memcpy(d,s,16)
#define AV_ZERO32(d) memset(d,0,4)
#define AV_ZERO64(d) memset(d,0,8)
#define AV_ZERO128(d) memset(d,0,16)
#define AV_SWAP64(a,b) do { uint64_t ffvp8_v = ffvp8_r64(a); AV_COPY64(a,b); ffvp8_w64(b,ffvp8_v); } while (0)

#define PACK_4U8(a,b,c,d) ((uint32_t)(a) | (uint32_t)(b)<<8 | (uint32_t)(c)<<16 | (uint32_t)(d)<<24)
#define NULL_IF_CONFIG_SMALL(x) x
#define ONLY_IF_THREADS_ENABLED(x) NULL

static av_always_inline unsigned bytestream_get_be16(const uint8_t **p)
{ unsigned v = (unsigned)(*p)[0] << 8 | (*p)[1]; *p += 2; return v; }
static av_always_inline unsigned bytestream_get_be24(const uint8_t **p)
{ unsigned v = (unsigned)(*p)[0] << 16 | (unsigned)(*p)[1] << 8 | (*p)[2]; *p += 3; return v; }

void *av_malloc(size_t size);
void *av_mallocz(size_t size);
void av_free(void *pointer);
void av_freep(void *pointer);
void av_log(void *context, int level, const char *format, ...);
void av_log_missing_feature(void *context, const char *feature, int want_sample);
int av_image_check_size(unsigned width, unsigned height, int log_offset, void *log_context);
void avcodec_set_dimensions(AVCodecContext *context, int width, int height);
void dsputil_init(DSPContext *dsp, AVCodecContext *context);
void ff_dsputil_init_x86(DSPContext *dsp);
void ff_dsputil_init_arm(DSPContext *dsp);
void ff_dsputil_init_aarch64(DSPContext *dsp);
int ff_thread_get_buffer(AVCodecContext *context, AVFrame *frame);
void ff_thread_release_buffer(AVCodecContext *context, AVFrame *frame);
void ff_thread_await_progress(AVFrame *frame, int progress, int field);
void ff_thread_report_progress(AVFrame *frame, int progress, int field);
void ff_thread_finish_setup(AVCodecContext *context);
int av_get_cpu_flags(void);
void ffvp8_set_cpu_flags_for_test(int flags);

#endif
