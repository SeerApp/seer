use anyhow::{bail, Result};
use solana_pubkey::Pubkey;
use storage::Storage;

use super::format::slice_skip_head_tail;
use super::{emit, Command};

pub(super) fn cmd(storage: &Storage, cmd: Command, short: bool) -> Result<()> {
    let Command::Regs {
        id,
        ix,
        skip,
        head,
        tail,
        start,
        end,
        order,
        pc,
        regs,
        changed,
        program,
        delta,
    } = cmd
    else {
        bail!("internal: regs");
    };
    let mut steps = glassbox::register_timeline(storage, id, ix)?;
    if let Some(order) = order {
        steps.retain(|s| s.order == order);
    } else {
        if let Some(start) = start {
            steps.retain(|s| s.order >= start);
        }
        if let Some(end) = end {
            steps.retain(|s| s.order <= end);
        }
    }
    if let Some(pc) = pc {
        steps.retain(|s| s.pc == pc);
    }
    if let Some(pk) = program {
        let want = pk.to_bytes();
        steps.retain(|s| s.pubkey == want);
    }
    if changed {
        steps.retain(|s| regs.iter().any(|&i| s.pre_regs[i] != s.post_regs[i]));
    }
    let sliced = slice_skip_head_tail(steps, skip, head, tail);
    let json_steps: Vec<serde_json::Value> = sliced
        .iter()
        .map(|s| {
            let r = if delta {
                let mut obj = serde_json::Map::new();
                for i in 0..11 {
                    if s.pre_regs[i] != s.post_regs[i] {
                        obj.insert(
                            i.to_string(),
                            serde_json::Value::String(s.post_regs[i].to_string()),
                        );
                    }
                }
                serde_json::Value::Object(obj)
            } else {
                serde_json::Value::Array(
                    s.pre_regs
                        .iter()
                        .map(|n| serde_json::Value::String(n.to_string()))
                        .collect(),
                )
            };
            serde_json::json!({
                "order": s.order,
                "pc": s.pc,
                "program": Pubkey::from(s.pubkey).to_string(),
                "r": r,
            })
        })
        .collect();
    let value = serde_json::json!({ "ix": ix, "steps": json_steps });
    let mut footer = Vec::new();
    if head != 0 && tail.is_none() {
        footer.push(format!(
            "seer regs {id} --ix {ix} --skip {} --head {head}",
            skip.saturating_add(head)
        ));
    }
    footer.push(format!("seer show {id}"));
    emit(value, &footer, short)
}
