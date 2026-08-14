//! VP8L prefix codes.
//!
//! This is the code CVE-2023-4863 was in: a length list that described more
//! symbols than the table had been sized for overflowed it. The shape of the
//! defence is the same as the C's — size the table from the length histogram,
//! reject a malformed list before writing anything, and check at the end that
//! exactly as much was filled as was reserved — but here an arithmetic slip
//! reaching past the table is a panic rather than a write into whatever
//! followed it.
//!
//! A table entry packs the bits to consume in its low eight bits and either the
//! symbol or a secondary-table offset above them. The root table is sized to
//! the longest code it holds, capped at [`TABLE_BITS`], so only codes longer
//! than that reach a secondary table and only then is the cap in force.
//!
//! All the tables of one image live in a single arena, and a reader holds its
//! extent in that arena rather than a pointer into it. That is what the C's
//! chunked allocator was for — a reader must not be invalidated by a later
//! table being added — and an offset gets it for free.

use super::bitreader::BitReader;
use crate::error::{Error, Result};

pub const MAX_CODE_LENGTH: usize = 15;
const NUM_CODE_LENGTH_CODES: usize = 19;
const MAX_CODE_LENGTH_CODE_LENGTH: usize = 7;

pub const TABLE_BITS: u32 = 8;
const TABLE_MASK: u32 = (1 << TABLE_BITS) - 1;

const CODE_LENGTH_CODE_ORDER: [u8; NUM_CODE_LENGTH_CODES] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// Where one prefix code's tables sit in the arena.
#[derive(Clone, Copy, Default)]
pub struct Reader {
    start: u32,
    len: u32,
    pub mask: u32,
}

/// A reader resolved against the arena it lives in.
///
/// `root` is exactly `mask + 1` entries long, so indexing it with `& (len - 1)`
/// is in bounds by construction and costs no check. `full` reaches the
/// secondary tables, which only long codes touch.
#[derive(Clone, Copy)]
pub struct Tree<'a> {
    root: &'a [u32],
    full: &'a [u32],
}

impl Reader {
    #[inline(always)]
    pub fn tree<'a>(&self, arena: &'a [u32]) -> Tree<'a> {
        let full = &arena[self.start as usize..][..self.len as usize];

        Tree {
            root: &full[..=self.mask as usize],
            full,
        }
    }
}

impl Tree<'_> {
    /// The symbol at the reader's position. Never refills: the caller fills
    /// before a run of these, as the pixel loop does.
    #[inline(always)]
    pub fn read(&self, br: &mut BitReader) -> u32 {
        let mut index = (br.prefetch() as usize) & (self.root.len() - 1);
        let mut entry = self.root[index];
        let mut bits = entry & 0xFF;

        if bits > TABLE_BITS {
            br.advance(TABLE_BITS as i32);
            let val = br.prefetch();

            index += (entry >> 8) as usize
                + (val & ((1 << (bits - TABLE_BITS)) - 1)) as usize;
            entry = self.full[index];
            bits = entry & 0xFF;
        }
        br.advance(bits as i32);
        entry >> 8
    }

    /// The one symbol a single-entry table holds, which is how the pixel loop
    /// recognises a channel that never varies.
    pub fn only_symbol(&self) -> u8 {
        (self.full[0] >> 8) as u8
    }
}

/// The length histogram a code-length list accumulates, and what sizing the
/// tables from it worked out.
pub struct Plan {
    pub count: [i32; MAX_CODE_LENGTH + 1],
    num_symbols: i32,
    root_bits: u32,
    total_size: usize,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            count: [0; MAX_CODE_LENGTH + 1],
            num_symbols: 0,
            root_bits: 0,
            total_size: 0,
        }
    }
}

const fn entry(bits: u32, value: u32) -> u32 {
    bits | value << 8
}

/// The next canonical code of the same length, in bit-reversed order.
///
/// `leading_zeros` rather than `ilog2`, which carries a panic path for zero
/// that this has already ruled out.
#[inline(always)]
fn next_key(key: u32, len: u32) -> u32 {
    let inv = !key & ((1u32 << len) - 1);

    if inv == 0 {
        return key;
    }
    let inv = 1u32 << (31 - inv.leading_zeros());
    (key & (inv - 1)) + inv
}

fn next_table_bits(
    count: &[i32; MAX_CODE_LENGTH + 1],
    len: u32,
    root_bits: u32,
) -> u32 {
    let mut left = 1i32 << (len - root_bits);
    let mut len = len;

    while (len as usize) < MAX_CODE_LENGTH {
        left -= count[len as usize];
        if left <= 0 {
            break;
        }
        len += 1;
        left <<= 1;
    }
    len - root_bits
}

