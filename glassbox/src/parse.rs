//! Parse Seer / agave-style SBPF disasm lines into structured ops.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemWidth {
    B = 1,
    H = 2,
    W = 4,
    Dw = 8,
}

impl MemWidth {
    pub fn bytes(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemRef {
    pub base_reg: usize,
    pub offset: i64,
}

impl MemRef {
    pub fn effective_addr(&self, regs: &[u64; 11]) -> u64 {
        regs[self.base_reg].wrapping_add(self.offset as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Lsh,
    Rsh,
    Arsh,
    Mov,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Sgt,
    Sge,
    Slt,
    Sle,
}

impl RelOp {
    pub fn logic_op(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "≠",
            Self::Gt => ">",
            Self::Ge => "≥",
            Self::Lt => "<",
            Self::Le => "≤",
            Self::Sgt => ">ₛ",
            Self::Sge => "≥ₛ",
            Self::Slt => "<ₛ",
            Self::Sle => "≤ₛ",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Reg(usize),
    Imm(i64),
}

impl Operand {
    pub fn concrete(&self, regs: &[u64; 11]) -> u64 {
        match self {
            Self::Reg(r) => regs[*r],
            Self::Imm(i) => *i as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// ldx* dst, [base+off]
    Load {
        width: MemWidth,
        dst: usize,
        mem: MemRef,
    },
    /// stx* [base+off], src
    StoreReg {
        width: MemWidth,
        mem: MemRef,
        src: usize,
    },
    /// st* [base+off], imm
    StoreImm {
        width: MemWidth,
        mem: MemRef,
        imm: i64,
    },
    /// ALU / mov: dst = dst OP src  (or dst = src for mov)
    Alu {
        op: BinOp,
        dst: usize,
        src: Operand,
        /// true ⇒ 32-bit ALU (upper bits cleared / 32-bit semantics approximated)
        bits32: bool,
    },
    Neg {
        dst: usize,
        bits32: bool,
    },
    /// Conditional jump.
    Jump {
        rel: RelOp,
        dst: usize,
        src: Operand,
        target: u64,
    },
    /// Unconditional jump — no path condition.
    JumpAlways {
        target: u64,
    },
    Call,
    Exit,
    Syscall {
        name: String,
    },
    /// lddw dst, imm64
    Lddw {
        dst: usize,
        imm: u64,
    },
    Nop,
    Unknown,
}

fn parse_reg(s: &str) -> Option<usize> {
    let s = s.trim();
    let s = s.strip_prefix('r')?;
    let n: usize = s.parse().ok()?;
    (n <= 10).then_some(n)
}

fn parse_imm(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16)
            .ok()
            .or_else(|| u64::from_str_radix(hex, 16).ok().map(|u| u as i64));
    }
    s.parse().ok()
}

fn parse_u_imm(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    s.parse::<i64>()
        .ok()
        .map(|i| i as u64)
        .or_else(|| s.parse().ok())
}

fn parse_mem_ref(s: &str) -> Option<MemRef> {
    let s = s.trim();
    let s = s.strip_prefix('[')?.strip_suffix(']')?;
    // rN+off / rN-off / rN
    let s = s.trim();
    if let Some(plus) = s.find('+') {
        let base = parse_reg(&s[..plus])?;
        let offset = parse_imm(&s[plus + 1..])?;
        return Some(MemRef {
            base_reg: base,
            offset,
        });
    }
    if let Some(idx) = s.rfind('-') {
        // r10-0x1f8 — avoid treating the minus in the reg as split; reg is rN at start
        if idx > 0 {
            let base = parse_reg(&s[..idx])?;
            let offset = parse_imm(&s[idx + 1..])?;
            return Some(MemRef {
                base_reg: base,
                offset: -offset,
            });
        }
    }
    let base = parse_reg(s)?;
    Some(MemRef {
        base_reg: base,
        offset: 0,
    })
}

fn width_from_suffix(s: &str) -> Option<MemWidth> {
    match s {
        "b" => Some(MemWidth::B),
        "h" => Some(MemWidth::H),
        "w" => Some(MemWidth::W),
        "dw" => Some(MemWidth::Dw),
        _ => None,
    }
}

fn parse_operand(s: &str) -> Option<Operand> {
    let s = s.trim();
    if let Some(r) = parse_reg(s) {
        return Some(Operand::Reg(r));
    }
    parse_imm(s).map(Operand::Imm)
}

fn alu_op(mnem: &str) -> Option<(BinOp, bool)> {
    let bits32 = mnem.ends_with("32");
    let base = mnem
        .strip_suffix("64")
        .or_else(|| mnem.strip_suffix("32"))
        .unwrap_or(mnem);
    let op = match base {
        "add" => BinOp::Add,
        "sub" => BinOp::Sub,
        "mul" => BinOp::Mul,
        "div" => BinOp::Div,
        "mod" => BinOp::Mod,
        "and" => BinOp::And,
        "or" => BinOp::Or,
        "xor" => BinOp::Xor,
        "lsh" => BinOp::Lsh,
        "rsh" => BinOp::Rsh,
        "arsh" => BinOp::Arsh,
        "mov" => BinOp::Mov,
        _ => return None,
    };
    Some((op, bits32))
}

fn rel_op(mnem: &str) -> Option<RelOp> {
    match mnem {
        "jeq" => Some(RelOp::Eq),
        "jne" | "jneq" => Some(RelOp::Ne),
        "jgt" => Some(RelOp::Gt),
        "jge" => Some(RelOp::Ge),
        "jlt" => Some(RelOp::Lt),
        "jle" => Some(RelOp::Le),
        "jsgt" => Some(RelOp::Sgt),
        "jsge" => Some(RelOp::Sge),
        "jslt" => Some(RelOp::Slt),
        "jsle" => Some(RelOp::Sle),
        _ => None,
    }
}

/// Parse a single disasm line into an [`Op`].
pub fn parse_disasm(line: &str) -> Op {
    let line = line.trim();
    if line.is_empty() {
        return Op::Nop;
    }

    // syscall name (optional key suffix)
    if let Some(rest) = line.strip_prefix("syscall") {
        let name = rest
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        if name.is_empty() || name.starts_with("0x") || name == "key" {
            return Op::Syscall {
                name: "unknown".into(),
            };
        }
        return Op::Syscall { name };
    }

    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in line.chars() {
        match ch {
            '[' => {
                depth += 1;
                cur.push(ch);
            }
            ']' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            ' ' | '\t' if depth == 0 && cur.is_empty() => {}
            ' ' | '\t' if depth == 0 && parts.is_empty() && !cur.is_empty() => {
                // first token boundary (mnemonic)
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }

    if parts.is_empty() {
        return Op::Unknown;
    }

    let mnem = parts[0].trim().to_lowercase();
    let args: Vec<&str> = parts[1..].iter().map(|s| s.trim()).collect();

    if mnem == "exit" {
        return Op::Exit;
    }
    if mnem == "call" || mnem == "callx" {
        return Op::Call;
    }
    if mnem == "ja" {
        if let Some(t) = args.first().and_then(|a| parse_u_imm(a)) {
            return Op::JumpAlways { target: t };
        }
        return Op::Unknown;
    }
    if mnem == "neg64" || mnem == "neg32" || mnem == "neg" {
        let bits32 = mnem.contains('3');
        if let Some(d) = args.first().and_then(|a| parse_reg(a)) {
            return Op::Neg { dst: d, bits32 };
        }
        return Op::Unknown;
    }
    if mnem == "lddw" {
        if args.len() >= 2 {
            if let (Some(d), Some(imm)) = (parse_reg(args[0]), parse_u_imm(args[1])) {
                return Op::Lddw { dst: d, imm };
            }
        }
        return Op::Unknown;
    }

    // loads: ldxb/ldxh/ldxw/ldxdw / ldb/ldh/ldw/lddw handled above
    if let Some(suf) = mnem.strip_prefix("ldx") {
        if let Some(width) = width_from_suffix(suf) {
            if args.len() >= 2 {
                if let (Some(dst), Some(mem)) = (parse_reg(args[0]), parse_mem_ref(args[1])) {
                    return Op::Load { width, dst, mem };
                }
            }
        }
    }
    if let Some(suf) = mnem.strip_prefix("ld").filter(|s| !s.starts_with('d')) {
        // ldb, ldh, ldw (not lddw)
        if let Some(width) = width_from_suffix(suf) {
            if args.len() >= 2 {
                if let (Some(dst), Some(mem)) = (parse_reg(args[0]), parse_mem_ref(args[1])) {
                    return Op::Load { width, dst, mem };
                }
            }
        }
    }

    if let Some(suf) = mnem.strip_prefix("stx") {
        if let Some(width) = width_from_suffix(suf) {
            if args.len() >= 2 {
                if let (Some(mem), Some(src)) = (parse_mem_ref(args[0]), parse_reg(args[1])) {
                    return Op::StoreReg { width, mem, src };
                }
            }
        }
    }
    if let Some(suf) = mnem.strip_prefix("st") {
        if let Some(width) = width_from_suffix(suf) {
            if args.len() >= 2 {
                if let (Some(mem), Some(imm)) = (parse_mem_ref(args[0]), parse_imm(args[1])) {
                    return Op::StoreImm { width, mem, imm };
                }
            }
        }
    }

    if let Some(rel) = rel_op(&mnem) {
        // jeq r1, r2, target  OR  jeq r1, imm, target
        if args.len() >= 3 {
            if let (Some(dst), Some(src), Some(target)) = (
                parse_reg(args[0]),
                parse_operand(args[1]),
                parse_u_imm(args[2]),
            ) {
                return Op::Jump {
                    rel,
                    dst,
                    src,
                    target,
                };
            }
        }
        return Op::Unknown;
    }

    if let Some((op, bits32)) = alu_op(&mnem) {
        if args.len() >= 2 {
            if let (Some(dst), Some(src)) = (parse_reg(args[0]), parse_operand(args[1])) {
                return Op::Alu {
                    op,
                    dst,
                    src,
                    bits32,
                };
            }
        }
        return Op::Unknown;
    }

    Op::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(base_reg: usize, offset: i64) -> MemRef {
        MemRef { base_reg, offset }
    }

    #[test]
    fn mem_ref_effective_addr_wraps() {
        let mut regs = [0u64; 11];
        regs[10] = 0x2000;
        assert_eq!(mem(10, -8).effective_addr(&regs), 0x1ff8);
        regs[1] = u64::MAX;
        assert_eq!(mem(1, 1).effective_addr(&regs), 0);
    }

    fn alu(op: BinOp, dst: usize, src: Operand, bits32: bool) -> Op {
        Op::Alu {
            op,
            dst,
            src,
            bits32,
        }
    }

    fn jump(rel: RelOp, dst: usize, src: Operand, target: u64) -> Op {
        Op::Jump {
            rel,
            dst,
            src,
            target,
        }
    }

    /// One row per mnemonic the parser claims to handle, plus the Unknowns we
    /// currently emit.
    #[test]
    fn table_every_mnemonic() {
        let cases: &[(&str, Op)] = &[
            // empty string is Nop; Artifacts drops those steps before the VM
            ("", Op::Nop),
            ("   ", Op::Nop),
            ("exit", Op::Exit),
            ("call", Op::Call),
            ("call 0x8", Op::Call),
            ("callx r4", Op::Call),
            (
                "callx r4  [indirect callx; target in register at runtime]",
                Op::Call,
            ),
            ("ja 0x1e980", Op::JumpAlways { target: 0x1e980 }),
            (
                "neg64 r1",
                Op::Neg {
                    dst: 1,
                    bits32: false,
                },
            ),
            (
                "neg32 r1",
                Op::Neg {
                    dst: 1,
                    bits32: true,
                },
            ),
            (
                "neg r1",
                Op::Neg {
                    dst: 1,
                    bits32: false,
                },
            ),
            (
                "lddw r1, 0x1122334455667788",
                Op::Lddw {
                    dst: 1,
                    imm: 0x1122334455667788,
                },
            ),
            (
                "ldxb r0, [r1]",
                Op::Load {
                    width: MemWidth::B,
                    dst: 0,
                    mem: mem(1, 0),
                },
            ),
            (
                "ldxh r0, [r1+2]",
                Op::Load {
                    width: MemWidth::H,
                    dst: 0,
                    mem: mem(1, 2),
                },
            ),
            (
                "ldxw r0, [r1+4]",
                Op::Load {
                    width: MemWidth::W,
                    dst: 0,
                    mem: mem(1, 4),
                },
            ),
            (
                "ldxdw r2, [r10-0x1f8]",
                Op::Load {
                    width: MemWidth::Dw,
                    dst: 2,
                    mem: mem(10, -0x1f8),
                },
            ),
            (
                "ldb r0, [r1]",
                Op::Load {
                    width: MemWidth::B,
                    dst: 0,
                    mem: mem(1, 0),
                },
            ),
            (
                "ldh r0, [r1]",
                Op::Load {
                    width: MemWidth::H,
                    dst: 0,
                    mem: mem(1, 0),
                },
            ),
            (
                "ldw r0, [r1]",
                Op::Load {
                    width: MemWidth::W,
                    dst: 0,
                    mem: mem(1, 0),
                },
            ),
            (
                "stxb [r10-8], r1",
                Op::StoreReg {
                    width: MemWidth::B,
                    mem: mem(10, -8),
                    src: 1,
                },
            ),
            (
                "stxh [r10-8], r1",
                Op::StoreReg {
                    width: MemWidth::H,
                    mem: mem(10, -8),
                    src: 1,
                },
            ),
            (
                "stxw [r10-8], r1",
                Op::StoreReg {
                    width: MemWidth::W,
                    mem: mem(10, -8),
                    src: 1,
                },
            ),
            (
                "stxdw [r10-8], r1",
                Op::StoreReg {
                    width: MemWidth::Dw,
                    mem: mem(10, -8),
                    src: 1,
                },
            ),
            (
                "stb [r10-8], 0xff",
                Op::StoreImm {
                    width: MemWidth::B,
                    mem: mem(10, -8),
                    imm: 0xff,
                },
            ),
            (
                "sth [r10-8], 0x100",
                Op::StoreImm {
                    width: MemWidth::H,
                    mem: mem(10, -8),
                    imm: 0x100,
                },
            ),
            (
                "stw [r10-8], 1",
                Op::StoreImm {
                    width: MemWidth::W,
                    mem: mem(10, -8),
                    imm: 1,
                },
            ),
            (
                "stdw [r10-8], -1",
                Op::StoreImm {
                    width: MemWidth::Dw,
                    mem: mem(10, -8),
                    imm: -1,
                },
            ),
            (
                "syscall sol_memcpy_ (key 0x1234)",
                Op::Syscall {
                    name: String::from("sol_memcpy_"),
                },
            ),
            (
                "syscall",
                Op::Syscall {
                    name: String::from("unknown"),
                },
            ),
            (
                "syscall 0x2",
                Op::Syscall {
                    name: String::from("unknown"),
                },
            ),
            ("add64 r1, 1", alu(BinOp::Add, 1, Operand::Imm(1), false)),
            ("sub64 r1, r2", alu(BinOp::Sub, 1, Operand::Reg(2), false)),
            ("mul64 r1, 3", alu(BinOp::Mul, 1, Operand::Imm(3), false)),
            ("div64 r1, 2", alu(BinOp::Div, 1, Operand::Imm(2), false)),
            ("mod64 r1, 5", alu(BinOp::Mod, 1, Operand::Imm(5), false)),
            (
                "and64 r1, 0xff",
                alu(BinOp::And, 1, Operand::Imm(0xff), false),
            ),
            ("or64 r1, r2", alu(BinOp::Or, 1, Operand::Reg(2), false)),
            ("xor64 r1, 1", alu(BinOp::Xor, 1, Operand::Imm(1), false)),
            ("lsh64 r1, 3", alu(BinOp::Lsh, 1, Operand::Imm(3), false)),
            ("rsh64 r1, 3", alu(BinOp::Rsh, 1, Operand::Imm(3), false)),
            ("arsh64 r1, 3", alu(BinOp::Arsh, 1, Operand::Imm(3), false)),
            ("mov64 r1, r2", alu(BinOp::Mov, 1, Operand::Reg(2), false)),
            ("add32 r1, 1", alu(BinOp::Add, 1, Operand::Imm(1), true)),
            ("mov32 r1, 0", alu(BinOp::Mov, 1, Operand::Imm(0), true)),
            ("add r1, 1", alu(BinOp::Add, 1, Operand::Imm(1), false)),
            (
                "jeq r3, r4, 0x1e980",
                jump(RelOp::Eq, 3, Operand::Reg(4), 0x1e980),
            ),
            ("jne r1, 0, 8", jump(RelOp::Ne, 1, Operand::Imm(0), 8)),
            ("jneq r1, 0, 8", jump(RelOp::Ne, 1, Operand::Imm(0), 8)),
            ("jgt r1, r2, 8", jump(RelOp::Gt, 1, Operand::Reg(2), 8)),
            ("jge r1, r2, 8", jump(RelOp::Ge, 1, Operand::Reg(2), 8)),
            ("jlt r1, r2, 8", jump(RelOp::Lt, 1, Operand::Reg(2), 8)),
            ("jle r1, r2, 8", jump(RelOp::Le, 1, Operand::Reg(2), 8)),
            ("jsgt r1, r2, 8", jump(RelOp::Sgt, 1, Operand::Reg(2), 8)),
            ("jsge r1, r2, 8", jump(RelOp::Sge, 1, Operand::Reg(2), 8)),
            ("jslt r1, r2, 8", jump(RelOp::Slt, 1, Operand::Reg(2), 8)),
            ("jsle r1, r2, 8", jump(RelOp::Sle, 1, Operand::Reg(2), 8)),
            // not modeled
            ("hor64 r1, r2", Op::Unknown),
            ("be64 r1", Op::Unknown),
            ("le64 r1", Op::Unknown),
            ("ldxq r0, [r1]", Op::Unknown),
            ("ja", Op::Unknown),
            ("neg64", Op::Unknown),
            ("add64 r1", Op::Unknown),
            ("jeq r1, r2", Op::Unknown),
            // syscall is matched on the raw prefix, before lowercasing
            ("SYSCALL sol_log_", Op::Unknown),
        ];
        for (line, expect) in cases {
            assert_eq!(parse_disasm(line), *expect, "line: {line:?}");
        }
    }
}
