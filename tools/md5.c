/*
 * Copyright © 2018, VideoLAN and dav1d authors
 * Copyright © 2018, Two Orioles, LLC
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright notice,
 *    this list of conditions and the following disclaimer.
 *
 * 2. Redistributions in binary form must reproduce the above copyright notice,
 *    this list of conditions and the following disclaimer in the documentation
 *    and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
 * AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
 * LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

#include "md5.h"

#include <string.h>

static const uint32_t k[64] = {
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a,
    0xa8304613, 0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
    0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
    0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8,
    0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
    0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
    0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
    0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
    0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
};

static uint32_t load_le32(const uint8_t *data) {
    return (uint32_t)data[0] | (uint32_t)data[1] << 8 |
        (uint32_t)data[2] << 16 | (uint32_t)data[3] << 24;
}

static uint32_t leftrotate(uint32_t value, int shift) {
    return value << shift | value >> (32 - shift);
}

#define F(i)                                                                   \
    do {                                                                       \
        a = b +                                                                \
            leftrotate(a + ((b & c) | (~b & d)) + k[(i) + 0] + words[(i) + 0], \
                       7);                                                     \
        d = a +                                                                \
            leftrotate(d + ((a & b) | (~a & c)) + k[(i) + 1] + words[(i) + 1], \
                       12);                                                    \
        c = d +                                                                \
            leftrotate(c + ((d & a) | (~d & b)) + k[(i) + 2] + words[(i) + 2], \
                       17);                                                    \
        b = c +                                                                \
            leftrotate(b + ((c & d) | (~c & a)) + k[(i) + 3] + words[(i) + 3], \
                       22);                                                    \
    } while (0)

#define G(i)                                                                   \
    do {                                                                       \
        a = b +                                                                \
            leftrotate(                                                        \
                a + ((d & b) | (~d & c)) + k[(i) + 0] + words[((i) + 1) & 15], \
                5);                                                            \
        d = a +                                                                \
            leftrotate(                                                        \
                d + ((c & a) | (~c & b)) + k[(i) + 1] + words[((i) + 6) & 15], \
                9);                                                            \
        c = d +                                                                \
            leftrotate(c + ((b & d) | (~b & a)) + k[(i) + 2] +                 \
                           words[((i) + 11) & 15],                             \
                       14);                                                    \
        b = c +                                                                \
            leftrotate(                                                        \
                b + ((a & c) | (~a & d)) + k[(i) + 3] + words[((i) + 0) & 15], \
                20);                                                           \
    } while (0)

#define H(i)                                                                  \
    do {                                                                      \
        a = b +                                                               \
            leftrotate(a + (b ^ c ^ d) + k[(i) + 0] + words[(5 - (i)) & 15],  \
                       4);                                                    \
        d = a +                                                               \
            leftrotate(d + (a ^ b ^ c) + k[(i) + 1] + words[(8 - (i)) & 15],  \
                       11);                                                   \
        c = d +                                                               \
            leftrotate(c + (d ^ a ^ b) + k[(i) + 2] + words[(11 - (i)) & 15], \
                       16);                                                   \
        b = c +                                                               \
            leftrotate(b + (c ^ d ^ a) + k[(i) + 3] + words[(14 - (i)) & 15], \
                       23);                                                   \
    } while (0)

#define I(i)                                                                   \
    do {                                                                       \
        a = b +                                                                \
            leftrotate(                                                        \
                a + (c ^ (b | ~d)) + k[(i) + 0] + words[(0 - (i)) & 15], 6);   \
        d = a +                                                                \
            leftrotate(                                                        \
                d + (b ^ (a | ~c)) + k[(i) + 1] + words[(7 - (i)) & 15], 10);  \
        c = d +                                                                \
            leftrotate(                                                        \
                c + (a ^ (d | ~b)) + k[(i) + 2] + words[(14 - (i)) & 15], 15); \
        b = c +                                                                \
            leftrotate(                                                        \
                b + (d ^ (c | ~a)) + k[(i) + 3] + words[(5 - (i)) & 15], 21);  \
    } while (0)

static void md5_body(WPDMD5Context *ctx, const uint8_t *data) {
    uint32_t words[16];
    uint32_t a = ctx->state[0];
    uint32_t b = ctx->state[1];
    uint32_t c = ctx->state[2];
    uint32_t d = ctx->state[3];

    for (int i = 0; i < 16; i++) words[i] = load_le32(data + i * 4);

    F(0);
    F(4);
    F(8);
    F(12);
    G(16);
    G(20);
    G(24);
    G(28);
    H(32);
    H(36);
    H(40);
    H(44);
    I(48);
    I(52);
    I(56);
    I(60);

    ctx->state[0] += a;
    ctx->state[1] += b;
    ctx->state[2] += c;
    ctx->state[3] += d;
}

void wpd_md5_init(WPDMD5Context *ctx) {
    ctx->state[0] = 0x67452301;
    ctx->state[1] = 0xefcdab89;
    ctx->state[2] = 0x98badcfe;
    ctx->state[3] = 0x10325476;
    ctx->len      = 0;
}

void wpd_md5_update(WPDMD5Context *ctx, const uint8_t *data, size_t len) {
    size_t buffered = ctx->len & 63;

    if (!len)
        return;

    if (buffered) {
        size_t copy = len < 64 - buffered ? len : 64 - buffered;
        memcpy(ctx->data + buffered, data, copy);
        data += copy;
        len -= copy;
        ctx->len += copy;
        if ((ctx->len & 63) == 0)
            md5_body(ctx, ctx->data);
    }

    while (len >= 64) {
        md5_body(ctx, data);
        data += 64;
        len -= 64;
        ctx->len += 64;
    }

    if (len) {
        memcpy(ctx->data, data, len);
        ctx->len += len;
    }
}

void wpd_md5_final(WPDMD5Context *ctx, uint8_t digest[16]) {
    static const uint8_t padding[64] = {0x80};
    uint8_t              length[8];
    uint64_t             bits = ctx->len << 3;
    size_t               pad  = (ctx->len & 63) < 56 ? 56 - (ctx->len & 63)
                                                     : 120 - (ctx->len & 63);

    for (int i = 0; i < 8; i++) length[i] = bits >> (i * 8);
    wpd_md5_update(ctx, padding, pad);
    wpd_md5_update(ctx, length, sizeof(length));

    for (int i = 0; i < 4; i++) {
        digest[i * 4 + 0] = ctx->state[i];
        digest[i * 4 + 1] = ctx->state[i] >> 8;
        digest[i * 4 + 2] = ctx->state[i] >> 16;
        digest[i * 4 + 3] = ctx->state[i] >> 24;
    }
}
