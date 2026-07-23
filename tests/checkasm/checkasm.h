/* Compatibility glue for FFmpeg-derived checkasm tests and checkasm 1.x. */
#ifndef FFVP8_TEST_CHECKASM_H
#define FFVP8_TEST_CHECKASM_H

#include <checkasm/checkasm.h>
#include <checkasm/test.h>
#include <checkasm/utils.h>

#include "compat.h"

#define rnd checkasm_rand
#define AV_CPU_FLAG_MMXEXT AV_CPU_FLAG_MMX2
#define LOCAL_ALIGNED_16(type, name, ...) \
    type name __VA_ARGS__ __attribute__((aligned(16)))

#endif
