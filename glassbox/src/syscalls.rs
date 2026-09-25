//! Solana syscall handlers for symbolic state updates.

use z3::ast::BV;
use z3::{FuncDecl, Sort};

use crate::grammar::Syscall;
use crate::regions::is_interesting;
use crate::state::{merge_origins, Memory, Registers, SymVal, SysvarOrigin};

fn can_resolve_range(state: &Memory, addr: u64, len: u64) -> bool {
    (0..len).all(|i| state.can_resolve(addr.wrapping_add(i)))
}

fn resolve_range(state: &mut Memory, addr: u64, len: u64) -> Option<Vec<SymVal>> {
    if !can_resolve_range(state, addr, len) {
        return None;
    }
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(state.resolve_byte(addr.wrapping_add(i))?);
    }
    Some(out)
}

fn concat_le(bytes: &[SymVal]) -> (BV, bool, Vec<SysvarOrigin>) {
    assert!(!bytes.is_empty());
    let mut env = bytes[0].environmental;
    let mut origins = bytes[0].origins.clone();
    let mut acc = bytes[0].bv.clone();
    for b in bytes.iter().skip(1) {
        env |= b.environmental;
        merge_origins(&mut origins, &b.origins);
        acc = b.bv.concat(&acc);
    }
    (acc, env, origins)
}

fn write_bv_le(
    state: &mut Memory,
    addr: u64,
    value: &BV,
    environmental: bool,
    origins: &[SysvarOrigin],
) {
    let bits = value.get_size();
    let nbytes = (bits as usize).div_ceil(8);
    let padded = if bits % 8 != 0 {
        value.zero_ext(8 - (bits % 8))
    } else {
        value.clone()
    };
    for i in 0..nbytes {
        let byte = padded
            .bvlshr(&BV::from_u64((i * 8) as u64, padded.get_size()))
            .extract(7, 0);
        state.write_byte(
            addr + i as u64,
            SymVal {
                bv: byte,
                environmental,
                origins: origins.to_vec(),
            },
        );
    }
}

fn sol_memcmp(state: &mut Memory, regs: &[u64; 11]) {
    let a = regs[1];
    let b = regs[2];
    let n = regs[3];
    let result = regs[4];
    if n > 256 {
        // Keep interpreted memcmp bounded for v1.
        return;
    }
    if !can_resolve_range(state, a, n) || !can_resolve_range(state, b, n) {
        return;
    }
    let Some(aa) = resolve_range(state, a, n) else {
        return;
    };
    let Some(bb) = resolve_range(state, b, n) else {
        return;
    };
    if !aa.iter().any(|x| x.environmental) && !bb.iter().any(|x| x.environmental) {
        return;
    }

    let zero32 = BV::from_i64(0, 32);
    let mut expr = zero32.clone();
    for i in (0..n as usize).rev() {
        let ai = aa[i].bv.zero_ext(24);
        let bi = bb[i].bv.zero_ext(24);
        let diff = ai.bvsub(&bi);
        let eq = aa[i].bv.eq(&bb[i].bv);
        expr = eq.ite(&expr, &diff);
    }
    write_bv_le(state, result, &expr, true, &[]);
}

fn sol_memcpy_like(state: &mut Memory, regs: &[u64; 11]) {
    let dst = regs[1];
    let src = regs[2];
    let n = regs[3];
    if n > 1_000_000 {
        return;
    }
    let mut tmp: Vec<Option<SymVal>> = Vec::with_capacity(n as usize);
    for i in 0..n {
        let addr = src.wrapping_add(i);
        if state.can_resolve(addr) {
            tmp.push(state.resolve_byte(addr));
        } else {
            tmp.push(None);
        }
    }
    for (i, cell) in tmp.into_iter().enumerate() {
        let d = dst.wrapping_add(i as u64);
        match cell {
            Some(b) => state.write_byte(d, b),
            None => {
                if state.has_byte(d) || is_interesting(d) {
                    state.write_fresh_unknown(d);
                }
            }
        }
    }
}

fn sol_memset(state: &mut Memory, regs: &[u64; 11]) {
    let dst = regs[1];
    let c = (regs[2] & 0xff) as u8;
    let n = regs[3];
    if n > 1_000_000 {
        return;
    }
    for i in 0..n {
        let d = dst.wrapping_add(i);
        if state.has_byte(d) || is_interesting(d) {
            state.write_concrete_byte(d, c);
        }
    }
}

