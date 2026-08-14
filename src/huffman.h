#ifndef WPD_HUFFMAN_H
#define WPD_HUFFMAN_H

#include "bitreader.h"

#define MAX_HUFFMAN_CODE_LENGTH 15
#define NUM_CODE_LENGTH_CODES 19
#define MAX_CODE_LENGTH_CODE_LENGTH 7

#define HUFF_TABLE_BITS 8
#define HUFF_TABLE_MASK ((1 << HUFF_TABLE_BITS) - 1)
#define HUFF_ARENA_CHUNK 4096

/* A table entry packs the bits to consume in its low eight bits and either the
   symbol or a secondary-table offset above them. The root table is sized to the
   longest code it holds, capped at HUFF_TABLE_BITS, so only codes longer than
   that reach a secondary table and only then is the cap in force. */
typedef struct HuffReader {
    const uint32_t *table;
    uint32_t        mask;
} HuffReader;

typedef struct HuffBlock {
    struct HuffBlock *next;
    size_t            used;
    size_t            size;
    uint32_t          data[];
} HuffBlock;

typedef struct HuffPlan {
    int count[MAX_HUFFMAN_CODE_LENGTH + 1];
    int num_symbols;
    int root_bits;
    int total_size;
} HuffPlan;

void huff_arena_free(HuffBlock **head);
int  huff_reader_build(HuffReader *r, HuffBlock **arena, HuffPlan *plan,
                       const uint8_t *code_lengths, int alphabet_size,
                       uint16_t *sorted);

/* The two forms a VP8L prefix-code length list comes in. Both accumulate into
   'plan->count', which the caller has zeroed, and write into 'code_lengths',
   which it has zeroed to 'alphabet_size'. */
void read_huffman_code_simple(LEBitReader *br, HuffPlan *plan,
                              uint8_t *code_lengths, int alphabet_size);
int  read_huffman_code_normal(LEBitReader *br, HuffPlan *plan,
                              uint8_t *code_lengths, int alphabet_size);

/* In the lossless pixel loop, so it stays inline. */
static wpd_always_inline int huff_read_symbol(const HuffReader *r,
                                              LEBitReader      *br) {
    uint32_t val   = br_prefetch(br);
    uint32_t index = val & r->mask;
    uint32_t entry = r->table[index];
    uint32_t bits  = entry & 0xFF;

    if (bits > HUFF_TABLE_BITS) {
        br_set_bit_pos(br, br->bit_pos + HUFF_TABLE_BITS);
        val   = br_prefetch(br);
        entry = r->table[index + (entry >> 8) +
                         (val & ((1u << (bits - HUFF_TABLE_BITS)) - 1))];
        bits  = entry & 0xFF;
    }
    br_set_bit_pos(br, br->bit_pos + (int)bits);
    return (int)(entry >> 8);
}

#endif
