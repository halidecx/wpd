#ifndef WPD_TEST_CHECKASM_H
#define WPD_TEST_CHECKASM_H

#include <checkasm/checkasm.h>
#include <checkasm/test.h>
#include <checkasm/utils.h>

#include "wpd_codec.h"

void checkasm_check_lossless(void);
void checkasm_check_vp8coeffs(void);
void checkasm_check_vp8dsp(void);
void checkasm_check_vp8pred(void);

#define rnd checkasm_rand
#define LOCAL_ALIGNED_16(type, name, ...) \
    type name __VA_ARGS__ __attribute__((aligned(16)))

#endif
