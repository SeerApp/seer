//! Address space: one cell per byte, concrete observation and optional symbol.

use std::collections::HashMap;

use z3::ast::BV;

use crate::input_abi::{self, AccountSpan};
use crate::parse::MemWidth;
use crate::regions::{
    input_offset, input_symbol_name, is_input, is_text, text_symbol_name, INPUT_BASE,
};

use super::{merge_origins, SymVal, SysvarOrigin, BYTE_BITS, WORD_BITS};

struct SysvarRegion {
    base: u64,
    size: u64,
    origin: SysvarOrigin,
}

#[derive(Clone, Default)]
struct Cell {
    sym: Option<SymVal>,
    concrete: Option<u8>,
}

/// Virtual address → byte cell (trace observation and/or symbolic value).
pub struct Memory {
    cells: HashMap<u64, Cell>,
    /// Output buffers filled by `sol_get_*_sysvar`. A load in a region writes
    /// concrete bytes with that origin into [`Self::cells`], overwriting a
    /// prior store at the same address.
    sysvar_regions: Vec<SysvarRegion>,
    /// Pubkey → this instruction’s `acc{i}`. First packing wins.
    pubkey_to_acc: HashMap<[u8; 32], u32>,
    /// Current packing slot → instruction `acc{i}`. Empty means identity.
    slot_to_acc: Vec<u32>,
}

