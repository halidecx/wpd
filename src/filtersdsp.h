#ifndef WPD_FILTERSDSP_H
#define WPD_FILTERSDSP_H

#include <stdint.h>

/* Reconstructs one alpha row in place. A null prev marks the top row, which
 * is left-predicted whatever the mode. */
typedef void (*unfilter_func)(const uint8_t *prev, uint8_t *row, int width);

typedef struct WPDFILTERSDSP {
    unfilter_func horizontal_unfilter;
    unfilter_func vertical_unfilter;
    unfilter_func gradient_unfilter;
} WPDFILTERSDSP;

void wpd_filters_dsp_init(WPDFILTERSDSP *dsp);

#endif
