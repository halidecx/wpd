#include "wpd_codec.h"
#include "vp8.h"
#if WPD_ARCH_ARM
#   include "arm/vp8.h"
#endif

/*
 * Static VP8 probability, prediction, token, and quantization tables.
 * Originally kept in vp8data.h; local to this translation unit.
 */
static const uint8_t vp8_pred4x4_mode[] =
{
    [DC_PRED8x8]    = DC_PRED,
    [VERT_PRED8x8]  = VERT_PRED,
    [HOR_PRED8x8]   = HOR_PRED,
    [PLANE_PRED8x8] = TM_VP8_PRED,
};

static const int8_t vp8_pred16x16_tree_intra[4][2] =
{
    { -MODE_I4x4, 1 },                      // '0'
     { 2, 3 },
      {  -DC_PRED8x8,  -VERT_PRED8x8 },     // '100', '101'
      { -HOR_PRED8x8, -PLANE_PRED8x8 },     // '110', '111'
};

static const int8_t vp8_pred16x16_tree_inter[4][2] =
{
    { -DC_PRED8x8, 1 },                     // '0'
     { 2, 3 },
      {  -VERT_PRED8x8, -HOR_PRED8x8 },     // '100', '101'
      { -PLANE_PRED8x8, -MODE_I4x4 },       // '110', '111'
};

static const int vp8_mode_contexts[6][4] = {
    {   7,   1,   1, 143 },
    {  14,  18,  14, 107 },
    { 135,  64,  57,  68 },
    {  60,  56, 128,  65 },
    { 159, 134, 128,  34 },
    { 234, 188, 128,  28 },
};

static const uint8_t vp8_mbsplits[5][16] = {
    {  0,  0,  0,  0,  0,  0,  0,  0,
       1,  1,  1,  1,  1,  1,  1,  1  },
    {  0,  0,  1,  1,  0,  0,  1,  1,
       0,  0,  1,  1,  0,  0,  1,  1  },
    {  0,  0,  1,  1,  0,  0,  1,  1,
       2,  2,  3,  3,  2,  2,  3,  3  },
    {  0,  1,  2,  3,  4,  5,  6,  7,
       8,  9, 10, 11, 12, 13, 14, 15  },
    {  0,  0,  0,  0,  0,  0,  0,  0,
       0,  0,  0,  0,  0,  0,  0,  0  }
};

static const uint8_t vp8_mbfirstidx[4][16] = {
    {  0,  8 }, {  0,  2 }, {  0,  2,  8,  10 },
    {  0,  1,  2,  3,  4,  5,  6,  7,
       8,  9, 10, 11, 12, 13, 14, 15 }
};

static const uint8_t vp8_mbsplit_count[4] = {   2,   2,   4,  16 };
static const uint8_t vp8_mbsplit_prob[3]  = { 110, 111, 150 };

static const uint8_t vp8_submv_prob[5][3] = {
    { 147, 136,  18 },
    { 106, 145,   1 },
    { 179, 121,   1 },
    { 223,   1,  34 },
    { 208,   1,   1 }
};

static const uint8_t vp8_pred16x16_prob_intra[4] = { 145, 156, 163, 128 };
static const uint8_t vp8_pred16x16_prob_inter[4] = { 112,  86, 140,  37 };

static const int8_t vp8_pred4x4_tree[9][2] =
{
    { -DC_PRED, 1 },                                    // '0'
     { -TM_VP8_PRED, 2 },                               // '10'
      { -VERT_PRED, 3 },                                // '110'
       { 4, 6 },
        { -HOR_PRED, 5 },                               // '11100'
         { -DIAG_DOWN_RIGHT_PRED, -VERT_RIGHT_PRED },   // '111010', '111011'
        { -DIAG_DOWN_LEFT_PRED, 7 },                    // '11110'
         { -VERT_LEFT_PRED, 8 },                        // '111110'
          { -HOR_DOWN_PRED, -HOR_UP_PRED },             // '1111110', '1111111'
};

static const int8_t vp8_pred8x8c_tree[3][2] =
{
    { -DC_PRED8x8, 1 },                 // '0'
     { -VERT_PRED8x8, 2 },              // '10
      { -HOR_PRED8x8, -PLANE_PRED8x8 }, // '110', '111'
};

static const uint8_t vp8_pred8x8c_prob_intra[3] = { 142, 114, 183 };
static const uint8_t vp8_pred8x8c_prob_inter[3] = { 162, 101, 204 };

static const uint8_t vp8_pred4x4_prob_inter[9] =
{
    120, 90, 79, 133, 87, 85, 80, 111, 151
};

static const uint8_t vp8_pred4x4_prob_intra[10][10][9] =
{
    {
        {  39,  53, 200,  87,  26,  21,  43, 232, 171 },
        {  56,  34,  51, 104, 114, 102,  29,  93,  77 },
        {  88,  88, 147, 150,  42,  46,  45, 196, 205 },
        { 107,  54,  32,  26,  51,   1,  81,  43,  31 },
        {  39,  28,  85, 171,  58, 165,  90,  98,  64 },
        {  34,  22, 116, 206,  23,  34,  43, 166,  73 },
        {  34,  19,  21, 102, 132, 188,  16,  76, 124 },
        {  68,  25, 106,  22,  64, 171,  36, 225, 114 },
        {  62,  18,  78,  95,  85,  57,  50,  48,  51 },
        {  43,  97, 183, 117,  85,  38,  35, 179,  61 },
    },
    {
        { 112, 113,  77,  85, 179, 255,  38, 120, 114 },
        {  40,  42,   1, 196, 245, 209,  10,  25, 109 },
        { 193, 101,  35, 159, 215, 111,  89,  46, 111 },
        { 100,  80,   8,  43, 154,   1,  51,  26,  71 },
        {  88,  43,  29, 140, 166, 213,  37,  43, 154 },
        {  61,  63,  30, 155,  67,  45,  68,   1, 209 },
        {  41,  40,   5, 102, 211, 183,   4,   1, 221 },
        { 142,  78,  78,  16, 255, 128,  34, 197, 171 },
        {  51,  50,  17, 168, 209, 192,  23,  25,  82 },
        {  60, 148,  31, 172, 219, 228,  21,  18, 111 },
    },
    {
        { 175,  69, 143,  80,  85,  82,  72, 155, 103 },
        {  56,  58,  10, 171, 218, 189,  17,  13, 152 },
        { 231, 120,  48,  89, 115, 113, 120, 152, 112 },
        { 144,  71,  10,  38, 171, 213, 144,  34,  26 },
        { 114,  26,  17, 163,  44, 195,  21,  10, 173 },
        { 121,  24,  80, 195,  26,  62,  44,  64,  85 },
        {  63,  20,   8, 114, 114, 208,  12,   9, 226 },
        { 170,  46,  55,  19, 136, 160,  33, 206,  71 },
        {  81,  40,  11,  96, 182,  84,  29,  16,  36 },
        { 152, 179,  64, 126, 170, 118,  46,  70,  95 },
    },
    {
        {  75,  79, 123,  47,  51, 128,  81, 171,   1 },
        {  57,  17,   5,  71, 102,  57,  53,  41,  49 },
        { 125,  98,  42,  88, 104,  85, 117, 175,  82 },
        { 115,  21,   2,  10, 102, 255, 166,  23,   6 },
        {  38,  33,  13, 121,  57,  73,  26,   1,  85 },
        {  41,  10,  67, 138,  77, 110,  90,  47, 114 },
        {  57,  18,  10, 102, 102, 213,  34,  20,  43 },
        { 101,  29,  16,  10,  85, 128, 101, 196,  26 },
        { 117,  20,  15,  36, 163, 128,  68,   1,  26 },
        {  95,  84,  53,  89, 128, 100, 113, 101,  45 },
    },
    {
        {  63,  59,  90, 180,  59, 166,  93,  73, 154 },
        {  40,  40,  21, 116, 143, 209,  34,  39, 175 },
        { 138,  31,  36, 171,  27, 166,  38,  44, 229 },
        {  57,  46,  22,  24, 128,   1,  54,  17,  37 },
        {  47,  15,  16, 183,  34, 223,  49,  45, 183 },
        {  46,  17,  33, 183,   6,  98,  15,  32, 183 },
        {  40,   3,   9, 115,  51, 192,  18,   6, 223 },
        {  65,  32,  73, 115,  28, 128,  23, 128, 205 },
        {  87,  37,   9, 115,  59,  77,  64,  21,  47 },
        {  67,  87,  58, 169,  82, 115,  26,  59, 179 },
    },
    {
        {  54,  57, 112, 184,   5,  41,  38, 166, 213 },
        {  30,  34,  26, 133, 152, 116,  10,  32, 134 },
        { 104,  55,  44, 218,   9,  54,  53, 130, 226 },
        {  75,  32,  12,  51, 192, 255, 160,  43,  51 },
        {  39,  19,  53, 221,  26, 114,  32,  73, 255 },
        {  31,   9,  65, 234,   2,  15,   1, 118,  73 },
        {  56,  21,  23, 111,  59, 205,  45,  37, 192 },
        {  88,  31,  35,  67, 102,  85,  55, 186,  85 },
        {  55,  38,  70, 124,  73, 102,   1,  34,  98 },
        {  64,  90,  70, 205,  40,  41,  23,  26,  57 },
    },
    {
        {  86,  40,  64, 135, 148, 224,  45, 183, 128 },
        {  22,  26,  17, 131, 240, 154,  14,   1, 209 },
        { 164,  50,  31, 137, 154, 133,  25,  35, 218 },
        {  83,  12,  13,  54, 192, 255,  68,  47,  28 },
        {  45,  16,  21,  91,  64, 222,   7,   1, 197 },
        {  56,  21,  39, 155,  60, 138,  23, 102, 213 },
        {  18,  11,   7,  63, 144, 171,   4,   4, 246 },
        {  85,  26,  85,  85, 128, 128,  32, 146, 171 },
        {  35,  27,  10, 146, 174, 171,  12,  26, 128 },
        {  51, 103,  44, 131, 131, 123,  31,   6, 158 },
    },
    {
        {  68,  45, 128,  34,   1,  47,  11, 245, 171 },
        {  62,  17,  19,  70, 146,  85,  55,  62,  70 },
        { 102,  61,  71,  37,  34,  53,  31, 243, 192 },
        {  75,  15,   9,   9,  64, 255, 184, 119,  16 },
        {  37,  43,  37, 154, 100, 163,  85, 160,   1 },
        {  63,   9,  92, 136,  28,  64,  32, 201,  85 },
        {  56,   8,  17, 132, 137, 255,  55, 116, 128 },
        {  86,   6,  28,   5,  64, 255,  25, 248,   1 },
        {  58,  15,  20,  82, 135,  57,  26, 121,  40 },
        {  69,  60,  71,  38,  73, 119,  28, 222,  37 },
    },
    {
        { 101,  75, 128, 139, 118, 146, 116, 128,  85 },
        {  56,  41,  15, 176, 236,  85,  37,   9,  62 },
        { 190,  80,  35,  99, 180,  80, 126,  54,  45 },
        { 146,  36,  19,  30, 171, 255,  97,  27,  20 },
        {  71,  30,  17, 119, 118, 255,  17,  18, 138 },
        { 101,  38,  60, 138,  55,  70,  43,  26, 142 },
        {  32,  41,  20, 117, 151, 142,  20,  21, 163 },
        { 138,  45,  61,  62, 219,   1,  81, 188,  64 },
        { 112,  19,  12,  61, 195, 128,  48,   4,  24 },
        {  85, 126,  47,  87, 176,  51,  41,  20,  32 },
    },
    {
        {  66, 102, 167,  99,  74,  62,  40, 234, 128 },
        {  41,  53,   9, 178, 241, 141,  26,   8, 107 },
        { 134, 183,  89, 137,  98, 101, 106, 165, 148 },
        { 104,  79,  12,  27, 217, 255,  87,  17,   7 },
        {  74,  43,  26, 146,  73, 166,  49,  23, 157 },
        {  65,  38, 105, 160,  51,  52,  31, 115, 128 },
        {  47,  41,  14, 110, 182, 183,  21,  17, 194 },
        {  87,  68,  71,  44, 114,  51,  15, 186,  23 },
        {  66,  45,  25, 102, 197, 189,  23,  18,  22 },
        {  72, 187, 100, 130, 157, 111,  32,  75,  80 },
    },
};

static const uint8_t vp8_coeff_band[16] =
{
    0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7
};

/* Inverse of vp8_coeff_band: mappings of bands to coefficient indexes.
 * Each list is -1-terminated. */
static const int8_t vp8_coeff_band_indexes[8][10] =
{
    {0, -1},
    {1, -1},
    {2, -1},
    {3, -1},
    {5, -1},
    {6, -1},
    {4, 7, 8, 9, 10, 11, 12, 13, 14, -1},
    {15, -1}
};

