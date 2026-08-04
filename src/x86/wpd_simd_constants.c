#include <stdint.h>

#define BYTE_VECTOR(name, value) \
    const uint8_t name[16] __attribute__((aligned(16))) = { \
        value, value, value, value, value, value, value, value, \
        value, value, value, value, value, value, value, value \
    }
#define WORD_VECTOR(name, value) \
    const uint16_t name[8] __attribute__((aligned(16))) = { \
        value, value, value, value, value, value, value, value \
    }

BYTE_VECTOR(wpd_pb_1, 1);
BYTE_VECTOR(wpd_pb_3, 3);
BYTE_VECTOR(wpd_pb_4, 4);
BYTE_VECTOR(wpd_pb_80, 0x80);
BYTE_VECTOR(wpd_pb_F8, 0xF8);
BYTE_VECTOR(wpd_pb_FE, 0xFE);
WORD_VECTOR(wpd_pw_3, 3);
WORD_VECTOR(wpd_pw_4, 4);
WORD_VECTOR(wpd_pw_8, 8);
WORD_VECTOR(wpd_pw_9, 9);
WORD_VECTOR(wpd_pw_18, 18);
WORD_VECTOR(wpd_pw_27, 27);
WORD_VECTOR(wpd_pw_63, 63);
