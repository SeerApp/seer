//! Goldens for in-memory disasm / lifted chunks (per-PC strings + lifted CFG).
//!
//! Set `SEER_TEST_SAVE` to rewrite `tests/fixtures/canonical_result/` instead of asserting.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const SEER_TEST_SAVE_ENV: &str = "SEER_TEST_SAVE";
const TEXT_VMA: u64 = 64;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn golden_dir() -> PathBuf {
    fixtures_dir().join("canonical_result")
}

fn seer_test_save_enabled() -> bool {
    std::env::var(SEER_TEST_SAVE_ENV).is_ok()
}

fn insn(opc: u8, dst: u8, src: u8, off: i16, imm: i32) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[0] = opc;
    bytes[1] = dst | src.checked_shl(4).expect("src nibble");
    bytes[2..4].copy_from_slice(&off.to_le_bytes());
    bytes[4..8].copy_from_slice(&imm.to_le_bytes());
    bytes
}

fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

#[allow(clippy::too_many_arguments)]
fn push_shdr(
    buf: &mut Vec<u8>,
    name: u32,
    typ: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
) {
    push_u32(buf, name);
    push_u32(buf, typ);
    push_u64(buf, flags);
    push_u64(buf, addr);
    push_u64(buf, offset);
    push_u64(buf, size);
    push_u32(buf, link);
    push_u32(buf, info);
    push_u64(buf, addralign);
    push_u64(buf, entsize);
}

/// Minimal SBF v0 `ET_DYN` ELF: cond jump, relative call, uncond jump, helper+exit.
///
/// ```text
/// entrypoint:
///   mov64 r0, 1
///   jeq r0, 1, +3      // -> helper
///   call helper
///   ja +1              // -> helper
///   mov64 r0, 0
/// helper:
///   add64 r0, 1
///   exit
/// ```
fn jumps_elf() -> Vec<u8> {
    // Classic eBPF opcodes (SBF v0). 0x15 is `jeq` / `jeq64` depending on sBPF version naming.
    const MOV64_IMM: u8 = 0xb7;
    const ADD64_IMM: u8 = 0x07;
    const JA: u8 = 0x05;
    const JEQ_IMM: u8 = 0x15;
    const CALL_IMM: u8 = 0x85;
    const EXIT: u8 = 0x95;

    let text: Vec<u8> = [
        insn(MOV64_IMM, 0, 0, 0, 1),
        insn(JEQ_IMM, 0, 0, 3, 1),
        insn(CALL_IMM, 0, 0, 0, 2),
        insn(JA, 0, 0, 1, 0),
        insn(MOV64_IMM, 0, 0, 0, 0),
        insn(ADD64_IMM, 0, 0, 0, 1),
        insn(EXIT, 0, 0, 0, 0),
    ]
    .into_iter()
    .flatten()
    .collect();
    let text_len = u64::try_from(text.len()).expect("text fits u64");

    let shstrtab: &[u8] = b"\0.text\0.shstrtab\0.strtab\0.symtab\0";
    let strtab: &[u8] = b"\0entrypoint\0helper\0";
    // null + entrypoint + helper
    let mut symtab = vec![0u8; 24];
    // entrypoint: st_name=1, GLOBAL FUNC, shndx=.text(1), value=TEXT_VMA, size=5 insns
    push_u32(&mut symtab, 1);
    symtab.push(0x12);
    symtab.push(0);
    push_u16(&mut symtab, 1);
    push_u64(&mut symtab, TEXT_VMA);
    push_u64(&mut symtab, 40);
    // helper at slot 5
    push_u32(&mut symtab, 12);
    symtab.push(0x12);
    symtab.push(0);
    push_u16(&mut symtab, 1);
    push_u64(&mut symtab, TEXT_VMA.checked_add(40).expect("helper vma"));
    push_u64(&mut symtab, 16);

    fn align8(n: u64) -> u64 {
        n.checked_add(7)
            .expect("align")
            .checked_div(8)
            .expect("align")
            .checked_mul(8)
            .expect("align")
    }

    let ehdr = 64u64;
    let text_off = ehdr;
    let shstrtab_off = align8(text_off.checked_add(text_len).expect("shstrtab off"));
    let strtab_off = align8(
        shstrtab_off
            .checked_add(u64::try_from(shstrtab.len()).expect("shstrtab"))
            .expect("strtab off"),
    );
    let symtab_off = align8(
        strtab_off
            .checked_add(u64::try_from(strtab.len()).expect("strtab"))
            .expect("symtab off"),
    );
    let shoff = align8(
        symtab_off
            .checked_add(u64::try_from(symtab.len()).expect("symtab"))
            .expect("shoff"),
    );

    let mut elf = Vec::new();
    elf.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    push_u16(&mut elf, 3); // ET_DYN
    push_u16(&mut elf, 247); // EM_BPF
    push_u32(&mut elf, 1);
    push_u64(&mut elf, TEXT_VMA);
    push_u64(&mut elf, 0);
    push_u64(&mut elf, shoff);
    push_u32(&mut elf, 0); // e_flags = SBF v0
    push_u16(&mut elf, 64);
    push_u16(&mut elf, 56);
    push_u16(&mut elf, 0);
    push_u16(&mut elf, 64);
    push_u16(&mut elf, 5);
    push_u16(&mut elf, 2); // .shstrtab

    fn pad_to(buf: &mut Vec<u8>, off: u64) {
        let want = usize::try_from(off).expect("off");
        assert!(buf.len() <= want, "section overlap");
        buf.resize(want, 0);
    }

    assert_eq!(u64::try_from(elf.len()).expect("ehdr"), ehdr);
    elf.extend_from_slice(&text);
    pad_to(&mut elf, shstrtab_off);
    elf.extend_from_slice(shstrtab);
    pad_to(&mut elf, strtab_off);
    elf.extend_from_slice(strtab);
    pad_to(&mut elf, symtab_off);
    elf.extend_from_slice(&symtab);
    pad_to(&mut elf, shoff);

    // Section headers must be in ascending sh_offset order for the sBPF parser.
    push_shdr(&mut elf, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    push_shdr(
        &mut elf, 1, 1, 6, // SHF_ALLOC | SHF_EXECINSTR
        TEXT_VMA, text_off, text_len, 0, 0, 8, 0,
    );
    push_shdr(
        &mut elf,
        7,
        3,
        0,
        0,
        shstrtab_off,
        u64::try_from(shstrtab.len()).expect("shstrtab"),
        0,
        0,
        1,
        0,
    );
    push_shdr(
        &mut elf,
        17,
        3,
        0,
        0,
        strtab_off,
        u64::try_from(strtab.len()).expect("strtab"),
        0,
        0,
        1,
        0,
    );
    push_shdr(
        &mut elf,
        25,
        2,
        0,
        0,
        symtab_off,
        u64::try_from(symtab.len()).expect("symtab"),
        3, // .strtab
        1,
        8,
        24,
    );
    elf
}

fn load_json_dir(dir: &Path) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.expect("dirent").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    for path in files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 json name")
            .to_string();
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let value: Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        out.insert(name, value);
    }
    out
}

