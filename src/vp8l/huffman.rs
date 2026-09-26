use super::bitreader::{BitReader, Fast};
use crate::error::{Error, Result};

pub const MAX_CODE_LENGTH: usize = 15;
const NUM_CODE_LENGTH_CODES: usize = 19;
const MAX_CODE_LENGTH_CODE_LENGTH: usize = 7;

pub const TABLE_BITS: u32 = 8;

const CODE_LENGTH_CODE_ORDER: [u8; NUM_CODE_LENGTH_CODES] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

#[derive(Clone, Copy, Default)]
pub struct Reader {
    start: u32,
    len: u32,
    pub mask: u32,
}

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

    /// `Tree::read` for a `Fast` reader. It finds the table in `arena` at
    /// each read, which keeps the five tables of a group out of registers.
    /// With `REFILL`, the refill goes between the lookup and the shift that
    /// consumes the code, off the chain from one code to the next: the lookup
    /// indexes with the bits already there, of which there are at least 15.
    #[inline(always)]
    pub fn read_fast<const REFILL: bool>(
        &self,
        arena: &[u32],
        f: &mut Fast,
        buf: &[u8],
    ) -> u32 {
        let table = &arena[self.start as usize..];

        // A single symbol takes no bits, and branching on it takes its lookup
        // off the chain of codes.
        if self.mask == 0 {
            if REFILL {
                f.refill(buf);
            }
            return table[0] >> 8;
        }
        let val = f.peek() as usize;
        let mut index = val & self.mask as usize;
        let mut entry = table[index];
        let bits = entry & 0xFF;

        if bits > MAX_CODE_LENGTH as u32 {
            let root_bits = (entry >> 8) & 0xF;

            index += (entry >> 12) as usize
                + ((val >> root_bits) & ((1 << (bits & 0xF)) - 1));
            entry = table[index];
        }
        if REFILL {
            f.refill(buf);
        }
        f.consume_entry(entry);
        entry >> 8
    }
}

impl Tree<'_> {
    #[inline(always)]
    pub fn read(&self, br: &mut BitReader) -> u32 {
        let mut index = (br.prefetch() as usize) & (self.root.len() - 1);
        let mut entry = self.root[index];
        let mut bits = entry & 0xFF;

        if bits > MAX_CODE_LENGTH as u32 {
            let root_bits = (entry >> 8) & 0xF;

            br.advance(root_bits as i32);
            let val = br.prefetch();

            index +=
                (entry >> 12) as usize + (val & ((1 << (bits & 0xF)) - 1)) as usize;
            entry = self.full[index];
            bits = (entry & 0xFF) - root_bits;
        }
        br.advance(bits as i32);
        entry >> 8
    }

    pub fn only_symbol(&self) -> u8 {
        (self.full[0] >> 8) as u8
    }
}

pub struct Plan {
    pub count: [i32; MAX_CODE_LENGTH + 1],
    num_symbols: i32,
    max_root_bits: u32,
    root_bits: u32,
    total_size: usize,
}

impl Default for Plan {
    fn default() -> Self {
        Self::with_root_bits(TABLE_BITS)
    }
}

impl Plan {
    /// A plan whose root table is indexed by up to `bits` bits, rather than
    /// `TABLE_BITS`.
    pub fn with_root_bits(bits: u32) -> Self {
        Self {
            count: [0; MAX_CODE_LENGTH + 1],
            num_symbols: 0,
            max_root_bits: bits,
            root_bits: 0,
            total_size: 0,
        }
    }
}

const fn entry(bits: u32, value: u32) -> u32 {
    bits | value << 8
}

