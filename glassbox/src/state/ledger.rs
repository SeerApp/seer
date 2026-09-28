//! Named environmental formulas: load temps and the path conditions over them.
//!
//! This is where packs are minted, ALU/branch formulas are rewritten, and
//! kept path conditions are recorded. Memory and registers are not here.

use std::collections::{HashMap, HashSet};

use z3::ast::{Ast, BV};

use crate::ancestry::{
    pack_is_num_accounts_child, pack_pure_acc_flag, pack_pure_data_len_account, AccFlag,
};
use crate::astwalk::ast_id;
use crate::path_condition::PathCondition;

use super::{LoadDef, SymVal, WORD_BITS};

/// Load-temp dictionary plus recorded path conditions.
pub struct Ledger {
    pub path_conditions: Vec<PathCondition>,
    pub load_defs: Vec<LoadDef>,
    load_temp_counter: u64,
    num_accounts_child_counter: u64,
    data_len_child_counters: HashMap<u32, u64>,
    pda_find_counter: u32,
    pda_create_counter: u32,
    pda_ids: HashMap<usize, (crate::rewrite::PdaSyscall, u32)>,
    acc_flag_child_counters: HashMap<(u32, AccFlag), u64>,
    load_def_names: HashSet<String>,
    load_expr_ids: HashMap<usize, String>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            path_conditions: Vec::new(),
            load_defs: Vec::new(),
            load_temp_counter: 0,
            num_accounts_child_counter: 0,
            data_len_child_counters: HashMap::new(),
            pda_find_counter: 0,
            pda_create_counter: 0,
            pda_ids: HashMap::new(),
            acc_flag_child_counters: HashMap::new(),
            load_def_names: HashSet::new(),
            load_expr_ids: HashMap::new(),
        }
    }

    pub fn assert_path(&mut self, pc: PathCondition) {
        self.path_conditions.push(pc);
    }

    /// Introduce a load temporary for a multi-byte pack, reusing an existing
    /// temp when the pack is already named or identical to a prior def.
    /// `abi_name` is used for input-ABI fields (`w_num_accounts`, `w_acc0_data_len`, …).
    pub fn mint_load_temp(
        &mut self,
        pack_expr: BV,
        environmental: bool,
        abi_name: Option<String>,
    ) -> SymVal {
        if !environmental {
            return SymVal {
                bv: pack_expr,
                environmental: false,
                origins: Vec::new(),
            };
        }

        let pack_expr = crate::rewrite::pack(&pack_expr, &self.load_defs);
        // Taint can remain after the formula folds to a numeral (`w * 0`).
        // Naming that numeral `w_k` makes `w_k = 0` look like a real constraint.
        if pack_expr.as_u64().is_some() {
            return SymVal::concrete(pack_expr);
        }
        if let Some(reused) = self.reuse_load_temp(&pack_expr) {
            return SymVal {
                bv: reused,
                environmental: true,
                origins: Vec::new(),
            };
        }

        let pda_name = self.pda_word_name(&pack_expr);
        let flag_name = self.acc_flag_word_name(&pack_expr);
        let name = match abi_name {
            Some(n) if !self.load_def_names.contains(&n) => n,
            _ if pda_name.is_some() => pda_name.unwrap(),
            _ if flag_name.is_some() => flag_name.unwrap(),
            _ if pack_is_num_accounts_child(&pack_expr, &self.load_defs) => {
                let n = crate::grammar::num_accounts_child(self.num_accounts_child_counter);
                self.num_accounts_child_counter += 1;
                n
            }
            _ => {
                if let Some(acc) = pack_pure_data_len_account(&pack_expr, &self.load_defs) {
                    let k = self.data_len_child_counters.entry(acc).or_insert(0);
                    let n = crate::grammar::acc_data_len_child(acc, *k);
                    *k += 1;
                    n
                } else {
                    let n = crate::grammar::generic_word(self.load_temp_counter);
                    self.load_temp_counter += 1;
                    n
                }
            }
        };
        self.remember_load_def(name.clone(), &pack_expr);
        let temp = BV::new_const(name.as_str(), WORD_BITS);
        self.load_defs.push(LoadDef {
            name,
            expr: pack_expr,
        });
        SymVal {
            bv: temp,
            environmental: true,
            origins: Vec::new(),
        }
    }

    fn acc_flag_word_name(&mut self, pack_expr: &BV) -> Option<String> {
        let (acc, flag) = pack_pure_acc_flag(pack_expr, &self.load_defs)?;
        let parent = crate::grammar::acc_flag_word(acc, flag);
        if !self.load_def_names.contains(&parent) {
            return Some(parent);
        }
        let k = self.acc_flag_child_counters.entry((acc, flag)).or_insert(0);
        let n = crate::grammar::acc_flag_child(acc, flag, *k);
        *k += 1;
        Some(n)
    }

    fn pda_word_name(&mut self, pack_expr: &BV) -> Option<String> {
        let (kind, apply_id, field) = crate::rewrite::match_pda_word(pack_expr)?;
        let (kind, idx) = *self.pda_ids.entry(apply_id).or_insert_with(|| {
            let i = match kind {
                crate::rewrite::PdaSyscall::Find => {
                    let i = self.pda_find_counter;
                    self.pda_find_counter += 1;
                    i
                }
                crate::rewrite::PdaSyscall::Create => {
                    let i = self.pda_create_counter;
                    self.pda_create_counter += 1;
                    i
                }
            };
            (kind, i)
        });
        let prefix = match kind {
            crate::rewrite::PdaSyscall::Find => "find",
            crate::rewrite::PdaSyscall::Create => "create",
        };
        Some(format!("w_pda_{prefix}{idx}_{field}"))
    }

    fn remember_load_def(&mut self, name: String, pack_expr: &BV) {
        self.load_def_names.insert(name.clone());
        self.load_expr_ids.insert(ast_id(pack_expr), name);
    }

    fn reuse_load_temp(&self, pack_expr: &BV) -> Option<BV> {
        if pack_expr.is_const() {
            let name = pack_expr.decl().name();
            if self.load_def_names.contains(&name) {
                return Some(pack_expr.clone());
            }
        }
        self.load_expr_ids
            .get(&ast_id(pack_expr))
            .map(|n| BV::new_const(n.as_str(), WORD_BITS))
    }
}
