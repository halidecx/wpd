
#include "huffman.h"

static const uint8_t code_length_code_order[NUM_CODE_LENGTH_CODES] = {
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15};

static wpd_always_inline uint32_t huff_entry(int bits, int value) {
    return (uint32_t)bits | (uint32_t)value << 8;
}

void huff_arena_free(HuffBlock **head) {
    HuffBlock *block = *head;

    while (block) {
        HuffBlock *next = block->next;
        wpd_free(block);
        block = next;
    }
    *head = NULL;
}

static uint32_t *huff_arena_alloc(HuffBlock **head, size_t n) {
    HuffBlock *block = *head;
    uint32_t  *table;

    if (!block || block->size - block->used < n) {
        size_t size = n > HUFF_ARENA_CHUNK ? n : HUFF_ARENA_CHUNK;
        block       = malloc(sizeof(*block) + size * sizeof(*block->data));
        if (!block)
            return NULL;
        block->next = *head;
        block->used = 0;
        block->size = size;
        *head       = block;
    }
    table = block->data + block->used;
    block->used += n;
    return table;
}

static wpd_always_inline uint32_t huff_next_key(uint32_t key, int len) {
    uint32_t inv = ~key & ((1u << len) - 1);

    if (!inv)
        return key;
    inv = 1u << (31 - __builtin_clz(inv));
    return (key & (inv - 1)) + inv;
}

static int huff_next_table_bits(const int *count, int len, int root_bits) {
    int left = 1 << (len - root_bits);

    while (len < MAX_HUFFMAN_CODE_LENGTH) {
        left -= count[len];
        if (left <= 0)
            break;
        len++;
        left <<= 1;
    }
    return len - root_bits;
}

static int huff_table_size(const HuffPlan *p) {
    int      count[MAX_HUFFMAN_CODE_LENGTH + 1];
    uint32_t key = 0, low = 0xFFFFFFFFu;
    int      len, total   = 1 << p->root_bits;

    memcpy(count, p->count, sizeof(count));
    for (len = 1; len <= p->root_bits; len++)
        for (; count[len] > 0; count[len]--) key = huff_next_key(key, len);

    for (; len <= MAX_HUFFMAN_CODE_LENGTH; len++) {
        for (; count[len] > 0; count[len]--) {
            if ((key & HUFF_TABLE_MASK) != low) {
                int sub_bits = huff_next_table_bits(count, len, p->root_bits);

                total += 1 << sub_bits;
                low = key & HUFF_TABLE_MASK;
            }
            key = huff_next_key(key, len);
        }
    }
    return total;
}

static void huff_count(HuffPlan *p, const uint8_t *code_lengths,
                       int code_lengths_size) {
    int symbol;

    memset(p->count, 0, sizeof(p->count));
    for (symbol = 0; symbol < code_lengths_size; symbol++)
        p->count[code_lengths[symbol]]++;
}

/* Sizes the tables and sorts the symbols by code length, given the length
   histogram the reader accumulated as it went. Codes are rejected here, before
   anything is written, so a malformed length list never produces a partially
   filled table. */
static int huff_analyze(HuffPlan *p, const uint8_t *code_lengths,
                        int code_lengths_size, uint16_t *sorted) {
    int offset[MAX_HUFFMAN_CODE_LENGTH + 2];
    int len, symbol, left, max_len;

    left           = 1;
    max_len        = 0;
    p->num_symbols = 0;
    offset[1]      = 0;
    for (len = 1; len <= MAX_HUFFMAN_CODE_LENGTH; len++) {
        left <<= 1;
        left -= p->count[len];
        if (left < 0)
            return 0;
        if (p->count[len])
            max_len = len;
        p->num_symbols += p->count[len];
        offset[len + 1] = offset[len] + p->count[len];
    }
    if (!p->num_symbols || p->num_symbols > code_lengths_size)
        return 0;
    if (left && p->num_symbols > 1)
        return 0;

    /* Sparse length lists are the common case, so step over whole zero runs
       instead of testing every symbol. */
    symbol = 0;
    while (symbol + 8 <= code_lengths_size) {
        if (!wpd_r64(code_lengths + symbol)) {
            symbol += 8;
            continue;
        }
        for (len = 0; len < 8; len++, symbol++)
            if (code_lengths[symbol]) {
                if (offset[code_lengths[symbol]] >= p->num_symbols)
                    return 0;
                sorted[offset[code_lengths[symbol]]++] = symbol;
            }
    }
    for (; symbol < code_lengths_size; symbol++)
        if (code_lengths[symbol]) {
            if (offset[code_lengths[symbol]] >= p->num_symbols)
                return 0;
            sorted[offset[code_lengths[symbol]]++] = symbol;
        }

    /* Every offset has to have advanced to where the next length started, or
       the histogram described a different list from the one just sorted. */
    for (len = 1, symbol = 0; len <= MAX_HUFFMAN_CODE_LENGTH; len++) {
        symbol += p->count[len];
        if (offset[len] != symbol)
            return 0;
    }

    if (p->num_symbols == 1) {
        p->root_bits  = 0;
        p->total_size = 1;
        return 1;
    }

    p->root_bits  = WPD_MIN(HUFF_TABLE_BITS, max_len);
    p->total_size = huff_table_size(p);
    return 1;
}