fn chunks_map(chunks: &[decode::JsonChunk]) -> BTreeMap<String, Value> {
    chunks
        .iter()
        .map(|chunk| {
            (
                format!("{}_{}.json", chunk.start_pc, chunk.end_pc),
                chunk.json.clone(),
            )
        })
        .collect()
}

fn assert_or_update_map(kind: &str, actual: &BTreeMap<String, Value>) {
    let expected_dir = golden_dir().join("jumps").join(kind);

    if seer_test_save_enabled() {
        if expected_dir.exists() {
            fs::remove_dir_all(&expected_dir).expect("clear golden dir");
        }
        fs::create_dir_all(&expected_dir).expect("create golden dir");
        for (name, value) in actual {
            let pretty = serde_json::to_string_pretty(value).expect("pretty json");
            fs::write(expected_dir.join(name), pretty).expect("write golden");
        }
        return;
    }

    let expected = load_json_dir(&expected_dir);
    assert_eq!(
        expected, *actual,
        "{kind} golden mismatch (set {SEER_TEST_SAVE_ENV} to regenerate)"
    );
}

#[test]
fn jumps_disasm_and_lifted_cfg_goldens() {
    let elf = jumps_elf();
    let disasm = decode::disasm_chunks(&elf).expect("disasm_chunks");
    let lifted = decode::lifted_chunks(&elf).expect("lifted_chunks");
    assert_or_update_map("disasm", &chunks_map(&disasm));
    assert_or_update_map("lifted", &chunks_map(&lifted));

    if !seer_test_save_enabled() {
        assert_eq!(disasm.len(), 1, "tiny program should be one disasm chunk");
        assert_eq!(lifted.len(), 1, "tiny program should be one lifted chunk");
        let chunk = &lifted[0].json;
        let blocks = chunk
            .get("blocks")
            .and_then(Value::as_object)
            .expect("lifted.blocks");
        assert!(
            blocks.len() >= 3,
            "expected several basic blocks, got {}",
            blocks.len()
        );
        let has_branch = blocks.values().any(|b| {
            b.get("succ")
                .and_then(Value::as_array)
                .is_some_and(|s| s.len() >= 2)
        });
        assert!(has_branch, "expected a conditional block with two succs");
    }
}

#[test]
fn store_is_lazy_and_covers_decoded_pcs() {
    let root = std::env::temp_dir().join(format!(
        "seer-disasm-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    let storage = storage::Storage::open_at(&root).expect("storage");
    let elf = jumps_elf();
    let hash = storage.blob.store(&elf).expect("store elf");
    decode::store_disasm(&storage, &hash).expect("store disasm");
    decode::store_disasm(&storage, &hash).expect("store disasm again");
    let rows = storage.db.program_disasm_chunks(&hash).expect("rows");
    assert_eq!(rows.len(), 1);
    let decoded = decode::disasm_chunks(&elf).expect("decode");
    assert_eq!(decoded.len(), 1);
    let stored: Value =
        serde_json::from_slice(&storage.blob.read(&rows[0].blob_hash).expect("blob"))
            .expect("json");
    assert_eq!(stored, decoded[0].json);
    decode::store_lifted(&storage, &hash).expect("store lifted");
    assert_eq!(
        storage
            .db
            .program_lifted_chunks(&hash)
            .expect("lifted")
            .len(),
        1
    );
    let _ = fs::remove_dir_all(root);
}
