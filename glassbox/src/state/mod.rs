mod ledger;
mod memory;
mod registers;
mod symval;

use z3::ast::{BV, Bool};

use crate::parse::{BinOp, MemWidth, Operand, RelOp};

pub use ledger::Ledger;
pub use memory::Memory;
pub(crate) use registers::Registers;
pub use symval::SymVal;

pub use crate::ancestry::LoadDef;
pub(crate) use crate::path_condition::merge_origins;
pub use crate::path_condition::{PathClass, PathCondition, SysvarOrigin};

pub(crate) const WORD_BITS: u32 = 64;
pub(crate) const BYTE_BITS: u32 = 8;

/// Path conditions, load temps, and mint counts extracted from a finished
/// scratchpad. Registers and live memory are gone.
pub(crate) struct ScratchpadDump {
    pub ledger: Ledger,
    pub memory_cells: usize,
    pub text_bytes: usize,
    pub input_bytes: usize,
}

/// Working symbolic scratchpad: memory, registers, and the symbol/path ledger.
pub struct SymbolicState {
    pub(crate) memory: Memory,
    pub(crate) registers: Registers,
    pub(crate) ledger: Ledger,
}

impl Default for SymbolicState {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolicState {
    pub fn new() -> Self {
        Self {
            memory: Memory::new(),
            registers: Registers::new(),
            ledger: Ledger::new(),
        }
    }

    /// Consume the scratchpad into the pieces display needs. Registers and
    /// live memory are dropped.
    pub(crate) fn into_dump(self) -> ScratchpadDump {
        let (memory_cells, text_bytes, input_bytes) = self.memory.counts();
        ScratchpadDump {
            ledger: self.ledger,
            memory_cells,
            text_bytes,
            input_bytes,
        }
    }

    pub fn load(&mut self, dst: usize, addr: u64, width: MemWidth, concrete: u64) {
        self.registers[dst] = self
            .memory
            .load_bytes(addr, width, concrete)
            .map(|packed| self.name_pack(addr, width, packed));
    }

    /// Name a multi-byte pack as a `w_*` temp; leave numerals and single bytes.
    fn name_pack(&mut self, addr: u64, width: MemWidth, packed: SymVal) -> SymVal {
        let n = width.bytes();
        let packed = if let Some(v) = crate::rewrite::eval_numeral(&packed.bv) {
            packed.with_bv(BV::from_u64(v, packed.bv.get_size()))
        } else {
            packed
        };
        if packed.bv.as_u64().is_some() || n == 1 {
            return packed;
        }
        let abi_name = self.memory.word_symbol(addr, n);
        let mut v = self
            .ledger
            .mint_load_temp(packed.bv, packed.environmental, abi_name);
        merge_origins(&mut v.origins, &packed.origins);
        v
    }

    pub fn store_reg(&mut self, addr: u64, width: MemWidth, src: usize, concrete: u64) {
        if let Some(v) = self.registers[src].clone() {
            self.memory.store_from_sym(addr, width, &v);
        } else {
            self.memory.store_imm(addr, width, concrete);
        }
    }

    pub fn alu(&mut self, op: BinOp, dst: usize, src: &Operand, bits32: bool, pre: &[u64; 11]) {
        let Some((dst_sym, src_sym)) = self.registers.operands(dst, src) else {
            self.registers.clear(dst);
            return;
        };
        let a = dst_sym
            .as_ref()
            .map(|s| s.bv.clone())
            .unwrap_or_else(|| BV::from_u64(pre[dst], WORD_BITS));
        let b = src_sym
            .as_ref()
            .map(|s| s.bv.clone())
            .unwrap_or_else(|| BV::from_u64(src.concrete(pre), WORD_BITS));
        let mut r = op.apply(&a, &b);
        if bits32 {
            r = truncate32(r);
        }
        self.registers[dst] = Some(SymVal::combine(
            dst_sym.as_ref(),
            src_sym.as_ref(),
            crate::rewrite::alu(&r, &self.ledger.load_defs),
        ));
    }

    pub fn neg(&mut self, dst: usize, bits32: bool) {
        let Some(v) = self.registers[dst].clone() else {
            return;
        };
        if !v.environmental {
            return;
        }
        let mut r = v.bv.bvneg();
        if bits32 {
            r = truncate32(r);
        }
        self.registers[dst] = Some(v.with_bv(crate::rewrite::alu(&r, &self.ledger.load_defs)));
    }