/* Because the index is the bit-reversed code, a code of length len owns every
   slot congruent to its key modulo 2^len. So the slots for all codes shorter
   than len are already correct in the first half of the table and only need
   copying into the second, leaving one store per symbol. */
static wpd_always_inline void huff_double_to(uint32_t *table, int *filled,
                                             int size) {
    int n = *filled;

    while (n < size) {
        memcpy(table + n, table, (size_t)n * sizeof(*table));
        n <<= 1;
    }
    *filled = n;
}

static int huff_fill(const HuffPlan *p, uint32_t *table,
                     const uint16_t *sorted) {
    int       count[MAX_HUFFMAN_CODE_LENGTH + 1];
    uint32_t  key       = 0;
    uint32_t  low       = 0xFFFFFFFFu;
    uint32_t *sub       = table;
    int       root_bits = p->root_bits;
    int       len, symbol = 0, filled = 1, sub_size = 1 << root_bits;
    int       total = 1 << root_bits;

    if (p->num_symbols == 1) {
        table[0] = huff_entry(0, sorted[0]);
        return 1;
    }

    memcpy(count, p->count, sizeof(count));
    table[0] = 0;
    for (len = 1; len <= root_bits; len++) {
        huff_double_to(table, &filled, 1 << len);
        for (; count[len] > 0; count[len]--) {
            table[key] = huff_entry(len, sorted[symbol++]);
            key        = huff_next_key(key, len);
        }
    }

    for (len = root_bits + 1; len <= MAX_HUFFMAN_CODE_LENGTH; len++) {
        for (; count[len] > 0; count[len]--) {
            if ((key & HUFF_TABLE_MASK) != low) {
                int sub_bits;

                sub += sub_size;
                sub_bits = huff_next_table_bits(count, len, root_bits);
                sub_size = 1 << sub_bits;
                total += sub_size;
                if (total > p->total_size)
                    return 0;
                low        = key & HUFF_TABLE_MASK;
                table[low] = huff_entry(sub_bits + root_bits,
                                        (int)(sub - table) - (int)low);
                filled     = 1;
                sub[0]     = 0;
            }
            huff_double_to(sub, &filled, 1 << (len - root_bits));
            sub[key >> root_bits] = huff_entry(len - root_bits,
                                               sorted[symbol++]);
            key                   = huff_next_key(key, len);
        }
    }

    return total == p->total_size;
}

int huff_reader_build(HuffReader *r, HuffBlock **arena, HuffPlan *plan,
                      const uint8_t *code_lengths, int alphabet_size,
                      uint16_t *sorted) {
    uint32_t *table;

    if (!huff_analyze(plan, code_lengths, alphabet_size, sorted))
        return WPD_ERROR_INVALID_DATA;

    table = huff_arena_alloc(arena, (size_t)plan->total_size);
    if (!table)
        return WPD_ERROR(ENOMEM);

    if (!huff_fill(plan, table, sorted))
        return WPD_ERROR_INVALID_DATA;

    r->table = table;
    r->mask  = (1u << plan->root_bits) - 1;
    return 0;
}