/// Caller input to restore on CPI return.
pub(crate) struct InputFrame {
    packing: Vec<AccountSpan>,
    parked_non_account: HashMap<u64, Cell>,
    slot_to_acc: Vec<u32>,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            cells: HashMap::new(),
            sysvar_regions: Vec::new(),
            pubkey_to_acc: HashMap::new(),
            slot_to_acc: Vec::new(),
        }
    }

    pub fn can_resolve(&self, addr: u64) -> bool {
        self.has_byte(addr) || is_text(addr) || is_input(addr)
    }

    pub fn has_byte(&self, addr: u64) -> bool {
        self.cells.get(&addr).is_some_and(|c| c.sym.is_some())
    }

    pub fn resolve_byte(&mut self, addr: u64) -> Option<SymVal> {
        if let Some(b) = self.cells.get(&addr).and_then(|c| c.sym.clone()) {
            return Some(b);
        }
        if is_text(addr) {
            return Some(self.mint_text(addr));
        }
        if is_input(addr) {
            return Some(self.mint_input(addr));
        }
        None
    }

    pub fn write_byte(&mut self, addr: u64, byte: SymVal) {
        debug_assert_eq!(byte.bv.get_size(), BYTE_BITS);
        self.cells.entry(addr).or_default().sym = Some(byte);
    }

    pub fn write_concrete_byte(&mut self, addr: u64, value: u8) {
        let cell = self.cells.entry(addr).or_default();
        cell.concrete = Some(value);
        cell.sym = Some(SymVal::concrete(BV::from_u64(value as u64, BYTE_BITS)));
    }

    pub fn write_fresh_unknown(&mut self, addr: u64) {
        self.write_byte(addr, SymVal::concrete(BV::fresh_const("w", BYTE_BITS)));
    }

    pub fn mark_sysvar_region(&mut self, base: u64, size: u64, syscall: &'static str, pc: u64) {
        self.sysvar_regions.push(SysvarRegion {
            base,
            size,
            origin: SysvarOrigin { syscall, pc },
        });
    }

    fn observe_input(&mut self, addr: u64, nbytes: usize, value: u64) {
        if !is_input(addr) {
            return;
        }
        for i in 0..nbytes {
            let b = ((value >> (8 * i)) & 0xff) as u8;
            self.cells
                .entry(addr.wrapping_add(i as u64))
                .or_default()
                .concrete = Some(b);
        }
    }

    fn input_concrete(&self, offset: u64) -> Option<u8> {
        self.cells
            .get(&INPUT_BASE.wrapping_add(offset))
            .and_then(|c| c.concrete)
    }

    fn sysvar_origin_at(&self, addr: u64) -> Option<SysvarOrigin> {
        self.sysvar_regions
            .iter()
            .rev()
            .find(|r| addr >= r.base && addr < r.base.wrapping_add(r.size))
            .map(|r| r.origin)
    }

    /// If `addr` sits in a sysvar buffer, write the concrete load into the map
    /// tagged with that syscall's origin.
    fn materialize_sysvar(&mut self, addr: u64, width: MemWidth, value: u64) {
        let Some(origin) = self.sysvar_origin_at(addr) else {
            return;
        };
        let n = width.bytes();
        for i in 0..n {
            let b = ((value >> (8 * i)) & 0xff) as u8;
            let cell = self.cells.entry(addr + i as u64).or_default();
            cell.concrete = Some(b);
            cell.sym = Some(SymVal {
                bv: BV::from_u64(b as u64, BYTE_BITS),
                environmental: true,
                origins: vec![origin],
            });
        }
    }

    pub fn word_symbol(&self, addr: u64, nbytes: usize) -> Option<String> {
        if is_input(addr) {
            let loc = self.field_at(input_offset(addr))?;
            input_abi::word_name_of(&loc, nbytes)
        } else {
            None
        }
    }

    /// Pack `width` bytes at `addr`. Notes the concrete value for input ABI
    /// naming and sysvar provenance. Does not mint `w_*` temps.
    pub(crate) fn load_bytes(
        &mut self,
        addr: u64,
        width: MemWidth,
        concrete: u64,
    ) -> Option<SymVal> {
        self.observe_input(addr, width.bytes(), concrete);
        self.materialize_sysvar(addr, width, concrete);
        let n = width.bytes();
        for i in 0..n {
            if !self.can_resolve(addr + i as u64) {
                return None;
            }
        }
        let mut bytes = Vec::with_capacity(n);
        for i in 0..n {
            bytes.push(self.resolve_byte(addr + i as u64)?);
        }
        let (pack, env, origins) = pack_le(&bytes);
        if pack.as_u64().is_some() {
            if origins.is_empty() {
                return Some(SymVal::concrete(pack));
            }
            return Some(SymVal {
                bv: pack,
                environmental: true,
                origins,
            });
        }
        Some(SymVal {
            bv: pack,
            environmental: env,
            origins,
        })
    }

    pub(crate) fn store_from_sym(&mut self, addr: u64, width: MemWidth, value: &SymVal) {
        let n = width.bytes();
        let v = if value.bv.get_size() < WORD_BITS {
            value.bv.zero_ext(WORD_BITS - value.bv.get_size())
        } else if value.bv.get_size() > WORD_BITS {
            value.bv.extract(WORD_BITS - 1, 0)
        } else {
            value.bv.clone()
        };
        for i in 0..n {
            let byte = v
                .bvlshr(&BV::from_u64((i * 8) as u64, WORD_BITS))
                .extract(7, 0);
            self.write_byte(
                addr + i as u64,
                SymVal {
                    bv: byte,
                    environmental: value.environmental,
                    origins: value.origins.clone(),
                },
            );
        }
    }

    pub(crate) fn store_imm(&mut self, addr: u64, width: MemWidth, value: u64) {
        let n = width.bytes();
        for i in 0..n {
            let b = ((value >> (8 * i)) & 0xff) as u8;
            self.write_concrete_byte(addr + i as u64, b);
        }
    }

    pub(crate) fn counts(&self) -> (usize, usize, usize) {
        let minted = self.cells.iter().filter(|(_, c)| c.sym.is_some());
        let mut memory_cells = 0;
        let mut text_bytes = 0;
        let mut input_bytes = 0;
        for (&addr, _) in minted {
            memory_cells += 1;
            if is_text(addr) {
                text_bytes += 1;
            } else if is_input(addr) {
                input_bytes += 1;
            }
        }
        (memory_cells, text_bytes, input_bytes)
    }

    fn mint_text(&mut self, addr: u64) -> SymVal {
        let name = text_symbol_name(addr);
        let v = SymVal::env(BV::new_const(name, BYTE_BITS));
        self.cells.entry(addr).or_default().sym = Some(v.clone());
        v
    }

    fn mint_input(&mut self, addr: u64) -> SymVal {
        let name = self
            .field_at(input_offset(addr))
            .map(|loc| input_abi::byte_name_of(&loc))
            .unwrap_or_else(|| input_symbol_name(addr));
        let v = SymVal::env(BV::new_const(name, BYTE_BITS));
        self.cells.entry(addr).or_default().sym = Some(v.clone());
        v
    }

    fn field_at(&self, offset: u64) -> Option<input_abi::FieldLoc> {
        let loc = input_abi::classify(|o| self.input_concrete(o), offset)?;
        Some(input_abi::remap_acc(loc, |slot| self.acc_of(slot)))
    }

    fn acc_of(&self, slot: u32) -> u32 {
        self.slot_to_acc.get(slot as usize).copied().unwrap_or(slot)
    }

    pub(crate) fn walk_packing(&self) -> Option<Vec<AccountSpan>> {
        input_abi::walk_packing(|o| self.input_concrete(o))
    }

    fn walk_packing_overlay(&self, overlay: &HashMap<u64, u8>) -> Option<Vec<AccountSpan>> {
        input_abi::walk_packing(|o| overlay.get(&o).copied().or_else(|| self.input_concrete(o)))
    }

    fn learn_pubkeys(&mut self, spans: &[AccountSpan]) {
        for s in spans {
            if let Some(pk) = s.pubkey {
                self.pubkey_to_acc.entry(pk).or_insert(s.slot);
            }
        }
    }

    fn assign_acc(&self, mut spans: Vec<AccountSpan>) -> Vec<AccountSpan> {
        for s in &mut spans {
            if let Some(pk) = s.pubkey {
                if let Some(&acc) = self.pubkey_to_acc.get(&pk) {
                    s.acc = acc;
                }
            }
        }
        spans
    }

    fn park_non_account(&mut self, packing: &[AccountSpan]) -> HashMap<u64, Cell> {
        let mut keep = std::collections::HashSet::new();
        for s in packing.iter().filter(|s| s.unique) {
            for i in 0..s.len {
                keep.insert(s.start.saturating_add(i));
            }
        }
        let mut parked = HashMap::new();
        self.cells.retain(|&addr, cell| {
            if is_input(addr) && !keep.contains(&input_offset(addr)) {
                parked.insert(addr, cell.clone());
                false
            } else {
                true
            }
        });
        parked
    }

    fn place_accounts(&mut self, from: &[AccountSpan], to: &[AccountSpan]) {
        let mut src = HashMap::new();
        for s in from.iter().filter(|s| s.unique) {
            src.insert(s.acc, (s.start, s.len));
        }
        let mut bag = HashMap::new();
        for t in to.iter().filter(|t| t.unique) {
            let Some(&(start, len)) = src.get(&t.acc) else {
                continue;
            };
            let n = len.min(t.len);
            for i in 0..n {
                if let Some(cell) = self
                    .cells
                    .remove(&(INPUT_BASE.wrapping_add(start).wrapping_add(i)))
                {
                    bag.insert(INPUT_BASE.wrapping_add(t.start).wrapping_add(i), cell);
                }
            }
        }
        self.cells.extend(bag);
    }

    /// Park caller packing, place existing `acc{i}` cells at the callee blob.
    pub(crate) fn enter_cpi(&mut self, overlay: &HashMap<u64, u8>) -> InputFrame {
        let from = self.walk_packing().unwrap_or_default();
        if self.pubkey_to_acc.is_empty() {
            self.learn_pubkeys(&from);
        }
        let from = self.assign_acc(from);
        let parked_non_account = self.park_non_account(&from);
        let slot_to_acc = std::mem::take(&mut self.slot_to_acc);
        if let Some(to) = self.walk_packing_overlay(overlay) {
            let to = self.assign_acc(to);
            self.place_accounts(&from, &to);
            self.slot_to_acc = to.iter().map(|s| s.acc).collect();
        }
        InputFrame {
            packing: from,
            parked_non_account,
            slot_to_acc,
        }
    }

    pub(crate) fn return_cpi(&mut self, frame: InputFrame) {
        if let Some(cur) = self.walk_packing() {
            let cur = self.assign_acc(cur);
            let _ = self.park_non_account(&cur);
            self.place_accounts(&cur, &frame.packing);
        }
        self.cells.extend(frame.parked_non_account);
        self.slot_to_acc = frame.slot_to_acc;
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

fn pack_le(bytes: &[SymVal]) -> (BV, bool, Vec<SysvarOrigin>) {
    assert!(!bytes.is_empty());
    let mut env = false;
    let mut origins = Vec::new();
    if bytes.len() == 1 {
        env = bytes[0].environmental;
        merge_origins(&mut origins, &bytes[0].origins);
        return (bytes[0].bv.zero_ext(WORD_BITS - 8), env, origins);
    }

    let mut word = bytes[0].bv.clone();
    env |= bytes[0].environmental;
    merge_origins(&mut origins, &bytes[0].origins);
    for b in bytes.iter().skip(1) {
        env |= b.environmental;
        merge_origins(&mut origins, &b.origins);
        word = b.bv.concat(&word);
    }
    if word.get_size() < WORD_BITS {
        word = word.zero_ext(WORD_BITS - word.get_size());
    } else if word.get_size() > WORD_BITS {
        word = word.extract(WORD_BITS - 1, 0);
    }
    (word, env, origins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::MemWidth;

    #[test]
    fn observed_input_byte_can_exist_without_a_symbol() {
        let mut mem = Memory::new();
        mem.observe_input(INPUT_BASE + 88, 8, 0);
        let cell = mem.cells.get(&(INPUT_BASE + 88)).unwrap();
        assert_eq!(cell.concrete, Some(0));
        assert!(cell.sym.is_none());
        assert_eq!(
            mem.word_symbol(INPUT_BASE + 10424, 8).as_deref(),
            Some("w_acc1_data_len")
        );

        let packed = mem
            .load_bytes(INPUT_BASE + 88, MemWidth::Dw, 0)
            .expect("data_len");
        assert!(packed.environmental);
        let cell = mem.cells.get(&(INPUT_BASE + 88)).unwrap();
        assert_eq!(cell.concrete, Some(0));
        assert!(cell
            .sym
            .as_ref()
            .is_some_and(|s| s.bv.to_string().contains("n_acc0_data_len")));
    }

    fn observe_bytes(mem: &mut Memory, offset: u64, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            mem.observe_input(INPUT_BASE + offset + i as u64, 1, *b as u64);
        }
    }

    #[test]
    fn cpi_reuses_caller_acc_cells() {
        let mut mem = Memory::new();
        mem.observe_input(INPUT_BASE, 8, 3);
        mem.observe_input(INPUT_BASE + 88, 8, 0);
        mem.observe_input(INPUT_BASE + 10424, 8, 0);
        let acc2 = 20680u64;
        mem.observe_input(INPUT_BASE + acc2 + 80, 8, 0);
        observe_bytes(&mut mem, 16, &[0xaa; 32]);
        observe_bytes(&mut mem, 10352, &[0xbb; 32]);
        observe_bytes(&mut mem, acc2 + 8, &[0xcc; 32]);

        let caller = mem.walk_packing().expect("caller packing");
        assert_eq!(caller[2].start, acc2);
        let lamports = INPUT_BASE + acc2 + 72;
        let before = mem
            .load_bytes(lamports, MemWidth::Dw, 0)
            .expect("caller acc2 lamports");
        let before_name = mem.resolve_byte(lamports).unwrap().bv.to_string();
        assert!(before_name.contains("n_acc2_lamports"), "{before_name}");

        let mut overlay = HashMap::new();
        for i in 0..8u64 {
            overlay.insert(i, if i == 0 { 1 } else { 0 });
            overlay.insert(88 + i, 0);
        }
        overlay.insert(8, input_abi::NON_DUP_MARKER);
        for i in 0..32u64 {
            overlay.insert(16 + i, 0xcc);
        }

        let frame = mem.enter_cpi(&overlay);
        let callee_lamports = INPUT_BASE + 80;
        let got = mem
            .resolve_byte(callee_lamports)
            .expect("placed caller cell")
            .bv
            .to_string();
        assert!(
            got.contains("n_acc2_lamports"),
            "callee slot 0 must still be acc2: {got}"
        );
        assert!(!got.contains("n_acc0_lamports"), "{got}");
        assert_eq!(got, before_name, "same cell, not a second mint");
        let packed = mem
            .load_bytes(callee_lamports, MemWidth::Dw, 0)
            .expect("callee load");
        assert_eq!(packed.bv.to_string(), before.bv.to_string());

        mem.return_cpi(frame);
        let back = mem.resolve_byte(lamports).expect("restored").bv.to_string();
        assert_eq!(back, before_name);
    }
}
