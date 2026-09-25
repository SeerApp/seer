use std::ops::{Index, IndexMut};

use crate::parse::Operand;

use super::SymVal;

const NUM_REGISTERS: usize = 11;

/// SBPF register file plus the callee-saved stack for `call`/`callx`/`exit`.
pub(crate) struct Registers {
    file: [Option<SymVal>; NUM_REGISTERS],
    call_saved: Vec<[Option<SymVal>; 4]>,
}

impl Registers {
    pub(crate) fn new() -> Self {
        Self {
            file: std::array::from_fn(|_| None),
            call_saved: Vec::new(),
        }
    }

    pub(crate) fn copy(&mut self, dst: usize, src: usize) {
        self.file[dst] = self.file[src].clone();
    }

    pub(crate) fn clear(&mut self, i: usize) {
        self.file[i] = None;
    }

    /// SBPF `r6`–`r9` are callee-saved; the VM restores them on `exit`.
    /// `r0`–`r5` are caller-saved: `r0` is the callee's return value.
    pub(crate) fn push_call_frame(&mut self) {
        self.call_saved.push([
            self.file[6].clone(),
            self.file[7].clone(),
            self.file[8].clone(),
            self.file[9].clone(),
        ]);
    }

    pub(crate) fn pop_call_frame(&mut self) {
        if let Some([r6, r7, r8, r9]) = self.call_saved.pop() {
            self.file[6] = r6;
            self.file[7] = r7;
            self.file[8] = r8;
            self.file[9] = r9;
        }
    }

    /// Environmental operands of a binary ALU or branch.
    /// `None` if neither side is tracked (immediates never count).
    pub(crate) fn operands(
        &self,
        dst: usize,
        src: &Operand,
    ) -> Option<(Option<SymVal>, Option<SymVal>)> {
        let dst_sym = self.file[dst].clone();
        let src_sym = self.operand_sym(src);
        let dst_env = dst_sym.as_ref().is_some_and(|s| s.environmental);
        let src_env =
            matches!(src, Operand::Reg(_)) && src_sym.as_ref().is_some_and(|s| s.environmental);
        if dst_env || src_env {
            Some((dst_sym, src_sym))
        } else {
            None
        }
    }

    fn operand_sym(&self, op: &Operand) -> Option<SymVal> {
        match op {
            Operand::Reg(r) => self.file[*r].clone(),
            Operand::Imm(i) => Some(SymVal::from_u64(*i as u64)),
        }
    }
}

impl Index<usize> for Registers {
    type Output = Option<SymVal>;

    fn index(&self, i: usize) -> &Self::Output {
        &self.file[i]
    }
}

impl IndexMut<usize> for Registers {
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        &mut self.file[i]
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::SymVal;
    use super::*;
    use z3::ast::{Ast, BV};

    #[test]
    fn pop_restores_callee_saved_not_r0() {
        let mut regs = Registers::new();
        regs[7] = Some(SymVal::env(BV::new_const("w_exp", 64)));
        regs[0] = Some(SymVal::env(BV::new_const("w_ret", 64)));
        regs.push_call_frame();
        regs[7] = Some(SymVal::env(BV::new_const("w_clobbered", 64)));
        regs[0] = Some(SymVal::env(BV::new_const("w_callee_ret", 64)));
        regs.pop_call_frame();
        assert_eq!(regs[7].as_ref().unwrap().bv.decl().name(), "w_exp");
        assert_eq!(regs[0].as_ref().unwrap().bv.decl().name(), "w_callee_ret");
    }
}