void read_huffman_code_simple(LEBitReader *br, HuffPlan *plan,
                              uint8_t *code_lengths, int alphabet_size) {
    int nb_symbols = br_bit(br) + 1;
    int symbol;

    /* The two symbols may repeat, and the histogram has to stay an exact
       count of the non-zero lengths for the counting sort to line up. */
    symbol = br_bit(br) ? br_bits(br, 8) : br_bit(br);
    if (symbol < alphabet_size && !code_lengths[symbol]) {
        code_lengths[symbol] = 1;
        plan->count[1]++;
    }

    if (nb_symbols == 2) {
        symbol = br_bits(br, 8);
        if (symbol < alphabet_size && !code_lengths[symbol]) {
            code_lengths[symbol] = 1;
            plan->count[1]++;
        }
    }
}

int read_huffman_code_normal(LEBitReader *br, HuffPlan *plan,
                             uint8_t *code_lengths, int alphabet_size) {
    /* Code lengths are 3 bits wide, so this table never needs a second level
       and is at most 128 entries wide. */
    uint32_t   code_len_table[1 << MAX_CODE_LENGTH_CODE_LENGTH];
    HuffReader code_len_reader;
    HuffPlan   code_len_plan;
    uint16_t   sorted[NUM_CODE_LENGTH_CODES];
    uint8_t    code_length_code_lengths[NUM_CODE_LENGTH_CODES] = {0};
    int        symbol, max_symbol, prev_code_len, ret;
    int        num_codes = 4 + br_bits(br, 4);

    for (int i = 0; i < num_codes; i++)
        code_length_code_lengths[code_length_code_order[i]] = br_bits(br, 3);

    if (br_bit(br)) {
        int bits   = 2 + 2 * br_bits(br, 3);
        max_symbol = 2 + br_bits(br, bits);
        if (max_symbol > alphabet_size) {
            wpd_log(NULL,
                    WPD_LOG_ERROR,
                    "max symbol %d > alphabet size %d\n",
                    max_symbol,
                    alphabet_size);
            return WPD_ERROR_INVALID_DATA;
        }
    } else {
        max_symbol = alphabet_size;
    }

    huff_count(&code_len_plan, code_length_code_lengths, NUM_CODE_LENGTH_CODES);
    if (!huff_analyze(&code_len_plan,
                      code_length_code_lengths,
                      NUM_CODE_LENGTH_CODES,
                      sorted))
        return WPD_ERROR_INVALID_DATA;
    if (code_len_plan.total_size > (int)WPD_ARRAY_SIZE(code_len_table))
        return WPD_ERROR_INVALID_DATA;
    if (!huff_fill(&code_len_plan, code_len_table, sorted))
        return WPD_ERROR_INVALID_DATA;
    code_len_reader.table = code_len_table;
    code_len_reader.mask  = (1u << code_len_plan.root_bits) - 1;

    prev_code_len = 8;
    symbol        = 0;
    while (symbol < alphabet_size) {
        int code_len;

        if (!max_symbol--)
            break;
        if (br_is_eos(br))
            break;
        br_fill(br);
        code_len = huff_read_symbol(&code_len_reader, br);
        if (code_len < 16) {
            code_lengths[symbol++] = code_len;
            if (code_len) {
                prev_code_len = code_len;
                plan->count[code_len]++;
            }
        } else {
            int repeat = 0, length = 0;
            switch (code_len) {
            default: ret = WPD_ERROR_INVALID_DATA; goto finish;
            case 16:
                repeat = 3 + br_bits(br, 2);
                length = prev_code_len;
                break;
            case 17: repeat = 3 + br_bits(br, 3); break;
            case 18: repeat = 11 + br_bits(br, 7); break;
            }
            if (symbol + repeat > alphabet_size) {
                wpd_log(NULL,
                        WPD_LOG_ERROR,
                        "invalid symbol %d + repeat %d > alphabet size %d\n",
                        symbol,
                        repeat,
                        alphabet_size);
                ret = WPD_ERROR_INVALID_DATA;
                goto finish;
            }
            /* The buffer arrives zeroed, so a run of zeros is just a skip. */
            if (length) {
                plan->count[length] += repeat;
                memset(code_lengths + symbol, length, repeat);
            }
            symbol += repeat;
        }
    }

    ret = 0;

finish:
    return ret;
}