fn table_size(p: &Plan) -> usize {
    let mut count = p.count;
    let mut key = 0u32;
    let mut low = 0xFFFF_FFFFu32;
    let mut total = 1usize << p.root_bits;

    /* Ranged over the whole histogram rather than over `root_bits`, so the
    index is one the compiler can see is in bounds. */
    for len in 1..=MAX_CODE_LENGTH as u32 {
        if len > p.root_bits {
            break;
        }
        while count[len as usize] > 0 {
            key = next_key(key, len);
            count[len as usize] -= 1;
        }
    }

    for len in p.root_bits + 1..=MAX_CODE_LENGTH as u32 {
        while count[len as usize] > 0 {
            if (key & TABLE_MASK) != low {
                total += 1 << next_table_bits(&count, len, p.root_bits);
                low = key & TABLE_MASK;
            }
            key = next_key(key, len);
            count[len as usize] -= 1;
        }
    }
    total
}

pub fn count_lengths(p: &mut Plan, lengths: &[u8]) {
    p.count = [0; MAX_CODE_LENGTH + 1];
    for &l in lengths {
        p.count[usize::from(l) & MAX_CODE_LENGTH] += 1;
    }
}

/// Sizes the tables and sorts the symbols by code length, given the histogram
/// the reader accumulated as it went. Codes are rejected here, before anything
/// is written, so a malformed length list never produces a partly filled table.
fn analyze(p: &mut Plan, lengths: &[u8], sorted: &mut [u16]) -> bool {
    let mut offset = [0usize; MAX_CODE_LENGTH + 2];
    let mut left = 1i32;
    let mut max_len = 0u32;

    p.num_symbols = 0;
    for len in 1..=MAX_CODE_LENGTH {
        left <<= 1;
        left -= p.count[len];
        if left < 0 {
            return false;
        }
        if p.count[len] != 0 {
            max_len = len as u32;
        }
        p.num_symbols += p.count[len];
        offset[len + 1] = offset[len] + p.count[len] as usize;
    }
    if p.num_symbols == 0 || p.num_symbols as usize > lengths.len() {
        return false;
    }
    if left != 0 && p.num_symbols > 1 {
        return false;
    }

    let num_symbols = p.num_symbols as usize;
    let sorted = &mut sorted[..num_symbols];

    /* Sparse length lists are the common case, so step over whole zero runs
    instead of testing every symbol. */
    let mut symbol = 0;
    while symbol + 8 <= lengths.len() {
        let run: [u8; 8] = lengths[symbol..symbol + 8].try_into().unwrap();

        if u64::from_ne_bytes(run) == 0 {
            symbol += 8;
            continue;
        }
        for _ in 0..8 {
            let l = usize::from(lengths[symbol]) & MAX_CODE_LENGTH;

            if l != 0 {
                if offset[l] >= num_symbols {
                    return false;
                }
                sorted[offset[l]] = symbol as u16;
                offset[l] += 1;
            }
            symbol += 1;
        }
    }
    while symbol < lengths.len() {
        let l = usize::from(lengths[symbol]) & MAX_CODE_LENGTH;

        if l != 0 {
            if offset[l] >= num_symbols {
                return false;
            }
            sorted[offset[l]] = symbol as u16;
            offset[l] += 1;
        }
        symbol += 1;
    }

    /* Every offset has to have advanced to where the next length started, or
    the histogram described a different list from the one just sorted. */
    let mut seen = 0usize;
    for len in 1..=MAX_CODE_LENGTH {
        seen += p.count[len] as usize;
        if offset[len] != seen {
            return false;
        }
    }

    if p.num_symbols == 1 {
        p.root_bits = 0;
        p.total_size = 1;
        return true;
    }

    p.root_bits = TABLE_BITS.min(max_len);
    p.total_size = table_size(p);
    true
}

/// Because the index is the bit-reversed code, a code of length `len` owns
/// every slot congruent to its key modulo `2^len`. So the slots for all codes
/// shorter than `len` are already correct in the first half of the table and
/// only need copying into the second, leaving one store per symbol.
#[inline(always)]
fn double_to(table: &mut [u32], filled: &mut usize, size: usize) {
    let mut n = *filled;

    while n < size {
        table.copy_within(..n, n);
        n <<= 1;
    }
    *filled = n;
}

