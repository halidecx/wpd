#ifndef WPD_TESTUTIL_H
#define WPD_TESTUTIL_H

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static inline uint8_t *read_file(const char *path, size_t *size) {
    FILE    *file = fopen(path, "rb");
    uint8_t *data;
    long     length;

    if (!file)
        return NULL;
    if (fseek(file, 0, SEEK_END) || (length = ftell(file)) < 0) {
        fclose(file);
        return NULL;
    }
    rewind(file);
    data = malloc((size_t)length);
    if (data && fread(data, 1, (size_t)length, file) != (size_t)length) {
        free(data);
        data = NULL;
    }
    fclose(file);
    *size = (size_t)length;
    return data;
}

static inline long compare_packed(const uint8_t *got, ptrdiff_t got_stride,
                                  const uint8_t *want, ptrdiff_t want_stride,
                                  int width, int height, int bpp) {
    long differing = 0;

    for (int y = 0; y < height; y++)
        for (int x = 0; x < width * bpp; x++)
            differing += got[(ptrdiff_t)y * got_stride + x] !=
                want[(ptrdiff_t)y * want_stride + x];
    return differing;
}

#endif