/// The root entry for codes longer than the root: `offset` from it to a table
/// of their `sub_bits` further bits. Its length is above any code's, which is
/// how a reader tells it apart, and it carries the root's length because that
/// depends on the tree.
const fn link(sub_bits: u32, root_bits: u32, offset: usize) -> u32 {
    entry(16 | sub_bits, root_bits | (offset as u32) << 4)
}

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
    let root_mask = (1u32 << p.root_bits) - 1;

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
            if (key & root_mask) != low {
                total += 1 << next_table_bits(&count, len, p.root_bits);
                low = key & root_mask;
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

/// The share of a code's space, out of `1 << MAX_CODE_LENGTH`, that codes
/// longer than `bits` take: about how often a code needs more than a root of
/// `bits` bits.
fn long_share(count: &[i32; MAX_CODE_LENGTH + 1], bits: u32) -> i32 {
    (bits as usize + 1..=MAX_CODE_LENGTH)
        .map(|len| count[len] << (MAX_CODE_LENGTH - len))
        .sum()
}

const LONG_SHARE: i32 = 1 << (MAX_CODE_LENGTH - 6);

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

    let mut seen = 0usize;

    #[allow(clippy::needless_range_loop)]
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
    while p.root_bits < p.max_root_bits.min(max_len)
        && long_share(&p.count, p.root_bits) > LONG_SHARE
    {
        p.root_bits += 1;
    }
    p.total_size = table_size(p);
    true
}

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
    let sorted = &sorted[..p.num_symbols.max(1) as usize];
    let mut count = p.count;
    let mut key = 0u32;
    let mut low = 0xFFFF_FFFFu32;
    let mut sub = 0usize;
    let root_bits = p.root_bits;
    let root_mask = (1u32 << root_bits) - 1;
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
            if (key & root_mask) != low {
                let sub_bits = next_table_bits(&count, len, root_bits);

                sub += sub_size;
                sub_size = 1 << sub_bits;
                total += sub_size;
                if total > p.total_size {
                    return false;
                }
                low = key & root_mask;
                table[low as usize] = link(sub_bits, root_bits, sub - low as usize);
                filled = 1;
                table[sub] = 0;
            }
            let span = 1usize << (len - root_bits);
            let slot = &mut table[sub..sub + span];

            double_to(slot, &mut filled, span);
            // The whole length, not what is left of it past the root, so that
            // `read_fast` consumes by the entry as it comes out of the table.
            slot[(key >> root_bits) as usize] = entry(len, u32::from(sorted[symbol]));
            symbol += 1;
            key = next_key(key, len);
            count[len as usize] -= 1;
        }
    }

    total == p.total_size
}

pub fn validate(plan: &mut Plan, lengths: &[u8], sorted: &mut [u16]) -> Result<()> {
    if analyze(plan, lengths, sorted) {
        Ok(())
    } else {
        Err(Error::InvalidData)
    }
}

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

    if start + plan.total_size > u32::MAX as usize {
        return Err(Error::NoMemory);
    }
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

/// Bits that index the tables built by `build_packed` and `build_fused`.
pub const PACKED_BITS: u32 = 8;

/// Appends a table that decodes the red, blue and alpha codes of a literal in
/// one lookup, for the next `PACKED_BITS` of the stream. An entry holds, from
/// its low byte up, the bits the three codes take, red, alpha and blue, or is
/// zero where they take more than `PACKED_BITS`.
pub fn build_packed(arena: &mut Vec<u32>, codes: [Reader; 3]) -> Result<u32> {
    let start = arena.len();
    let size = 1usize << PACKED_BITS;

    if start + size > u32::MAX as usize {
        return Err(Error::NoMemory);
    }
    arena.try_reserve(size).map_err(|_| Error::NoMemory)?;
    arena.resize(start + size, 0);

    let (tables, packed) = arena.split_at_mut(start);
    let roots = codes.map(|c| &tables[c.start as usize..][..=c.mask as usize]);

    for (i, slot) in packed.iter_mut().enumerate() {
        let mut val = i;
        let mut used = 0;
        let mut symbols = [0u8; 3];

        for (symbol, root) in symbols.iter_mut().zip(roots) {
            let entry = root[val & (root.len() - 1)];
            let bits = entry & 0xFF;

            used += bits;
            val >>= bits.min(PACKED_BITS);
            *symbol = (entry >> 8) as u8;
        }
        if used <= PACKED_BITS {
            let [r, b, a] = symbols;

            *slot = used | u32::from(r) << 8 | u32::from(a) << 16 | u32::from(b) << 24;
        }
    }
    Ok(start as u32)
}