fn fill(p: &Plan, table: &mut [u32], sorted: &[u16]) -> bool {
    /* Exactly as many symbols are written as were counted, so cutting the
    list to that length is what lets the writes below index it unchecked. */
    let sorted = &sorted[..p.num_symbols.max(1) as usize];
    let mut count = p.count;
    let mut key = 0u32;
    let mut low = 0xFFFF_FFFFu32;
    let mut sub = 0usize;
    let root_bits = p.root_bits;
    let mut symbol = 0usize;
    let mut filled = 1usize;
    let mut sub_size = 1usize << root_bits;
    let mut total = 1usize << root_bits;

    if p.num_symbols == 1 {
        table[0] = entry(0, u32::from(sorted[0]));
        return true;
    }

    table[0] = 0;
    for len in 1..=MAX_CODE_LENGTH as u32 {
        if len > root_bits {
            break;
        }
        double_to(&mut table[..1 << root_bits], &mut filled, 1 << len);
        while count[len as usize] > 0 {
            table[key as usize] = entry(len, u32::from(sorted[symbol]));
            symbol += 1;
            key = next_key(key, len);
            count[len as usize] -= 1;
        }
    }

    for len in root_bits + 1..=MAX_CODE_LENGTH as u32 {
        while count[len as usize] > 0 {
            if (key & TABLE_MASK) != low {
                let sub_bits = next_table_bits(&count, len, root_bits);

                sub += sub_size;
                sub_size = 1 << sub_bits;
                total += sub_size;
                if total > p.total_size {
                    return false;
                }
                low = key & TABLE_MASK;
                table[low as usize] =
                    entry(sub_bits + root_bits, (sub - low as usize) as u32);
                filled = 1;
                table[sub] = 0;
            }
            let span = 1usize << (len - root_bits);
            let slot = &mut table[sub..sub + span];

            double_to(slot, &mut filled, span);
            slot[(key >> root_bits) as usize] =
                entry(len - root_bits, u32::from(sorted[symbol]));
            symbol += 1;
            key = next_key(key, len);
            count[len as usize] -= 1;
        }
    }

    total == p.total_size
}

/// Builds one prefix code into `arena`, returning where it landed.
pub fn build(
    arena: &mut Vec<u32>,
    plan: &mut Plan,
    lengths: &[u8],
    sorted: &mut [u16],
) -> Result<Reader> {
    if !analyze(plan, lengths, sorted) {
        return Err(Error::InvalidData);
    }

    let start = arena.len();

    arena
        .try_reserve(plan.total_size)
        .map_err(|_| Error::NoMemory)?;
    arena.resize(start + plan.total_size, 0);

    if !fill(plan, &mut arena[start..], sorted) {
        return Err(Error::InvalidData);
    }
    Ok(Reader {
        start: start as u32,
        len: plan.total_size as u32,
        mask: (1u32 << plan.root_bits) - 1,
    })
}

/// The short form of a length list: one or two symbols, each of length one.
///
/// The two symbols may repeat, and the histogram has to stay an exact count of
/// the non-zero lengths for the counting sort to line up.
pub fn read_simple_code(
    br: &mut BitReader,
    buf: &[u8],
    plan: &mut Plan,
    lengths: &mut [u8],
) {
    let nb_symbols = br.bit(buf) + 1;
    let mark = |symbol: u32, plan: &mut Plan, lengths: &mut [u8]| {
        let symbol = symbol as usize;

        if symbol < lengths.len() && lengths[symbol] == 0 {
            lengths[symbol] = 1;
            plan.count[1] += 1;
        }
    };

    let first = if br.bit(buf) != 0 {
        br.bits(buf, 8)
    } else {
        br.bit(buf)
    };
    mark(first, plan, lengths);

    if nb_symbols == 2 {
        let second = br.bits(buf, 8);
        mark(second, plan, lengths);
    }
}