    pub fn branch(
        &self,
        rel: RelOp,
        dst: usize,
        src: &Operand,
        taken: bool,
        pre: &[u64; 11],
        order: u64,
        pc: u64,
        disasm: String,
    ) -> Option<PathCondition> {
        let (dst_sym, src_sym) = self.registers.operands(dst, src)?;
        let mut origins = dst_sym
            .as_ref()
            .map(|s| s.origins.clone())
            .unwrap_or_default();
        if let Some(s) = &src_sym {
            merge_origins(&mut origins, &s.origins);
        }
        let a = dst_sym
            .as_ref()
            .map(|s| s.bv.clone())
            .unwrap_or_else(|| BV::from_u64(pre[dst], WORD_BITS));
        let b = src_sym
            .as_ref()
            .map(|s| s.bv.clone())
            .unwrap_or_else(|| BV::from_u64(src.concrete(pre), WORD_BITS));
        let cond = rel.apply(&a, &b);
        let formula = if taken { cond } else { cond.not() };
        Some(PathCondition {
            order,
            pc,
            disasm,
            taken,
            rel,
            lhs: pre[dst],
            rhs: src.concrete(pre),
            formula,
            origins,
        })
    }
}

impl BinOp {
    fn apply(self, a: &BV, b: &BV) -> BV {
        match self {
            Self::Add => a.bvadd(b),
            Self::Sub => a.bvsub(b),
            Self::Mul => a.bvmul(b),
            Self::Div => a.bvudiv(b),
            Self::Mod => a.bvurem(b),
            Self::And => a.bvand(b),
            Self::Or => a.bvor(b),
            Self::Xor => a.bvxor(b),
            Self::Lsh => a.bvshl(b),
            Self::Rsh => a.bvlshr(b),
            Self::Arsh => a.bvashr(b),
            Self::Mov => b.clone(),
        }
    }
}

impl RelOp {
    fn apply(self, a: &BV, b: &BV) -> Bool {
        match self {
            Self::Eq => a.eq(b),
            Self::Ne => a.eq(b).not(),
            Self::Gt => a.bvugt(b),
            Self::Ge => a.bvuge(b),
            Self::Lt => a.bvult(b),
            Self::Le => a.bvule(b),
            Self::Sgt => a.bvsgt(b),
            Self::Sge => a.bvsge(b),
            Self::Slt => a.bvslt(b),
            Self::Sle => a.bvsle(b),
        }
    }
}

fn truncate32(v: BV) -> BV {
    v.extract(31, 0).zero_ext(32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regions::INPUT_BASE;
    use z3::ast::BV;

    #[test]
    fn mint_names_num_accounts_only_lineage() {
        let mut state = SymbolicState::new();
        state.load(0, INPUT_BASE, MemWidth::Dw, 0);
        let na = state.registers[0].as_ref().unwrap().bv.clone();
        let child = state
            .ledger
            .mint_load_temp(na.bvmul(&BV::from_u64(0x30, 64)), true, None);
        assert_eq!(child.bv.to_string(), "w_num_accounts_0");

        let grandchild =
            state
                .ledger
                .mint_load_temp(child.bv.bvadd(&BV::from_u64(0x3000_0000, 64)), true, None);
        assert_eq!(grandchild.bv.to_string(), "w_num_accounts_1");

        let other = BV::new_const("n_acc0_dup", 8).zero_ext(56);
        let mixed = state.ledger.mint_load_temp(na.bvadd(&other), true, None);
        assert_eq!(mixed.bv.to_string(), "w_0");
    }

    #[test]
    fn mint_names_data_len_only_lineage() {
        let mut state = SymbolicState::new();
        state.load(0, INPUT_BASE + 88, MemWidth::Dw, 0);
        let dl = state.registers[0].as_ref().unwrap().bv.clone();
        assert_eq!(dl.to_string(), "w_acc0_data_len");

        let child = state
            .ledger
            .mint_load_temp(dl.bvadd(&BV::from_u64(0x20, 64)), true, None);
        assert_eq!(child.bv.to_string(), "w_acc0_data_len_0");

        let grandchild =
            state
                .ledger
                .mint_load_temp(child.bv.bvadd(&BV::from_u64(0x8, 64)), true, None);
        assert_eq!(grandchild.bv.to_string(), "w_acc0_data_len_1");

        state.load(0, INPUT_BASE + 10424, MemWidth::Dw, 0);
        let dl1 = state.registers[0].as_ref().unwrap().bv.clone();
        assert_eq!(dl1.to_string(), "w_acc1_data_len");
        let child1 = state
            .ledger
            .mint_load_temp(dl1.bvmul(&BV::from_u64(2, 64)), true, None);
        assert_eq!(child1.bv.to_string(), "w_acc1_data_len_0");

        let other = BV::new_const("n_acc0_dup", 8).zero_ext(56);
        let mixed = state.ledger.mint_load_temp(dl.bvadd(&other), true, None);
        assert_eq!(mixed.bv.to_string(), "w_0");

        let cross = state.ledger.mint_load_temp(dl.bvadd(&dl1), true, None);
        assert_eq!(cross.bv.to_string(), "w_1");
    }

    #[test]
    fn mint_names_signer_writable_executable_words() {
        let mut state = SymbolicState::new();
        let writable = BV::new_const("n_acc0_writable", 8).zero_ext(56);
        assert_eq!(
            state
                .ledger
                .mint_load_temp(writable, true, None)
                .bv
                .to_string(),
            "w_acc0_writable"
        );
        let signer = BV::new_const("n_acc1_signer", 8)
            .concat(&BV::from_u64(0, 8))
            .zero_ext(48);
        assert_eq!(
            state
                .ledger
                .mint_load_temp(signer, true, None)
                .bv
                .to_string(),
            "w_acc1_signer"
        );
        let exec = BV::new_const("n_acc7_executable", 8).zero_ext(56);
        let parent = state.ledger.mint_load_temp(exec, true, None);
        assert_eq!(parent.bv.to_string(), "w_acc7_executable");
        let child = state
            .ledger
            .mint_load_temp(parent.bv.bvadd(&BV::from_u64(1, 64)), true, None);
        assert_eq!(child.bv.to_string(), "w_acc7_executable_0");
    }

    #[test]
    fn mint_reuses_realigned_pda_word() {
        let mut state = SymbolicState::new();
        let pda = z3::FuncDecl::new(
            "uif_sol_create_program_address",
            &[&z3::Sort::bitvector(64)],
            &z3::Sort::bitvector(256),
        )
        .apply(&[&BV::from_u64(0, 64)])
        .as_bv()
        .unwrap();
        let w171 = state
            .ledger
            .mint_load_temp(pda.extract(255, 192), true, None);
        let w172 = state
            .ledger
            .mint_load_temp(pda.extract(191, 128), true, None);
        assert_eq!(w171.bv.to_string(), "w_pda_create0_3");
        assert_eq!(w172.bv.to_string(), "w_pda_create0_2");
        let shifted = w171.bv.extract(55, 0).concat(&w172.bv.extract(63, 56));
        let w177 = state.ledger.mint_load_temp(shifted, true, None);
        let realigned = w171.bv.extract(63, 56).concat(&w177.bv.extract(63, 8));
        let got = state.ledger.mint_load_temp(realigned, true, None);
        assert_eq!(got.bv.to_string(), "w_pda_create0_3");
        assert_eq!(state.ledger.load_defs.len(), 3);
    }

    #[test]
    fn mint_names_pda_find_and_create_words() {
        let mut state = SymbolicState::new();
        let create = z3::FuncDecl::new(
            "uif_sol_create_program_address",
            &[&z3::Sort::bitvector(64)],
            &z3::Sort::bitvector(256),
        )
        .apply(&[&BV::from_u64(0, 64)])
        .as_bv()
        .unwrap();
        let create2 = z3::FuncDecl::new(
            "uif_sol_create_program_address",
            &[&z3::Sort::bitvector(64)],
            &z3::Sort::bitvector(256),
        )
        .apply(&[&BV::from_u64(1, 64)])
        .as_bv()
        .unwrap();
        let find = z3::FuncDecl::new(
            "uif_sol_try_find_program_address",
            &[&z3::Sort::bitvector(64)],
            &z3::Sort::bitvector(264),
        )
        .apply(&[&BV::from_u64(2, 64)])
        .as_bv()
        .unwrap();
        assert_eq!(
            state
                .ledger
                .mint_load_temp(create.extract(63, 0), true, None)
                .bv
                .to_string(),
            "w_pda_create0_0"
        );
        assert_eq!(
            state
                .ledger
                .mint_load_temp(create.extract(255, 192), true, None)
                .bv
                .to_string(),
            "w_pda_create0_3"
        );
        assert_eq!(
            state
                .ledger
                .mint_load_temp(create2.extract(255, 192), true, None)
                .bv
                .to_string(),
            "w_pda_create1_3"
        );
        assert_eq!(
            state
                .ledger
                .mint_load_temp(find.extract(255, 192), true, None)
                .bv
                .to_string(),
            "w_pda_find0_3"
        );
        assert_eq!(
            state
                .ledger
                .mint_load_temp(find.extract(263, 256).zero_ext(56), true, None)
                .bv
                .to_string(),
            "w_pda_find0_bump"
        );
    }

    #[test]
    fn mint_does_not_name_folded_numerals() {
        let mut state = SymbolicState::new();
        let v = state.ledger.mint_load_temp(BV::from_u64(0, 64), true, None);
        assert!(!v.environmental);
        assert_eq!(v.bv.as_u64(), Some(0));
        assert!(state.ledger.load_defs.is_empty());

        let v = state.ledger.mint_load_temp(
            BV::from_u64(3, 64).bvmul(&BV::from_u64(0, 64)),
            true,
            None,
        );
        assert!(!v.environmental);
        assert_eq!(v.bv.as_u64(), Some(0));
        assert!(state.ledger.load_defs.is_empty());
    }
}