static const uint8_t vp8_dct_cat1_prob[] = { 159, 0 };
static const uint8_t vp8_dct_cat2_prob[] = { 165, 145, 0 };
static const uint8_t vp8_dct_cat3_prob[] = { 173, 148, 140, 0 };
static const uint8_t vp8_dct_cat4_prob[] = { 176, 155, 140, 135, 0 };
static const uint8_t vp8_dct_cat5_prob[] = { 180, 157, 141, 134, 130, 0 };
static const uint8_t vp8_dct_cat6_prob[] = { 254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129, 0 };

// only used for cat3 and above; cat 1 and 2 are referenced directly
const uint8_t * const ff_vp8_dct_cat_prob[] =
{
    vp8_dct_cat3_prob,
    vp8_dct_cat4_prob,
    vp8_dct_cat5_prob,
    vp8_dct_cat6_prob,
};

static const uint8_t vp8_token_default_probs[4][8][3][NUM_DCT_TOKENS-1] =
{
    {
        {
            { 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
        {
            { 253, 136, 254, 255, 228, 219, 128, 128, 128, 128, 128 },
            { 189, 129, 242, 255, 227, 213, 255, 219, 128, 128, 128 },
            { 106, 126, 227, 252, 214, 209, 255, 255, 128, 128, 128 },
        },
        {
            {   1,  98, 248, 255, 236, 226, 255, 255, 128, 128, 128 },
            { 181, 133, 238, 254, 221, 234, 255, 154, 128, 128, 128 },
            {  78, 134, 202, 247, 198, 180, 255, 219, 128, 128, 128 },
        },
        {
            {   1, 185, 249, 255, 243, 255, 128, 128, 128, 128, 128 },
            { 184, 150, 247, 255, 236, 224, 128, 128, 128, 128, 128 },
            {  77, 110, 216, 255, 236, 230, 128, 128, 128, 128, 128 },
        },
        {
            {   1, 101, 251, 255, 241, 255, 128, 128, 128, 128, 128 },
            { 170, 139, 241, 252, 236, 209, 255, 255, 128, 128, 128 },
            {  37, 116, 196, 243, 228, 255, 255, 255, 128, 128, 128 },
        },
        {
            {   1, 204, 254, 255, 245, 255, 128, 128, 128, 128, 128 },
            { 207, 160, 250, 255, 238, 128, 128, 128, 128, 128, 128 },
            { 102, 103, 231, 255, 211, 171, 128, 128, 128, 128, 128 },
        },
        {
            {   1, 152, 252, 255, 240, 255, 128, 128, 128, 128, 128 },
            { 177, 135, 243, 255, 234, 225, 128, 128, 128, 128, 128 },
            {  80, 129, 211, 255, 194, 224, 128, 128, 128, 128, 128 },
        },
        {
            {   1,   1, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 246,   1, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 255, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
    },
    {
        {
            { 198,  35, 237, 223, 193, 187, 162, 160, 145, 155,  62 },
            { 131,  45, 198, 221, 172, 176, 220, 157, 252, 221,   1 },
            {  68,  47, 146, 208, 149, 167, 221, 162, 255, 223, 128 },
        },
        {
            {   1, 149, 241, 255, 221, 224, 255, 255, 128, 128, 128 },
            { 184, 141, 234, 253, 222, 220, 255, 199, 128, 128, 128 },
            {  81,  99, 181, 242, 176, 190, 249, 202, 255, 255, 128 },
        },
        {
            {   1, 129, 232, 253, 214, 197, 242, 196, 255, 255, 128 },
            {  99, 121, 210, 250, 201, 198, 255, 202, 128, 128, 128 },
            {  23,  91, 163, 242, 170, 187, 247, 210, 255, 255, 128 },
        },
        {
            {   1, 200, 246, 255, 234, 255, 128, 128, 128, 128, 128 },
            { 109, 178, 241, 255, 231, 245, 255, 255, 128, 128, 128 },
            {  44, 130, 201, 253, 205, 192, 255, 255, 128, 128, 128 },
        },
        {
            {   1, 132, 239, 251, 219, 209, 255, 165, 128, 128, 128 },
            {  94, 136, 225, 251, 218, 190, 255, 255, 128, 128, 128 },
            {  22, 100, 174, 245, 186, 161, 255, 199, 128, 128, 128 },
        },
        {
            {   1, 182, 249, 255, 232, 235, 128, 128, 128, 128, 128 },
            { 124, 143, 241, 255, 227, 234, 128, 128, 128, 128, 128 },
            {  35,  77, 181, 251, 193, 211, 255, 205, 128, 128, 128 },
        },
        {
            {   1, 157, 247, 255, 236, 231, 255, 255, 128, 128, 128 },
            { 121, 141, 235, 255, 225, 227, 255, 255, 128, 128, 128 },
            {  45,  99, 188, 251, 195, 217, 255, 224, 128, 128, 128 },
        },
        {
            {   1,   1, 251, 255, 213, 255, 128, 128, 128, 128, 128 },
            { 203,   1, 248, 255, 255, 128, 128, 128, 128, 128, 128 },
            { 137,   1, 177, 255, 224, 255, 128, 128, 128, 128, 128 },
        },
    },
    {
        {
            { 253,   9, 248, 251, 207, 208, 255, 192, 128, 128, 128 },
            { 175,  13, 224, 243, 193, 185, 249, 198, 255, 255, 128 },
            {  73,  17, 171, 221, 161, 179, 236, 167, 255, 234, 128 },
        },
        {
            {   1,  95, 247, 253, 212, 183, 255, 255, 128, 128, 128 },
            { 239,  90, 244, 250, 211, 209, 255, 255, 128, 128, 128 },
            { 155,  77, 195, 248, 188, 195, 255, 255, 128, 128, 128 },
        },
        {
            {   1,  24, 239, 251, 218, 219, 255, 205, 128, 128, 128 },
            { 201,  51, 219, 255, 196, 186, 128, 128, 128, 128, 128 },
            {  69,  46, 190, 239, 201, 218, 255, 228, 128, 128, 128 },
        },
        {
            {   1, 191, 251, 255, 255, 128, 128, 128, 128, 128, 128 },
            { 223, 165, 249, 255, 213, 255, 128, 128, 128, 128, 128 },
            { 141, 124, 248, 255, 255, 128, 128, 128, 128, 128, 128 },
        },
        {
            {   1,  16, 248, 255, 255, 128, 128, 128, 128, 128, 128 },
            { 190,  36, 230, 255, 236, 255, 128, 128, 128, 128, 128 },
            { 149,   1, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
        {
            {   1, 226, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 247, 192, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 240, 128, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
        {
            {   1, 134, 252, 255, 255, 128, 128, 128, 128, 128, 128 },
            { 213,  62, 250, 255, 255, 128, 128, 128, 128, 128, 128 },
            {  55,  93, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
        {
            { 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
    },
    {
        {
            { 202,  24, 213, 235, 186, 191, 220, 160, 240, 175, 255 },
            { 126,  38, 182, 232, 169, 184, 228, 174, 255, 187, 128 },
            {  61,  46, 138, 219, 151, 178, 240, 170, 255, 216, 128 },
        },
        {
            {   1, 112, 230, 250, 199, 191, 247, 159, 255, 255, 128 },
            { 166, 109, 228, 252, 211, 215, 255, 174, 128, 128, 128 },
            {  39,  77, 162, 232, 172, 180, 245, 178, 255, 255, 128 },
        },
        {
            {   1,  52, 220, 246, 198, 199, 249, 220, 255, 255, 128 },
            { 124,  74, 191, 243, 183, 193, 250, 221, 255, 255, 128 },
            {  24,  71, 130, 219, 154, 170, 243, 182, 255, 255, 128 },
        },
        {
            {   1, 182, 225, 249, 219, 240, 255, 224, 128, 128, 128 },
            { 149, 150, 226, 252, 216, 205, 255, 171, 128, 128, 128 },
            {  28, 108, 170, 242, 183, 194, 254, 223, 255, 255, 128 },
        },
        {
            {   1,  81, 230, 252, 204, 203, 255, 192, 128, 128, 128 },
            { 123, 102, 209, 247, 188, 196, 255, 233, 128, 128, 128 },
            {  20,  95, 153, 243, 164, 173, 255, 203, 128, 128, 128 },
        },
        {
            {   1, 222, 248, 255, 216, 213, 128, 128, 128, 128, 128 },
            { 168, 175, 246, 252, 235, 205, 255, 255, 128, 128, 128 },
            {  47, 116, 215, 255, 211, 212, 255, 255, 128, 128, 128 },
        },
        {
            {   1, 121, 236, 253, 212, 214, 255, 255, 128, 128, 128 },
            { 141,  84, 213, 252, 201, 202, 255, 219, 128, 128, 128 },
            {  42,  80, 160, 240, 162, 185, 255, 205, 128, 128, 128 },
        },
        {
            {   1,   1, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 244,   1, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
            { 238,   1, 255, 128, 128, 128, 128, 128, 128, 128, 128 },
        },
    },
};

static const uint8_t vp8_token_update_probs[4][8][3][NUM_DCT_TOKENS-1] =
{
    {
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 176, 246, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 223, 241, 252, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 249, 253, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 244, 252, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 234, 254, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 246, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 239, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 248, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 251, 255, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 251, 254, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 254, 253, 255, 254, 255, 255, 255, 255, 255, 255 },
            { 250, 255, 254, 255, 254, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
    },
    {
        {
            { 217, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 225, 252, 241, 253, 255, 255, 254, 255, 255, 255, 255 },
            { 234, 250, 241, 250, 253, 255, 253, 254, 255, 255, 255 },
        },
        {
            { 255, 254, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 223, 254, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 238, 253, 254, 254, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 248, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 249, 254, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 247, 254, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 252, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 254, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 254, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 250, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
    },
    {
        {
            { 186, 251, 250, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 234, 251, 244, 254, 255, 255, 255, 255, 255, 255, 255 },
            { 251, 251, 243, 253, 254, 255, 254, 255, 255, 255, 255 },
        },
        {
            { 255, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 236, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 251, 253, 253, 254, 254, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 254, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 254, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 254, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 254, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
    },
    {
        {
            { 248, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 250, 254, 252, 254, 255, 255, 255, 255, 255, 255, 255 },
            { 248, 254, 249, 253, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 253, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 246, 253, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 252, 254, 251, 254, 254, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 254, 252, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 248, 254, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 253, 255, 254, 254, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 251, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 245, 251, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 253, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 251, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 252, 253, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 254, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 252, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 249, 255, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 254, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 250, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
        {
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 254, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
            { 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255 },
        },
    },
};

static const uint8_t zigzag_scan[16]={
    0+0*4, 1+0*4, 0+1*4, 0+2*4,
    1+1*4, 2+0*4, 3+0*4, 2+1*4,
    1+2*4, 0+3*4, 1+3*4, 2+2*4,
    3+1*4, 3+2*4, 2+3*4, 3+3*4,
};

static const uint8_t vp8_dc_qlookup[VP8_MAX_QUANT+1] =
{
      4,   5,   6,   7,   8,   9,  10,  10,  11,  12,  13,  14,  15,  16,  17,  17,
     18,  19,  20,  20,  21,  21,  22,  22,  23,  23,  24,  25,  25,  26,  27,  28,
     29,  30,  31,  32,  33,  34,  35,  36,  37,  37,  38,  39,  40,  41,  42,  43,
     44,  45,  46,  46,  47,  48,  49,  50,  51,  52,  53,  54,  55,  56,  57,  58,
     59,  60,  61,  62,  63,  64,  65,  66,  67,  68,  69,  70,  71,  72,  73,  74,
     75,  76,  76,  77,  78,  79,  80,  81,  82,  83,  84,  85,  86,  87,  88,  89,
     91,  93,  95,  96,  98, 100, 101, 102, 104, 106, 108, 110, 112, 114, 116, 118,
    122, 124, 126, 128, 130, 132, 134, 136, 138, 140, 143, 145, 148, 151, 154, 157,
};

static const uint16_t vp8_ac_qlookup[VP8_MAX_QUANT+1] =
{
      4,   5,   6,   7,   8,   9,  10,  11,  12,  13,  14,  15,  16,  17,  18,  19,
     20,  21,  22,  23,  24,  25,  26,  27,  28,  29,  30,  31,  32,  33,  34,  35,
     36,  37,  38,  39,  40,  41,  42,  43,  44,  45,  46,  47,  48,  49,  50,  51,
     52,  53,  54,  55,  56,  57,  58,  60,  62,  64,  66,  68,  70,  72,  74,  76,
     78,  80,  82,  84,  86,  88,  90,  92,  94,  96,  98, 100, 102, 104, 106, 108,
    110, 112, 114, 116, 119, 122, 125, 128, 131, 134, 137, 140, 143, 146, 149, 152,
    155, 158, 161, 164, 167, 170, 173, 177, 181, 185, 189, 193, 197, 201, 205, 209,
    213, 217, 221, 225, 229, 234, 239, 245, 249, 254, 259, 264, 269, 274, 279, 284,
};

static const uint8_t vp8_mv_update_prob[2][19] = {
    { 237,
      246,
      253, 253, 254, 254, 254, 254, 254,
      254, 254, 254, 254, 254, 250, 250, 252, 254, 254 },
    { 231,
      243,
      245, 253, 254, 254, 254, 254, 254,
      254, 254, 254, 254, 254, 251, 251, 254, 254, 254 }
};

static const uint8_t vp8_mv_default_prob[2][19] = {
    { 162,
      128,
      225, 146, 172, 147, 214, 39, 156,
      128, 129, 132,  75, 145, 178, 206, 239, 254, 254 },
    { 164,
      128,
      204, 170, 119, 235, 140, 230, 228,
      128, 130, 130,  74, 148, 180, 203, 236, 254, 254 }
};



static void free_buffers(VP8Context *s)
{
    wpd_freep(&s->macroblocks_base);
    wpd_freep(&s->filter_strength);
    wpd_freep(&s->intra4x4_pred_mode_top);
    wpd_freep(&s->top_nnz);
    wpd_freep(&s->edge_emu_buffer);
    wpd_freep(&s->top_border);

    s->macroblocks = NULL;
}

static void wpd_release_picture(WpdFrame *frame)
{
    for (int p = 0; p < 3; p++) {
        wpd_free(frame->allocation[p]);
        frame->allocation[p] = frame->data[p] = NULL;
        frame->linesize[p] = 0;
    }
}

static int wpd_alloc_picture(WpdCodecContext *context, WpdFrame *frame)
{
    const int widths[3] = {
        context->width, (context->width + 1) / 2, (context->width + 1) / 2
    };
    const int heights[3] = {
        context->height, (context->height + 1) / 2, (context->height + 1) / 2
    };

    for (int p = 0; p < 3; p++) {
        int stride = (widths[p] + 63) & ~31;
        size_t size = (size_t)(heights[p] + 64) * stride;
        frame->allocation[p] = wpd_mallocz(size);
        if (!frame->allocation[p]) {
            wpd_release_picture(frame);
            return WPD_ERROR(ENOMEM);
        }
        frame->linesize[p] = stride;
        frame->data[p] = frame->allocation[p] + 32 * stride + 32;
    }
    return 0;
}

static int vp8_alloc_frame(VP8Context *s, WpdFrame *f)
{
    int ret;
    if ((ret = wpd_alloc_picture(s->avctx, f)) < 0)
        return ret;
    if (s->num_maps_to_be_freed && !s->maps_are_invalid) {
        f->ref_index[0] = s->segmentation_maps[--s->num_maps_to_be_freed];
    } else if (!(f->ref_index[0] = wpd_mallocz(s->mb_width * s->mb_height))) {
        wpd_release_picture(f);
        return WPD_ERROR(ENOMEM);
    }
    return 0;
}

static void vp8_release_frame(VP8Context *s, WpdFrame *f, int prefer_delayed_free, int can_direct_free)
{
    if (f->ref_index[0]) {
        if (prefer_delayed_free) {
            /* Upon a size change, we want to free the maps but other threads may still
             * be using them, so queue them. Upon a seek, all threads are inactive so
             * we want to cache one to prevent re-allocation in the next decoding
             * iteration, but the rest we can free directly. */
            int max_queued_maps = can_direct_free ? 1 : WPD_ARRAY_ELEMS(s->segmentation_maps);
            if (s->num_maps_to_be_freed < max_queued_maps) {
                s->segmentation_maps[s->num_maps_to_be_freed++] = f->ref_index[0];
            } else if (can_direct_free) /* vp8_decode_flush(), but our queue is full */ {
                wpd_free(f->ref_index[0]);
            } /* else: MEMLEAK (should never happen, but better that than crash) */
            f->ref_index[0] = NULL;
        } else /* vp8_decode_free() */ {
            wpd_free(f->ref_index[0]);
        }
    }
    wpd_release_picture(f);
}

static void vp8_decode_flush_impl(WpdCodecContext *avctx,
                                  int prefer_delayed_free, int can_direct_free, int free_mem)
{
    VP8Context *s = avctx->priv_data;
    int i;

    if (!avctx->is_copy) {
        for (i = 0; i < 5; i++)
            if (s->frames[i].data[0])
                vp8_release_frame(s, &s->frames[i], prefer_delayed_free, can_direct_free);
    }
    memset(s->framep, 0, sizeof(s->framep));

    if (free_mem) {
        free_buffers(s);
        s->maps_are_invalid = 1;
    }
}

static int update_dimensions(VP8Context *s, int width, int height)
{
    if (width  != s->avctx->width || ((width+15)/16 != s->mb_width || (height+15)/16 != s->mb_height) && s->macroblocks_base ||
        height != s->avctx->height) {
        if (wpd_check_image_size(width, height, 0, s->avctx))
            return WPD_ERROR_INVALID_DATA;

        vp8_decode_flush_impl(s->avctx, 1, 0, 1);

        wpd_set_dimensions(s->avctx, width, height);
    }

    s->mb_width  = (s->avctx->coded_width +15) / 16;
    s->mb_height = (s->avctx->coded_height+15) / 16;

    s->macroblocks_base        = wpd_mallocz((s->mb_width+s->mb_height*2+1)*sizeof(*s->macroblocks));
    s->filter_strength         = wpd_mallocz(s->mb_width*sizeof(*s->filter_strength));
    s->intra4x4_pred_mode_top  = wpd_mallocz(s->mb_width*4);
    s->top_nnz                 = wpd_mallocz(s->mb_width*sizeof(*s->top_nnz));
    s->top_border              = wpd_mallocz((s->mb_width+1)*sizeof(*s->top_border));

    if (!s->macroblocks_base || !s->filter_strength || !s->intra4x4_pred_mode_top ||
        !s->top_nnz || !s->top_border)
        return WPD_ERROR(ENOMEM);

    s->macroblocks        = s->macroblocks_base + 1;

    return 0;
}

static void parse_segment_info(VP8Context *s)
{
    VP56RangeCoder *c = &s->c;
    int i;

    s->segmentation.update_map = vp8_rac_get(c);

    if (vp8_rac_get(c)) { // update segment feature data
        s->segmentation.absolute_vals = vp8_rac_get(c);

        for (i = 0; i < 4; i++)
            s->segmentation.base_quant[i]   = vp8_rac_get_sint(c, 7);

        for (i = 0; i < 4; i++)
            s->segmentation.filter_level[i] = vp8_rac_get_sint(c, 6);
    }
    if (s->segmentation.update_map)
        for (i = 0; i < 3; i++)
            s->prob->segmentid[i] = vp8_rac_get(c) ? vp8_rac_get_uint(c, 8) : 255;
}

static void update_lf_deltas(VP8Context *s)
{
    VP56RangeCoder *c = &s->c;
    int i;

    for (i = 0; i < 4; i++) {
        if (vp8_rac_get(c)) {
            s->lf_delta.ref[i] = vp8_rac_get_uint(c, 6);

            if (vp8_rac_get(c))
                s->lf_delta.ref[i] = -s->lf_delta.ref[i];
        }
    }

    for (i = MODE_I4x4; i <= VP8_MVMODE_SPLIT; i++) {
        if (vp8_rac_get(c)) {
            s->lf_delta.mode[i] = vp8_rac_get_uint(c, 6);

            if (vp8_rac_get(c))
                s->lf_delta.mode[i] = -s->lf_delta.mode[i];
        }
    }
}

static int setup_partitions(VP8Context *s, const uint8_t *buf, int buf_size)
{
    const uint8_t *sizes = buf;
    int i;

    s->num_coeff_partitions = 1 << vp8_rac_get_uint(&s->c, 2);

    buf      += 3*(s->num_coeff_partitions-1);
    buf_size -= 3*(s->num_coeff_partitions-1);
    if (buf_size < 0)
        return -1;

    for (i = 0; i < s->num_coeff_partitions-1; i++) {
        int size = WPD_RL24(sizes + 3*i);
        if (buf_size - size < 0)
            return -1;

        wpd_vp56_init_range_decoder(&s->coeff_partition[i], buf, size);
        buf      += size;
        buf_size -= size;
    }
    wpd_vp56_init_range_decoder(&s->coeff_partition[i], buf, buf_size);

    return 0;
}

static void get_quants(VP8Context *s)
{
    VP56RangeCoder *c = &s->c;
    int i, base_qi;

    int yac_qi     = vp8_rac_get_uint(c, 7);
    int ydc_delta  = vp8_rac_get_sint(c, 4);
    int y2dc_delta = vp8_rac_get_sint(c, 4);
    int y2ac_delta = vp8_rac_get_sint(c, 4);
    int uvdc_delta = vp8_rac_get_sint(c, 4);
    int uvac_delta = vp8_rac_get_sint(c, 4);

    for (i = 0; i < 4; i++) {
        if (s->segmentation.enabled) {
            base_qi = s->segmentation.base_quant[i];
            if (!s->segmentation.absolute_vals)
                base_qi += yac_qi;
        } else
            base_qi = yac_qi;

        s->qmat[i].luma_qmul[0]    =       vp8_dc_qlookup[wpd_clip_uintp2(base_qi + ydc_delta , 7)];
        s->qmat[i].luma_qmul[1]    =       vp8_ac_qlookup[wpd_clip_uintp2(base_qi             , 7)];
        s->qmat[i].luma_dc_qmul[0] =   2 * vp8_dc_qlookup[wpd_clip_uintp2(base_qi + y2dc_delta, 7)];
        s->qmat[i].luma_dc_qmul[1] = vp8_ac_qlookup[wpd_clip_uintp2(base_qi + y2ac_delta, 7)] * 101581 >> 16;
        s->qmat[i].chroma_qmul[0]  =       vp8_dc_qlookup[wpd_clip_uintp2(base_qi + uvdc_delta, 7)];
        s->qmat[i].chroma_qmul[1]  =       vp8_ac_qlookup[wpd_clip_uintp2(base_qi + uvac_delta, 7)];

        s->qmat[i].luma_dc_qmul[1] = WPD_MAX(s->qmat[i].luma_dc_qmul[1], 8);
        s->qmat[i].chroma_qmul[0]  = WPD_MIN(s->qmat[i].chroma_qmul[0], 132);
    }
}

/**
 * Determine which buffers golden and altref should be updated with after this frame.
 * The spec isn't clear here, so I'm going by my understanding of what libvpx does
 *
 * Intra frames update all 3 references
 * Inter frames update VP56_FRAME_PREVIOUS if the update_last flag is set
 * If the update (golden|altref) flag is set, it's updated with the current frame
 *      if update_last is set, and VP56_FRAME_PREVIOUS otherwise.
 * If the flag is not set, the number read means:
 *      0: no update
 *      1: VP56_FRAME_PREVIOUS
 *      2: update golden with altref, or update altref with golden
 */
static VP56Frame ref_to_update(VP8Context *s, int update, VP56Frame ref)
{
    VP56RangeCoder *c = &s->c;

    if (update)
        return VP56_FRAME_CURRENT;

    switch (vp8_rac_get_uint(c, 2)) {
    case 1:
        return VP56_FRAME_PREVIOUS;
    case 2:
        return (ref == VP56_FRAME_GOLDEN) ? VP56_FRAME_GOLDEN2 : VP56_FRAME_GOLDEN;
    }
    return VP56_FRAME_NONE;
}

static void update_refs(VP8Context *s)
{
    VP56RangeCoder *c = &s->c;

    int update_golden = vp8_rac_get(c);
    int update_altref = vp8_rac_get(c);

    s->update_golden = ref_to_update(s, update_golden, VP56_FRAME_GOLDEN);
    s->update_altref = ref_to_update(s, update_altref, VP56_FRAME_GOLDEN2);
}

static int decode_frame_header(VP8Context *s, const uint8_t *buf, int buf_size)
{
    VP56RangeCoder *c = &s->c;
    int header_size, hscale, vscale, i, j, k, l, m, ret;
    int width  = s->avctx->width;
    int height = s->avctx->height;

    s->keyframe  = !(buf[0] & 1);
    s->profile   =  (buf[0]>>1) & 7;
    s->invisible = !(buf[0] & 0x10);
    header_size  = WPD_RL24(buf) >> 5;
    buf      += 3;
    buf_size -= 3;

    if (s->profile > 3)
        wpd_log(s->avctx, WPD_LOG_WARNING, "Unknown profile %d\n", s->profile);

    if (!s->profile)
        memcpy(s->put_pixels_tab, s->vp8dsp.put_vp8_epel_pixels_tab, sizeof(s->put_pixels_tab));
    else    // profile 1-3 use bilinear, 4+ aren't defined so whatever
        memcpy(s->put_pixels_tab, s->vp8dsp.put_vp8_bilinear_pixels_tab, sizeof(s->put_pixels_tab));

    if (header_size > buf_size - 7*s->keyframe) {
        wpd_log(s->avctx, WPD_LOG_ERROR, "Header size larger than data provided\n");
        return WPD_ERROR_INVALID_DATA;
    }

    if (s->keyframe) {
        if (WPD_RL24(buf) != 0x2a019d) {
            wpd_log(s->avctx, WPD_LOG_ERROR, "Invalid start code 0x%x\n", WPD_RL24(buf));
            return WPD_ERROR_INVALID_DATA;
        }
        width  = WPD_RL16(buf+3) & 0x3fff;
        height = WPD_RL16(buf+5) & 0x3fff;
        hscale = buf[4] >> 6;
        vscale = buf[6] >> 6;
        buf      += 7;
        buf_size -= 7;

        if (hscale || vscale)
            wpd_log_missing_feature(s->avctx, "Upscaling", 1);

        s->update_golden = s->update_altref = VP56_FRAME_CURRENT;
        for (i = 0; i < 4; i++)
            for (j = 0; j < 16; j++)
                memcpy(s->prob->token[i][j], vp8_token_default_probs[i][vp8_coeff_band[j]],
                       sizeof(s->prob->token[i][j]));
        memcpy(s->prob->pred16x16, vp8_pred16x16_prob_inter, sizeof(s->prob->pred16x16));
        memcpy(s->prob->pred8x8c , vp8_pred8x8c_prob_inter , sizeof(s->prob->pred8x8c));
        memcpy(s->prob->mvc      , vp8_mv_default_prob     , sizeof(s->prob->mvc));
        memset(&s->segmentation, 0, sizeof(s->segmentation));
    }

    if (!s->macroblocks_base || /* first frame */
        width != s->avctx->width || height != s->avctx->height || (width+15)/16 != s->mb_width || (height+15)/16 != s->mb_height) {
        if ((ret = update_dimensions(s, width, height)) < 0)
            return ret;
    }

    wpd_vp56_init_range_decoder(c, buf, header_size);
    buf      += header_size;
    buf_size -= header_size;

    if (s->keyframe) {
        if (vp8_rac_get(c))
            wpd_log(s->avctx, WPD_LOG_WARNING, "Unspecified colorspace\n");
        vp8_rac_get(c); // whether we can skip clamping in dsp functions
    }

    if ((s->segmentation.enabled = vp8_rac_get(c)))
        parse_segment_info(s);
    else
        s->segmentation.update_map = 0; // FIXME: move this to some init function?

    s->filter.simple    = vp8_rac_get(c);
    s->filter.level     = vp8_rac_get_uint(c, 6);
    s->filter.sharpness = vp8_rac_get_uint(c, 3);

    if ((s->lf_delta.enabled = vp8_rac_get(c)))
        if (vp8_rac_get(c))
            update_lf_deltas(s);

    if (setup_partitions(s, buf, buf_size)) {
        wpd_log(s->avctx, WPD_LOG_ERROR, "Invalid partitions\n");
        return WPD_ERROR_INVALID_DATA;
    }

    get_quants(s);

    if (!s->keyframe) {
        update_refs(s);
        s->sign_bias[VP56_FRAME_GOLDEN]               = vp8_rac_get(c);
        s->sign_bias[VP56_FRAME_GOLDEN2 /* altref */] = vp8_rac_get(c);
    }

    // if we aren't saving this frame's probabilities for future frames,
    // make a copy of the current probabilities
    if (!(s->update_probabilities = vp8_rac_get(c)))
        s->prob[1] = s->prob[0];

    s->update_last = s->keyframe || vp8_rac_get(c);

    for (i = 0; i < 4; i++)
        for (j = 0; j < 8; j++)
            for (k = 0; k < 3; k++)
                for (l = 0; l < NUM_DCT_TOKENS-1; l++)
                    if (vp56_rac_get_prob_branchy(c, vp8_token_update_probs[i][j][k][l])) {
                        int prob = vp8_rac_get_uint(c, 8);
                        for (m = 0; vp8_coeff_band_indexes[j][m] >= 0; m++)
                            s->prob->token[i][vp8_coeff_band_indexes[j][m]][k][l] = prob;
                    }

    if ((s->mbskip_enabled = vp8_rac_get(c)))
        s->prob->mbskip = vp8_rac_get_uint(c, 8);

    if (!s->keyframe) {
        s->prob->intra  = vp8_rac_get_uint(c, 8);
        s->prob->last   = vp8_rac_get_uint(c, 8);
        s->prob->golden = vp8_rac_get_uint(c, 8);

        if (vp8_rac_get(c))
            for (i = 0; i < 4; i++)
                s->prob->pred16x16[i] = vp8_rac_get_uint(c, 8);
        if (vp8_rac_get(c))
            for (i = 0; i < 3; i++)
                s->prob->pred8x8c[i]  = vp8_rac_get_uint(c, 8);

        // 17.2 MV probability update
        for (i = 0; i < 2; i++)
            for (j = 0; j < 19; j++)
                if (vp56_rac_get_prob_branchy(c, vp8_mv_update_prob[i][j]))
                    s->prob->mvc[i][j] = vp8_rac_get_nn(c);
    }

    return 0;
}

static wpd_always_inline void clamp_mv(VP8Context *s, VP56mv *dst, const VP56mv *src)
{
    dst->x = wpd_clip(src->x, s->mv_min.x, s->mv_max.x);
    dst->y = wpd_clip(src->y, s->mv_min.y, s->mv_max.y);
}

/**
 * Motion vector coding, 17.1.
 */
static int read_mv_component(VP56RangeCoder *c, const uint8_t *p)
{
    int bit, x = 0;

    if (vp56_rac_get_prob_branchy(c, p[0])) {
        int i;

        for (i = 0; i < 3; i++)
            x += vp56_rac_get_prob(c, p[9 + i]) << i;
        for (i = 9; i > 3; i--)
            x += vp56_rac_get_prob(c, p[9 + i]) << i;
        if (!(x & 0xFFF0) || vp56_rac_get_prob(c, p[12]))
            x += 8;
    } else {
        // small_mvtree
        const uint8_t *ps = p+2;
        bit = vp56_rac_get_prob(c, *ps);
        ps += 1 + 3*bit;
        x  += 4*bit;
        bit = vp56_rac_get_prob(c, *ps);
        ps += 1 + bit;
        x  += 2*bit;
        x  += vp56_rac_get_prob(c, *ps);
    }

    return (x && vp56_rac_get_prob(c, p[1])) ? -x : x;
}

static wpd_always_inline
const uint8_t *get_submv_prob(uint32_t left, uint32_t top)
{
    if (left == top)
        return vp8_submv_prob[4-!!left];
    if (!top)
        return vp8_submv_prob[2];
    return vp8_submv_prob[1-!!left];
}

/**
 * Split motion vector prediction, 16.4.
 * @returns the number of motion vectors parsed (2, 4 or 16)
 */
static wpd_always_inline
int decode_splitmvs(VP8Context *s, VP56RangeCoder *c, VP8Macroblock *mb)
{
    int part_idx;
    int n, num;
    VP8Macroblock *top_mb  = &mb[2];
    VP8Macroblock *left_mb = &mb[-1];
    const uint8_t *mbsplits_left = vp8_mbsplits[left_mb->partitioning],
                  *mbsplits_top = vp8_mbsplits[top_mb->partitioning],
                  *mbsplits_cur, *firstidx;
    VP56mv *top_mv  = top_mb->bmv;
    VP56mv *left_mv = left_mb->bmv;
    VP56mv *cur_mv  = mb->bmv;

    if (vp56_rac_get_prob_branchy(c, vp8_mbsplit_prob[0])) {
        if (vp56_rac_get_prob_branchy(c, vp8_mbsplit_prob[1])) {
            part_idx = VP8_SPLITMVMODE_16x8 + vp56_rac_get_prob(c, vp8_mbsplit_prob[2]);
        } else {
            part_idx = VP8_SPLITMVMODE_8x8;
        }
    } else {
        part_idx = VP8_SPLITMVMODE_4x4;
    }

    num = vp8_mbsplit_count[part_idx];
    mbsplits_cur = vp8_mbsplits[part_idx],
    firstidx = vp8_mbfirstidx[part_idx];
    mb->partitioning = part_idx;

    for (n = 0; n < num; n++) {
        int k = firstidx[n];
        uint32_t left, above;
        const uint8_t *submv_prob;

        if (!(k & 3))
            left = WPD_RN32A(&left_mv[mbsplits_left[k + 3]]);
        else
            left  = WPD_RN32A(&cur_mv[mbsplits_cur[k - 1]]);
        if (k <= 3)
            above = WPD_RN32A(&top_mv[mbsplits_top[k + 12]]);
        else
            above = WPD_RN32A(&cur_mv[mbsplits_cur[k - 4]]);

        submv_prob = get_submv_prob(left, above);

        if (vp56_rac_get_prob_branchy(c, submv_prob[0])) {
            if (vp56_rac_get_prob_branchy(c, submv_prob[1])) {
                if (vp56_rac_get_prob_branchy(c, submv_prob[2])) {
                    mb->bmv[n].y = mb->mv.y + read_mv_component(c, s->prob->mvc[0]);
                    mb->bmv[n].x = mb->mv.x + read_mv_component(c, s->prob->mvc[1]);
                } else {
                    WPD_ZERO32(&mb->bmv[n]);
                }
            } else {
                WPD_WN32A(&mb->bmv[n], above);
            }
        } else {
            WPD_WN32A(&mb->bmv[n], left);
        }
    }

    return num;
}

static wpd_always_inline
void decode_mvs(VP8Context *s, VP8Macroblock *mb, int mb_x, int mb_y)
{
    VP8Macroblock *mb_edge[3] = { mb + 2 /* top */,
                                  mb - 1 /* left */,
                                  mb + 1 /* top-left */ };
    enum { CNT_ZERO, CNT_NEAREST, CNT_NEAR, CNT_SPLITMV };
    enum { VP8_EDGE_TOP, VP8_EDGE_LEFT, VP8_EDGE_TOPLEFT };
    int idx = CNT_ZERO;
    int cur_sign_bias = s->sign_bias[mb->ref_frame];
    int8_t *sign_bias = s->sign_bias;
    VP56mv near_mv[4];
    uint8_t cnt[4] = { 0 };
    VP56RangeCoder *c = &s->c;

    WPD_ZERO32(&near_mv[0]);
    WPD_ZERO32(&near_mv[1]);
    WPD_ZERO32(&near_mv[2]);

    /* Process MB on top, left and top-left */
    #define MV_EDGE_CHECK(n)\
    {\
        VP8Macroblock *edge = mb_edge[n];\
        int edge_ref = edge->ref_frame;\
        if (edge_ref != VP56_FRAME_CURRENT) {\
            uint32_t mv = WPD_RN32A(&edge->mv);\
            if (mv) {\
                if (cur_sign_bias != sign_bias[edge_ref]) {\
                    /* SWAR negate of the values in mv. */\
                    mv = ~mv;\
                    mv = ((mv&0x7fff7fff) + 0x00010001) ^ (mv&0x80008000);\
                }\
                if (!n || mv != WPD_RN32A(&near_mv[idx]))\
                    WPD_WN32A(&near_mv[++idx], mv);\
                cnt[idx]      += 1 + (n != 2);\
            } else\
                cnt[CNT_ZERO] += 1 + (n != 2);\
        }\
    }

    MV_EDGE_CHECK(0)
    MV_EDGE_CHECK(1)
    MV_EDGE_CHECK(2)

    mb->partitioning = VP8_SPLITMVMODE_NONE;
    if (vp56_rac_get_prob_branchy(c, vp8_mode_contexts[cnt[CNT_ZERO]][0])) {
        mb->mode = VP8_MVMODE_MV;

        /* If we have three distinct MVs, merge first and last if they're the same */
        if (cnt[CNT_SPLITMV] && WPD_RN32A(&near_mv[1 + VP8_EDGE_TOP]) == WPD_RN32A(&near_mv[1 + VP8_EDGE_TOPLEFT]))
            cnt[CNT_NEAREST] += 1;

        /* Swap near and nearest if necessary */
        if (cnt[CNT_NEAR] > cnt[CNT_NEAREST]) {
            WPD_SWAP(uint8_t,     cnt[CNT_NEAREST],     cnt[CNT_NEAR]);
            WPD_SWAP( VP56mv, near_mv[CNT_NEAREST], near_mv[CNT_NEAR]);
        }

        if (vp56_rac_get_prob_branchy(c, vp8_mode_contexts[cnt[CNT_NEAREST]][1])) {
            if (vp56_rac_get_prob_branchy(c, vp8_mode_contexts[cnt[CNT_NEAR]][2])) {

                /* Choose the best mv out of 0,0 and the nearest mv */
                clamp_mv(s, &mb->mv, &near_mv[CNT_ZERO + (cnt[CNT_NEAREST] >= cnt[CNT_ZERO])]);
                cnt[CNT_SPLITMV] = ((mb_edge[VP8_EDGE_LEFT]->mode    == VP8_MVMODE_SPLIT) +
                                    (mb_edge[VP8_EDGE_TOP]->mode     == VP8_MVMODE_SPLIT)) * 2 +
                                    (mb_edge[VP8_EDGE_TOPLEFT]->mode == VP8_MVMODE_SPLIT);

                if (vp56_rac_get_prob_branchy(c, vp8_mode_contexts[cnt[CNT_SPLITMV]][3])) {
                    mb->mode = VP8_MVMODE_SPLIT;
                    mb->mv = mb->bmv[decode_splitmvs(s, c, mb) - 1];
                } else {
                    mb->mv.y += read_mv_component(c, s->prob->mvc[0]);
                    mb->mv.x += read_mv_component(c, s->prob->mvc[1]);
                    mb->bmv[0] = mb->mv;
                }
            } else {
                clamp_mv(s, &mb->mv, &near_mv[CNT_NEAR]);
                mb->bmv[0] = mb->mv;
            }
        } else {
            clamp_mv(s, &mb->mv, &near_mv[CNT_NEAREST]);
            mb->bmv[0] = mb->mv;
        }
    } else {
        mb->mode = VP8_MVMODE_ZERO;
        WPD_ZERO32(&mb->mv);
        mb->bmv[0] = mb->mv;
    }
}

static wpd_always_inline
void decode_intra4x4_modes(VP8Context *s, VP56RangeCoder *c,
                           int mb_x, int keyframe)
{
    uint8_t *intra4x4 = s->intra4x4_pred_mode_mb;
    if (keyframe) {
        int x, y;
        uint8_t* const top = s->intra4x4_pred_mode_top + 4 * mb_x;
        uint8_t* const left = s->intra4x4_pred_mode_left;
        for (y = 0; y < 4; y++) {
            for (x = 0; x < 4; x++) {
                const uint8_t *ctx;
                ctx = vp8_pred4x4_prob_intra[top[x]][left[y]];
                *intra4x4 = vp8_rac_get_tree(c, vp8_pred4x4_tree, ctx);
                left[y] = top[x] = *intra4x4;
                intra4x4++;
            }
        }
    } else {
        int i;
        for (i = 0; i < 16; i++)
            intra4x4[i] = vp8_rac_get_tree(c, vp8_pred4x4_tree, vp8_pred4x4_prob_inter);
    }
}

static wpd_always_inline
void decode_mb_mode(VP8Context *s, VP8Macroblock *mb, int mb_x, int mb_y, uint8_t *segment, uint8_t *ref)
{
    VP56RangeCoder *c = &s->c;

    if (s->segmentation.update_map) {
        int bit  = vp56_rac_get_prob(c, s->prob->segmentid[0]);
        *segment = vp56_rac_get_prob(c, s->prob->segmentid[1+bit]) + 2*bit;
    } else if (s->segmentation.enabled)
        *segment = ref ? *ref : *segment;
    s->segment = *segment;

    mb->skip = s->mbskip_enabled ? vp56_rac_get_prob(c, s->prob->mbskip) : 0;

    if (s->keyframe) {
        mb->mode = vp8_rac_get_tree(c, vp8_pred16x16_tree_intra, vp8_pred16x16_prob_intra);

        if (mb->mode == MODE_I4x4) {
            decode_intra4x4_modes(s, c, mb_x, 1);
        } else {
            const uint32_t modes = vp8_pred4x4_mode[mb->mode] * 0x01010101u;
            WPD_WN32A(s->intra4x4_pred_mode_top + 4 * mb_x, modes);
            WPD_WN32A(s->intra4x4_pred_mode_left, modes);
        }

        s->chroma_pred_mode = vp8_rac_get_tree(c, vp8_pred8x8c_tree, vp8_pred8x8c_prob_intra);
        mb->ref_frame = VP56_FRAME_CURRENT;
    } else if (vp56_rac_get_prob_branchy(c, s->prob->intra)) {
        // inter MB, 16.2
        if (vp56_rac_get_prob_branchy(c, s->prob->last))
            mb->ref_frame = vp56_rac_get_prob(c, s->prob->golden) ?
                VP56_FRAME_GOLDEN2 /* altref */ : VP56_FRAME_GOLDEN;
        else
            mb->ref_frame = VP56_FRAME_PREVIOUS;
        s->ref_count[mb->ref_frame-1]++;

        // motion vectors, 16.3
        decode_mvs(s, mb, mb_x, mb_y);
    } else {
        // intra MB, 16.1
        mb->mode = vp8_rac_get_tree(c, vp8_pred16x16_tree_inter, s->prob->pred16x16);

        if (mb->mode == MODE_I4x4)
            decode_intra4x4_modes(s, c, mb_x, 0);

        s->chroma_pred_mode = vp8_rac_get_tree(c, vp8_pred8x8c_tree, s->prob->pred8x8c);
        mb->ref_frame = VP56_FRAME_CURRENT;
        mb->partitioning = VP8_SPLITMVMODE_NONE;
        WPD_ZERO32(&mb->bmv[0]);
    }
}

#ifndef decode_block_coeffs_internal
/**
 * @param c arithmetic bitstream reader context
 * @param block destination for block coefficients
 * @param probs probabilities to use when reading trees from the bitstream
 * @param i initial coeff index, 0 unless a separate DC block is coded
 * @param qmul array holding the dc/ac dequant factor at position 0/1
 * @return 0 if no coeffs were decoded
 *         otherwise, the index of the last coeff decoded plus one
 */
static int decode_block_coeffs_internal(VP56RangeCoder *c, WpdDctElem block[16],
                                        uint8_t probs[16][3][NUM_DCT_TOKENS-1],
                                        int i, uint8_t *token_prob, int16_t qmul[2])
{
    goto skip_eob;
    do {
        int coeff;
        if (!vp56_rac_get_prob_branchy(c, token_prob[0]))   // DCT_EOB
            return i;

skip_eob:
        if (!vp56_rac_get_prob_branchy(c, token_prob[1])) { // DCT_0
            if (++i == 16)
                return i; // invalid input; blocks should end with EOB
            token_prob = probs[i][0];
            goto skip_eob;
        }

        if (!vp56_rac_get_prob_branchy(c, token_prob[2])) { // DCT_1
            coeff = 1;
            token_prob = probs[i+1][1];
        } else {
            if (!vp56_rac_get_prob_branchy(c, token_prob[3])) { // DCT 2,3,4
                coeff = vp56_rac_get_prob_branchy(c, token_prob[4]);
                if (coeff)
                    coeff += vp56_rac_get_prob(c, token_prob[5]);
                coeff += 2;
            } else {
                // DCT_CAT*
                if (!vp56_rac_get_prob_branchy(c, token_prob[6])) {
                    if (!vp56_rac_get_prob_branchy(c, token_prob[7])) { // DCT_CAT1
                        coeff  = 5 + vp56_rac_get_prob(c, vp8_dct_cat1_prob[0]);
                    } else {                                    // DCT_CAT2
                        coeff  = 7;
                        coeff += vp56_rac_get_prob(c, vp8_dct_cat2_prob[0]) << 1;
                        coeff += vp56_rac_get_prob(c, vp8_dct_cat2_prob[1]);
                    }
                } else {    // DCT_CAT3 and up
                    int a = vp56_rac_get_prob(c, token_prob[8]);
                    int b = vp56_rac_get_prob(c, token_prob[9+a]);
                    int cat = (a<<1) + b;
                    coeff  = 3 + (8<<cat);
                    coeff += vp8_rac_get_coeff(c, ff_vp8_dct_cat_prob[cat]);
                }
            }
            token_prob = probs[i+1][2];
        }
        block[zigzag_scan[i]] = (vp8_rac_get(c) ? -coeff : coeff) * qmul[!!i];
    } while (++i < 16);

    return i;
}
#endif

/**
 * @param c arithmetic bitstream reader context
 * @param block destination for block coefficients
 * @param probs probabilities to use when reading trees from the bitstream
 * @param i initial coeff index, 0 unless a separate DC block is coded
 * @param zero_nhood the initial prediction context for number of surrounding
 *                   all-zero blocks (only left/top, so 0-2)
 * @param qmul array holding the dc/ac dequant factor at position 0/1
 * @return 0 if no coeffs were decoded
 *         otherwise, the index of the last coeff decoded plus one
 */
static wpd_always_inline
int decode_block_coeffs(VP56RangeCoder *c, WpdDctElem block[16],
                        uint8_t probs[16][3][NUM_DCT_TOKENS-1],
                        int i, int zero_nhood, int16_t qmul[2])
{
    uint8_t *token_prob = probs[i][zero_nhood];
    if (!vp56_rac_get_prob_branchy(c, token_prob[0]))   // DCT_EOB
        return 0;
    return decode_block_coeffs_internal(c, block, probs, i, token_prob, qmul);
}

static wpd_always_inline
void decode_mb_coeffs(VP8Context *s, VP56RangeCoder *c, VP8Macroblock *mb,
                      uint8_t t_nnz[9], uint8_t l_nnz[9])
{
    int i, x, y, luma_start = 0, luma_ctx = 3;
    int nnz_pred, nnz, nnz_total = 0;
    int segment = s->segment;
    int block_dc = 0;

    if (mb->mode != MODE_I4x4 && mb->mode != VP8_MVMODE_SPLIT) {
        nnz_pred = t_nnz[8] + l_nnz[8];

        // decode DC values and do hadamard
        nnz = decode_block_coeffs(c, s->block_dc, s->prob->token[1], 0, nnz_pred,
                                  s->qmat[segment].luma_dc_qmul);
        l_nnz[8] = t_nnz[8] = !!nnz;
        if (nnz) {
            nnz_total += nnz;
            block_dc = 1;
            if (nnz == 1)
                s->vp8dsp.vp8_luma_dc_wht_dc(s->block, s->block_dc);
            else
                s->vp8dsp.vp8_luma_dc_wht(s->block, s->block_dc);
        }
        luma_start = 1;
        luma_ctx = 0;
    }

    // luma blocks
    for (y = 0; y < 4; y++)
        for (x = 0; x < 4; x++) {
            nnz_pred = l_nnz[y] + t_nnz[x];
            nnz = decode_block_coeffs(c, s->block[y][x], s->prob->token[luma_ctx], luma_start,
                                      nnz_pred, s->qmat[segment].luma_qmul);
            // nnz+block_dc may be one more than the actual last index, but we don't care
            s->non_zero_count_cache[y][x] = nnz + block_dc;
            t_nnz[x] = l_nnz[y] = !!nnz;
            nnz_total += nnz;
        }

    // chroma blocks
    // TODO: what to do about dimensions? 2nd dim for luma is x,
    // but for chroma it's (y<<1)|x
    for (i = 4; i < 6; i++)
        for (y = 0; y < 2; y++)
            for (x = 0; x < 2; x++) {
                nnz_pred = l_nnz[i+2*y] + t_nnz[i+2*x];
                nnz = decode_block_coeffs(c, s->block[i][(y<<1)+x], s->prob->token[2], 0,
                                          nnz_pred, s->qmat[segment].chroma_qmul);
                s->non_zero_count_cache[i][(y<<1)+x] = nnz;
                t_nnz[i+2*x] = l_nnz[i+2*y] = !!nnz;
                nnz_total += nnz;
            }

    // if there were no coded coeffs despite the macroblock not being marked skip,
    // we MUST not do the inner loop filter and should not do IDCT
    // Since skip isn't used for bitstream prediction, just manually set it.
    if (!nnz_total)
        mb->skip = 1;
}

static wpd_always_inline
void backup_mb_border(uint8_t *top_border, uint8_t *src_y, uint8_t *src_cb, uint8_t *src_cr,
                      int linesize, int uvlinesize, int simple)
{
    WPD_COPY128(top_border, src_y + 15*linesize);
    if (!simple) {
        WPD_COPY64(top_border+16, src_cb + 7*uvlinesize);
        WPD_COPY64(top_border+24, src_cr + 7*uvlinesize);
    }
}

static wpd_always_inline
void xchg_mb_border(uint8_t *top_border, uint8_t *src_y, uint8_t *src_cb, uint8_t *src_cr,
                    int linesize, int uvlinesize, int mb_x, int mb_y, int mb_width,
                    int simple, int xchg)
{
    uint8_t *top_border_m1 = top_border-32;     // for TL prediction
    src_y  -=   linesize;
    src_cb -= uvlinesize;
    src_cr -= uvlinesize;

#define XCHG(a,b,xchg) do {                     \
        if (xchg) WPD_SWAP64(b,a);               \
        else      WPD_COPY64(b,a);               \
    } while (0)

    XCHG(top_border_m1+8, src_y-8, xchg);
    XCHG(top_border,      src_y,   xchg);
    XCHG(top_border+8,    src_y+8, 1);
    if (mb_x < mb_width-1)
        XCHG(top_border+32, src_y+16, 1);

    // only copy chroma for normal loop filter
    // or to initialize the top row to 127
    if (!simple || !mb_y) {
        XCHG(top_border_m1+16, src_cb-8, xchg);
        XCHG(top_border_m1+24, src_cr-8, xchg);
        XCHG(top_border+16,    src_cb, 1);
        XCHG(top_border+24,    src_cr, 1);
    }
}

static wpd_always_inline
int check_dc_pred8x8_mode(int mode, int mb_x, int mb_y)
{
    if (!mb_x) {
        return mb_y ? TOP_DC_PRED8x8 : DC_128_PRED8x8;
    } else {
        return mb_y ? mode : LEFT_DC_PRED8x8;
    }
}

static wpd_always_inline
int check_tm_pred8x8_mode(int mode, int mb_x, int mb_y)
{
    if (!mb_x) {
        return mb_y ? VERT_PRED8x8 : DC_129_PRED8x8;
    } else {
        return mb_y ? mode : HOR_PRED8x8;
    }
}

static wpd_always_inline
int check_intra_pred8x8_mode(int mode, int mb_x, int mb_y)
{
    if (mode == DC_PRED8x8) {
        return check_dc_pred8x8_mode(mode, mb_x, mb_y);
    } else {
        return mode;
    }
}

static wpd_always_inline
int check_intra_pred8x8_mode_emuedge(int mode, int mb_x, int mb_y)
{
    switch (mode) {
    case DC_PRED8x8:
        return check_dc_pred8x8_mode(mode, mb_x, mb_y);
    case VERT_PRED8x8:
        return !mb_y ? DC_127_PRED8x8 : mode;
    case HOR_PRED8x8:
        return !mb_x ? DC_129_PRED8x8 : mode;
    case PLANE_PRED8x8 /*TM*/:
        return check_tm_pred8x8_mode(mode, mb_x, mb_y);
    }
    return mode;
}

static wpd_always_inline
int check_tm_pred4x4_mode(int mode, int mb_x, int mb_y)
{
    if (!mb_x) {
        return mb_y ? VERT_VP8_PRED : DC_129_PRED;
    } else {
        return mb_y ? mode : HOR_VP8_PRED;
    }
}

static wpd_always_inline
int check_intra_pred4x4_mode_emuedge(int mode, int mb_x, int mb_y, int *copy_buf)
{
    switch (mode) {
    case VERT_PRED:
        if (!mb_x && mb_y) {
            *copy_buf = 1;
            return mode;
        }
        /* fall-through */
    case DIAG_DOWN_LEFT_PRED:
    case VERT_LEFT_PRED:
        return !mb_y ? DC_127_PRED : mode;
    case HOR_PRED:
        if (!mb_y) {
            *copy_buf = 1;
            return mode;
        }
        /* fall-through */
    case HOR_UP_PRED:
        return !mb_x ? DC_129_PRED : mode;
    case TM_VP8_PRED:
        return check_tm_pred4x4_mode(mode, mb_x, mb_y);
    case DC_PRED: // 4x4 DC uses different edge handling from 16x16/8x8 DC
    case DIAG_DOWN_RIGHT_PRED:
    case VERT_RIGHT_PRED:
    case HOR_DOWN_PRED:
        if (!mb_y || !mb_x)
            *copy_buf = 1;
        return mode;
    }
    return mode;
}

static wpd_always_inline
void intra_predict(VP8Context *s, uint8_t *dst[3], VP8Macroblock *mb,
                   int mb_x, int mb_y)
{
    WpdCodecContext *avctx = s->avctx;
    int x, y, mode, nnz;
    uint32_t tr;

    // for the first row, we need to run xchg_mb_border to init the top edge to 127
    // otherwise, skip it if we aren't going to deblock
    if (!(avctx->flags & WPD_CODEC_FLAG_EMU_EDGE && !mb_y) && (s->deblock_filter || !mb_y))
        xchg_mb_border(s->top_border[mb_x+1], dst[0], dst[1], dst[2],
                       s->linesize, s->uvlinesize, mb_x, mb_y, s->mb_width,
                       s->filter.simple, 1);

    if (mb->mode < MODE_I4x4) {
        if (avctx->flags & WPD_CODEC_FLAG_EMU_EDGE) { // tested
            mode = check_intra_pred8x8_mode_emuedge(mb->mode, mb_x, mb_y);
        } else {
            mode = check_intra_pred8x8_mode(mb->mode, mb_x, mb_y);
        }
    s->pred.pred16x16[mode](dst[0], s->linesize);
    } else {
        uint8_t *ptr = dst[0];
        uint8_t *intra4x4 = s->intra4x4_pred_mode_mb;
        uint8_t tr_top[4] = { 127, 127, 127, 127 };

        // all blocks on the right edge of the macroblock use bottom edge
        // the top macroblock for their topright edge
        uint8_t *tr_right = ptr - s->linesize + 16;

        // if we're on the right edge of the frame, said edge is extended
        // from the top macroblock
        if (!(!mb_y && avctx->flags & WPD_CODEC_FLAG_EMU_EDGE) &&
            mb_x == s->mb_width-1) {
            tr = tr_right[-1]*0x01010101u;
            tr_right = (uint8_t *)&tr;
        }

        if (mb->skip)
            WPD_ZERO128(s->non_zero_count_cache);

        for (y = 0; y < 4; y++) {
            uint8_t *topright = ptr + 4 - s->linesize;
            for (x = 0; x < 4; x++) {
                int copy = 0, linesize = s->linesize;
                uint8_t *dst = ptr+4*x;
                WPD_DECLARE_ALIGNED(4, uint8_t, copy_dst)[5*8];

                if ((y == 0 || x == 3) && mb_y == 0 && avctx->flags & WPD_CODEC_FLAG_EMU_EDGE) {
                    topright = tr_top;
                } else if (x == 3)
                    topright = tr_right;

                if (avctx->flags & WPD_CODEC_FLAG_EMU_EDGE) { // mb_x+x or mb_y+y is a hack but works
                    mode = check_intra_pred4x4_mode_emuedge(intra4x4[x], mb_x + x, mb_y + y, &copy);
                    if (copy) {
                        dst = copy_dst + 12;
                        linesize = 8;
                        if (!(mb_y + y)) {
                            copy_dst[3] = 127U;
                            WPD_WN32A(copy_dst+4, 127U * 0x01010101U);
                        } else {
                            WPD_COPY32(copy_dst+4, ptr+4*x-s->linesize);
                            if (!(mb_x + x)) {
                                copy_dst[3] = 129U;
                            } else {
                                copy_dst[3] = ptr[4*x-s->linesize-1];
                            }
                        }
                        if (!(mb_x + x)) {
                            copy_dst[11] =
                            copy_dst[19] =
                            copy_dst[27] =
                            copy_dst[35] = 129U;
                        } else {
                            copy_dst[11] = ptr[4*x              -1];
                            copy_dst[19] = ptr[4*x+s->linesize  -1];
                            copy_dst[27] = ptr[4*x+s->linesize*2-1];
                            copy_dst[35] = ptr[4*x+s->linesize*3-1];
                        }
                    }
                } else {
                    mode = intra4x4[x];
                }
                s->pred.pred4x4[mode](dst, topright, linesize);
                if (copy) {
                    WPD_COPY32(ptr+4*x              , copy_dst+12);
                    WPD_COPY32(ptr+4*x+s->linesize  , copy_dst+20);
                    WPD_COPY32(ptr+4*x+s->linesize*2, copy_dst+28);
                    WPD_COPY32(ptr+4*x+s->linesize*3, copy_dst+36);
                }

                nnz = s->non_zero_count_cache[y][x];
                if (nnz) {
                    if (nnz == 1)
                        s->vp8dsp.vp8_idct_dc_add(ptr+4*x, s->block[y][x], s->linesize);
                    else
                        s->vp8dsp.vp8_idct_add(ptr+4*x, s->block[y][x], s->linesize);
                }
                topright += 4;
            }

            ptr   += 4*s->linesize;
            intra4x4 += 4;
        }
    }

    if (avctx->flags & WPD_CODEC_FLAG_EMU_EDGE) {
        mode = check_intra_pred8x8_mode_emuedge(s->chroma_pred_mode, mb_x, mb_y);
    } else {
        mode = check_intra_pred8x8_mode(s->chroma_pred_mode, mb_x, mb_y);
    }
    s->pred.pred8x8[mode](dst[1], s->uvlinesize);
    s->pred.pred8x8[mode](dst[2], s->uvlinesize);

    if (!(avctx->flags & WPD_CODEC_FLAG_EMU_EDGE && !mb_y) && (s->deblock_filter || !mb_y))
        xchg_mb_border(s->top_border[mb_x+1], dst[0], dst[1], dst[2],
                       s->linesize, s->uvlinesize, mb_x, mb_y, s->mb_width,
                       s->filter.simple, 0);
}

static const uint8_t subpel_idx[3][8] = {
    { 0, 1, 2, 1, 2, 1, 2, 1 }, // nr. of left extra pixels,
                                // also function pointer index
    { 0, 3, 5, 3, 5, 3, 5, 3 }, // nr. of extra pixels required
    { 0, 2, 3, 2, 3, 2, 3, 2 }, // nr. of right extra pixels
};

/**
 * luma MC function
 *
 * @param s VP8 decoding context
 * @param dst target buffer for block data at block position
 * @param ref reference picture buffer at origin (0, 0)
 * @param mv motion vector (relative to block position) to get pixel data from
 * @param x_off horizontal position of block from origin (0, 0)
 * @param y_off vertical position of block from origin (0, 0)
 * @param block_w width of block (16, 8 or 4)
 * @param block_h height of block (always same as block_w)
 * @param width width of src/dst plane data
 * @param height height of src/dst plane data
 * @param linesize size of a single line of plane data, including padding
 * @param mc_func motion compensation function pointers (bilinear or sixtap MC)
 */
static wpd_always_inline
void vp8_mc_luma(VP8Context *s, uint8_t *dst, WpdFrame *ref, const VP56mv *mv,
                 int x_off, int y_off, int block_w, int block_h,
                 int width, int height, int linesize,
                 vp8_mc_func mc_func[3][3])
{
    uint8_t *src = ref->data[0];

    if (WPD_RN32A(mv)) {

        int mx = (mv->x * 2)&7, mx_idx = subpel_idx[0][mx];
        int my = (mv->y * 2)&7, my_idx = subpel_idx[0][my];

        x_off += mv->x >> 2;
        y_off += mv->y >> 2;

        // edge emulation
        src += y_off * linesize + x_off;
        if (x_off < mx_idx || x_off >= width  - block_w - subpel_idx[2][mx] ||
            y_off < my_idx || y_off >= height - block_h - subpel_idx[2][my]) {
            s->dsp.emulated_edge_mc(s->edge_emu_buffer, src - my_idx * linesize - mx_idx,
                                    linesize, linesize,
                                    block_w + subpel_idx[1][mx], block_h + subpel_idx[1][my],
                                    x_off - mx_idx, y_off - my_idx, width, height);
            src = s->edge_emu_buffer + mx_idx + linesize * my_idx;
        }
        mc_func[my_idx][mx_idx](dst, linesize, src, linesize, block_h, mx, my);
    } else {
        mc_func[0][0](dst, linesize, src + y_off * linesize + x_off, linesize, block_h, 0, 0);
    }
}

/**
 * chroma MC function
 *
 * @param s VP8 decoding context
 * @param dst1 target buffer for block data at block position (U plane)
 * @param dst2 target buffer for block data at block position (V plane)
 * @param ref reference picture buffer at origin (0, 0)
 * @param mv motion vector (relative to block position) to get pixel data from
 * @param x_off horizontal position of block from origin (0, 0)
 * @param y_off vertical position of block from origin (0, 0)
 * @param block_w width of block (16, 8 or 4)
 * @param block_h height of block (always same as block_w)
 * @param width width of src/dst plane data
 * @param height height of src/dst plane data
 * @param linesize size of a single line of plane data, including padding
 * @param mc_func motion compensation function pointers (bilinear or sixtap MC)
 */
static wpd_always_inline
void vp8_mc_chroma(VP8Context *s, uint8_t *dst1, uint8_t *dst2, WpdFrame *ref,
                   const VP56mv *mv, int x_off, int y_off,
                   int block_w, int block_h, int width, int height, int linesize,
                   vp8_mc_func mc_func[3][3])
{
    uint8_t *src1 = ref->data[1], *src2 = ref->data[2];

    if (WPD_RN32A(mv)) {
        int mx = mv->x&7, mx_idx = subpel_idx[0][mx];
        int my = mv->y&7, my_idx = subpel_idx[0][my];

        x_off += mv->x >> 3;
        y_off += mv->y >> 3;

        // edge emulation
        src1 += y_off * linesize + x_off;
        src2 += y_off * linesize + x_off;
        if (x_off < mx_idx || x_off >= width  - block_w - subpel_idx[2][mx] ||
            y_off < my_idx || y_off >= height - block_h - subpel_idx[2][my]) {
            s->dsp.emulated_edge_mc(s->edge_emu_buffer, src1 - my_idx * linesize - mx_idx,
                                    linesize, linesize,
                                    block_w + subpel_idx[1][mx], block_h + subpel_idx[1][my],
                                    x_off - mx_idx, y_off - my_idx, width, height);
            src1 = s->edge_emu_buffer + mx_idx + linesize * my_idx;
            mc_func[my_idx][mx_idx](dst1, linesize, src1, linesize, block_h, mx, my);

            s->dsp.emulated_edge_mc(s->edge_emu_buffer, src2 - my_idx * linesize - mx_idx,
                                    linesize, linesize,
                                    block_w + subpel_idx[1][mx], block_h + subpel_idx[1][my],
                                    x_off - mx_idx, y_off - my_idx, width, height);
            src2 = s->edge_emu_buffer + mx_idx + linesize * my_idx;
            mc_func[my_idx][mx_idx](dst2, linesize, src2, linesize, block_h, mx, my);
        } else {
            mc_func[my_idx][mx_idx](dst1, linesize, src1, linesize, block_h, mx, my);
            mc_func[my_idx][mx_idx](dst2, linesize, src2, linesize, block_h, mx, my);
        }
    } else {
        mc_func[0][0](dst1, linesize, src1 + y_off * linesize + x_off, linesize, block_h, 0, 0);
        mc_func[0][0](dst2, linesize, src2 + y_off * linesize + x_off, linesize, block_h, 0, 0);
    }
}

static wpd_always_inline
void vp8_mc_part(VP8Context *s, uint8_t *dst[3],
                 WpdFrame *ref_frame, int x_off, int y_off,
                 int bx_off, int by_off,
                 int block_w, int block_h,
                 int width, int height, VP56mv *mv)
{
    VP56mv uvmv = *mv;

    /* Y */
    vp8_mc_luma(s, dst[0] + by_off * s->linesize + bx_off,
                ref_frame, mv, x_off + bx_off, y_off + by_off,
                block_w, block_h, width, height, s->linesize,
                s->put_pixels_tab[block_w == 8]);

    /* U/V */
    if (s->profile == 3) {
        uvmv.x &= ~7;
        uvmv.y &= ~7;
    }
    x_off   >>= 1; y_off   >>= 1;
    bx_off  >>= 1; by_off  >>= 1;
    width   >>= 1; height  >>= 1;
    block_w >>= 1; block_h >>= 1;
    vp8_mc_chroma(s, dst[1] + by_off * s->uvlinesize + bx_off,
                  dst[2] + by_off * s->uvlinesize + bx_off, ref_frame,
                  &uvmv, x_off + bx_off, y_off + by_off,
                  block_w, block_h, width, height, s->uvlinesize,
                  s->put_pixels_tab[1 + (block_w == 4)]);
}

/* Fetch pixels for estimated mv 4 macroblocks ahead.
 * Optimized for 64-byte cache lines. */
static wpd_always_inline void prefetch_motion(VP8Context *s, VP8Macroblock *mb, int mb_x, int mb_y, int mb_xy, int ref)
{
    /* Don't prefetch refs that haven't been used very often this frame. */
    if (s->ref_count[ref-1] > (mb_xy >> 5)) {
        int x_off = mb_x << 4, y_off = mb_y << 4;
        int mx = (mb->mv.x>>2) + x_off + 8;
        int my = (mb->mv.y>>2) + y_off;
        uint8_t **src= s->framep[ref]->data;
        int off= mx + (my + (mb_x&3)*4)*s->linesize + 64;
        /* A mistimed prefetch does not affect decoder output. */
        s->dsp.prefetch(src[0]+off, s->linesize, 4);
        off= (mx>>1) + ((my>>1) + (mb_x&7))*s->uvlinesize + 64;
        s->dsp.prefetch(src[1]+off, src[2]-src[1], 2);
    }
}

/**
 * Apply motion vectors to prediction buffer, chapter 18.
 */
static wpd_always_inline
void inter_predict(VP8Context *s, uint8_t *dst[3], VP8Macroblock *mb,
                   int mb_x, int mb_y)
{
    int x_off = mb_x << 4, y_off = mb_y << 4;
    int width = 16*s->mb_width, height = 16*s->mb_height;
    WpdFrame *ref = s->framep[mb->ref_frame];
    VP56mv *bmv = mb->bmv;

    switch (mb->partitioning) {
    case VP8_SPLITMVMODE_NONE:
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    0, 0, 16, 16, width, height, &mb->mv);
        break;
    case VP8_SPLITMVMODE_4x4: {
        int x, y;
        VP56mv uvmv;

        /* Y */
        for (y = 0; y < 4; y++) {
            for (x = 0; x < 4; x++) {
                vp8_mc_luma(s, dst[0] + 4*y*s->linesize + x*4,
                            ref, &bmv[4*y + x],
                            4*x + x_off, 4*y + y_off, 4, 4,
                            width, height, s->linesize,
                            s->put_pixels_tab[2]);
            }
        }

        /* U/V */
        x_off >>= 1; y_off >>= 1; width >>= 1; height >>= 1;
        for (y = 0; y < 2; y++) {
            for (x = 0; x < 2; x++) {
                uvmv.x = mb->bmv[ 2*y    * 4 + 2*x  ].x +
                         mb->bmv[ 2*y    * 4 + 2*x+1].x +
                         mb->bmv[(2*y+1) * 4 + 2*x  ].x +
                         mb->bmv[(2*y+1) * 4 + 2*x+1].x;
                uvmv.y = mb->bmv[ 2*y    * 4 + 2*x  ].y +
                         mb->bmv[ 2*y    * 4 + 2*x+1].y +
                         mb->bmv[(2*y+1) * 4 + 2*x  ].y +
                         mb->bmv[(2*y+1) * 4 + 2*x+1].y;
                uvmv.x = (uvmv.x + 2 + (uvmv.x >> (WPD_INT_BITS-1))) >> 2;
                uvmv.y = (uvmv.y + 2 + (uvmv.y >> (WPD_INT_BITS-1))) >> 2;
                if (s->profile == 3) {
                    uvmv.x &= ~7;
                    uvmv.y &= ~7;
                }
                vp8_mc_chroma(s, dst[1] + 4*y*s->uvlinesize + x*4,
                              dst[2] + 4*y*s->uvlinesize + x*4, ref, &uvmv,
                              4*x + x_off, 4*y + y_off, 4, 4,
                              width, height, s->uvlinesize,
                              s->put_pixels_tab[2]);
            }
        }
        break;
    }
    case VP8_SPLITMVMODE_16x8:
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    0, 0, 16, 8, width, height, &bmv[0]);
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    0, 8, 16, 8, width, height, &bmv[1]);
        break;
    case VP8_SPLITMVMODE_8x16:
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    0, 0, 8, 16, width, height, &bmv[0]);
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    8, 0, 8, 16, width, height, &bmv[1]);
        break;
    case VP8_SPLITMVMODE_8x8:
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    0, 0, 8, 8, width, height, &bmv[0]);
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    8, 0, 8, 8, width, height, &bmv[1]);
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    0, 8, 8, 8, width, height, &bmv[2]);
        vp8_mc_part(s, dst, ref, x_off, y_off,
                    8, 8, 8, 8, width, height, &bmv[3]);
        break;
    }
}

static wpd_always_inline void idct_mb(VP8Context *s, uint8_t *dst[3], VP8Macroblock *mb)
{
    int x, y, ch;

    if (mb->mode != MODE_I4x4) {
        uint8_t *y_dst = dst[0];
        for (y = 0; y < 4; y++) {
            uint32_t nnz4 = WPD_RL32(s->non_zero_count_cache[y]);
            if (nnz4) {
                if (nnz4&~0x01010101) {
                    for (x = 0; x < 4; x++) {
                        if ((uint8_t)nnz4 == 1)
                            s->vp8dsp.vp8_idct_dc_add(y_dst+4*x, s->block[y][x], s->linesize);
                        else if((uint8_t)nnz4 > 1)
                            s->vp8dsp.vp8_idct_add(y_dst+4*x, s->block[y][x], s->linesize);
                        nnz4 >>= 8;
                        if (!nnz4)
                            break;
                    }
                } else {
                    s->vp8dsp.vp8_idct_dc_add4y(y_dst, s->block[y], s->linesize);
                }
            }
            y_dst += 4*s->linesize;
        }
    }

    for (ch = 0; ch < 2; ch++) {
        uint32_t nnz4 = WPD_RL32(s->non_zero_count_cache[4+ch]);
        if (nnz4) {
            uint8_t *ch_dst = dst[1+ch];
            if (nnz4&~0x01010101) {
                for (y = 0; y < 2; y++) {
                    for (x = 0; x < 2; x++) {
                        if ((uint8_t)nnz4 == 1)
                            s->vp8dsp.vp8_idct_dc_add(ch_dst+4*x, s->block[4+ch][(y<<1)+x], s->uvlinesize);
                        else if((uint8_t)nnz4 > 1)
                            s->vp8dsp.vp8_idct_add(ch_dst+4*x, s->block[4+ch][(y<<1)+x], s->uvlinesize);
                        nnz4 >>= 8;
                        if (!nnz4)
                            goto chroma_idct_end;
                    }
                    ch_dst += 4*s->uvlinesize;
                }
            } else {
                s->vp8dsp.vp8_idct_dc_add4uv(ch_dst, s->block[4+ch], s->uvlinesize);
            }
        }
chroma_idct_end: ;
    }
}

static wpd_always_inline void filter_level_for_mb(VP8Context *s, VP8Macroblock *mb, VP8FilterStrength *f )
{
    int interior_limit, filter_level;

    if (s->segmentation.enabled) {
        filter_level = s->segmentation.filter_level[s->segment];
        if (!s->segmentation.absolute_vals)
            filter_level += s->filter.level;
    } else
        filter_level = s->filter.level;

    if (s->lf_delta.enabled) {
        filter_level += s->lf_delta.ref[mb->ref_frame];
        filter_level += s->lf_delta.mode[mb->mode];
    }

    filter_level = wpd_clip_uintp2(filter_level, 6);

    interior_limit = filter_level;
    if (s->filter.sharpness) {
        interior_limit >>= (s->filter.sharpness + 3) >> 2;
        interior_limit = WPD_MIN(interior_limit, 9 - s->filter.sharpness);
    }
    interior_limit = WPD_MAX(interior_limit, 1);

    f->filter_level = filter_level;
    f->inner_limit = interior_limit;
    f->inner_filter = !mb->skip || mb->mode == MODE_I4x4 || mb->mode == VP8_MVMODE_SPLIT;
}

static wpd_always_inline void filter_mb(VP8Context *s, uint8_t *dst[3], VP8FilterStrength *f, int mb_x, int mb_y)
{
    int mbedge_lim, bedge_lim, hev_thresh;
    int filter_level = f->filter_level;
    int inner_limit = f->inner_limit;
    int inner_filter = f->inner_filter;
    int linesize = s->linesize;
    int uvlinesize = s->uvlinesize;
    static const uint8_t hev_thresh_lut[2][64] = {
        { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1,
          2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
          3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
          3, 3, 3, 3 },
        { 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1,
          1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
          2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
          2, 2, 2, 2 }
    };

    if (!filter_level)
        return;

     bedge_lim = 2*filter_level + inner_limit;
    mbedge_lim = bedge_lim + 4;

    hev_thresh = hev_thresh_lut[s->keyframe][filter_level];

    if (mb_x) {
        s->vp8dsp.vp8_h_loop_filter16y(dst[0],     linesize,
                                       mbedge_lim, inner_limit, hev_thresh);
        s->vp8dsp.vp8_h_loop_filter8uv(dst[1],     dst[2],      uvlinesize,
                                       mbedge_lim, inner_limit, hev_thresh);
    }

    if (inner_filter) {
        s->vp8dsp.vp8_h_loop_filter16y_inner(dst[0]+ 4, linesize, bedge_lim,
                                             inner_limit, hev_thresh);
        s->vp8dsp.vp8_h_loop_filter16y_inner(dst[0]+ 8, linesize, bedge_lim,
                                             inner_limit, hev_thresh);
        s->vp8dsp.vp8_h_loop_filter16y_inner(dst[0]+12, linesize, bedge_lim,
                                             inner_limit, hev_thresh);
        s->vp8dsp.vp8_h_loop_filter8uv_inner(dst[1] + 4, dst[2] + 4,
                                             uvlinesize,  bedge_lim,
                                             inner_limit, hev_thresh);
    }

    if (mb_y) {
        s->vp8dsp.vp8_v_loop_filter16y(dst[0],     linesize,
                                       mbedge_lim, inner_limit, hev_thresh);
        s->vp8dsp.vp8_v_loop_filter8uv(dst[1],     dst[2],      uvlinesize,
                                       mbedge_lim, inner_limit, hev_thresh);
    }

    if (inner_filter) {
        s->vp8dsp.vp8_v_loop_filter16y_inner(dst[0]+ 4*linesize,
                                             linesize,    bedge_lim,
                                             inner_limit, hev_thresh);
        s->vp8dsp.vp8_v_loop_filter16y_inner(dst[0]+ 8*linesize,
                                             linesize,    bedge_lim,
                                             inner_limit, hev_thresh);
        s->vp8dsp.vp8_v_loop_filter16y_inner(dst[0]+12*linesize,
                                             linesize,    bedge_lim,
                                             inner_limit, hev_thresh);
        s->vp8dsp.vp8_v_loop_filter8uv_inner(dst[1] + 4 * uvlinesize,
                                             dst[2] + 4 * uvlinesize,
                                             uvlinesize,  bedge_lim,
                                             inner_limit, hev_thresh);
    }
}

static wpd_always_inline void filter_mb_simple(VP8Context *s, uint8_t *dst, VP8FilterStrength *f, int mb_x, int mb_y)
{
    int mbedge_lim, bedge_lim;
    int filter_level = f->filter_level;
    int inner_limit = f->inner_limit;
    int inner_filter = f->inner_filter;
    int linesize = s->linesize;

    if (!filter_level)
        return;

     bedge_lim = 2*filter_level + inner_limit;
    mbedge_lim = bedge_lim + 4;

    if (mb_x)
        s->vp8dsp.vp8_h_loop_filter_simple(dst, linesize, mbedge_lim);
    if (inner_filter) {
        s->vp8dsp.vp8_h_loop_filter_simple(dst+ 4, linesize, bedge_lim);
        s->vp8dsp.vp8_h_loop_filter_simple(dst+ 8, linesize, bedge_lim);
        s->vp8dsp.vp8_h_loop_filter_simple(dst+12, linesize, bedge_lim);
    }

    if (mb_y)
        s->vp8dsp.vp8_v_loop_filter_simple(dst, linesize, mbedge_lim);
    if (inner_filter) {
        s->vp8dsp.vp8_v_loop_filter_simple(dst+ 4*linesize, linesize, bedge_lim);
        s->vp8dsp.vp8_v_loop_filter_simple(dst+ 8*linesize, linesize, bedge_lim);
        s->vp8dsp.vp8_v_loop_filter_simple(dst+12*linesize, linesize, bedge_lim);
    }
}

static void filter_mb_row(VP8Context *s, WpdFrame *curframe, int mb_y)
{
    VP8FilterStrength *f = s->filter_strength;
    uint8_t *dst[3] = {
        curframe->data[0] + 16*mb_y*s->linesize,
        curframe->data[1] +  8*mb_y*s->uvlinesize,
        curframe->data[2] +  8*mb_y*s->uvlinesize
    };
    int mb_x;

    for (mb_x = 0; mb_x < s->mb_width; mb_x++) {
        backup_mb_border(s->top_border[mb_x+1], dst[0], dst[1], dst[2], s->linesize, s->uvlinesize, 0);
        filter_mb(s, dst, f++, mb_x, mb_y);
        dst[0] += 16;
        dst[1] += 8;
        dst[2] += 8;
    }
}

static void filter_mb_row_simple(VP8Context *s, WpdFrame *curframe, int mb_y)
{
    VP8FilterStrength *f = s->filter_strength;
    uint8_t *dst = curframe->data[0] + 16*mb_y*s->linesize;
    int mb_x;

    for (mb_x = 0; mb_x < s->mb_width; mb_x++) {
        backup_mb_border(s->top_border[mb_x+1], dst, NULL, NULL, s->linesize, 0, 1);
        filter_mb_simple(s, dst, f++, mb_x, mb_y);
        dst += 16;
    }
}

static void release_queued_segmaps(VP8Context *s, int is_close)
{
    int leave_behind = is_close ? 0 : !s->maps_are_invalid;
    while (s->num_maps_to_be_freed > leave_behind)
        wpd_freep(&s->segmentation_maps[--s->num_maps_to_be_freed]);
    s->maps_are_invalid = 0;
}

int vp8_decode_frame(WpdCodecContext *avctx, void *data, int *data_size,
                            WpdPacket *avpkt)
{
    VP8Context *s = avctx->priv_data;
    int ret, mb_x, mb_y, i, y, referenced;
    enum WpdDiscard skip_thresh;
    WpdFrame *wpd_uninit(curframe), *prev_frame;

    release_queued_segmaps(s, 0);

    if ((ret = decode_frame_header(s, avpkt->data, avpkt->size)) < 0)
        goto err;

    prev_frame = s->framep[VP56_FRAME_CURRENT];

    referenced = s->update_last || s->update_golden == VP56_FRAME_CURRENT
                                || s->update_altref == VP56_FRAME_CURRENT;

    skip_thresh = !referenced ? WPD_DISCARD_NONREF :
                    !s->keyframe ? WPD_DISCARD_NONKEY : WPD_DISCARD_ALL;

    if (avctx->skip_frame >= skip_thresh) {
        s->invisible = 1;
        memcpy(&s->next_framep[0], &s->framep[0], sizeof(s->framep[0]) * 4);
        goto skip_decode;
    }
    s->deblock_filter = s->filter.level && avctx->skip_loop_filter < skip_thresh;

    // release no longer referenced frames
    for (i = 0; i < 5; i++)
        if (s->frames[i].data[0] &&
            &s->frames[i] != prev_frame &&
            &s->frames[i] != s->framep[VP56_FRAME_PREVIOUS] &&
            &s->frames[i] != s->framep[VP56_FRAME_GOLDEN] &&
            &s->frames[i] != s->framep[VP56_FRAME_GOLDEN2])
            vp8_release_frame(s, &s->frames[i], 1, 0);

    // find a free buffer
    for (i = 0; i < 5; i++)
        if (&s->frames[i] != prev_frame &&
            &s->frames[i] != s->framep[VP56_FRAME_PREVIOUS] &&
            &s->frames[i] != s->framep[VP56_FRAME_GOLDEN] &&
            &s->frames[i] != s->framep[VP56_FRAME_GOLDEN2]) {
            curframe = s->framep[VP56_FRAME_CURRENT] = &s->frames[i];
            break;
        }
    if (i == 5) {
        wpd_log(avctx, WPD_LOG_FATAL, "Ran out of free frames!\n");
        abort();
    }
    if (curframe->data[0])
        vp8_release_frame(s, curframe, 1, 0);

    // Given that arithmetic probabilities are updated every frame, it's quite likely
    // that the values we have on a random interframe are complete junk if we didn't
    // start decode on a keyframe. So just don't display anything rather than junk.
    if (!s->keyframe && (!s->framep[VP56_FRAME_PREVIOUS] ||
                         !s->framep[VP56_FRAME_GOLDEN] ||
                         !s->framep[VP56_FRAME_GOLDEN2])) {
        wpd_log(avctx, WPD_LOG_WARNING, "Discarding interframe without a prior keyframe!\n");
        ret = WPD_ERROR_INVALID_DATA;
        goto err;
    }

    curframe->key_frame = s->keyframe;
    curframe->pict_type = s->keyframe ? WPD_PICTURE_TYPE_I : WPD_PICTURE_TYPE_P;
    curframe->reference = referenced ? 3 : 0;
    if ((ret = vp8_alloc_frame(s, curframe))) {
        wpd_log(avctx, WPD_LOG_ERROR, "get_buffer() failed!\n");
        goto err;
    }

    // check if golden and altref are swapped
    if (s->update_altref != VP56_FRAME_NONE) {
        s->next_framep[VP56_FRAME_GOLDEN2]  = s->framep[s->update_altref];
    } else {
        s->next_framep[VP56_FRAME_GOLDEN2]  = s->framep[VP56_FRAME_GOLDEN2];
    }
    if (s->update_golden != VP56_FRAME_NONE) {
        s->next_framep[VP56_FRAME_GOLDEN]   = s->framep[s->update_golden];
    } else {
        s->next_framep[VP56_FRAME_GOLDEN]   = s->framep[VP56_FRAME_GOLDEN];
    }
    if (s->update_last) {
        s->next_framep[VP56_FRAME_PREVIOUS] = curframe;
    } else {
        s->next_framep[VP56_FRAME_PREVIOUS] = s->framep[VP56_FRAME_PREVIOUS];
    }
    s->next_framep[VP56_FRAME_CURRENT]      = curframe;

    s->linesize   = curframe->linesize[0];
    s->uvlinesize = curframe->linesize[1];

    if (!s->edge_emu_buffer)
        s->edge_emu_buffer = wpd_malloc(21*s->linesize);

    memset(s->top_nnz, 0, s->mb_width*sizeof(*s->top_nnz));

    /* Zero macroblock structures for top/top-left prediction from outside the frame. */
    memset(s->macroblocks + s->mb_height*2 - 1, 0, (s->mb_width+1)*sizeof(*s->macroblocks));

    // top edge of 127 for intra prediction
    if (!(avctx->flags & WPD_CODEC_FLAG_EMU_EDGE)) {
        s->top_border[0][15] = s->top_border[0][23] = 127;
        memset(s->top_border[1]-1, 127, s->mb_width*sizeof(*s->top_border)+1);
    }
    memset(s->ref_count, 0, sizeof(s->ref_count));
    if (s->keyframe)
        memset(s->intra4x4_pred_mode_top, DC_PRED, s->mb_width*4);

#define MARGIN (16 << 2)
    s->mv_min.y = -MARGIN;
    s->mv_max.y = ((s->mb_height - 1) << 6) + MARGIN;

    for (mb_y = 0; mb_y < s->mb_height; mb_y++) {
        VP56RangeCoder *c = &s->coeff_partition[mb_y & (s->num_coeff_partitions-1)];
        VP8Macroblock *mb = s->macroblocks + (s->mb_height - mb_y - 1)*2;
        int mb_xy = mb_y*s->mb_width;
        uint8_t *dst[3] = {
            curframe->data[0] + 16*mb_y*s->linesize,
            curframe->data[1] +  8*mb_y*s->uvlinesize,
            curframe->data[2] +  8*mb_y*s->uvlinesize
        };

        memset(mb - 1, 0, sizeof(*mb));   // zero left macroblock
        memset(s->left_nnz, 0, sizeof(s->left_nnz));
        WPD_WN32A(s->intra4x4_pred_mode_left, DC_PRED*0x01010101);

        // left edge of 129 for intra prediction
        if (!(avctx->flags & WPD_CODEC_FLAG_EMU_EDGE)) {
            for (i = 0; i < 3; i++)
                for (y = 0; y < 16>>!!i; y++)
                    dst[i][y*curframe->linesize[i]-1] = 129;
            if (mb_y == 1) // top left edge is also 129
                s->top_border[0][15] = s->top_border[0][23] = s->top_border[0][31] = 129;
        }

        s->mv_min.x = -MARGIN;
        s->mv_max.x = ((s->mb_width  - 1) << 6) + MARGIN;
        for (mb_x = 0; mb_x < s->mb_width; mb_x++, mb_xy++, mb++) {
            /* Prefetch the current frame, 4 MBs ahead */
            s->dsp.prefetch(dst[0] + (mb_x&3)*4*s->linesize + 64, s->linesize, 4);
            s->dsp.prefetch(dst[1] + (mb_x&7)*s->uvlinesize + 64, dst[2] - dst[1], 2);

            decode_mb_mode(s, mb, mb_x, mb_y, curframe->ref_index[0] + mb_xy,
                           prev_frame && prev_frame->ref_index[0] ? prev_frame->ref_index[0] + mb_xy : NULL);

            prefetch_motion(s, mb, mb_x, mb_y, mb_xy, VP56_FRAME_PREVIOUS);

            if (!mb->skip)
                decode_mb_coeffs(s, c, mb, s->top_nnz[mb_x], s->left_nnz);

            if (mb->mode <= MODE_I4x4)
                intra_predict(s, dst, mb, mb_x, mb_y);
            else
                inter_predict(s, dst, mb, mb_x, mb_y);

            prefetch_motion(s, mb, mb_x, mb_y, mb_xy, VP56_FRAME_GOLDEN);

            if (!mb->skip) {
                idct_mb(s, dst, mb);
            } else {
                WPD_ZERO64(s->left_nnz);
                WPD_WN64(s->top_nnz[mb_x], 0);   // array of 9, so unaligned

                // Reset DC block predictors if they would exist if the mb had coefficients
                if (mb->mode != MODE_I4x4 && mb->mode != VP8_MVMODE_SPLIT) {
                    s->left_nnz[8]      = 0;
                    s->top_nnz[mb_x][8] = 0;
                }
            }

            if (s->deblock_filter)
                filter_level_for_mb(s, mb, &s->filter_strength[mb_x]);

            prefetch_motion(s, mb, mb_x, mb_y, mb_xy, VP56_FRAME_GOLDEN2);

            dst[0] += 16;
            dst[1] += 8;
            dst[2] += 8;
            s->mv_min.x -= 64;
            s->mv_max.x -= 64;
        }
        if (s->deblock_filter) {
            if (s->filter.simple)
                filter_mb_row_simple(s, curframe, mb_y);
            else
                filter_mb_row(s, curframe, mb_y);
        }
        s->mv_min.y -= 64;
        s->mv_max.y -= 64;

    }

    memcpy(&s->framep[0], &s->next_framep[0], sizeof(s->framep[0]) * 4);

skip_decode:
    // if future frames don't use the updated probabilities,
    // reset them to the values we saved
    if (!s->update_probabilities)
        s->prob[0] = s->prob[1];

    if (!s->invisible) {
        *(WpdFrame*)data = *curframe;
        *data_size = sizeof(WpdFrame);
    }

    return avpkt->size;
err:
    memcpy(&s->next_framep[0], &s->framep[0], sizeof(s->framep[0]) * 4);
    return ret;
}

wpd_cold int vp8_decode_init(WpdCodecContext *avctx)
{
    VP8Context *s = avctx->priv_data;

    s->avctx = avctx;
    avctx->pix_fmt = WPD_PIXEL_FORMAT_YUV420P;

    wpd_dsp_init(&s->dsp, avctx);
    ff_vp8_pred_init(&s->pred);
    ff_vp8dsp_init(&s->vp8dsp);

    return 0;
}

wpd_cold int vp8_decode_free(WpdCodecContext *avctx)
{
    vp8_decode_flush_impl(avctx, 0, 1, 1);
    release_queued_segmaps(avctx->priv_data, 1);
    return 0;
}
