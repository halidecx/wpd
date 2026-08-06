#ifndef WPD_TEST_CHECKASM_H
#define WPD_TEST_CHECKASM_H

#include <checkasm/checkasm.h>
#include <checkasm/test.h>
#include <checkasm/utils.h>

#include "wpd_codec.h"

#define rnd checkasm_rand
#define LOCAL_ALIGNED_16(type, name, ...) \
    type name __VA_ARGS__ __attribute__((aligned(16)))

#endif