/// Appends a table that decodes a whole literal in one lookup, for the next
/// `PACKED_BITS` of the stream, from the green code and the table
/// `build_packed` made at `packed`. An entry is the pixel with the bits its
/// codes take in place of alpha, which has to have a single symbol, or zero
/// where green is not a literal or the codes take more than `PACKED_BITS`.
///
/// A literal the table cannot decode costs a mispredicted branch on top of
/// the lookups it takes anyway, so the table is kept only if it decodes
/// three quarters of the code space.
pub fn build_fused(
    arena: &mut Vec<u32>,
    green: Reader,
    packed: u32,
) -> Result<Option<u32>> {
    let start = arena.len();
    let size = 1usize << PACKED_BITS;

    if start + size > u32::MAX as usize {
        return Err(Error::NoMemory);
    }
    arena.try_reserve(size).map_err(|_| Error::NoMemory)?;
    arena.resize(start + size, 0);

    let (tables, fused) = arena.split_at_mut(start);
    let root = &tables[green.start as usize..][..=green.mask as usize];
    let packed = &tables[packed as usize..][..size];

    for (i, slot) in fused.iter_mut().enumerate() {
        let entry = root[i & (root.len() - 1)];
        let bits = entry & 0xFF;
        let g = entry >> 8;

        if bits > PACKED_BITS || g >= 256 {
            continue;
        }

        let rba = packed[i >> bits];
        let used = bits + (rba & 0xFF);

        if rba != 0 && used <= PACKED_BITS {
            *slot = used | (rba & 0xFF00) | g << 16 | (rba & 0xFF00_0000);
        }
    }
    if fused.iter().filter(|&&e| e != 0).count() < size * 3 / 4 {
        arena.truncate(start);
        return Ok(None);
    }
    Ok(Some(start as u32))
}

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

