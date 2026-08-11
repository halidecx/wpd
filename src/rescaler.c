#include "rescaler.h"

#include "wpd_compat.h"

#include <string.h>

#define ROUNDER (WPD_RESCALER_ONE >> 1)
#define MULT_FIX(x, y) (((uint64_t)(x) * (y) + ROUNDER) >> WPD_RESCALER_RFIX)
#define MULT_FIX_FLOOR(x, y) (((uint64_t)(x) * (y)) >> WPD_RESCALER_RFIX)
#define FRAC(x, y) ((uint32_t)(((uint64_t)(x) << WPD_RESCALER_RFIX) / (y)))

void wpd_rescaler_init(WPDRescaler *r, int src_width, int src_height,
                       uint8_t *dst, int dst_width, int dst_height,
                       int dst_stride, int num_channels, uint32_t *work) {
    const int x_add = src_width, x_sub = dst_width;
    const int y_add = src_height, y_sub = dst_height;

    r->x_expand     = src_width < dst_width;
    r->y_expand     = src_height < dst_height;
    r->src_width    = src_width;
    r->src_height   = src_height;
    r->dst_width    = dst_width;
    r->dst_height   = dst_height;
    r->src_y        = 0;
    r->dst_y        = 0;
    r->dst          = dst;
    r->dst_stride   = dst_stride;
    r->num_channels = num_channels;
    r->irow         = work;
    r->frow         = work + (size_t)num_channels * dst_width;
    r->fx_scale     = 0;
    r->fxy_scale    = 0;
    memset(
        work, 0, 2 * (size_t)num_channels * (size_t)dst_width * sizeof(*work));

    r->x_add = r->x_expand ? x_sub - 1 : x_add;
    r->x_sub = r->x_expand ? x_add - 1 : x_sub;
    if (!r->x_expand)
        r->fx_scale = FRAC(1, r->x_sub);

    r->y_add   = r->y_expand ? y_add - 1 : y_add;
    r->y_sub   = r->y_expand ? y_sub - 1 : y_sub;
    r->y_accum = r->y_expand ? r->y_sub : r->y_add;
    if (!r->y_expand) {
        const uint64_t num   = (uint64_t)dst_height * WPD_RESCALER_ONE;
        const uint64_t den   = (uint64_t)r->x_add * r->y_add;
        const uint64_t ratio = num / den;

        r->fxy_scale = ratio != (uint32_t)ratio ? 0 : (uint32_t)ratio;
        r->fy_scale  = FRAC(1, r->y_sub);
    } else {
        r->fy_scale = FRAC(1, r->x_add);
    }
}

static void import_row_expand(WPDRescaler *r, const uint8_t *src) {
    const int x_stride  = r->num_channels;
    const int x_out_max = r->dst_width * r->num_channels;

    for (int channel = 0; channel < x_stride; channel++) {
        int      x_in  = channel;
        int      x_out = channel;
        int      accum = r->x_add;
        uint32_t left  = src[x_in];
        uint32_t right = r->src_width > 1 ? src[x_in + x_stride] : left;

        x_in += x_stride;
        for (;;) {
            r->frow[x_out] = right * (uint32_t)r->x_add +
                (left - right) * (uint32_t)accum;
            x_out += x_stride;
            if (x_out >= x_out_max)
                break;
            accum -= r->x_sub;
            if (accum < 0) {
                left = right;
                x_in += x_stride;
                right = src[x_in];
                accum += r->x_add;
            }
        }
    }
}

static void import_row_shrink(WPDRescaler *r, const uint8_t *src) {
    const int x_stride  = r->num_channels;
    const int x_out_max = r->dst_width * r->num_channels;

    for (int channel = 0; channel < x_stride; channel++) {
        int      x_in  = channel;
        int      x_out = channel;
        uint32_t sum   = 0;
        int      accum = 0;

        while (x_out < x_out_max) {
            uint32_t base = 0;

            accum += r->x_add;
            while (accum > 0) {
                accum -= r->x_sub;
                base = src[x_in];
                sum += base;
                x_in += x_stride;
            }
            {
                const uint32_t frac = base * (uint32_t)(-accum);

                r->frow[x_out] = sum * (uint32_t)r->x_sub - frac;
                sum            = (uint32_t)MULT_FIX(frac, r->fx_scale);
            }
            x_out += x_stride;
        }
    }
}

int wpd_rescaler_needed_lines(const WPDRescaler *r, int max_num_lines) {
    const int num_lines = (r->y_accum + r->y_sub - 1) / r->y_sub;

    return num_lines > max_num_lines ? max_num_lines : num_lines;
}

int wpd_rescaler_import(WPDRescaler *r, int num_lines, const uint8_t *src,
                        int src_stride) {
    int total_imported = 0;

    while (total_imported < num_lines && !wpd_rescaler_has_pending_output(r)) {
        if (r->y_expand) {
            uint32_t *tmp = r->irow;

            r->irow = r->frow;
            r->frow = tmp;
        }
        if (r->x_expand)
            import_row_expand(r, src);
        else
            import_row_shrink(r, src);
        if (!r->y_expand)
            for (int x = 0; x < r->num_channels * r->dst_width; x++)
                r->irow[x] += r->frow[x];
        r->src_y++;
        src += src_stride;
        total_imported++;
        r->y_accum -= r->y_sub;
    }
    return total_imported;
}

