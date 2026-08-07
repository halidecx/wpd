#ifndef WPD_TOOLS_MD5_H
#define WPD_TOOLS_MD5_H

#include <stddef.h>
#include <stdint.h>

typedef struct WPDMD5Context {
    uint32_t state[4];
    uint8_t  data[64];
    uint64_t len;
} WPDMD5Context;

void wpd_md5_init(WPDMD5Context *ctx);
void wpd_md5_update(WPDMD5Context *ctx, const uint8_t *data, size_t len);
void wpd_md5_final(WPDMD5Context *ctx, uint8_t digest[16]);

#endif
