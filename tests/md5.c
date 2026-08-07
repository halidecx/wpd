#include "md5.h"

#include <stdio.h>
#include <string.h>

static int check_md5(const char *input, const char *expected, size_t chunk) {
    WPDMD5Context ctx;
    uint8_t       digest[16];
    char          actual[33];
    size_t        len = strlen(input);

    wpd_md5_init(&ctx);
    for (size_t offset = 0; offset < len;) {
        size_t size = len - offset < chunk ? len - offset : chunk;
        wpd_md5_update(&ctx, (const uint8_t *)input + offset, size);
        offset += size;
    }
    wpd_md5_final(&ctx, digest);
    for (int i = 0; i < 16; i++) snprintf(actual + i * 2, 3, "%02x", digest[i]);

    if (!strcmp(actual, expected))
        return 0;
    fprintf(stderr, "md5(%s): expected %s, got %s\n", input, expected, actual);
    return 1;
}

int main(void) {
    static const struct {
        const char *input;
        const char *digest;
    } vectors[] = {
        {"", "d41d8cd98f00b204e9800998ecf8427e"},
        {"a", "0cc175b9c0f1b6a831c399e269772661"},
        {"abc", "900150983cd24fb0d6963f7d28e17f72"},
        {"message digest", "f96b697d7cb7938d525a2f31aaf161d0"},
        {"abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"},
        {"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
         "d174ab98d277d9f5a5611c2c9f419d9f"},
        {"1234567890123456789012345678901234567890123456789012345678901234567"
         "8901234567890",
         "57edf4a22be3c955ac49da2e2107b67a"},
    };
    int status = 0;

    for (size_t i = 0; i < sizeof(vectors) / sizeof(*vectors); i++) {
        status |= check_md5(
            vectors[i].input, vectors[i].digest, strlen(vectors[i].input) + 1);
        status |= check_md5(vectors[i].input, vectors[i].digest, 1);
    }
    return status;
}
