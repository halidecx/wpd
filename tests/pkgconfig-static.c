#include "wpd.h"

int main(void) {
    WPDDecoder *decoder = wpd_decoder_create();

    if (!decoder)
        return 1;
    wpd_decoder_free(decoder);
    return 0;
}