fn uif_bytes(state: &mut Memory, name: &str, input: &[SymVal], out: u64, out_len: u32) {
    if input.is_empty() || !input.iter().any(|b| b.environmental) {
        return;
    }
    let (arg, env, origins) = concat_le(input);
    let domain = Sort::bitvector(arg.get_size());
    let range = Sort::bitvector(out_len * 8);
    let f = FuncDecl::new(name, &[&domain], &range);
    let result = f.apply(&[&arg]).as_bv().expect("UIF range is BV");
    write_bv_le(state, out, &result, env, &origins);
}

const MAX_SEEDS: u64 = 16;
const MAX_SEED_LEN: u64 = 32;

fn resolve_u64_le(state: &mut Memory, addr: u64) -> Option<u64> {
    let bytes = resolve_range(state, addr, 8)?;
    let mut v = 0u64;
    for (i, b) in bytes.iter().enumerate() {
        let bits = b.bv.get_size();
        let wide = if bits < 64 {
            b.bv.zero_ext(64 - bits)
        } else {
            b.bv.clone()
        };
        let x = crate::rewrite::eval_numeral(&wide)?;
        if x > 0xff {
            return None;
        }
        v |= x << (8 * i);
    }
    Some(v)
}

/// Walk the `r1`/`r2` seed slice table (`{ptr: u64, len: u64}` × n).
fn collect_seed_bytes(state: &mut Memory, seeds_addr: u64, seeds_len: u64) -> Option<Vec<SymVal>> {
    if seeds_len > MAX_SEEDS {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..seeds_len {
        let entry = seeds_addr.wrapping_add(i * 16);
        let ptr = resolve_u64_le(state, entry)?;
        let len = resolve_u64_le(state, entry.wrapping_add(8))?;
        if len > MAX_SEED_LEN {
            return None;
        }
        if len == 0 {
            continue;
        }
        out.extend(resolve_range(state, ptr, len)?);
    }
    Some(out)
}

fn pda_input(state: &mut Memory, regs: &[u64; 11]) -> Vec<SymVal> {
    let mut input = collect_seed_bytes(state, regs[1], regs[2]).unwrap_or_default();
    if let Some(prog) = resolve_range(state, regs[3], 32) {
        input.extend(prog);
    }
    input
}

fn sol_create_program_address(state: &mut Memory, sc: Syscall, regs: &[u64; 11]) {
    let input = pda_input(state, regs);
    uif_bytes(state, &sc.uif(), &input, regs[4], 32);
}

fn sol_try_find_program_address(state: &mut Memory, sc: Syscall, regs: &[u64; 11]) {
    let input = pda_input(state, regs);
    if input.is_empty() || !input.iter().any(|b| b.environmental) {
        return;
    }
    let (arg, env, origins) = concat_le(&input);
    let f = FuncDecl::new(
        sc.uif(),
        &[&Sort::bitvector(arg.get_size())],
        &Sort::bitvector(33 * 8),
    );
    let result = f.apply(&[&arg]).as_bv().expect("UIF range is BV");
    write_bv_le(state, regs[4], &result.extract(255, 0), env, &origins);
    write_bv_le(state, regs[5], &result.extract(263, 256), env, &origins);
}

fn mark_sysvar(state: &mut Memory, regs: &[u64; 11], sc: Syscall, pc: u64) {
    let size = sc.sysvar_size().expect("sysvar syscall");
    state.mark_sysvar_region(regs[1], size as u64, sc.as_str(), pc);
}

/// Whether [`apply_syscall`] modelled the syscall or left a coverage gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallStatus {
    /// State updated, or a deliberate no-op (log, abort, invoke).
    Handled,
    /// Recognized but not modelled yet, or an unknown name.
    Unhandled,
}