/// The long form: a prefix code over code lengths, then the lengths themselves.
pub fn read_normal_code(
    br: &mut BitReader,
    buf: &[u8],
    plan: &mut Plan,
    lengths: &mut [u8],
) -> Result<()> {
    /* Code lengths are 3 bits wide, so this table never needs a second level
    and is at most 128 entries wide, which is why it lives on the stack. */
    let mut arena = [0u32; 1 << MAX_CODE_LENGTH_CODE_LENGTH];
    let mut sorted = [0u16; NUM_CODE_LENGTH_CODES];
    let mut code_length_lengths = [0u8; NUM_CODE_LENGTH_CODES];
    let mut len_plan = Plan::default();
    let alphabet_size = lengths.len();
    let num_codes = 4 + br.bits(buf, 4) as usize;

    for &slot in CODE_LENGTH_CODE_ORDER.iter().take(num_codes) {
        code_length_lengths[usize::from(slot)] = br.bits(buf, 3) as u8;
    }

    let mut max_symbol = if br.bit(buf) != 0 {
        let bits = 2 + 2 * br.bits(buf, 3);
        let max = 2 + br.bits(buf, bits) as usize;

        if max > alphabet_size {
            crate::log::error(&format!(
                "max symbol {max} > alphabet size {alphabet_size}"
            ));
            return Err(Error::InvalidData);
        }
        max
    } else {
        alphabet_size
    };

    count_lengths(&mut len_plan, &code_length_lengths);
    if !analyze(&mut len_plan, &code_length_lengths, &mut sorted) {
        return Err(Error::InvalidData);
    }
    if len_plan.total_size > arena.len() {
        return Err(Error::InvalidData);
    }
    if !fill(&len_plan, &mut arena[..len_plan.total_size], &sorted) {
        return Err(Error::InvalidData);
    }
    let reader = Reader {
        start: 0,
        len: len_plan.total_size as u32,
        mask: (1u32 << len_plan.root_bits) - 1,
    };
    let tree = reader.tree(&arena);

    let mut prev_code_len = 8u8;
    let mut symbol = 0usize;

    while symbol < alphabet_size {
        if max_symbol == 0 {
            break;
        }
        max_symbol -= 1;
        if br.is_eos(buf) {
            break;
        }
        br.fill(buf);

        let code_len = tree.read(br);

        if code_len < 16 {
            lengths[symbol] = code_len as u8;
            symbol += 1;
            if code_len != 0 {
                prev_code_len = code_len as u8;
                plan.count[code_len as usize] += 1;
            }
            continue;
        }

        let (repeat, length) = match code_len {
            16 => ((3 + br.bits(buf, 2)) as usize, prev_code_len),
            17 => ((3 + br.bits(buf, 3)) as usize, 0),
            18 => ((11 + br.bits(buf, 7)) as usize, 0),
            _ => return Err(Error::InvalidData),
        };

        if symbol + repeat > alphabet_size {
            crate::log::error(&format!(
                "invalid symbol {symbol} + repeat {repeat} > alphabet size \
                 {alphabet_size}"
            ));
            return Err(Error::InvalidData);
        }
        /* The buffer arrives zeroed, so a run of zeros is just a skip. */
        if length != 0 {
            plan.count[usize::from(length)] += repeat as i32;
            lengths[symbol..symbol + repeat].fill(length);
        }
        symbol += repeat;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_from(lengths: &[u8]) -> Option<(Vec<u32>, Reader)> {
        let mut arena = Vec::new();
        let mut plan = Plan::default();
        let mut sorted = vec![0u16; lengths.len()];

        count_lengths(&mut plan, lengths);
        build(&mut arena, &mut plan, lengths, &mut sorted)
            .ok()
            .map(|r| (arena, r))
    }

    #[test]
    fn a_single_symbol_needs_no_bits() {
        let (arena, reader) = build_from(&[0, 1, 0, 0]).unwrap();
        let tree = reader.tree(&arena);
        let buf = [0u8; 8];
        let mut br = BitReader::new(&buf);

        assert_eq!(tree.read(&mut br), 1);
        assert_eq!(tree.only_symbol(), 1);
    }

    #[test]
    fn an_over_subscribed_code_is_rejected() {
        assert!(build_from(&[1, 1, 1]).is_none());
    }

    #[test]
    fn an_incomplete_code_is_rejected() {
        assert!(build_from(&[1, 2, 0, 0]).is_none());
    }

    #[test]
    fn a_balanced_code_round_trips() {
        let (arena, reader) = build_from(&[1, 2, 3, 3]).unwrap();
        let tree = reader.tree(&arena);
        /* Canonical codes, bit-reversed: 0 -> 0, 1 -> 01, 2 -> 011, 3 -> 111. */
        let buf = [0b0111_1010u8, 0b0000_0001, 0, 0, 0, 0, 0, 0];
        let mut br = BitReader::new(&buf);

        assert_eq!(tree.read(&mut br), 0);
        assert_eq!(tree.read(&mut br), 1);
        assert_eq!(tree.read(&mut br), 3);
    }
}
