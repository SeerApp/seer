//! A contiguous bit-window of an opaque root. Concat lists are high-then-low.

use z3::ast::BV;

use crate::astwalk::ast_id;

#[derive(Clone)]
pub(crate) struct BitSlice {
    pub root: BV,
    pub hi: u32,
    pub lo: u32,
}

impl BitSlice {
    pub(crate) fn all(bv: &BV) -> Self {
        Self {
            root: bv.clone(),
            hi: bv.get_size() - 1,
            lo: 0,
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.hi - self.lo + 1
    }

    pub(crate) fn is_full(&self) -> bool {
        self.lo == 0 && self.hi + 1 == self.root.get_size()
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.root.as_u64() == Some(0)
    }
}

pub(crate) fn zero_slice(width: u32) -> BitSlice {
    BitSlice {
        root: BV::from_u64(0, width),
        hi: width - 1,
        lo: 0,
    }
}

pub(crate) fn slices_width(slices: &[BitSlice]) -> u32 {
    slices.iter().map(BitSlice::width).sum()
}

pub(crate) fn merge_adjacent_slices(slices: Vec<BitSlice>) -> Vec<BitSlice> {
    let mut out: Vec<BitSlice> = Vec::new();
    for s in slices {
        if let Some(prev) = out.last_mut() {
            if ast_id(&prev.root) == ast_id(&s.root) && prev.lo == s.hi + 1 {
                prev.lo = s.lo;
                continue;
            }
            // Glue adjacent numeral bytes into one constant.
            if prev.is_full() && s.is_full() {
                if let (Some(a), Some(b)) = (prev.root.as_u64(), s.root.as_u64()) {
                    let wa = prev.root.get_size();
                    let wb = s.root.get_size();
                    if wa + wb <= 64 {
                        prev.root = BV::from_u64((a << wb) | b, wa + wb);
                        prev.hi = wa + wb - 1;
                        prev.lo = 0;
                        continue;
                    }
                }
            }
        }
        out.push(s);
    }
    out
}

pub(crate) fn slice_window(slices: &[BitSlice], hi: u32, lo: u32) -> Option<Vec<BitSlice>> {
    let w: u32 = slices.iter().map(BitSlice::width).sum();
    if lo > hi || hi >= w {
        return None;
    }
    let mut skip = w - 1 - hi;
    let mut take = hi - lo + 1;
    let mut out = Vec::new();
    for s in slices {
        let sw = s.width();
        if skip >= sw {
            skip -= sw;
            continue;
        }
        let local_hi = s.hi - skip;
        skip = 0;
        let avail = local_hi - s.lo + 1;
        if take >= avail {
            out.push(BitSlice {
                root: s.root.clone(),
                hi: local_hi,
                lo: s.lo,
            });
            take -= avail;
            if take == 0 {
                break;
            }
        } else {
            out.push(BitSlice {
                root: s.root.clone(),
                hi: local_hi,
                lo: local_hi + 1 - take,
            });
            take = 0;
            break;
        }
    }
    if take != 0 || skip != 0 {
        return None;
    }
    Some(merge_adjacent_slices(out))
}

pub(crate) fn rebuild_slices(slices: &[BitSlice]) -> Option<BV> {
    let mut acc: Option<BV> = None;
    for s in slices {
        let piece = if s.is_full() {
            s.root.clone()
        } else {
            s.root.extract(s.hi, s.lo)
        };
        acc = Some(match acc {
            None => piece,
            Some(high) => high.concat(&piece),
        });
    }
    acc
}