static void export_row_expand(WPDRescaler *r) {
    uint8_t        *dst       = r->dst;
    uint32_t       *irow      = r->irow;
    const int       x_out_max = r->dst_width * r->num_channels;
    const uint32_t *frow      = r->frow;

    if (r->y_accum == 0) {
        for (int x_out = 0; x_out < x_out_max; x_out++) {
            const uint32_t j = frow[x_out];
            const int      v = (int)MULT_FIX(j, r->fy_scale);

            dst[x_out] = v > 255 ? 255u : (uint8_t)v;
        }
    } else {
        const uint32_t b = FRAC(-r->y_accum, r->y_sub);
        const uint32_t a = (uint32_t)(WPD_RESCALER_ONE - b);

        for (int x_out = 0; x_out < x_out_max; x_out++) {
            const uint64_t i = (uint64_t)a * frow[x_out] +
                (uint64_t)b * irow[x_out];
            const uint32_t j = (uint32_t)((i + ROUNDER) >> WPD_RESCALER_RFIX);
            const int      v = (int)MULT_FIX(j, r->fy_scale);

            dst[x_out] = v > 255 ? 255u : (uint8_t)v;
        }
    }
}

static void export_row_shrink(WPDRescaler *r) {
    uint8_t        *dst       = r->dst;
    uint32_t       *irow      = r->irow;
    const int       x_out_max = r->dst_width * r->num_channels;
    const uint32_t *frow      = r->frow;
    const uint32_t  yscale    = r->fy_scale * (uint32_t)(-r->y_accum);

    if (yscale) {
        for (int x_out = 0; x_out < x_out_max; x_out++) {
            const uint32_t frac = (uint32_t)MULT_FIX_FLOOR(frow[x_out], yscale);
            const int      v = (int)MULT_FIX(irow[x_out] - frac, r->fxy_scale);

            dst[x_out]  = v > 255 ? 255u : (uint8_t)v;
            irow[x_out] = frac;
        }
    } else {
        for (int x_out = 0; x_out < x_out_max; x_out++) {
            const int v = (int)MULT_FIX(irow[x_out], r->fxy_scale);

            dst[x_out]  = v > 255 ? 255u : (uint8_t)v;
            irow[x_out] = 0;
        }
    }
}

void wpd_rescaler_export_row(WPDRescaler *r) {
    if (r->y_accum > 0)
        return;
    if (r->y_expand)
        export_row_expand(r);
    else if (r->fxy_scale)
        export_row_shrink(r);
    else
        /* src_width == 1 and dst_width <= 2, where the ratio does not fit. */
        for (int i = 0; i < r->num_channels * r->dst_width; i++) {
            r->dst[i]  = (uint8_t)r->irow[i];
            r->irow[i] = 0;
        }
    r->y_accum += r->y_add;
    r->dst += r->dst_stride;
    r->dst_y++;
}

int wpd_rescaler_export(WPDRescaler *r) {
    int total_exported = 0;

    while (wpd_rescaler_has_pending_output(r)) {
        wpd_rescaler_export_row(r);
        total_exported++;
    }
    return total_exported;
}

void wpd_rescale_plane(uint8_t *dst, int dst_stride, int dst_width,
                       int dst_height, const uint8_t *src, int src_stride,
                       int src_width, int src_height, int num_channels,
                       uint32_t *work) {
    WPDRescaler r;
    int         row = 0;

    wpd_rescaler_init(&r,
                      src_width,
                      src_height,
                      dst,
                      dst_width,
                      dst_height,
                      dst_stride,
                      num_channels,
                      work);
    while (row < src_height) {
        row += wpd_rescaler_import(&r,
                                   src_height - row,
                                   src + (ptrdiff_t)row * src_stride,
                                   src_stride);
        wpd_rescaler_export(&r);
    }
}

#define MFIX 24
#define MHALF (1u << (MFIX - 1))
#define KINV_255 ((1u << MFIX) / 255u)

static wpd_always_inline unsigned alpha_mult(unsigned x, uint32_t scale) {
    return ((x & 0xff) * scale + MHALF) >> MFIX;
}

static wpd_always_inline uint32_t alpha_scale(unsigned a, int inverse) {
    return inverse ? (255u << MFIX) / a : a * KINV_255;
}

void wpd_premultiply_argb_row(uint8_t *argb, int num_pixels, int inverse) {
    for (int x = 0; x < num_pixels; x++) {
        uint8_t       *p = argb + 4 * x;
        const unsigned a = p[0];
        uint32_t       scale;

        if (a == 0xff)
            continue;
        if (a == 0) {
            p[1] = p[2] = p[3] = 0;
            continue;
        }
        scale = alpha_scale(a, inverse);
        p[1]  = (uint8_t)alpha_mult(p[1], scale);
        p[2]  = (uint8_t)alpha_mult(p[2], scale);
        p[3]  = (uint8_t)alpha_mult(p[3], scale);
    }
}

void wpd_multiply_row(uint8_t *plane, const uint8_t *alpha, int num_pixels,
                      int inverse) {
    for (int x = 0; x < num_pixels; x++) {
        const unsigned a = alpha[x];

        if (a == 255)
            continue;
        plane[x] = a ? (uint8_t)alpha_mult(plane[x], alpha_scale(a, inverse))
                     : 0;
    }
}