pub fn read_normal_code(
    br: &mut BitReader,
    buf: &[u8],
    plan: &mut Plan,
    lengths: &mut [u8],
) -> Result<()> {
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
            crate::log::error_args(format_args!(
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
            crate::log::error_args(format_args!(
                "invalid symbol {symbol} + repeat {repeat} > alphabet size \
                 {alphabet_size}"
            ));
            return Err(Error::InvalidData);
        }
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
    use super::super::bitreader::FAST_MARGIN;
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
    fn validation_agrees_with_building_without_a_table() {
        for lengths in [
            &[0u8, 1, 0, 0][..],
            &[1, 2, 3, 3],
            &[1, 1, 1],
            &[1, 2, 0, 0],
        ] {
            let mut plan = Plan::default();
            let mut sorted = vec![0u16; lengths.len()];

            count_lengths(&mut plan, lengths);

            let validated = validate(&mut plan, lengths, &mut sorted).is_ok();

            assert_eq!(validated, build_from(lengths).is_some(), "{lengths:?}");
        }
    }

    #[test]
    fn a_balanced_code_round_trips() {
        let (arena, reader) = build_from(&[1, 2, 3, 3]).unwrap();
        let tree = reader.tree(&arena);
        let buf = [0b0111_1010u8, 0b0000_0001, 0, 0, 0, 0, 0, 0];
        let mut br = BitReader::new(&buf);

        assert_eq!(tree.read(&mut br), 0);
        assert_eq!(tree.read(&mut br), 1);
        assert_eq!(tree.read(&mut br), 3);
    }

    #[test]
    fn a_packed_entry_is_the_three_codes_read_in_turn() {
        let mut arena = Vec::new();
        let codes: [&[u8]; 3] =
            [&[1, 2, 3, 3], &[1, 2, 3, 4, 5, 6, 7, 8, 9, 9], &[0, 0, 1]];
        let readers = codes.map(|lengths| {
            let mut plan = Plan::default();
            let mut sorted = vec![0u16; lengths.len()];

            count_lengths(&mut plan, lengths);
            build(&mut arena, &mut plan, lengths, &mut sorted).unwrap()
        });
        let at = build_packed(&mut arena, readers).unwrap() as usize;
        let trees = readers.map(|r| r.tree(&arena));
        let mut packed = 0;

        for i in 0..1usize << PACKED_BITS {
            let buf = (i as u64 | 0xA5A5 << PACKED_BITS).to_le_bytes();
            let mut br = BitReader::new(&buf);
            let mut used = 0;
            let [r, b, a] = [0, 1, 2].map(|k| {
                let symbol = trees[k].read(&mut br) as usize;

                if readers[k].mask != 0 {
                    used += u32::from(codes[k][symbol]);
                }
                symbol as u8
            });
            let want = if used <= PACKED_BITS {
                packed += 1;
                u32::from_le_bytes([used as u8, r, a, b])
            } else {
                0
            };

            assert_eq!(arena[at + i], want, "index {i:#x}");
        }
        assert!(packed > 0 && packed < 1 << PACKED_BITS);
    }

    #[test]
    fn a_wider_root_reads_the_same_symbols() {
        // Half the code space in 7 bits, the rest in ever longer codes, so
        // that a root of 8 bits leaves a second lookup for half the codes.
        let lengths: Vec<u8> = [(7, 64), (9, 128), (11, 256), (13, 512), (15, 2048)]
            .iter()
            .flat_map(|&(len, n)| [len; 1].repeat(n))
            .collect();
        let mut arena = Vec::new();
        let readers = [8, 9, 11].map(|bits| {
            let mut plan = Plan::with_root_bits(bits);
            let mut sorted = vec![0u16; lengths.len()];

            count_lengths(&mut plan, &lengths);
            build(&mut arena, &mut plan, &lengths, &mut sorted).unwrap()
        });
        let buf: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();

        assert_eq!(readers.map(|r| r.mask), [0xFF, 0x1FF, 0x7FF]);

        let mut want = BitReader::new(&buf);
        let mut slow = readers.map(|_| BitReader::new(&buf));
        let mut fast = readers.map(|_| BitReader::new(&buf).fast(&buf).unwrap());

        for i in 0.. {
            if fast.iter().any(|f| f.pos() + FAST_MARGIN > buf.len()) {
                assert!(i > 1000);
                break;
            }
            want.fill(&buf);

            let symbol = readers[0].tree(&arena).read(&mut want);

            for k in 0..readers.len() {
                slow[k].fill(&buf);
                assert_eq!(
                    readers[k].tree(&arena).read(&mut slow[k]),
                    symbol,
                    "read {i}"
                );
                assert_eq!(
                    readers[k].read_fast::<true>(&arena, &mut fast[k], &buf),
                    symbol,
                    "read {i}"
                );
            }
        }
    }

    /// Builds green, red, blue and alpha codes, with green's literals taking
    /// three quarters of its code space, or half without `three_quarters`,
    /// and the fused table over them.
    fn build_fused_from(three_quarters: bool) -> (Vec<u32>, [Reader; 4], Option<u32>) {
        let mut green = vec![0u8; 280];

        if three_quarters {
            [green[0], green[1], green[256], green[257]] = [1, 2, 3, 3];
        } else {
            [green[0], green[256], green[257]] = [1, 2, 2];
        }

        let mut arena = Vec::new();
        let codes: [&[u8]; 4] = [&green, &[1, 2, 3, 3], &[1, 1], &[0, 0, 1]];
        let readers = codes.map(|lengths| {
            let mut plan = Plan::default();
            let mut sorted = vec![0u16; lengths.len()];

            count_lengths(&mut plan, lengths);
            build(&mut arena, &mut plan, lengths, &mut sorted).unwrap()
        });
        let [g, r, b, a] = readers;
        let packed = build_packed(&mut arena, [r, b, a]).unwrap();
        let len = arena.len();
        let fused = build_fused(&mut arena, g, packed).unwrap();

        assert_eq!(arena.len(), len + fused.map_or(0, |_| 1 << PACKED_BITS));
        (arena, readers, fused)
    }

    #[test]
    fn a_fused_entry_is_the_literal_read_code_by_code() {
        let (arena, readers, at) = build_fused_from(true);
        let at = at.unwrap() as usize;
        let codes = [[1u32, 2, 3, 3], [1, 1, 0, 0], [0; 4]];
        let trees = readers.map(|r| r.tree(&arena));
        let mut fused = 0;

        for i in 0..1usize << PACKED_BITS {
            let buf = (i as u64 | 0xA5A5 << PACKED_BITS).to_le_bytes();
            let mut br = BitReader::new(&buf);
            let [g, r, b, a] = trees.map(|t| t.read(&mut br));
            let green_bits = match g {
                0 => 1,
                1 => 2,
                _ => 3,
            };
            let used = green_bits + codes[0][r as usize] + codes[1][b as usize];
            let want = if g < 256 && used <= PACKED_BITS {
                fused += 1;
                u32::from_le_bytes([used as u8, r as u8, g as u8, b as u8])
            } else {
                0
            };

            assert_eq!(a, 2);
            assert_eq!(arena[at + i], want, "index {i:#x}");
        }
        assert_eq!(fused, 3 << (PACKED_BITS - 2));
    }

    #[test]
    fn a_fused_table_that_decodes_too_little_is_dropped() {
        assert!(build_fused_from(false).2.is_none());
    }
}