pub(crate) fn apply_syscall(
    state: &mut Memory,
    registers: &mut Registers,
    name: &str,
    pre: &[u64; 11],
    post: &[u64; 11],
    pc: u64,
) -> SyscallStatus {
    let Some(sc) = Syscall::parse(name) else {
        return SyscallStatus::Unhandled;
    };
    match sc {
        Syscall::Abort
        | Syscall::Panic
        | Syscall::Log
        | Syscall::Log64
        | Syscall::LogPubkey
        | Syscall::LogComputeUnits
        | Syscall::LogData
        | Syscall::InvokeSignedRust
        | Syscall::InvokeSignedC => SyscallStatus::Handled,

        Syscall::Memcpy | Syscall::Memmove => {
            sol_memcpy_like(state, pre);
            SyscallStatus::Handled
        }
        Syscall::Memset => {
            sol_memset(state, pre);
            SyscallStatus::Handled
        }
        Syscall::Memcmp => {
            sol_memcmp(state, pre);
            SyscallStatus::Handled
        }

        Syscall::Sha256
        | Syscall::Keccak256
        | Syscall::Blake3
        | Syscall::Sha512
        | Syscall::Poseidon
        | Syscall::GetReturnData => SyscallStatus::Unhandled,

        Syscall::CreateProgramAddress => {
            sol_create_program_address(state, sc, pre);
            SyscallStatus::Handled
        }
        Syscall::TryFindProgramAddress => {
            sol_try_find_program_address(state, sc, pre);
            SyscallStatus::Handled
        }

        Syscall::GetClockSysvar
        | Syscall::GetRentSysvar
        | Syscall::GetEpochScheduleSysvar
        | Syscall::GetEpochRewardsSysvar
        | Syscall::GetFeesSysvar
        | Syscall::GetLastRestartSlot => {
            mark_sysvar(state, pre, sc, pc);
            SyscallStatus::Handled
        }
        Syscall::GetSysvar => {
            // dest = r2, length = r4 (offset r3 is into the sysvar, not dest).
            state.mark_sysvar_region(pre[2], pre[4], sc.as_str(), pc);
            registers[0] = Some(SymVal::from_u64(post[0]));
            SyscallStatus::Handled
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regions::INPUT_BASE;
    use crate::state::Memory;
    use z3::ast::BV;

    fn write_u64(mem: &mut Memory, addr: u64, v: u64) {
        for i in 0..8u64 {
            mem.write_concrete_byte(addr + i, ((v >> (8 * i)) & 0xff) as u8);
        }
    }

    fn uif_arg_bits(bv: &BV) -> Option<u32> {
        use z3::ast::{Ast, Dynamic};
        use z3::DeclKind;
        let mut cur = Dynamic::from(bv);
        loop {
            match cur.decl().kind() {
                DeclKind::Extract | DeclKind::Blshr => cur = cur.nth_child(0)?,
                _ if crate::grammar::is_syscall_uif(&cur.decl().name()) => {
                    return cur.nth_child(0)?.as_bv().map(|a| a.get_size());
                }
                _ => return None,
            }
        }
    }

    fn apply(mem: &mut Memory, name: &str, pre: &[u64; 11], post: &[u64; 11]) -> SyscallStatus {
        let mut regs = Registers::new();
        apply_syscall(mem, &mut regs, name, pre, post, 0)
    }

    #[test]
    fn log_is_handled_hash_is_not() {
        let mut mem = Memory::new();
        let z = [0u64; 11];
        assert_eq!(apply(&mut mem, "sol_log_", &z, &z), SyscallStatus::Handled);
        assert_eq!(
            apply(&mut mem, "sol_sha256", &z, &z),
            SyscallStatus::Unhandled
        );
    }

    #[test]
    fn create_program_address_uif_includes_seed_bytes() {
        let mut mem = Memory::new();
        const STACK: u64 = 0x2_0000_0000;
        const OUT: u64 = 0x3_0000_0000;
        let prog = INPUT_BASE;
        let seed = INPUT_BASE + 32;
        for off in 0..32 {
            let _ = mem.resolve_byte(prog + off);
            let _ = mem.resolve_byte(seed + off);
        }
        write_u64(&mut mem, STACK, seed);
        write_u64(&mut mem, STACK + 8, 32);
        let mut pre = [0u64; 11];
        pre[1] = STACK;
        pre[2] = 1;
        pre[3] = prog;
        pre[4] = OUT;
        assert_eq!(
            apply(&mut mem, "sol_create_program_address", &pre, &pre),
            SyscallStatus::Handled
        );
        let out = mem.resolve_byte(OUT).expect("pda byte");
        assert!(out.environmental);
        let s = out.bv.to_string();
        assert_eq!(uif_arg_bits(&out.bv), Some(512), "seeds ‖ program_id; {s}");
        assert!(s.contains("uif_sol_create_program_address"), "{s}");
        assert!(s.contains("n_acc0_pubkey"), "seed pubkey missing: {s}");
        assert!(
            s.contains("n_num_accounts") || s.contains("n_program_id"),
            "{s}"
        );
    }

    #[test]
    fn create_program_address_falls_back_to_program_id() {
        let mut mem = Memory::new();
        const OUT: u64 = 0x3_0000_0000;
        for off in 0..32 {
            let _ = mem.resolve_byte(INPUT_BASE + off);
        }
        let mut pre = [0u64; 11];
        pre[1] = 0x2_0000_0000;
        pre[2] = 1;
        pre[3] = INPUT_BASE;
        pre[4] = OUT;
        apply(&mut mem, "sol_create_program_address", &pre, &pre);
        let out = mem.resolve_byte(OUT).expect("pda byte");
        assert_eq!(uif_arg_bits(&out.bv), Some(256), "program id only");
    }
}
